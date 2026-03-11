//! MOBI6 format importer with chapter splitting.
//!
//! MOBI6 files are legacy Kindle format with a single HTML stream.
//! This importer splits the HTML at `<mbp:pagebreak>` boundaries to produce
//! multiple chapters, falling back to a single chapter if no pagebreaks exist.
//!
//! For KF8/AZW3 files, use Azw3Importer instead.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::dom::Stylesheet;
use crate::import::{ChapterId, Importer, SpineEntry, resolve_path_based_href};
use crate::io::{ByteSource, FileSource};
use crate::mobi::{
    Compression, Encoding, HuffCdicReader, MobiHeader, NULL_INDEX, PdbInfo, TocNode,
    build_toc_from_ncx, detect_image_type, filepos, is_metadata_record, palmdoc, parse_exth,
    parse_ncx_index, read_index, strip_trailing_data,
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
        self.chapter_paths.get(id.0 as usize).map(|s| s.as_str())
    }

    fn load_raw(&mut self, id: ChapterId) -> io::Result<Vec<u8>> {
        self.chapter_cache
            .get(id.0 as usize)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Chapter {} not found", id.0),
                )
            })
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

        for (chapter_id, chapter) in chapters {
            let chapter_path = match self.chapter_paths.get(chapter_id.0 as usize) {
                Some(p) => p.as_str(),
                None => continue,
            };

            for node_id in chapter.iter_dfs() {
                if let Some(id) = chapter.semantics.id(node_id) {
                    let key = format!("{}#{}", chapter_path, id);
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
                self.chapter_paths
                    .iter()
                    .position(|cp| cp == p)
                    .map(|i| ChapterId(i as u32))
            },
            |k| self.element_id_map.get(k).copied(),
        )
    }
}

impl MobiImporter {
    /// Create an importer from a ByteSource.
    ///
    /// Text is extracted eagerly to determine chapter boundaries for the spine.
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

        // Parse NCX index BEFORE text transformation (needed for anchor insertion
        // and fallback split points)
        let codec = match mobi.encoding {
            Encoding::Utf8 => "utf-8",
            _ => "cp1252",
        };

        let ncx_entries = if mobi.ncx_index != NULL_INDEX {
            let mut read_record = |idx: usize| -> io::Result<Vec<u8>> {
                let (start, end) = pdb.record_range(idx, file_len)?;
                source.read_at(start, (end - start) as usize)
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
            let nodes = build_toc_from_ncx(ncx, |entry| {
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

        let mut importer = Self {
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
            assets: Vec::new(),
            css_cache: HashMap::new(),
            element_id_map: HashMap::new(),
        };

        importer.assets = importer.discover_assets();

        Ok(importer)
    }

    /// Discover asset paths by scanning image records.
    fn discover_assets(&self) -> Vec<PathBuf> {
        let mut assets = Vec::new();

        if self.mobi.first_image_index == NULL_INDEX {
            return assets;
        }

        let first_img = self.mobi.first_image_index as usize;
        for i in first_img..self.pdb.num_records as usize {
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
        source.read_at(start, (end - start) as usize)
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

/// Result of splitting MOBI HTML into chapters.
struct ChapterSplit {
    /// Split chapter content (complete XHTML documents).
    chapters: Vec<Vec<u8>>,
    /// Chapter file paths.
    chapter_paths: Vec<String>,
    /// Maps "fileposN" → chapter index.
    filepos_to_chapter: HashMap<String, usize>,
}

/// Split transformed MOBI HTML into chapters at `<mbp:pagebreak>` boundaries.
///
/// Falls back to NCX position-based splitting if no pagebreaks are found.
/// Falls back to a single chapter if neither pagebreaks nor NCX positions exist.
fn split_mobi_html(html: &[u8], ncx_positions: Option<&[u32]>) -> ChapterSplit {
    let html_str = String::from_utf8_lossy(html);

    // Extract <head> content and <body> content
    let (head_content, body_content) = extract_head_and_body(&html_str);

    // Find pagebreak positions in the body content
    let pagebreak_positions = find_pagebreaks(body_content.as_bytes());

    // Split body: pagebreaks first, NCX fallback, then single chapter
    let body_chunks = if !pagebreak_positions.is_empty() {
        split_at_pagebreaks(&body_content, &pagebreak_positions)
    } else if let Some(positions) = ncx_positions {
        let ncx_chunks = split_at_ncx_anchors(&body_content, positions);
        if ncx_chunks.len() > 1 {
            ncx_chunks
        } else {
            vec![body_content.to_string()]
        }
    } else {
        vec![body_content.to_string()]
    };

    // Filter out empty chunks
    let body_chunks: Vec<String> = body_chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect();

    // Build chapter documents and filepos map
    let mut chapters = Vec::with_capacity(body_chunks.len());
    let mut chapter_paths = Vec::with_capacity(body_chunks.len());
    let mut filepos_to_chapter: HashMap<String, usize> = HashMap::new();

    for (i, chunk) in body_chunks.iter().enumerate() {
        let chapter_path = format!("chapter_{}.xhtml", i);
        chapter_paths.push(chapter_path);

        // Scan this chunk for filepos anchors and record their chapter
        collect_filepos_anchors(chunk, i, &mut filepos_to_chapter);

        // Wrap chunk as complete XHTML
        let doc = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <!DOCTYPE html>\n\
             <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
             <head>\n{}</head>\n\
             <body>\n{}\n</body>\n\
             </html>",
            head_content, chunk
        );
        chapters.push(doc.into_bytes());
    }

    // Rewrite cross-chapter links
    rewrite_cross_chapter_links(&mut chapters, &filepos_to_chapter, &chapter_paths);

    // Neutralize bare filename links (OEB source references that don't exist in EPUB)
    neutralize_bare_filename_links(&mut chapters);

    // Ensure at least one chapter
    if chapters.is_empty() {
        chapters.push(html.to_vec());
        chapter_paths.push("chapter_0.xhtml".to_string());
    }

    ChapterSplit {
        chapters,
        chapter_paths,
        filepos_to_chapter,
    }
}

/// Split MOBI HTML using only NCX positions, bypassing pagebreak detection.
///
/// Used when pagebreak-based splitting fails to produce multiple chapters
/// but NCX index entries provide valid split points.
fn split_mobi_html_ncx_only(html: &[u8], ncx_positions: &[u32]) -> ChapterSplit {
    let html_str = String::from_utf8_lossy(html);
    let (head_content, body_content) = extract_head_and_body(&html_str);

    let body_chunks = {
        let ncx_chunks = split_at_ncx_anchors(&body_content, ncx_positions);
        if ncx_chunks.len() > 1 {
            ncx_chunks
        } else {
            vec![body_content.to_string()]
        }
    };

    // Filter out empty chunks
    let body_chunks: Vec<String> = body_chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect();

    // Build chapter documents and filepos map
    let mut chapters = Vec::with_capacity(body_chunks.len());
    let mut chapter_paths = Vec::with_capacity(body_chunks.len());
    let mut filepos_to_chapter: HashMap<String, usize> = HashMap::new();

    for (i, chunk) in body_chunks.iter().enumerate() {
        let chapter_path = format!("chapter_{}.xhtml", i);
        chapter_paths.push(chapter_path);
        collect_filepos_anchors(chunk, i, &mut filepos_to_chapter);

        let doc = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <!DOCTYPE html>\n\
             <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
             <head>\n{}</head>\n\
             <body>\n{}\n</body>\n\
             </html>",
            head_content, chunk
        );
        chapters.push(doc.into_bytes());
    }

    rewrite_cross_chapter_links(&mut chapters, &filepos_to_chapter, &chapter_paths);
    neutralize_bare_filename_links(&mut chapters);

    if chapters.is_empty() {
        chapters.push(html.to_vec());
        chapter_paths.push("chapter_0.xhtml".to_string());
    }

    ChapterSplit {
        chapters,
        chapter_paths,
        filepos_to_chapter,
    }
}

/// Extract the content inside `<head>...</head>` and `<body>...</body>`.
///
/// Returns (head_inner, body_inner). If tags aren't found, returns reasonable
/// defaults.
fn extract_head_and_body(html: &str) -> (String, String) {
    let html_lower = html.to_ascii_lowercase();

    // Find <head> content
    let head_content = if let Some(head_start) = html_lower.find("<head") {
        let after_tag = html[head_start..].find('>').map(|p| head_start + p + 1);
        let head_end = html_lower.find("</head>");
        match (after_tag, head_end) {
            (Some(start), Some(end)) if start <= end => html[start..end].to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };

    // Find <body> content
    let body_content = if let Some(body_start) = html_lower.find("<body") {
        let after_tag = html[body_start..].find('>').map(|p| body_start + p + 1);
        let body_end = html_lower.rfind("</body>");
        match (after_tag, body_end) {
            (Some(start), Some(end)) if start <= end => html[start..end].to_string(),
            (Some(start), None) => html[start..].to_string(),
            _ => html.to_string(),
        }
    } else {
        html.to_string()
    };

    (head_content, body_content)
}

/// A pagebreak location: byte range of the `<mbp:pagebreak...>` tag in the body.
struct PagebreakPos {
    /// Start byte offset of the `<` character.
    start: usize,
    /// End byte offset (one past the `>` character).
    end: usize,
}

/// Find all `<mbp:pagebreak...>` tags in body content.
///
/// Matches variants: `<mbp:pagebreak/>`, `<mbp:pagebreak />`,
/// `<mbp:pagebreak>`, with optional attributes, case-insensitive.
fn find_pagebreaks(body: &[u8]) -> Vec<PagebreakPos> {
    let mut results = Vec::new();
    let body_lower: Vec<u8> = body.iter().map(|b| b.to_ascii_lowercase()).collect();
    let needle = b"<mbp:pagebreak";

    let mut pos = 0;
    while pos + needle.len() < body_lower.len() {
        if let Some(rel) = body_lower[pos..]
            .windows(needle.len())
            .position(|w| w == needle)
        {
            let tag_start = pos + rel;
            // Find the closing > for this tag
            if let Some(close_rel) = body[tag_start..].iter().position(|&b| b == b'>') {
                let tag_end = tag_start + close_rel + 1;
                results.push(PagebreakPos {
                    start: tag_start,
                    end: tag_end,
                });
                pos = tag_end;
            } else {
                pos = tag_start + needle.len();
            }
        } else {
            break;
        }
    }

    results
}

/// Split body content at pagebreak positions.
///
/// The pagebreak tags themselves are removed. Content before the first
/// pagebreak becomes the first chunk, etc.
fn split_at_pagebreaks(body: &str, pagebreaks: &[PagebreakPos]) -> Vec<String> {
    let mut chunks = Vec::with_capacity(pagebreaks.len() + 1);
    let mut last_end = 0;

    for pb in pagebreaks {
        chunks.push(body[last_end..pb.start].to_string());
        last_end = pb.end;
    }

    // Content after the last pagebreak
    chunks.push(body[last_end..].to_string());

    chunks
}

/// Scan a chapter chunk for `<a id="fileposN"` anchors and record them.
fn collect_filepos_anchors(chunk: &str, chapter_idx: usize, map: &mut HashMap<String, usize>) {
    let needle = "id=\"filepos";
    let mut search_pos = 0;

    while let Some(rel) = chunk[search_pos..].find(needle) {
        let value_start = search_pos + rel + needle.len();
        // Read digits until closing quote
        let value_end = chunk[value_start..]
            .find('"')
            .map(|p| value_start + p)
            .unwrap_or(value_start);

        if value_end > value_start {
            let filepos_key = format!("filepos{}", &chunk[value_start..value_end]);
            map.insert(filepos_key, chapter_idx);
        }

        search_pos = value_end + 1;
        if search_pos >= chunk.len() {
            break;
        }
    }
}

/// Rewrite `href="#fileposN"` links that point to anchors in other chapters.
///
/// If the target filepos is in a different chapter, rewrites to
/// `href="chapter_M.xhtml#fileposN"`.
fn rewrite_cross_chapter_links(
    chapters: &mut [Vec<u8>],
    filepos_to_chapter: &HashMap<String, usize>,
    chapter_paths: &[String],
) {
    let needle = b"href=\"#filepos";

    for (chapter_idx, chapter) in chapters.iter_mut().enumerate() {
        let mut output = Vec::with_capacity(chapter.len());
        let mut pos = 0;

        while pos < chapter.len() {
            if pos + needle.len() < chapter.len() && chapter[pos..].starts_with(needle) {
                // Found href="#filepos...", extract the filepos key
                let value_start = pos + b"href=\"#".len();
                let quote_end = chapter[value_start..]
                    .iter()
                    .position(|&b| b == b'"')
                    .map(|p| value_start + p);

                if let Some(end) = quote_end {
                    let filepos_key =
                        String::from_utf8_lossy(&chapter[value_start..end]).to_string();
                    let target_chapter = filepos_to_chapter
                        .get(&filepos_key)
                        .copied()
                        .unwrap_or(chapter_idx);

                    if target_chapter != chapter_idx {
                        // Cross-chapter link: rewrite
                        output.extend_from_slice(b"href=\"");
                        output.extend_from_slice(chapter_paths[target_chapter].as_bytes());
                        output.push(b'#');
                        output.extend_from_slice(filepos_key.as_bytes());
                        output.push(b'"');
                    } else {
                        // Same chapter: keep as-is
                        output.extend_from_slice(&chapter[pos..end + 1]);
                    }
                    pos = end + 1;
                    continue;
                }
            }

            output.push(chapter[pos]);
            pos += 1;
        }

        *chapter = output;
    }
}

/// Split body content at NCX anchor positions.
///
/// Finds `id="fileposN"` attributes in the body for each NCX position,
/// locates the enclosing `<a` tag, and splits just before it.
/// Content before the first anchor becomes the first chunk (preamble/front matter).
///
/// Handles both inserted anchors (`<a id="fileposN" />`) and pre-existing
/// anchors where `id` isn't the first attribute (`<a class="c1" id="fileposN">`).
fn split_at_ncx_anchors(body: &str, positions: &[u32]) -> Vec<String> {
    if positions.is_empty() {
        return vec![body.to_string()];
    }

    let body_bytes = body.as_bytes();

    // Find byte offsets of each NCX anchor in the body
    let mut split_offsets = Vec::new();
    for &pos in positions {
        let needle = format!("id=\"filepos{}\"", pos);
        if let Some(id_offset) = body.find(&needle) {
            // Scan backward to find the opening '<' of the enclosing tag
            let tag_start = body_bytes[..id_offset]
                .iter()
                .rposition(|&b| b == b'<')
                .unwrap_or(id_offset);
            if tag_start > 0 {
                split_offsets.push(tag_start);
            }
        }
    }

    split_offsets.sort_unstable();
    split_offsets.dedup();

    if split_offsets.is_empty() {
        return vec![body.to_string()];
    }

    let mut chunks = Vec::with_capacity(split_offsets.len() + 1);
    let mut last_end = 0;

    for &offset in &split_offsets {
        if offset > last_end {
            chunks.push(body[last_end..offset].to_string());
        }
        last_end = offset;
    }

    // Content after the last split point
    if last_end < body.len() {
        chunks.push(body[last_end..].to_string());
    }

    chunks
}

/// Neutralize bare filename links that reference OEB source files.
///
/// Some older MOBI files retain original OEB package filenames as `href` values
/// (e.g. `HREF="cover.htm"`, `HREF="Book_oeb_01_r1.html"`). These use uppercase
/// `HREF` and coexist with a lowercase `href="#fileposN"` on the same tag.
/// Since HTML parsers take the first attribute, the uppercase OEB link wins.
///
/// This function removes the entire `HREF="filename.html"` attribute (case-
/// insensitive) when it points to a bare filename, letting the correct lowercase
/// `href="#fileposN"` take effect. Falls back to replacing with `href="#"` if
/// there's only one href attribute.
fn neutralize_bare_filename_links(chapters: &mut [Vec<u8>]) {
    for chapter in chapters.iter_mut() {
        let mut output = Vec::with_capacity(chapter.len());
        let mut pos = 0;

        while pos < chapter.len() {
            // Case-insensitive match for href=" (handles HREF=", Href=", etc.)
            if pos + 6 <= chapter.len()
                && chapter[pos..pos + 5].eq_ignore_ascii_case(b"href=")
                && chapter[pos + 5] == b'"'
            {
                let value_start = pos + 6;
                if let Some(quote_rel) = chapter[value_start..].iter().position(|&b| b == b'"') {
                    let value = &chapter[value_start..value_start + quote_rel];
                    if is_bare_filename_link(value) {
                        let attr_end = value_start + quote_rel + 1; // past closing "

                        // Check if there's already a lowercase href on this tag
                        // by looking ahead in the same tag for href="# or href="chapter_
                        let remaining_tag = &chapter[attr_end..];
                        let has_correct_href = remaining_tag
                            .windows(6)
                            .take_while(|w| !w.starts_with(b">") && !w.starts_with(b"<"))
                            .any(|w| w == b"href=\"");

                        if has_correct_href {
                            // Remove the OEB HREF attribute entirely (skip it)
                            // Also skip trailing whitespace
                            pos = attr_end;
                            while pos < chapter.len() && chapter[pos] == b' ' {
                                pos += 1;
                            }
                            continue;
                        } else {
                            // No correct href follows — neutralize to href="#"
                            output.extend_from_slice(b"href=\"#\"");
                            pos = attr_end;
                            continue;
                        }
                    }
                }
            }

            output.push(chapter[pos]);
            pos += 1;
        }

        *chapter = output;
    }
}

/// Check if an href value is a bare filename link to an .htm/.html file.
///
/// Returns true for values like `cover.htm`, `Book_oeb_01_r1.html`,
/// `Book_oeb_ftn_r1.html#f1` (with fragment).
/// Returns false for `#filepos123`, `http://...`, `chapter_0.xhtml`, etc.
fn is_bare_filename_link(href: &[u8]) -> bool {
    let href_str = String::from_utf8_lossy(href);
    // Strip fragment for extension check
    let path_part = href_str.split('#').next().unwrap_or(&href_str);
    let path_lower = path_part.to_ascii_lowercase();

    (path_lower.ends_with(".htm") || path_lower.ends_with(".html"))
        && !href_str.starts_with('#')
        && !href_str.contains("://")
        && !path_lower.ends_with(".xhtml")
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
        if let Ok((start, end)) = pdb.record_range(i, file_len) {
            let read_len = 16.min((end - start) as usize);
            let mut header = [0u8; 16];
            if source.read_at_into(start, &mut header[..read_len]).is_ok() {
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_head_and_body() {
        let html = r#"<html><head><title>Test</title><link rel="stylesheet" href="style.css"/></head><body><p>Hello</p></body></html>"#;
        let (head, body) = extract_head_and_body(html);
        assert!(head.contains("<title>Test</title>"));
        assert!(head.contains("style.css"));
        assert_eq!(body, "<p>Hello</p>");
    }

    #[test]
    fn test_extract_head_and_body_no_tags() {
        let html = "<p>Just content</p>";
        let (head, body) = extract_head_and_body(html);
        assert!(head.is_empty());
        assert_eq!(body, html);
    }

    #[test]
    fn test_find_pagebreaks() {
        let body = b"<p>Ch1</p><mbp:pagebreak/><p>Ch2</p><mbp:pagebreak /><p>Ch3</p>";
        let pbs = find_pagebreaks(body);
        assert_eq!(pbs.len(), 2);
        assert_eq!(&body[pbs[0].start..pbs[0].end], b"<mbp:pagebreak/>");
        assert_eq!(&body[pbs[1].start..pbs[1].end], b"<mbp:pagebreak />");
    }

    #[test]
    fn test_find_pagebreaks_case_insensitive() {
        let body = b"<p>A</p><MBP:PAGEBREAK/><p>B</p>";
        let pbs = find_pagebreaks(body);
        assert_eq!(pbs.len(), 1);
    }

    #[test]
    fn test_find_pagebreaks_with_attributes() {
        let body = b"<p>A</p><mbp:pagebreak kindle:kindlefix=\"true\"/><p>B</p>";
        let pbs = find_pagebreaks(body);
        assert_eq!(pbs.len(), 1);
    }

    #[test]
    fn test_find_pagebreaks_none() {
        let body = b"<p>No breaks here</p>";
        let pbs = find_pagebreaks(body);
        assert!(pbs.is_empty());
    }

    #[test]
    fn test_split_at_pagebreaks() {
        let body = "<p>Ch1</p><mbp:pagebreak/><p>Ch2</p><mbp:pagebreak /><p>Ch3</p>";
        let pbs = find_pagebreaks(body.as_bytes());
        let chunks = split_at_pagebreaks(body, &pbs);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], "<p>Ch1</p>");
        assert_eq!(chunks[1], "<p>Ch2</p>");
        assert_eq!(chunks[2], "<p>Ch3</p>");
    }

    #[test]
    fn test_split_mobi_html_with_pagebreaks() {
        let html = br#"<html><head><title>T</title></head><body>
<h1>Chapter 1</h1><p>Text1</p>
<mbp:pagebreak/>
<h1>Chapter 2</h1><p>Text2</p>
<mbp:pagebreak/>
<h1>Chapter 3</h1><p>Text3</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapter_paths.len(), 3);
        assert_eq!(split.chapter_paths[0], "chapter_0.xhtml");
        assert_eq!(split.chapter_paths[1], "chapter_1.xhtml");
        assert_eq!(split.chapter_paths[2], "chapter_2.xhtml");

        // Each chapter should be a complete XHTML document
        for ch in &split.chapters {
            let s = String::from_utf8_lossy(ch);
            assert!(s.contains("<html"), "Missing <html>: {}", s);
            assert!(s.contains("</html>"), "Missing </html>: {}", s);
            assert!(s.contains("<head>"), "Missing <head>: {}", s);
            assert!(s.contains("<body>"), "Missing <body>: {}", s);
        }

        // Check content
        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        let ch1 = String::from_utf8_lossy(&split.chapters[1]);
        let ch2 = String::from_utf8_lossy(&split.chapters[2]);
        assert!(ch0.contains("Chapter 1"));
        assert!(ch1.contains("Chapter 2"));
        assert!(ch2.contains("Chapter 3"));
    }

    #[test]
    fn test_split_mobi_html_no_pagebreaks() {
        let html = b"<html><head></head><body><p>Single chapter</p></body></html>";
        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 1);
        assert_eq!(split.chapter_paths[0], "chapter_0.xhtml");

        let ch = String::from_utf8_lossy(&split.chapters[0]);
        assert!(ch.contains("Single chapter"));
    }

    #[test]
    fn test_split_mobi_html_empty_chunks_filtered() {
        // Pagebreak at very start → first chunk is empty → filtered out
        let html = b"<html><head></head><body><mbp:pagebreak/><p>Only chapter</p></body></html>";
        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 1);
        let ch = String::from_utf8_lossy(&split.chapters[0]);
        assert!(ch.contains("Only chapter"));
    }

    #[test]
    fn test_collect_filepos_anchors() {
        let chunk = r#"<a id="filepos100" /><p>Text</p><a id="filepos500" />"#;
        let mut map = HashMap::new();
        collect_filepos_anchors(chunk, 2, &mut map);

        assert_eq!(map.get("filepos100"), Some(&2));
        assert_eq!(map.get("filepos500"), Some(&2));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_cross_chapter_link_rewriting() {
        // Chapter 0 has filepos100, Chapter 1 has filepos500
        let ch0 = concat!(
            "<html><body>",
            "<a id=\"filepos100\" />",
            "<a href=\"#filepos100\">self</a>",
            "<a href=\"#filepos500\">cross</a>",
            "</body></html>",
        );
        let ch1 = concat!(
            "<html><body>",
            "<a id=\"filepos500\" />",
            "<p>Ch2</p>",
            "</body></html>",
        );
        let mut chapters = vec![ch0.as_bytes().to_vec(), ch1.as_bytes().to_vec()];

        let mut map = HashMap::new();
        map.insert("filepos100".to_string(), 0);
        map.insert("filepos500".to_string(), 1);

        let paths = vec!["chapter_0.xhtml".to_string(), "chapter_1.xhtml".to_string()];

        rewrite_cross_chapter_links(&mut chapters, &map, &paths);

        let ch0 = String::from_utf8_lossy(&chapters[0]);
        // Same-chapter link should be unchanged
        assert!(
            ch0.contains(r##"href="#filepos100""##),
            "Same-chapter link should be unchanged: {}",
            ch0
        );
        // Cross-chapter link should be rewritten
        assert!(
            ch0.contains(r##"href="chapter_1.xhtml#filepos500""##),
            "Cross-chapter link should be rewritten: {}",
            ch0
        );
    }

    #[test]
    fn test_head_content_shared_across_chapters() {
        let html =
            br#"<html><head><title>Book</title><link rel="stylesheet" href="s.css"/></head><body>
<p>Ch1</p><mbp:pagebreak/><p>Ch2</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 2);
        for ch in &split.chapters {
            let s = String::from_utf8_lossy(ch);
            assert!(
                s.contains("<title>Book</title>"),
                "Head should contain title: {}",
                s
            );
            assert!(
                s.contains("s.css"),
                "Head should contain stylesheet link: {}",
                s
            );
        }
    }

    #[test]
    fn test_filepos_to_chapter_mapping() {
        let html = br#"<html><head></head><body>
<a id="filepos10" /><p>Ch1</p>
<mbp:pagebreak/>
<a id="filepos200" /><p>Ch2</p>
<mbp:pagebreak/>
<a id="filepos500" /><p>Ch3</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        assert_eq!(split.filepos_to_chapter.get("filepos10"), Some(&0));
        assert_eq!(split.filepos_to_chapter.get("filepos200"), Some(&1));
        assert_eq!(split.filepos_to_chapter.get("filepos500"), Some(&2));
    }

    #[test]
    fn test_toc_uses_chapter_paths() {
        // Simulate what from_source does: build TOC with chapter paths
        let html = br#"<html><head></head><body>
<a id="filepos0" /><p>Ch1</p>
<mbp:pagebreak/>
<a id="filepos100" /><p>Ch2</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        // Simulate NCX-based TOC construction
        let filepos0_ch = split
            .filepos_to_chapter
            .get("filepos0")
            .copied()
            .unwrap_or(0);
        let filepos100_ch = split
            .filepos_to_chapter
            .get("filepos100")
            .copied()
            .unwrap_or(0);

        let href0 = format!("{}#filepos0", split.chapter_paths[filepos0_ch]);
        let href1 = format!("{}#filepos100", split.chapter_paths[filepos100_ch]);

        assert_eq!(href0, "chapter_0.xhtml#filepos0");
        assert_eq!(href1, "chapter_1.xhtml#filepos100");
    }

    // ====================================================================
    // NCX fallback splitting tests
    // ====================================================================

    #[test]
    fn test_split_ncx_fallback_basic() {
        // HTML without pagebreaks but with filepos anchors at NCX positions
        let html = br#"<html><head><title>Book</title></head><body>
<a id="filepos0" /><h1>Preamble</h1><p>Front matter</p>
<a id="filepos100" /><h1>Chapter 1</h1><p>Text1</p>
<a id="filepos500" /><h1>Chapter 2</h1><p>Text2</p>
</body></html>"#;

        let ncx_positions = vec![0, 100, 500];
        let split = split_mobi_html(html, Some(&ncx_positions));

        // Should split at filepos100 and filepos500 (filepos0 is at body start, skipped)
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapter_paths[0], "chapter_0.xhtml");
        assert_eq!(split.chapter_paths[1], "chapter_1.xhtml");
        assert_eq!(split.chapter_paths[2], "chapter_2.xhtml");

        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        let ch1 = String::from_utf8_lossy(&split.chapters[1]);
        let ch2 = String::from_utf8_lossy(&split.chapters[2]);
        assert!(
            ch0.contains("Preamble"),
            "Ch0 should have preamble: {}",
            ch0
        );
        assert!(
            ch1.contains("Chapter 1"),
            "Ch1 should have Chapter 1: {}",
            ch1
        );
        assert!(
            ch2.contains("Chapter 2"),
            "Ch2 should have Chapter 2: {}",
            ch2
        );
    }

    #[test]
    fn test_split_ncx_fallback_filepos_to_chapter_map() {
        let html = br#"<html><head></head><body>
<a id="filepos0" /><p>Preamble</p>
<a id="filepos200" /><h1>Ch1</h1><a id="filepos300" /><p>More ch1</p>
<a id="filepos800" /><h1>Ch2</h1>
</body></html>"#;

        // Only split at 200 and 800 (skip sub-position 300)
        let ncx_positions = vec![0, 200, 800];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 3);

        // filepos300 should be in chapter 1 (same chapter as filepos200)
        assert_eq!(split.filepos_to_chapter.get("filepos0"), Some(&0));
        assert_eq!(split.filepos_to_chapter.get("filepos200"), Some(&1));
        assert_eq!(split.filepos_to_chapter.get("filepos300"), Some(&1));
        assert_eq!(split.filepos_to_chapter.get("filepos800"), Some(&2));
    }

    #[test]
    fn test_split_ncx_no_matching_anchors() {
        // NCX positions that don't match any filepos anchors → single chapter
        let html = b"<html><head></head><body><p>No anchors here</p></body></html>";

        let ncx_positions = vec![100, 200, 300];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 1);
    }

    #[test]
    fn test_split_ncx_empty_positions() {
        let html = b"<html><head></head><body><p>Content</p></body></html>";

        let ncx_positions: Vec<u32> = vec![];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 1);
    }

    #[test]
    fn test_pagebreaks_preferred_over_ncx() {
        // When both pagebreaks and NCX positions exist, pagebreaks should be used
        let html = br#"<html><head></head><body>
<a id="filepos0" /><p>Ch1</p>
<mbp:pagebreak/>
<a id="filepos100" /><p>Ch2</p>
<mbp:pagebreak/>
<a id="filepos200" /><p>Ch3</p>
</body></html>"#;

        // Pass NCX positions that would create a different split
        let ncx_positions = vec![0, 200];
        let split = split_mobi_html(html, Some(&ncx_positions));

        // Should get 3 chapters from pagebreaks, not 2 from NCX
        assert_eq!(split.chapters.len(), 3);
    }

    #[test]
    fn test_ncx_cross_chapter_links() {
        // NCX-split chapters should have cross-chapter links rewritten
        let html = br##"<html><head></head><body>
<a id="filepos0" /><a href="#filepos500">Go to Ch2</a><p>Ch1</p>
<a id="filepos500" /><a href="#filepos0">Back to Ch1</a><p>Ch2</p>
</body></html>"##;

        let ncx_positions = vec![0, 500];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 2);

        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        let ch1 = String::from_utf8_lossy(&split.chapters[1]);

        // Cross-chapter links should be rewritten
        assert!(
            ch0.contains(r##"href="chapter_1.xhtml#filepos500""##),
            "Ch0 cross-link should be rewritten: {}",
            ch0
        );
        assert!(
            ch1.contains(r##"href="chapter_0.xhtml#filepos0""##),
            "Ch1 cross-link should be rewritten: {}",
            ch1
        );
    }

    // ====================================================================
    // OEB filename link neutralization tests
    // ====================================================================

    #[test]
    fn test_neutralize_bare_filename_links() {
        let html = br#"<a href="cover.htm">Cover</a> and <a href="Book_oeb_01_r1.html">Ch1</a>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            result.contains(r##"href="#""##),
            "Bare .htm link should be neutralized: {}",
            result
        );
        assert!(
            !result.contains("cover.htm"),
            "Original .htm reference should be removed: {}",
            result
        );
        assert!(
            !result.contains("oeb_01_r1.html"),
            "Original .html reference should be removed: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_preserves_filepos_links() {
        let html =
            br##"<a href="#filepos100">Ch1</a> and <a href="chapter_0.xhtml#filepos200">Ch2</a>"##;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            result.contains(r##"href="#filepos100""##),
            "filepos link should be preserved: {}",
            result
        );
        assert!(
            result.contains("chapter_0.xhtml"),
            "xhtml link should be preserved: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_preserves_xhtml_links() {
        let html = br#"<a href="chapter_1.xhtml">Link</a>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            result.contains("chapter_1.xhtml"),
            "xhtml link should be preserved: {}",
            result
        );
    }

    #[test]
    fn test_is_bare_filename_link_cases() {
        assert!(is_bare_filename_link(b"cover.htm"));
        assert!(is_bare_filename_link(b"Book_oeb_01_r1.html"));
        assert!(is_bare_filename_link(b"Cover.HTML"));
        assert!(is_bare_filename_link(b"file.HTM"));

        assert!(!is_bare_filename_link(b"#filepos100"));
        assert!(!is_bare_filename_link(b"chapter_0.xhtml"));
        assert!(!is_bare_filename_link(b"http://example.com/file.html"));
        assert!(!is_bare_filename_link(b"https://example.com/page.htm"));
        assert!(!is_bare_filename_link(b"#"));
        assert!(!is_bare_filename_link(b"image.jpg"));

        // Fragment handling
        assert!(is_bare_filename_link(b"Book_oeb_ftn_r1.html#f1"));
        assert!(is_bare_filename_link(b"cover.htm#section"));
        assert!(!is_bare_filename_link(b"chapter_0.xhtml#filepos100"));
    }

    #[test]
    fn test_neutralize_uppercase_href() {
        // Real MOBI pattern: uppercase HREF with OEB link + lowercase href with filepos
        let html = br##"<A HREF="Asim_oeb_tp_r1.html"  href="#filepos1129"> Title Page</A>"##;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            !result.contains("oeb_tp_r1.html"),
            "Uppercase HREF OEB link should be removed: {}",
            result
        );
        assert!(
            result.contains(r##"href="#filepos1129""##),
            "Lowercase filepos href should be preserved: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_uppercase_href_no_fallback() {
        // Uppercase HREF without a lowercase href fallback
        let html = br#"<A HREF="cover.htm"> Cover</A>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            !result.contains("cover.htm"),
            "OEB link should be neutralized: {}",
            result
        );
        assert!(
            result.contains(r##"href="#""##),
            "Should have fallback href: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_href_with_fragment() {
        let html = br#"<a href="Book_oeb_ftn_r1.html#f1">Note</a>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            !result.contains("oeb_ftn_r1.html"),
            "OEB link with fragment should be neutralized: {}",
            result
        );
    }

    #[test]
    fn test_ncx_split_with_oeb_links_neutralized() {
        // Simulate a MOBI with NCX-split chapters and OEB filename links
        let html = br#"<html><head></head><body>
<a id="filepos0" /><a href="cover.htm">Cover</a>
<a href="Book_oeb_01_r1.html">Ch1</a>
<a href="Book_oeb_02_r1.html">Ch2</a>
<p>Preamble content</p>
<a id="filepos500" /><h1>Chapter 1</h1><p>Text1</p>
<a id="filepos1000" /><h1>Chapter 2</h1><p>Text2</p>
</body></html>"#;

        let ncx_positions = vec![0, 500, 1000];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 3);

        // OEB links in preamble should be neutralized
        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        assert!(
            !ch0.contains("cover.htm"),
            "OEB links should be neutralized: {}",
            ch0
        );
        assert!(
            !ch0.contains("oeb_01_r1.html"),
            "OEB links should be neutralized: {}",
            ch0
        );

        // Content should still be there
        assert!(ch0.contains("Cover"), "Link text should be preserved");
        assert!(ch0.contains("Ch1"), "Link text should be preserved");
    }
}
