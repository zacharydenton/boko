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

use crate::book::{Landmark, Metadata, TocEntry};
use crate::import::{ChapterId, Importer, SpineEntry};
use crate::io::{ByteSource, FileSource};
use crate::mobi::{
    Compression, Encoding, HuffCdicReader, MobiFormat, MobiHeader, PdbInfo, TocNode,
    build_toc_from_ncx, detect_image_type, is_metadata_record, parse_exth, parse_fdst,
    strip_trailing_data, NULL_INDEX, palmdoc,
};
use crate::mobi::parser::{
    DivElement, SkeletonFile,
    parse_div_index, parse_ncx_index, parse_skel_index, read_index,
};

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
                io::Error::new(io::ErrorKind::NotFound, format!("Invalid asset path: {}", key))
            })?;

        self.load_image_record(idx)
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
            let (entries, _) = read_index(&mut read_record_offset, mobi.skel_index as usize, codec)?;
            parse_skel_index(&entries)
        } else {
            Vec::new()
        };

        // Parse div index
        let elems = if mobi.div_index != NULL_INDEX {
            let (entries, cncx) = read_index(&mut read_record_offset, mobi.div_index as usize, codec)?;
            parse_div_index(&entries, &cncx)
        } else {
            Vec::new()
        };

        // Parse NCX for TOC
        let ncx = if mobi.ncx_index != NULL_INDEX {
            let (entries, cncx) = read_index(&mut read_record_offset, mobi.ncx_index as usize, codec)?;
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

        // Build hierarchical TOC
        let toc = {
            let nodes = build_toc_from_ncx(&ncx, |entry| {
                // KF8 uses pos_fid (file ID + offset) or falls back to byte position
                if let Some((elem_idx, _offset)) = entry.pos_fid
                    && let Some(elem) = elems.get(elem_idx as usize) {
                        return format!("part{:04}.html", elem.file_number);
                    }
                find_file_for_position(&files, entry.pos)
                    .map(|f| format!("part{:04}.html", f.file_number))
                    .unwrap_or_else(|| "part0000.html".to_string())
            });
            nodes.into_iter().map(toc_node_to_entry).collect()
        };

        // Find cover image
        if let Some(exth) = exth
            && let Some(cover_idx) = exth.cover_offset {
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
            kf8: Kf8Structure { flow_table, files, elems },
            text_cache: None,
            chapter_cache: HashMap::new(),
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
        let (html_start, html_end) = self.kf8.flow_table.first().copied().unwrap_or((0, text.len()));
        let html_text = &text[html_start..html_end.min(text.len())];

        // Build all parts and return the requested one
        let parts = build_parts(html_text, &self.kf8.files, &self.kf8.elems);

        parts
            .get(chapter_id as usize)
            .map(|(_, content)| content.clone())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Chapter {} not found", chapter_id),
                )
            })
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
        metadata.identifier = exth.isbn.clone()
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
