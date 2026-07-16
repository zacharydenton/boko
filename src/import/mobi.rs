//! MOBI6 format importer with chapter splitting.
//!
//! MOBI6 files are legacy Kindle format with a single HTML stream.
//! This importer splits the HTML at `<mbp:pagebreak>` boundaries to produce
//! multiple chapters, falling back to a single chapter if no pagebreaks exist.
//!
//! For KF8/AZW3 files, use Azw3Importer instead.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::dom::Stylesheet;
use crate::import::{ChapterId, Importer, SpineEntry, resolve_path_based_href};
use crate::io::{ByteSource, FileSource};
use crate::mobi::split::{split_mobi_html, split_mobi_html_ncx_only};
use crate::mobi::{
    Compression, Encoding, HuffCdicReader, MobiHeader, NULL_INDEX, PdbInfo, TocNode,
    build_toc_from_ncx, decode_font_record, detect_font_type, detect_image_type, filepos,
    is_metadata_record, palmdoc, parse_exth, parse_ncx_index, read_index, strip_trailing_data,
};
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, Landmark, Metadata, TocEntry};

/// MOBI6 format importer with chapter splitting.
///
/// Splits MOBI HTML at `<mbp:pagebreak>` boundaries. Falls back to a single
/// chapter if no pagebreaks are found.
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

    /// Table of contents.
    toc: Vec<TocEntry>,

    /// Landmarks (structural navigation points).
    landmarks: Vec<Landmark>,

    /// Reading order.
    spine: Vec<SpineEntry>,

    /// Split chapter content (complete XHTML documents).
    chapter_cache: Vec<Vec<u8>>,

    /// Chapter file paths ("chapter_0.xhtml", "chapter_1.xhtml", ...).
    chapter_paths: Vec<String>,

    /// Discovered asset paths.
    assets: Vec<String>,

    /// Cached parsed stylesheets.
    css_cache: RwLock<HashMap<String, Arc<Stylesheet>>>,

    // --- Link resolution ---
    /// Maps "path#id" -> GlobalNodeId (built during index_anchors)
    element_id_map: RwLock<HashMap<String, GlobalNodeId>>,
}

impl Importer for MobiImporter {
    fn open(path: &Path) -> crate::Result<Self> {
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

    fn landmarks(&self) -> &[Landmark] {
        &self.landmarks
    }

    fn spine(&self) -> &[SpineEntry] {
        &self.spine
    }

    fn source_id(&self, id: ChapterId) -> Option<&str> {
        self.chapter_paths.get(id.0 as usize).map(|s| s.as_str())
    }

    fn load_raw(&self, id: ChapterId) -> crate::Result<Vec<u8>> {
        self.chapter_cache
            .get(id.0 as usize)
            .cloned()
            .ok_or_else(|| crate::Error::NotFound {
                what: format!("Chapter {}", id.0),
            })
    }

    fn list_assets(&self) -> &[String] {
        &self.assets
    }

    fn load_asset(&self, path: &str) -> crate::Result<Vec<u8>> {
        // Parse index from path (images/image_XXXX.ext or fonts/font_XXXX.ext).
        // Images and fonts share the same record-index space, so the prefix
        // selects naming but the underlying lookup is the same.
        let idx: usize = path
            .strip_prefix("images/image_")
            .or_else(|| path.strip_prefix("fonts/font_"))
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| crate::Error::NotFound {
                what: format!("asset {}", path),
            })?;

        Ok(self.load_image_record(idx)?)
    }

    fn load_stylesheet(&self, path: &str) -> Option<Arc<Stylesheet>> {
        if let Ok(cache) = self.css_cache.read()
            && let Some(sheet) = cache.get(path)
        {
            return Some(Arc::clone(sheet));
        }
        let css_bytes = self.load_asset(path).ok()?;
        let css_str = String::from_utf8_lossy(&css_bytes);
        let sheet = Arc::new(Stylesheet::parse(&css_str));
        match self.css_cache.write() {
            Ok(mut cache) => Some(Arc::clone(cache.entry(path.to_string()).or_insert(sheet))),
            Err(_) => Some(sheet),
        }
    }

    fn index_anchors(&self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        let mut element_id_map = HashMap::new();

        for (chapter_id, chapter) in chapters {
            let chapter_path = match self.chapter_paths.get(chapter_id.0 as usize) {
                Some(p) => p.as_str(),
                None => continue,
            };

            for node_id in chapter.iter_dfs() {
                if let Some(id) = chapter.semantics.id(node_id) {
                    let key = format!("{}#{}", chapter_path, id);
                    element_id_map.insert(key, GlobalNodeId::new(*chapter_id, node_id));
                }
            }
        }

        if let Ok(mut map) = self.element_id_map.write() {
            *map = element_id_map;
        }
    }

    fn resolve_href(&self, from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        let from_path = self.source_id(from_chapter)?;
        resolve_path_based_href(
            from_path,
            href,
            |p| {
                self.chapter_paths
                    .iter()
                    .position(|cp| cp == p)
                    .map(|i| ChapterId(i as u32))
            },
            |k| {
                self.element_id_map
                    .read()
                    .ok()
                    .and_then(|m| m.get(k).copied())
            },
        )
    }
}

impl MobiImporter {
    /// Create an importer from a ByteSource.
    ///
    /// Text is extracted eagerly to determine chapter boundaries for the spine.
    pub fn from_source(source: Arc<dyn ByteSource>) -> crate::Result<Self> {
        let file_len = source.len();

        // Read PDB header
        let header_start = source.read_at(0, 78)?;
        if header_start.len() < 78 {
            return Err(crate::Error::Malformed {
                format: crate::Format::Mobi,
                context: "file too short for PDB header".into(),
            });
        }

        let num_records = u16::from_be_bytes([header_start[76], header_start[77]]) as usize;
        let header_size = 78 + num_records * 8;
        let header_bytes = source.read_at(0, header_size)?;
        let (pdb, _) = PdbInfo::parse(&header_bytes)?;

        if pdb.num_records < 2 {
            return Err(crate::Error::Malformed {
                format: crate::Format::Mobi,
                context: "not enough PDB records".into(),
            });
        }

        // Read record 0 (MOBI header)
        let (start, end) = pdb.record_range(0, file_len)?;
        let record0_len = usize::try_from(end - start).map_err(|_| crate::Error::Malformed {
            format: crate::Format::Mobi,
            context: "record 0 too large".into(),
        })?;
        let record0 = source.read_at(start, record0_len)?;
        let mobi = MobiHeader::parse(&record0)?;

        if mobi.encryption != 0 {
            return Err(crate::Error::DrmProtected(crate::Format::Mobi));
        }

        // Parse EXTH metadata
        let exth = parse_exth(&record0, &mobi);

        // Build metadata
        let mut metadata = build_metadata(&pdb, &mobi, &exth);

        // Discover assets to get cover image path with correct extension
        let assets = discover_assets_from_source(&source, &pdb, &mobi, file_len);

        // Find cover image using discovered asset path. EXTH cover_offset is
        // a record index relative to first_image_index, and the asset names
        // embed exactly that index — so match by name. Indexing the compacted
        // asset list positionally would desync whenever a metadata or
        // unrecognized record precedes the cover.
        if let Some(ref exth) = exth
            && let Some(cover_idx) = exth.cover_offset
        {
            let needle = format!("images/image_{cover_idx:04}.");
            if let Some(cover_path) = assets.iter().find(|p| p.starts_with(&needle)) {
                metadata.cover_image = Some(cover_path.clone());
            }
        }

        // Parse NCX index BEFORE text transformation (needed for anchor insertion
        // and fallback split points)
        let codec = match mobi.encoding {
            Encoding::Utf8 => "utf-8",
            _ => "cp1252",
        };

        let ncx_entries = if mobi.ncx_index != NULL_INDEX {
            let mut read_record = |idx: usize| -> io::Result<Vec<u8>> {
                let (start, end) = pdb.record_range(idx, file_len)?;
                let len = usize::try_from(end - start)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "record too large"))?;
                source.read_at(start, len)
            };

            match read_index(&mut read_record, mobi.ncx_index as usize, codec) {
                Ok((entries, cncx)) => Some(parse_ncx_index(&entries, &cncx)),
                Err(_) => None,
            }
        } else {
            None
        };

        // Extract NCX positions for anchor insertion and fallback splitting
        let ncx_positions: Vec<u32> = ncx_entries
            .as_ref()
            .map(|entries| entries.iter().map(|e| e.pos).collect())
            .unwrap_or_default();

        // Extract and transform text eagerly (needed to determine chapter count)
        let text = extract_text_from_source(&source, &pdb, &mobi, file_len)?;
        let wrapped = wrap_text_as_html(&text, &metadata.title, &mobi);

        // Transform HTML (without NCX anchors initially for clean pagebreak splitting)
        let transformed = filepos::transform_mobi_html(&wrapped, &assets, &[]);

        // Try pagebreak-based splitting first. If it produces only 1 chapter
        // and NCX positions are available, re-transform with NCX anchors and
        // force NCX-based splitting (bypassing the pagebreak check that failed).
        let (split, transformed) = {
            let initial = split_mobi_html(&transformed, None);
            if initial.chapters.len() > 1 || ncx_positions.is_empty() {
                (initial, transformed)
            } else {
                // Re-transform with NCX anchors at byte positions, force NCX split
                let with_ncx = filepos::transform_mobi_html(&wrapped, &assets, &ncx_positions);
                let ncx_split = split_mobi_html_ncx_only(&with_ncx, &ncx_positions);
                if ncx_split.chapters.len() > 1 {
                    (ncx_split, with_ncx)
                } else {
                    (initial, transformed)
                }
            }
        };
        // Keep transformed for potential later use
        let _ = transformed;

        // Build spine from split chapters
        let spine: Vec<SpineEntry> = (0..split.chapters.len())
            .map(|i| SpineEntry {
                id: ChapterId(i as u32),
                size_estimate: split.chapters[i].len(),
            })
            .collect();

        // Build TOC from NCX entries (using split result for chapter mapping)
        let toc = if let Some(ref ncx) = ncx_entries {
            let nodes = build_toc_from_ncx(ncx, |_, entry| {
                let filepos_key = format!("filepos{}", entry.pos);
                let chapter_idx = split
                    .filepos_to_chapter
                    .get(&filepos_key)
                    .copied()
                    .unwrap_or(0);
                let chapter_path = &split.chapter_paths[chapter_idx];
                format!("{}#filepos{}", chapter_path, entry.pos)
            });
            nodes.into_iter().map(toc_node_to_entry).collect()
        } else {
            vec![TocEntry::new(&metadata.title, &split.chapter_paths[0])]
        };

        // `assets` was already discovered above for the cover/filepos
        // transformation; reuse it instead of re-scanning every record.
        let importer = Self {
            source,
            pdb,
            mobi,
            file_len,
            metadata,
            toc,
            landmarks: Vec::new(),
            spine,
            chapter_cache: split.chapters,
            chapter_paths: split.chapter_paths,
            assets,
            css_cache: RwLock::new(HashMap::new()),
            element_id_map: RwLock::new(HashMap::new()),
        };

        Ok(importer)
    }

    /// Load an image or font record by index.
    ///
    /// If the record is a Kindle `FONT` container, it is decoded (XOR + zlib)
    /// to extract the raw font bytes. Otherwise the record is returned as-is.
    fn load_image_record(&self, idx: usize) -> io::Result<Vec<u8>> {
        let first_img = self.mobi.first_image_index as usize;
        let record_idx = first_img + idx;
        let data = self.read_record(record_idx)?;

        if data.starts_with(b"FONT") {
            return decode_font_record(&data);
        }

        Ok(data)
    }

    /// Read a record by index.
    fn read_record(&self, idx: usize) -> io::Result<Vec<u8>> {
        let (start, end) = self.pdb.record_range(idx, self.file_len)?;
        let len = usize::try_from(end - start)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "record too large"))?;
        self.source.read_at(start, len)
    }
}

// ============================================================================
// Text extraction (standalone, for use during from_source)
// ============================================================================

/// Extract and decompress text content from a MOBI source.
fn extract_text_from_source(
    source: &Arc<dyn ByteSource>,
    pdb: &PdbInfo,
    mobi: &MobiHeader,
    file_len: u64,
) -> io::Result<Vec<u8>> {
    let mut text = Vec::new();

    let read_record = |idx: usize| -> io::Result<Vec<u8>> {
        let (start, end) = pdb.record_range(idx, file_len)?;
        let len = usize::try_from(end - start)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "record too large"))?;
        source.read_at(start, len)
    };

    // Build decompressor if needed
    let mut huff_reader =
        if mobi.compression == Compression::Huffman && mobi.huff_record_index != NULL_INDEX {
            let huff_data = read_record(mobi.huff_record_index as usize)?;
            let mut cdics = Vec::new();
            for i in 0..mobi.huff_record_count.saturating_sub(1) {
                let cdic_idx = mobi.huff_record_index as usize + 1 + i as usize;
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
    for i in 1..=mobi.text_record_count as usize {
        let record = read_record(i)?;
        let stripped = strip_trailing_data(&record, mobi.extra_data_flags);

        let decompressed = match mobi.compression {
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

// ============================================================================
// Chapter splitting
// ============================================================================

/// Discover asset paths by scanning image and font records (standalone function for early use).
fn discover_assets_from_source(
    source: &Arc<dyn ByteSource>,
    pdb: &PdbInfo,
    mobi: &MobiHeader,
    file_len: u64,
) -> Vec<String> {
    let mut assets = Vec::new();

    if mobi.first_image_index == NULL_INDEX {
        return assets;
    }

    let first_img = mobi.first_image_index as usize;
    for i in first_img..pdb.num_records as usize {
        if let Ok((start, end)) = pdb.record_range(i, file_len) {
            // min against a small constant before the cast so a >4 GiB
            // record length can't truncate on 32-bit targets.
            let read_len = (end - start).min(16) as usize;
            let mut header = [0u8; 16];
            if source.read_at_into(start, &mut header[..read_len]).is_ok() {
                let header = &header[..read_len];
                if is_metadata_record(header) {
                    continue;
                }
                let idx = i - first_img;
                if let Some(media_type) = detect_image_type(header) {
                    let ext = match media_type {
                        "image/jpeg" => "jpg",
                        "image/png" => "png",
                        "image/gif" => "gif",
                        _ => "bin",
                    };
                    assets.push(format!("images/image_{idx:04}.{ext}"));
                } else if let Some(font_ext) = detect_font_type(header) {
                    assets.push(format!("fonts/font_{idx:04}.{font_ext}"));
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
    // Decode using the header-declared encoding *before* wrapping. Previously
    // the bytes were run through `from_utf8_lossy` — which replaces every
    // cp1252 byte >= 0x80 (curly quotes, accents, …) with U+FFFD — while still
    // declaring `windows-1252`, destroying all non-ASCII text.
    let hint = match mobi.encoding {
        Encoding::Utf8 => "utf-8",
        _ => "windows-1252",
    };
    let content = crate::util::decode_text(text, Some(hint));
    let content_str = content.trim();

    // Already a full HTML document: keep the original bytes (it carries its own
    // charset declaration).
    if content_str.starts_with("<!DOCTYPE") || content_str.starts_with("<html") {
        return text.to_vec();
    }

    // Wrap as HTML. The body is now decoded UTF-8, so declare utf-8.
    let html = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
<title>{title}</title>
<meta charset="utf-8"/>
</head>
<body>
{content}
</body>
</html>"#,
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
    entry.play_order = Some(node.ncx_index);
    entry.children = node.children.into_iter().map(toc_node_to_entry).collect();
    entry
}

// ============================================================================
// Tests
// ============================================================================
