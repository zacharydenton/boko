//! Core data types and runtime handle for ebooks.
//!
//! This module provides:
//! - Format-agnostic types (`Metadata`, `TocEntry`, `Resource`, `SpineItem`)
//! - The `Book` runtime handle for reading ebooks via importers

use std::collections::BTreeMap;
use std::io::{self, Seek, Write};
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::export::{Azw3Exporter, EpubExporter, Exporter};
use crate::import::{Azw3Importer, ChapterId, EpubImporter, Importer, MobiImporter, SpineEntry};
use crate::ir::IRChapter;

// ============================================================================
// Data Types
// ============================================================================

/// Ebook file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// EPUB format (EPUB 2 or 3)
    Epub,
    /// AZW3/KF8 format (modern Kindle)
    Azw3,
    /// MOBI format (legacy Kindle)
    Mobi,
}

/// A resource (image, font, CSS, etc.) with its data and media type.
#[derive(Debug, Clone)]
pub struct Resource {
    pub data: Vec<u8>,
    pub media_type: String,
}

/// Book metadata (Dublin Core + extensions)
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub title: String,
    pub authors: Vec<String>,
    pub language: String,
    pub identifier: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub date: Option<String>,
    pub rights: Option<String>,
    pub cover_image: Option<String>,
}

/// A table of contents entry (hierarchical)
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TocEntry {
    pub title: String,
    pub href: String,
    pub children: Vec<TocEntry>,
    /// Play order for sorting (from NCX playOrder attribute)
    pub play_order: Option<usize>,
}

impl Ord for TocEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.play_order.cmp(&other.play_order)
    }
}

impl PartialOrd for TocEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ============================================================================
// Book Runtime Handle
// ============================================================================

/// Runtime handle for an ebook.
///
/// `Book` wraps a format-specific `Importer` backend and provides
/// unified access to metadata, table of contents, and content.
///
/// # Example
///
/// ```no_run
/// use boko::Book;
///
/// let mut book = Book::open("input.epub")?;
/// println!("Title: {}", book.metadata().title);
///
/// // Load chapter content (collect spine first to avoid borrow issues)
/// let spine: Vec<_> = book.spine().to_vec();
/// for entry in spine {
///     let raw = book.load_raw(entry.id)?;
///     println!("Chapter {}: {} bytes", entry.id.0, raw.len());
/// }
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct Book {
    backend: Box<dyn Importer>,
    /// Cache of parsed IR chapters to avoid re-parsing during normalized export.
    /// Uses RwLock for thread-safe access and Arc for cheap cloning.
    ir_cache: Arc<RwLock<BTreeMap<ChapterId, Arc<IRChapter>>>>,
}

impl Format {
    /// Detect format from file extension.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        path.as_ref()
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| match ext.to_lowercase().as_str() {
                "epub" => Some(Format::Epub),
                "azw3" => Some(Format::Azw3),
                "mobi" => Some(Format::Mobi),
                _ => None,
            })
    }
}

impl Book {
    /// Open an ebook file, auto-detecting the format.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let format = Format::from_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown file format: {}", path.display()),
            )
        })?;
        Self::open_format(path, format)
    }

    /// Open an ebook file with an explicit format.
    pub fn open_format(path: impl AsRef<Path>, format: Format) -> io::Result<Self> {
        let backend: Box<dyn Importer> = match format {
            Format::Epub => Box::new(EpubImporter::open(path.as_ref())?),
            Format::Azw3 => Box::new(Azw3Importer::open(path.as_ref())?),
            Format::Mobi => Box::new(MobiImporter::open(path.as_ref())?),
        };
        Ok(Self {
            backend,
            ir_cache: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    /// Book metadata.
    pub fn metadata(&self) -> &Metadata {
        self.backend.metadata()
    }

    /// Table of contents.
    pub fn toc(&self) -> &[TocEntry] {
        self.backend.toc()
    }

    /// Reading order (spine).
    pub fn spine(&self) -> &[SpineEntry] {
        self.backend.spine()
    }

    /// Get the internal source path for a chapter.
    pub fn source_id(&self, id: ChapterId) -> Option<&str> {
        self.backend.source_id(id)
    }

    /// Load raw chapter bytes.
    pub fn load_raw(&mut self, id: ChapterId) -> io::Result<Vec<u8>> {
        self.backend.load_raw(id)
    }

    /// Load a chapter as normalized IR.
    ///
    /// This parses the chapter's HTML content and any linked or inline CSS,
    /// producing a normalized tree structure suitable for rendering.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::{Book, Role};
    ///
    /// let mut book = Book::open("input.epub")?;
    /// let spine: Vec<_> = book.spine().to_vec();
    ///
    /// for entry in spine {
    ///     let chapter = book.load_chapter(entry.id)?;
    ///     for id in chapter.iter_dfs() {
    ///         let node = chapter.node(id).unwrap();
    ///         if node.role == Role::Paragraph {
    ///             // Process paragraph...
    ///         }
    ///     }
    /// }
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn load_chapter(&mut self, id: ChapterId) -> io::Result<IRChapter> {
        self.backend.load_chapter(id)
    }

    /// Load a chapter as IR with caching.
    ///
    /// This method caches parsed IR chapters to avoid re-parsing when the same
    /// chapter is loaded multiple times (e.g., during normalized export).
    /// Returns an `Arc<IRChapter>` for cheap cloning and thread-safe sharing.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::Book;
    ///
    /// let mut book = Book::open("input.epub")?;
    /// let spine: Vec<_> = book.spine().to_vec();
    ///
    /// // First call parses the chapter
    /// let chapter1 = book.load_chapter_cached(spine[0].id)?;
    ///
    /// // Second call returns cached version (cheap Arc clone)
    /// let chapter2 = book.load_chapter_cached(spine[0].id)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn load_chapter_cached(&mut self, id: ChapterId) -> io::Result<Arc<IRChapter>> {
        // Fast path: check read lock first
        {
            let cache = self.ir_cache.read().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "IR cache lock poisoned")
            })?;
            if let Some(chapter) = cache.get(&id) {
                return Ok(Arc::clone(chapter));
            }
        }

        // Slow path: load chapter (no lock held during IO)
        let chapter = self.backend.load_chapter(id)?;
        let chapter_arc = Arc::new(chapter);

        // Write to cache
        {
            let mut cache = self.ir_cache.write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "IR cache lock poisoned")
            })?;
            cache.insert(id, Arc::clone(&chapter_arc));
        }

        Ok(chapter_arc)
    }

    /// Clear the IR cache.
    ///
    /// Call this to free memory after normalized export is complete.
    pub fn clear_cache(&mut self) {
        if let Ok(mut cache) = self.ir_cache.write() {
            cache.clear();
        }
    }

    /// Load an asset by path.
    pub fn load_asset(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        self.backend.load_asset(path)
    }

    /// List all assets.
    pub fn list_assets(&self) -> Vec<std::path::PathBuf> {
        self.backend.list_assets()
    }

    /// Export the book to a different format.
    ///
    /// # Supported Export Formats
    ///
    /// | Format | Support |
    /// |--------|---------|
    /// | EPUB   | ✓       |
    /// | AZW3   | ✓       |
    /// | MOBI   | ✗       |
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::{Book, Format};
    /// use std::fs::File;
    ///
    /// let mut book = Book::open("input.azw3")?;
    /// let mut file = File::create("output.epub")?;
    /// book.export(Format::Epub, &mut file)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn export<W: Write + Seek>(&mut self, format: Format, writer: &mut W) -> io::Result<()> {
        match format {
            Format::Epub => EpubExporter::new().export(self, writer),
            Format::Azw3 => Azw3Exporter::new().export(self, writer),
            Format::Mobi => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "MOBI export is not supported; use AZW3 instead",
            )),
        }
    }
}

// ============================================================================
// Constructors
// ============================================================================

impl TocEntry {
    pub fn new(title: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            href: href.into(),
            children: Vec::new(),
            play_order: None,
        }
    }
}
