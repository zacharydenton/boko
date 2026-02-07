//! MOBI6 format importer - handles all IO with lazy loading.
//!
//! MOBI6 files are legacy Kindle format with a single HTML stream.
//! For KF8/AZW3 files, use Azw3Importer instead.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::import::{ChapterId, Importer, SpineEntry, resolve_path_based_href};
use crate::io::{ByteSource, FileSource};
use crate::mobi::{
    Compression, Encoding, HuffCdicReader, MobiHeader, NULL_INDEX, PdbInfo, TocNode,
    build_toc_from_ncx, detect_image_type, filepos, is_metadata_record, palmdoc, parse_exth,
    parse_ncx_index, read_index, strip_trailing_data,
};
use crate::dom::Stylesheet;
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, Landmark, Metadata, TocEntry};

/// MOBI6 format importer with lazy loading.
///
/// MOBI6 files have a single HTML stream (no chapters).
/// Text content is loaded only when `load_raw()` is called.
pub struct MobiImporter {
    /// Random-access byte source.
    source: Arc<dyn ByteSource>,

    /// PDB header info.
    pdb: PdbInfo,

    /// MOBI header info.
    mobi: MobiHeader,

    /// File length.
    file_len: u64,

    /// Book metadata.
    metadata: Metadata,

    /// Table of contents (single entry for MOBI6).
    toc: Vec<TocEntry>,

    /// Landmarks (structural navigation points).
    landmarks: Vec<Landmark>,

    /// Reading order (single entry for MOBI6).
    spine: Vec<SpineEntry>,

    /// Cached decompressed content (loaded on first request).
    content_cache: Option<Vec<u8>>,

    /// Discovered asset paths.
    assets: Vec<PathBuf>,

    /// Cached parsed stylesheets.
    css_cache: HashMap<String, Stylesheet>,

    // --- Link resolution ---
    /// Maps "path#id" -> GlobalNodeId (built during index_anchors)
    element_id_map: HashMap<String, GlobalNodeId>,
}

impl Importer for MobiImporter {
    fn open(path: &Path) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let source = Arc::new(FileSource::new(file)?);
        Self::from_source(source)
    }

    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn toc_mut(&mut self) -> &mut [TocEntry] {
        &mut self.toc
    }

    fn landmarks(&self) -> &[Landmark] {
        &self.landmarks
    }

    fn spine(&self) -> &[SpineEntry] {
        &self.spine
    }

    fn source_id(&self, id: ChapterId) -> Option<&str> {
        if id.0 == 0 {
            Some("content.html")
        } else {
            None
        }
    }

    fn load_raw(&mut self, id: ChapterId) -> io::Result<Vec<u8>> {
        if id.0 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Chapter {} not found (MOBI6 has single chapter)", id.0),
            ));
        }

        // Return cached content if available
        if let Some(ref content) = self.content_cache {
            return Ok(content.clone());
        }

        // Extract and cache content
        let text = self.extract_text()?;
        let wrapped = wrap_text_as_html(&text, &self.metadata.title, &self.mobi);

        // Transform MOBI-specific attributes to standard HTML
        // This adds id anchors at filepos target positions for link resolution
        let assets = self.discover_assets();
        let content = filepos::transform_mobi_html(&wrapped, &assets);

        self.content_cache = Some(content.clone());
        Ok(content)
    }

    fn list_assets(&self) -> &[PathBuf] {
        &self.assets
    }

    fn load_asset(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        let key = path.to_string_lossy();

        // Parse index from path (images/image_XXXX.ext)
        let idx: usize = key
            .strip_prefix("images/image_")
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Invalid asset path: {}", key),
                )
            })?;

        self.load_image_record(idx)
    }

    fn load_stylesheet(&mut self, path: &Path) -> Option<Stylesheet> {
        let key = path.to_string_lossy().replace('\\', "/");
        if let Some(sheet) = self.css_cache.get(&key) {
            return Some(sheet.clone());
        }
        let css_bytes = self.load_asset(path).ok()?;
        let css_str = String::from_utf8_lossy(&css_bytes);
        let sheet = Stylesheet::parse(&css_str);
        self.css_cache.insert(key, sheet.clone());
        Some(sheet)
    }

    fn index_anchors(&mut self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        self.element_id_map.clear();

        // Build path#id → GlobalNodeId map from chapters (same format as EPUB)
        // MOBI has a single chapter at "content.html"
        for (chapter_id, chapter) in chapters {
            for node_id in chapter.iter_dfs() {
                if let Some(id) = chapter.semantics.id(node_id) {
                    let key = format!("content.html#{}", id);
                    self.element_id_map
                        .insert(key, GlobalNodeId::new(*chapter_id, node_id));
                }
            }
        }
    }

    fn resolve_href(&self, from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        let from_path = self.source_id(from_chapter)?;
        resolve_path_based_href(
            from_path,
            href,
            |p| {
                if p == "content.html" {
                    Some(ChapterId(0))
                } else {
                    None
                }
            },
            |k| self.element_id_map.get(k).copied(),
        )
    }
}

impl MobiImporter {
    /// Create an importer from a ByteSource (metadata only, content deferred).
    pub fn from_source(source: Arc<dyn ByteSource>) -> io::Result<Self> {
        let file_len = source.len();

        // Read PDB header
        let header_start = source.read_at(0, 78)?;
        if header_start.len() < 78 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too short for PDB header",
            ));
        }

        let num_records = u16::from_be_bytes([header_start[76], header_start[77]]) as usize;
        let header_size = 78 + num_records * 8;
        let header_bytes = source.read_at(0, header_size)?;
        let (pdb, _) = PdbInfo::parse(&header_bytes)?;

        if pdb.num_records < 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not enough records",
            ));
        }

        // Read record 0 (MOBI header)
        let (start, end) = pdb.record_range(0, file_len)?;
        let record0 = source.read_at(start, (end - start) as usize)?;
        let mobi = MobiHeader::parse(&record0)?;

        if mobi.encryption != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Encrypted files are not supported",
            ));
        }

        // Parse EXTH metadata
        let exth = parse_exth(&record0, &mobi);

        // Build metadata
        let mut metadata = build_metadata(&pdb, &mobi, &exth);

        // Discover assets to get cover image path with correct extension
        let assets = discover_assets_from_source(&source, &pdb, &mobi, file_len);

        // Find cover image using discovered asset path
        if let Some(ref exth) = exth
            && let Some(cover_idx) = exth.cover_offset
        {
            // cover_offset is 0-indexed relative to first image
            if let Some(cover_path) = assets.get(cover_idx as usize) {
                metadata.cover_image = Some(cover_path.to_string_lossy().to_string());
            }
        }

        // Parse NCX index for TOC (if available)
        let codec = match mobi.encoding {
            Encoding::Utf8 => "utf-8",
            _ => "cp1252",
        };

        let toc = if mobi.ncx_index != NULL_INDEX {
            let mut read_record = |idx: usize| -> io::Result<Vec<u8>> {
                let (start, end) = pdb.record_range(idx, file_len)?;
                source.read_at(start, (end - start) as usize)
            };

            match read_index(&mut read_record, mobi.ncx_index as usize, codec) {
                Ok((entries, cncx)) => {
                    let ncx = parse_ncx_index(&entries, &cncx);
                    // MOBI6 uses byte positions as filepos anchors
                    let nodes = build_toc_from_ncx(&ncx, |entry| {
                        format!("content.html#filepos{}", entry.pos)
                    });
                    nodes.into_iter().map(toc_node_to_entry).collect()
                }
                Err(_) => vec![TocEntry::new(&metadata.title, "content.html")],
            }
        } else {
            vec![TocEntry::new(&metadata.title, "content.html")]
        };

        // MOBI6 has a single "chapter" - the entire book
        let size_estimate = (mobi.text_record_count as usize) * (mobi.text_record_size as usize);
        let spine = vec![SpineEntry {
            id: ChapterId(0),
            size_estimate,
        }];

        let mut importer = Self {
            source,
            pdb,
            mobi,
            file_len,
            metadata,
            toc,
            landmarks: Vec::new(), // MOBI6 format doesn't have landmarks
            spine,
            content_cache: None,
            assets: Vec::new(),
            css_cache: HashMap::new(),
            element_id_map: HashMap::new(),
        };

        importer.assets = importer.discover_assets();

        Ok(importer)
    }

    /// Extract and decompress text content.
    fn extract_text(&self) -> io::Result<Vec<u8>> {
        let mut text = Vec::new();

        let read_record = |idx: usize| -> io::Result<Vec<u8>> {
            let (start, end) = self.pdb.record_range(idx, self.file_len)?;
            self.source.read_at(start, (end - start) as usize)
        };

        // Build decompressor if needed
        let mut huff_reader = if self.mobi.compression == Compression::Huffman
            && self.mobi.huff_record_index != NULL_INDEX
        {
            let huff_data = read_record(self.mobi.huff_record_index as usize)?;
            let mut cdics = Vec::new();
            for i in 0..self.mobi.huff_record_count {
                let cdic_idx = self.mobi.huff_record_index as usize + 1 + i as usize;
                if let Ok(cdic) = read_record(cdic_idx) {
                    cdics.push(cdic);
                }
            }
            let cdic_refs: Vec<&[u8]> = cdics.iter().map(|c| c.as_slice()).collect();
            Some(HuffCdicReader::new(&huff_data, &cdic_refs)?)
        } else {
            None
        };

        // Read and decompress text records
        for i in 1..=self.mobi.text_record_count as usize {
            let record = read_record(i)?;
            let stripped = strip_trailing_data(&record, self.mobi.extra_data_flags);

            let decompressed = match self.mobi.compression {
                Compression::None => stripped.to_vec(),
                Compression::PalmDoc => palmdoc::decompress(stripped)?,
                Compression::Huffman => {
                    if let Some(ref mut reader) = huff_reader {
                        reader.decompress(stripped)?
                    } else {
                        stripped.to_vec()
                    }
                }
                Compression::Unknown(_) => stripped.to_vec(),
            };

            text.extend_from_slice(&decompressed);
        }

        Ok(text)
    }

    /// Discover asset paths by scanning image records.
    fn discover_assets(&self) -> Vec<PathBuf> {
        let mut assets = Vec::new();

        if self.mobi.first_image_index == NULL_INDEX {
            return assets;
        }

        let first_img = self.mobi.first_image_index as usize;
        for i in first_img..self.pdb.num_records as usize {
            // Only read first 16 bytes to detect type (magic bytes)
            if let Ok((start, end)) = self.pdb.record_range(i, self.file_len) {
                let read_len = 16.min((end - start) as usize);
                let mut header = [0u8; 16];
                if self
                    .source
                    .read_at_into(start, &mut header[..read_len])
                    .is_ok()
                {
                    let header = &header[..read_len];
                    if is_metadata_record(header) {
                        continue;
                    }
                    if let Some(media_type) = detect_image_type(header) {
                        let ext = match media_type {
                            "image/jpeg" => "jpg",
                            "image/png" => "png",
                            "image/gif" => "gif",
                            _ => "bin",
                        };
                        let idx = i - first_img;
                        assets.push(PathBuf::from(format!("images/image_{idx:04}.{ext}")));
                    }
                }
            }
        }

        assets
    }

    /// Load an image record by index.
    fn load_image_record(&self, idx: usize) -> io::Result<Vec<u8>> {
        let first_img = self.mobi.first_image_index as usize;
        let record_idx = first_img + idx;
        self.read_record(record_idx)
    }

    /// Read a record by index.
    fn read_record(&self, idx: usize) -> io::Result<Vec<u8>> {
        let (start, end) = self.pdb.record_range(idx, self.file_len)?;
        self.source.read_at(start, (end - start) as usize)
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Discover asset paths by scanning image records (standalone function for early use).
fn discover_assets_from_source(
    source: &Arc<dyn ByteSource>,
    pdb: &PdbInfo,
    mobi: &MobiHeader,
    file_len: u64,
) -> Vec<PathBuf> {
    let mut assets = Vec::new();

    if mobi.first_image_index == NULL_INDEX {
        return assets;
    }

    let first_img = mobi.first_image_index as usize;
    for i in first_img..pdb.num_records as usize {
        // Only read first 16 bytes to detect type (magic bytes)
        if let Ok((start, end)) = pdb.record_range(i, file_len) {
            let read_len = 16.min((end - start) as usize);
            let mut header = [0u8; 16];
            if source
                .read_at_into(start, &mut header[..read_len])
                .is_ok()
            {
                let header = &header[..read_len];
                if is_metadata_record(header) {
                    continue;
                }
                if let Some(media_type) = detect_image_type(header) {
                    let ext = match media_type {
                        "image/jpeg" => "jpg",
                        "image/png" => "png",
                        "image/gif" => "gif",
                        _ => "bin",
                    };
                    let idx = i - first_img;
                    assets.push(PathBuf::from(format!("images/image_{idx:04}.{ext}")));
                }
            }
        }
    }

    assets
}

fn build_metadata(
    pdb: &PdbInfo,
    mobi: &MobiHeader,
    exth: &Option<crate::mobi::ExthHeader>,
) -> Metadata {
    let title = exth
        .as_ref()
        .and_then(|e| e.title.clone())
        .or_else(|| {
            if !mobi.title.is_empty() {
                Some(mobi.title.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| pdb.name.clone());

    let mut metadata = Metadata {
        title,
        ..Default::default()
    };

    if let Some(exth) = exth {
        metadata.authors = exth.authors.clone();
        metadata.publisher = exth.publisher.clone();
        metadata.description = exth.description.clone();
        metadata.subjects = exth.subjects.clone();
        metadata.date = exth.pub_date.clone();
        metadata.rights = exth.rights.clone();
        metadata.language = exth.language.clone().unwrap_or_default();
        metadata.identifier = exth
            .isbn
            .clone()
            .or_else(|| exth.asin.clone())
            .or_else(|| exth.source.clone())
            .unwrap_or_default();
    }

    metadata
}

/// Wrap raw text as HTML.
fn wrap_text_as_html(text: &[u8], title: &str, mobi: &MobiHeader) -> Vec<u8> {
    let charset = match mobi.encoding {
        Encoding::Utf8 => "utf-8",
        _ => "windows-1252",
    };

    let content = String::from_utf8_lossy(text);
    let content_str = content.trim();

    // Check if content already has HTML structure
    if content_str.starts_with("<!DOCTYPE") || content_str.starts_with("<html") {
        return text.to_vec();
    }

    // Wrap as HTML
    let html = format!(
        r#"<?xml version="1.0" encoding="{charset}"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
<title>{title}</title>
<meta charset="{charset}"/>
</head>
<body>
{content}
</body>
</html>"#,
        charset = charset,
        title = html_escape(title),
        content = content,
    );

    html.into_bytes()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Convert TocNode to TocEntry recursively.
fn toc_node_to_entry(node: TocNode) -> TocEntry {
    let mut entry = TocEntry::new(&node.title, &node.href);
    entry.children = node.children.into_iter().map(toc_node_to_entry).collect();
    entry
}

// Tests for MOBI HTML transformation are in mobi::filepos module
