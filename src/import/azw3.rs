//! AZW3/KF8 format importer - handles all IO with lazy loading.
//!
//! AZW3 files use the KF8 (Kindle Format 8) structure with:
//! - Skeleton files for HTML structure
//! - Div elements for content fragments
//! - NCX index for table of contents

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::import::{ChapterId, Importer, SpineEntry, resolve_path_based_href};
use crate::io::{ByteSource, FileSource};
use crate::mobi::parser::{
    DivElement, SkeletonFile, parse_div_index, parse_ncx_index, parse_skel_index, read_index,
};
use crate::mobi::{
    Compression, Encoding, HuffCdicReader, MobiFormat, MobiHeader, NULL_INDEX, PdbInfo, TocNode,
    build_toc_from_ncx, detect_image_type, is_metadata_record, palmdoc, parse_exth, parse_fdst,
    strip_trailing_data, transform,
};
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, Landmark, Metadata, TocEntry};

/// AZW3/KF8 format importer with lazy loading.
pub struct Azw3Importer {
    /// Random-access byte source.
    source: Arc<dyn ByteSource>,

    /// PDB header info.
    pdb: PdbInfo,

    /// MOBI header info.
    mobi: MobiHeader,

    /// Record offset for KF8 content (0 for pure KF8, >0 for combo files).
    record_offset: usize,

    /// File length.
    file_len: u64,

    /// Book metadata.
    metadata: Metadata,

    /// Table of contents.
    toc: Vec<TocEntry>,

    /// Landmarks (structural navigation points).
    landmarks: Vec<Landmark>,

    /// Reading order (spine).
    spine: Vec<SpineEntry>,

    /// Chapter paths (filenames).
    chapter_paths: Vec<String>,

    /// KF8 structure for chapter reconstruction.
    kf8: Kf8Structure,

    /// Cached decompressed text (loaded on first chapter request).
    text_cache: Option<Vec<u8>>,

    /// Cached chapter content.
    chapter_cache: HashMap<u32, Vec<u8>>,

    // --- Link resolution ---
    /// Maps "path#id" -> GlobalNodeId (built during index_anchors)
    element_id_map: HashMap<String, GlobalNodeId>,

    // --- TOC resolution ---
    /// NCX positions for TOC entries, keyed by (title, chapter_path).
    toc_positions: HashMap<(String, String), TocPosition>,
}

/// Position metadata for a TOC entry (from NCX).
#[derive(Debug, Clone, Copy)]
struct TocPosition {
    /// Byte position in the text stream.
    byte_pos: u32,
    /// File number (skeleton file).
    file_num: u32,
}

/// KF8 structure info parsed from indices.
struct Kf8Structure {
    /// Flow table from FDST (byte ranges in decompressed text).
    flow_table: Vec<(usize, usize)>,
    /// Skeleton files (chapter structure).
    files: Vec<SkeletonFile>,
    /// Div elements (content fragments).
    elems: Vec<DivElement>,
}

impl Importer for Azw3Importer {
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
        // Check chapter cache first
        if let Some(content) = self.chapter_cache.get(&id.0) {
            return Ok(content.clone());
        }

        // Ensure text is loaded
        if self.text_cache.is_none() {
            self.text_cache = Some(self.extract_text()?);
        }

        // Build the requested chapter
        let text = self.text_cache.as_ref().unwrap();
        let content = self.build_chapter(id.0, text)?;

        self.chapter_cache.insert(id.0, content.clone());
        Ok(content)
    }

    fn list_assets(&self) -> Vec<PathBuf> {
        // Asset discovery is done lazily - scan image records
        self.discover_assets()
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

    fn index_anchors(&mut self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        self.element_id_map.clear();

        // Build path#id → GlobalNodeId map from chapters (same format as EPUB)
        for (chapter_id, chapter) in chapters {
            // Get the chapter's source path
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

    fn resolve_toc(&mut self) {
        // Load text if not cached
        if self.text_cache.is_none() {
            if let Ok(text) = self.extract_text() {
                self.text_cache = Some(text);
            } else {
                return;
            }
        }

        let text = self.text_cache.as_ref().unwrap();

        // Get HTML flow (flow 0)
        let (html_start, html_end) = self
            .kf8
            .flow_table
            .first()
            .copied()
            .unwrap_or((0, text.len()));
        let html_text = &text[html_start..html_end.min(text.len())];

        // Build file_starts for find_nearest_id_fast
        let file_starts: Vec<(u32, u32)> = self
            .kf8
            .files
            .iter()
            .map(|f| (f.start_pos, f.file_number as u32))
            .collect();

        // Resolve TOC entries using stored positions
        resolve_toc_with_positions(&mut self.toc, &self.toc_positions, html_text, &file_starts);
    }
}

/// Recursively resolve TOC entry hrefs with fragment IDs using position map.
fn resolve_toc_with_positions(
    entries: &mut [TocEntry],
    positions: &HashMap<(String, String), TocPosition>,
    html_text: &[u8],
    file_starts: &[(u32, u32)],
) {
    for entry in entries {
        // Look up position by (title, chapter_path)
        let chapter_path = entry.href.split('#').next().unwrap_or(&entry.href);
        let key = (entry.title.clone(), chapter_path.to_string());

        if let Some(pos) = positions.get(&key) {
            // Find nearest ID at this position
            if let Some(id) = transform::find_nearest_id_fast(
                html_text,
                pos.byte_pos as usize,
                pos.file_num as usize,
                file_starts,
            ) {
                // Update href with fragment
                if !entry.href.contains('#') {
                    entry.href = format!("{}#{}", entry.href, id);
                }
            }
        }

        // Recurse into children
        resolve_toc_with_positions(&mut entry.children, positions, html_text, file_starts);
    }
}

impl Azw3Importer {
    /// Create an importer from a ByteSource (metadata only, text deferred).
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

        // Helper to read a record
        let read_record = |idx: usize| -> io::Result<Vec<u8>> {
            let (start, end) = pdb.record_range(idx, file_len)?;
            source.read_at(start, (end - start) as usize)
        };

        // Parse record 0 (MOBI header)
        let record0 = read_record(0)?;
        let mobi = MobiHeader::parse(&record0)?;

        if mobi.encryption != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Encrypted files are not supported",
            ));
        }

        // Parse EXTH metadata
        let exth = parse_exth(&record0, &mobi);

        // Detect format and get record offset
        let format = detect_format(&mobi, &exth, &pdb, &read_record)?;
        let record_offset = format.record_offset();

        // For combo files, re-parse KF8 header
        let mobi = if record_offset > 0 {
            let kf8_record0 = read_record(record_offset)?;
            MobiHeader::parse(&kf8_record0)?
        } else {
            mobi
        };

        // Verify this is KF8
        if !format.is_kf8() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not a KF8/AZW3 file - use MobiImporter for MOBI6 files",
            ));
        }

        // Build metadata
        let mut metadata = build_metadata(&pdb, &mobi, &exth);

        // Parse KF8 indices (without reading text content)
        let codec = match mobi.encoding {
            Encoding::Utf8 => "utf-8",
            _ => "cp1252",
        };

        let mut read_record_offset = |idx: usize| -> io::Result<Vec<u8>> {
            let actual_idx = idx + record_offset;
            let (start, end) = pdb.record_range(actual_idx, file_len)?;
            source.read_at(start, (end - start) as usize)
        };

        // Parse FDST
        let flow_table = if mobi.fdst_index != NULL_INDEX {
            let fdst_record = read_record_offset(mobi.fdst_index as usize)?;
            parse_fdst(&fdst_record)?
        } else {
            Vec::new()
        };

        // Parse skeleton index
        let files = if mobi.skel_index != NULL_INDEX {
            let (entries, _) =
                read_index(&mut read_record_offset, mobi.skel_index as usize, codec)?;
            parse_skel_index(&entries)
        } else {
            Vec::new()
        };

        // Parse div index
        let elems = if mobi.div_index != NULL_INDEX {
            let (entries, cncx) =
                read_index(&mut read_record_offset, mobi.div_index as usize, codec)?;
            parse_div_index(&entries, &cncx)
        } else {
            Vec::new()
        };

        // Parse NCX for TOC
        let ncx = if mobi.ncx_index != NULL_INDEX {
            let (entries, cncx) =
                read_index(&mut read_record_offset, mobi.ncx_index as usize, codec)?;
            parse_ncx_index(&entries, &cncx)
        } else {
            Vec::new()
        };

        // Build spine from skeleton files
        let mut spine = Vec::new();
        let mut chapter_paths = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let filename = format!("part{:04}.html", file.file_number);
            chapter_paths.push(filename);
            spine.push(SpineEntry {
                id: ChapterId(i as u32),
                size_estimate: file.length as usize,
            });
        }

        // Build hierarchical TOC and collect positions for later resolution
        let mut toc_positions = HashMap::new();
        let toc = {
            let nodes = build_toc_from_ncx(&ncx, |entry| {
                // KF8 uses pos_fid (frag_idx, offset) - calculate actual byte position
                // frag_idx is index into fragment/div table, offset is added to insert_pos
                let (file_num, byte_pos) = if let Some((frag_idx, offset)) = entry.pos_fid
                    && let Some(elem) = elems.get(frag_idx as usize)
                {
                    // Position is elem's insert_pos + offset (like KindleUnpack)
                    (elem.file_number as usize, elem.insert_pos + offset)
                } else {
                    // Fall back to absolute position
                    let file_num = find_file_for_position(&files, entry.pos)
                        .map(|f| f.file_number)
                        .unwrap_or(0);
                    (file_num, entry.pos)
                };

                let chapter_path = format!("part{:04}.html", file_num);

                // Store position keyed by (title, chapter_path)
                // Use unescaped title to match TocEntry.title
                let title = quick_xml::escape::unescape(&entry.text)
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| entry.text.clone());
                let key = (title, chapter_path.clone());
                toc_positions.insert(
                    key,
                    TocPosition {
                        byte_pos,
                        file_num: file_num as u32,
                    },
                );

                chapter_path
            });
            nodes.into_iter().map(toc_node_to_entry).collect()
        };

        // Find cover image
        if let Some(exth) = exth
            && let Some(cover_idx) = exth.cover_offset
        {
            metadata.cover_image = Some(format!("images/image_{:04}.jpg", cover_idx));
        }

        Ok(Self {
            source,
            pdb,
            mobi,
            record_offset,
            file_len,
            metadata,
            toc,
            landmarks: Vec::new(), // AZW3 format doesn't have landmarks
            spine,
            chapter_paths,
            kf8: Kf8Structure {
                flow_table,
                files,
                elems,
            },
            text_cache: None,
            chapter_cache: HashMap::new(),
            element_id_map: HashMap::new(),
            toc_positions,
        })
    }

    /// Extract and decompress text content (called on first chapter request).
    fn extract_text(&self) -> io::Result<Vec<u8>> {
        let mut text = Vec::new();

        let read_record = |idx: usize| -> io::Result<Vec<u8>> {
            let actual_idx = idx + self.record_offset;
            let (start, end) = self.pdb.record_range(actual_idx, self.file_len)?;
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

    /// Build a specific chapter from cached text.
    fn build_chapter(&self, chapter_id: u32, text: &[u8]) -> io::Result<Vec<u8>> {
        // Get HTML content (flow 0)
        let (html_start, html_end) = self
            .kf8
            .flow_table
            .first()
            .copied()
            .unwrap_or((0, text.len()));
        let html_text = &text[html_start..html_end.min(text.len())];

        // Build all parts and return the requested one
        let parts = build_parts(html_text, &self.kf8.files, &self.kf8.elems);

        let content = parts
            .get(chapter_id as usize)
            .map(|(_, content)| content.clone())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Chapter {} not found", chapter_id),
                )
            })?;

        // Transform kindle: references to standard EPUB-style paths
        // This converts kindle:pos:fid:XXXX:off:YYYY to partNNNN.html#id
        let file_starts: Vec<(u32, u32)> = self
            .kf8
            .files
            .iter()
            .map(|f| (f.start_pos, f.file_number as u32))
            .collect();

        let transformed = transform::transform_kindle_refs(
            &content,
            &self.kf8.elems,
            html_text,
            &file_starts,
        );

        // Strip Amazon-specific attributes (aid, data-Amzn*)
        let cleaned = transform::strip_kindle_attributes_fast(&transformed);

        Ok(cleaned)
    }

    /// Discover asset paths by scanning image records.
    fn discover_assets(&self) -> Vec<PathBuf> {
        let mut assets = Vec::new();

        if self.mobi.first_image_index == NULL_INDEX {
            return assets;
        }

        let first_img = self.mobi.first_image_index as usize + self.record_offset;
        for i in first_img..self.pdb.num_records as usize {
            // Only read first 16 bytes to detect type (magic bytes)
            if let Ok((start, end)) = self.pdb.record_range(i, self.file_len) {
                let read_len = 16.min((end - start) as usize);
                if let Ok(header) = self.source.read_at(start, read_len) {
                    if is_metadata_record(&header) {
                        continue;
                    }
                    if let Some(media_type) = detect_image_type(&header) {
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
        let first_img = self.mobi.first_image_index as usize + self.record_offset;
        let record_idx = first_img + idx;
        self.read_record(record_idx)
    }

    /// Read a record by absolute index.
    fn read_record(&self, idx: usize) -> io::Result<Vec<u8>> {
        let (start, end) = self.pdb.record_range(idx, self.file_len)?;
        self.source.read_at(start, (end - start) as usize)
    }
}

// ============================================================================
// Shared helpers
// ============================================================================

fn detect_format(
    mobi: &MobiHeader,
    exth: &Option<crate::mobi::ExthHeader>,
    pdb: &PdbInfo,
    read_record: &dyn Fn(usize) -> io::Result<Vec<u8>>,
) -> io::Result<MobiFormat> {
    if mobi.mobi_version == 8 {
        return Ok(MobiFormat::Kf8);
    }

    if let Some(kf8_idx) = exth.as_ref().and_then(|e| e.kf8_boundary) {
        let boundary_idx = kf8_idx as usize - 1;
        if boundary_idx > 0 && boundary_idx < pdb.num_records as usize {
            let boundary = read_record(boundary_idx)?;
            if boundary.starts_with(b"BOUNDARY") {
                return Ok(MobiFormat::Combo {
                    kf8_record_offset: kf8_idx as usize,
                });
            }
        }
    }

    Ok(MobiFormat::Mobi6)
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

/// Build chapter parts by combining skeletons with div content.
fn build_parts(
    text: &[u8],
    files: &[SkeletonFile],
    elems: &[DivElement],
) -> Vec<(String, Vec<u8>)> {
    let mut parts = Vec::new();
    let mut div_ptr = 0;

    for file in files {
        let skel_start = file.start_pos as usize;
        let skel_end = skel_start + file.length as usize;

        if skel_end > text.len() {
            continue;
        }

        let mut skeleton = text[skel_start..skel_end].to_vec();
        let mut baseptr = skel_end;

        for _i in 0..file.div_count {
            if div_ptr >= elems.len() {
                break;
            }

            let elem = &elems[div_ptr];
            let part_len = elem.length as usize;

            if baseptr + part_len > text.len() {
                div_ptr += 1;
                continue;
            }

            let part = &text[baseptr..baseptr + part_len];
            let insert_pos = (elem.insert_pos as usize).saturating_sub(skel_start);

            if insert_pos <= skeleton.len() {
                let mut new_skeleton = Vec::with_capacity(skeleton.len() + part.len());
                new_skeleton.extend_from_slice(&skeleton[..insert_pos]);
                new_skeleton.extend_from_slice(part);
                new_skeleton.extend_from_slice(&skeleton[insert_pos..]);
                skeleton = new_skeleton;
            }

            baseptr += part_len;
            div_ptr += 1;
        }

        let filename = format!("part{:04}.html", file.file_number);
        parts.push((filename, skeleton));
    }

    if parts.is_empty() && !text.is_empty() {
        parts.push(("part0000.html".to_string(), text.to_vec()));
    }

    parts
}

fn find_file_for_position(files: &[SkeletonFile], pos: u32) -> Option<&SkeletonFile> {
    for file in files {
        if pos >= file.start_pos && pos < file.start_pos + file.length {
            return Some(file);
        }
    }
    files.first()
}

/// Convert TocNode to TocEntry recursively.
fn toc_node_to_entry(node: TocNode) -> TocEntry {
    let mut entry = TocEntry::new(&node.title, &node.href);
    entry.children = node.children.into_iter().map(toc_node_to_entry).collect();
    entry
}
