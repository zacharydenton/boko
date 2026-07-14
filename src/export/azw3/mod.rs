//! AZW3/KF8 exporter.
//!
//! Creates KF8 (Kindle Format 8) files from Book structures.

use std::collections::{HashMap, HashSet};
use std::io::{self, Seek, Write};

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::mobi::index::{
    GuideBuildEntry, NcxBuildEntry, build_chunk_indx, build_cncx, build_guide_indx, build_ncx_indx,
    build_skel_indx, calculate_cncx_offsets,
};
use crate::mobi::skeleton::{Chunker, ChunkerResult};
use crate::mobi::writer_transform::{
    rewrite_css_references_fast, rewrite_html_references_fast, write_base32_4, write_base32_10,
};
use crate::model::{Book, Resource, TocEntry};
use crate::util::guess_media_type;

use super::Exporter;

// Constants
const RECORD_SIZE: usize = 4096;
const NULL_INDEX: u32 = 0xFFFF_FFFF;
const XOR_KEY_LEN: usize = 20;

mod guide;
mod kf8;

use kf8::Kf8Builder;

/// Configuration for AZW3 export.
#[derive(Debug, Clone, Default)]
pub struct Azw3Config {
    /// If true, normalize content through IR pipeline for clean, consistent output.
    /// Default is false (passthrough mode preserves original HTML/CSS).
    pub normalize: bool,
}

/// AZW3/KF8 format exporter.
///
/// Creates KF8 files compatible with modern Kindle devices.
pub struct Azw3Exporter {
    config: Azw3Config,
}

impl Azw3Exporter {
    /// Create a new exporter with default configuration.
    pub fn new() -> Self {
        Self {
            config: Azw3Config::default(),
        }
    }

    /// Configure the exporter with custom settings.
    pub fn with_config(mut self, config: Azw3Config) -> Self {
        self.config = config;
        self
    }
}

impl Default for Azw3Exporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter for Azw3Exporter {
    fn export<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> crate::Result<()> {
        // Normalize when explicitly requested OR when the source format requires
        // it (e.g. KFX raw content is binary Ion, not HTML) — otherwise the
        // builder would chunk and compress that binary as if it were XHTML.
        let normalize = self.config.normalize || book.requires_normalized_export();
        let builder = Kf8Builder::new(book, normalize)?;
        Ok(builder.write(writer)?)
    }
}

/// Internal context for collecting book data.
struct BookContext {
    /// Maps href -> Resource (data + media_type)
    resources: HashMap<String, Resource>,
    /// Spine items as (href, data) pairs
    spine: Vec<SpineItem>,
    /// TOC entries
    toc: Vec<TocEntry>,
    /// Metadata
    metadata: crate::model::Metadata,
    /// Landmarks (used to build the K8 guide index).
    landmarks: Vec<crate::model::Landmark>,
}

impl BookContext {
    fn landmarks(&self) -> &[crate::model::Landmark] {
        &self.landmarks
    }
}

/// A reading-order entry. Chapter bytes live in `BookContext::resources`
/// (keyed by href) — storing them here too doubled peak memory for the whole
/// text payload and read every spine document from the archive twice.
struct SpineItem {
    href: String,
}

impl BookContext {
    /// Collect all data from a Book into internal structures.
    fn from_book(book: &mut Book, normalize: bool) -> crate::Result<Self> {
        if normalize {
            Self::from_normalized(book)
        } else {
            Self::from_raw(book)
        }
    }

    /// Collect raw (passthrough) content from the book.
    fn from_raw(book: &mut Book) -> crate::Result<Self> {
        // Collect metadata and TOC (these are borrowed, so clone)
        let metadata = book.metadata().clone();
        let toc = book.toc().to_vec();

        // Collect spine items; their bytes go straight into `resources`.
        let spine_entries: Vec<_> = book.spine().to_vec();
        let mut spine = Vec::with_capacity(spine_entries.len());
        let mut resources = HashMap::new();

        for entry in &spine_entries {
            let href = book
                .source_id(entry.id)
                .unwrap_or("unknown.xhtml")
                .to_string();
            let data = book.load_raw(entry.id)?;
            resources.insert(
                href.clone(),
                Resource {
                    data,
                    media_type: "application/xhtml+xml",
                },
            );
            spine.push(SpineItem { href });
        }

        // Collect assets, skipping spine documents already loaded above.
        let asset_paths: Vec<_> = book.list_assets().to_vec();
        for path in asset_paths {
            let path_str = path.to_string_lossy().to_string();
            if resources.contains_key(&path_str) {
                continue;
            }
            let data = book.load_asset(&path)?;
            let media_type = guess_media_type(&path_str);

            resources.insert(path_str, Resource { data, media_type });
        }

        Ok(Self {
            resources,
            spine,
            toc,
            metadata,
            landmarks: book.landmarks().to_vec(),
        })
    }

    /// Collect normalized content from the book through IR pipeline.
    fn from_normalized(book: &mut Book) -> crate::Result<Self> {
        use super::normalize::normalize_book;

        let normalized = normalize_book(book)?;

        // Collect metadata and TOC
        let metadata = book.metadata().clone();
        let toc = book.toc().to_vec();

        let mut resources = HashMap::new();

        // Add unified CSS as a resource
        if !normalized.css.is_empty() {
            resources.insert(
                "style.css".to_string(),
                Resource {
                    data: normalized.css.into_bytes(),
                    media_type: "text/css",
                },
            );
        }

        // Build spine from normalized chapters; bytes stored once in `resources`.
        let mut spine = Vec::with_capacity(normalized.chapters.len());
        for (i, chapter) in normalized.chapters.iter().enumerate() {
            let href = format!("chapter_{}.xhtml", i);
            resources.insert(
                href.clone(),
                Resource {
                    data: chapter.document.as_bytes().to_vec(),
                    media_type: "application/xhtml+xml",
                },
            );
            spine.push(SpineItem { href });
        }

        // Add referenced assets
        for asset_path in &normalized.assets {
            if let Ok(data) = book.load_asset(std::path::Path::new(asset_path)) {
                let media_type = guess_media_type(asset_path);
                resources.insert(asset_path.clone(), Resource { data, media_type });
            }
        }

        Ok(Self {
            resources,
            spine,
            toc,
            metadata,
            landmarks: book.landmarks().to_vec(),
        })
    }
}

