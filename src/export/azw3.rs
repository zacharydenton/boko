//! AZW3/KF8 exporter.
//!
//! Creates KF8 (Kindle Format 8) files from Book structures.

use std::collections::{HashMap, HashSet};
use std::io::{self, Seek, Write};
use std::path::Path;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::book::{Book, Resource, TocEntry};
use crate::mobi::index::{
    NcxBuildEntry, build_chunk_indx, build_cncx, build_ncx_indx, build_skel_indx,
    calculate_cncx_offsets,
};
use crate::mobi::skeleton::{Chunker, ChunkerResult};
use crate::mobi::writer_transform::{
    rewrite_css_references_fast, rewrite_html_references_fast, write_base32_4, write_base32_10,
};

use super::Exporter;

// Constants
const RECORD_SIZE: usize = 4096;
const NULL_INDEX: u32 = 0xFFFF_FFFF;
const XOR_KEY_LEN: usize = 20;

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
    fn export<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> io::Result<()> {
        let builder = Kf8Builder::new(book, self.config.normalize)?;
        builder.write(writer)
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
    metadata: crate::book::Metadata,
}

struct SpineItem {
    href: String,
    data: Vec<u8>,
}

impl BookContext {
    /// Collect all data from a Book into internal structures.
    fn from_book(book: &mut Book, normalize: bool) -> io::Result<Self> {
        if normalize {
            Self::from_normalized(book)
        } else {
            Self::from_raw(book)
        }
    }

    /// Collect raw (passthrough) content from the book.
    fn from_raw(book: &mut Book) -> io::Result<Self> {
        // Collect metadata and TOC (these are borrowed, so clone)
        let metadata = book.metadata().clone();
        let toc = book.toc().to_vec();

        // Collect spine items
        let spine_entries: Vec<_> = book.spine().to_vec();
        let mut spine = Vec::with_capacity(spine_entries.len());

        for entry in &spine_entries {
            let href = book
                .source_id(entry.id)
                .unwrap_or("unknown.xhtml")
                .to_string();
            let data = book.load_raw(entry.id)?;
            spine.push(SpineItem { href, data });
        }

        // Collect assets
        let asset_paths = book.list_assets();
        let mut resources = HashMap::new();

        for path in asset_paths {
            let path_str = path.to_string_lossy().to_string();
            let data = book.load_asset(&path)?;
            let media_type = guess_media_type(&path_str);

            resources.insert(path_str, Resource { data, media_type });
        }

        // Also add spine items as resources (needed for internal lookups)
        for item in &spine {
            if !resources.contains_key(&item.href) {
                resources.insert(
                    item.href.clone(),
                    Resource {
                        data: item.data.clone(),
                        media_type: "application/xhtml+xml".to_string(),
                    },
                );
            }
        }

        Ok(Self {
            resources,
            spine,
            toc,
            metadata,
        })
    }

    /// Collect normalized content from the book through IR pipeline.
    fn from_normalized(book: &mut Book) -> io::Result<Self> {
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
                    media_type: "text/css".to_string(),
                },
            );
        }

        // Build spine from normalized chapters
        let mut spine = Vec::with_capacity(normalized.chapters.len());
        for (i, chapter) in normalized.chapters.iter().enumerate() {
            let href = format!("chapter_{}.xhtml", i);
            let data = chapter.document.as_bytes().to_vec();

            // Add as resource
            resources.insert(
                href.clone(),
                Resource {
                    data: data.clone(),
                    media_type: "application/xhtml+xml".to_string(),
                },
            );

            spine.push(SpineItem { href, data });
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
        })
    }
}

struct Kf8Builder {
    ctx: BookContext,
    records: Vec<Vec<u8>>,
    text_length: usize,
    last_text_record: u16,
    first_resource_record: u32,
    skel_index: u32,
    frag_index: u32,
    ncx_index: u32,
    chunker_result: Option<ChunkerResult>,
    /// Maps resource href to 1-indexed resource record number
    resource_map: HashMap<String, usize>,
    /// CSS flows (flow 0 is text, flows 1+ are CSS)
    css_flows: Vec<String>,
    /// Total flows length (text + CSS)
    flows_length: usize,
    /// Ordered list of image hrefs
    image_hrefs: Vec<String>,
    /// Ordered list of font hrefs
    font_hrefs: Vec<String>,
    /// Counter for link placeholders
    link_counter: usize,
    /// Maps placeholder -> (target_file_href, target_fragment)
    link_map: HashMap<String, (String, String)>,
}

impl Kf8Builder {
    fn new(book: &mut Book, normalize: bool) -> io::Result<Self> {
        let ctx = BookContext::from_book(book, normalize)?;

        let mut builder = Self {
            ctx,
            records: vec![Vec::new()], // Placeholder for record 0
            text_length: 0,
            last_text_record: 0,
            first_resource_record: NULL_INDEX,
            skel_index: NULL_INDEX,
            frag_index: NULL_INDEX,
            ncx_index: NULL_INDEX,
            chunker_result: None,
            resource_map: HashMap::new(),
            css_flows: Vec::new(),
            flows_length: 0,
            image_hrefs: Vec::new(),
            font_hrefs: Vec::new(),
            link_counter: 0,
            link_map: HashMap::new(),
        };

        builder.collect_resources()?;
        builder.build_text_records()?;
        builder.write_resource_records()?;
        builder.build_kf8_indices()?;
        builder.build_fdst_record()?;
        builder.build_flis_fcis_eof()?;
        builder.build_record0()?;

        Ok(builder)
    }

    fn collect_resources(&mut self) -> io::Result<()> {
        // Collect images
        self.image_hrefs = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| r.media_type.starts_with("image/"))
            .map(|(href, _)| href.clone())
            .collect();
        self.image_hrefs.sort();

        // Collect fonts
        self.font_hrefs = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| {
                r.media_type.contains("font")
                    || r.media_type == "application/x-font-ttf"
                    || r.media_type == "application/x-font-opentype"
                    || r.media_type == "application/vnd.ms-opentype"
                    || r.media_type == "font/ttf"
                    || r.media_type == "font/otf"
                    || r.media_type == "font/woff"
            })
            .map(|(href, _)| href.clone())
            .collect();
        self.font_hrefs.sort();

        // Collect CSS
        let mut css_hrefs: Vec<_> = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .map(|(href, _)| href.clone())
            .collect();
        css_hrefs.sort();

        // Store CSS flows
        for href in &css_hrefs {
            if let Some(resource) = self.ctx.resources.get(href) {
                let css = String::from_utf8_lossy(&resource.data).to_string();
                self.css_flows.push(css);
            }
        }

        // Build resource_map
        let mut resource_idx = 1usize;

        for href in &self.image_hrefs {
            self.resource_map.insert(href.clone(), resource_idx);
            resource_idx += 1;
        }

        for href in &self.font_hrefs {
            self.resource_map.insert(href.clone(), resource_idx);
            resource_idx += 1;
        }

        Ok(())
    }

    fn build_text_records(&mut self) -> io::Result<()> {
        // Build CSS href -> flow index map
        let mut css_hrefs: Vec<_> = self
            .ctx
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .map(|(href, _)| href.clone())
            .collect();
        css_hrefs.sort();

        let mut css_flow_map: HashMap<String, usize> = HashMap::new();
        for (i, href) in css_hrefs.iter().enumerate() {
            css_flow_map.insert(href.clone(), i + 1);
        }

        // Build spine hrefs set
        let spine_hrefs: HashSet<&str> = self.ctx.spine.iter().map(|s| s.href.as_str()).collect();

        // Process HTML files
        let mut html_files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut link_counter = 0usize;

        for spine_item in &self.ctx.spine {
            if let Some(resource) = self.ctx.resources.get(&spine_item.href)
                && resource.media_type == "application/xhtml+xml"
            {
                let result = rewrite_html_references_fast(
                    &resource.data,
                    &spine_item.href,
                    &css_flow_map,
                    &self.resource_map,
                    &spine_hrefs,
                    &self.ctx.resources,
                    link_counter,
                );

                // Collect links
                for link in result.links {
                    let mut base32_buf = [0u8; 10];
                    write_base32_10(link_counter + 1, &mut base32_buf);
                    let placeholder = format!(
                        "kindle:pos:fid:0000:off:{}",
                        std::str::from_utf8(&base32_buf).unwrap()
                    );
                    self.link_map
                        .insert(placeholder, (link.target_file, link.fragment));
                    link_counter += 1;
                }

                html_files.push((spine_item.href.clone(), result.html));
            }
        }
        self.link_counter = link_counter;

        // Rewrite CSS
        let rewritten_css: Vec<Vec<u8>> = self
            .css_flows
            .iter()
            .map(|css| rewrite_css_references_fast(css.as_bytes(), &self.resource_map))
            .collect();

        // Process with chunker
        let mut chunker = Chunker::new();
        let chunker_result = chunker.process(&html_files);

        // Resolve link placeholders
        let resolved_text = self.resolve_link_placeholders(
            &chunker_result.text,
            &chunker_result.id_map,
            &chunker_result.aid_offset_map,
            &chunker_result.filepos_map,
        );
        self.text_length = resolved_text.len();

        // Combine flows
        let mut all_flows = resolved_text;
        for css in &rewritten_css {
            all_flows.extend_from_slice(css);
        }
        self.flows_length = all_flows.len();

        // Split into records and compress
        let mut pos = 0;
        while pos < all_flows.len() {
            let end = (pos + RECORD_SIZE).min(all_flows.len());
            let chunk = &all_flows[pos..end];

            let compressed = crate::mobi::palmdoc::compress(chunk);
            let mut record = compressed;
            record.push(0); // Trailing byte

            self.records.push(record);
            pos = end;
        }

        self.last_text_record = (self.records.len() - 1) as u16;
        self.chunker_result = Some(chunker_result);

        self.css_flows = rewritten_css
            .into_iter()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect();

        Ok(())
    }

    fn resolve_link_placeholders(
        &self,
        text: &[u8],
        id_map: &HashMap<(String, String), String>,
        aid_offset_map: &HashMap<String, (usize, usize, usize)>,
        filepos_map: &HashMap<String, Vec<(usize, String)>>,
    ) -> Vec<u8> {
        use memchr::memmem;

        const PREFIX: &[u8] = b"kindle:pos:fid:0000:off:";
        const PLACEHOLDER_LEN: usize = 34;

        let finder = memmem::Finder::new(PREFIX);
        let mut replacements: Vec<(usize, usize, [u8; 34])> = Vec::new();

        let mut search_start = 0;
        while let Some(pos) = finder.find(&text[search_start..]) {
            let start = search_start + pos;
            let end = start + PLACEHOLDER_LEN;

            if end <= text.len() {
                let placeholder = std::str::from_utf8(&text[start..end]).unwrap_or("");

                if let Some((target_file, fragment)) = self.link_map.get(placeholder) {
                    // Try to resolve the link target
                    let resolved = if fragment.starts_with("filepos") {
                        // MOBI filepos reference - use filepos_map
                        resolve_filepos_to_offset(
                            target_file,
                            fragment,
                            filepos_map,
                            aid_offset_map,
                        )
                    } else {
                        // Standard element ID - use id_map
                        let key = (target_file.clone(), fragment.clone());
                        let aid = id_map
                            .get(&key)
                            .or_else(|| id_map.get(&(target_file.clone(), String::new())));

                        aid.and_then(|aid| {
                            aid_offset_map
                                .get(aid)
                                .map(|&(seq_num, offset_in_chunk, _)| (seq_num, offset_in_chunk))
                        })
                    };

                    if let Some((seq_num, offset_in_chunk)) = resolved {
                        let mut replacement = [0u8; 34];
                        replacement[..15].copy_from_slice(b"kindle:pos:fid:");
                        let mut fid_buf = [0u8; 4];
                        write_base32_4(seq_num, &mut fid_buf);
                        replacement[15..19].copy_from_slice(&fid_buf);
                        replacement[19..24].copy_from_slice(b":off:");
                        let mut off_buf = [0u8; 10];
                        write_base32_10(offset_in_chunk, &mut off_buf);
                        replacement[24..34].copy_from_slice(&off_buf);
                        replacements.push((start, end, replacement));
                    }
                }
            }
            search_start = start + 1;
        }

        if replacements.is_empty() {
            return text.to_vec();
        }

        replacements.sort_by_key(|(start, _, _)| *start);

        let mut result = Vec::with_capacity(text.len());
        let mut last_end = 0;

        for (start, end, replacement) in replacements {
            result.extend_from_slice(&text[last_end..start]);
            result.extend_from_slice(&replacement);
            last_end = end;
        }
        result.extend_from_slice(&text[last_end..]);

        result
    }

    fn write_resource_records(&mut self) -> io::Result<()> {
        if !self.image_hrefs.is_empty() || !self.font_hrefs.is_empty() {
            self.first_resource_record = self.records.len() as u32;
        }

        // Write images
        for href in &self.image_hrefs.clone() {
            if let Some(resource) = self.ctx.resources.get(href) {
                self.records.push(resource.data.clone());
            }
        }

        // Write fonts as FONT records
        for href in &self.font_hrefs.clone() {
            if let Some(resource) = self.ctx.resources.get(href) {
                let font_record = write_font_record(&resource.data)?;
                self.records.push(font_record);
            }
        }

        Ok(())
    }

    fn build_kf8_indices(&mut self) -> io::Result<()> {
        if let Some(ref chunker_result) = self.chunker_result {
            // Build SKEL index
            if !chunker_result.skel_table.is_empty() {
                self.skel_index = self.records.len() as u32;
                let skel_records = build_skel_indx(&chunker_result.skel_table);
                for record in skel_records {
                    self.records.push(record);
                }
            }

            // Build Fragment/Chunk index
            if !chunker_result.chunk_table.is_empty() {
                let selectors: Vec<String> = chunker_result
                    .chunk_table
                    .iter()
                    .map(|c| c.selector.clone())
                    .collect();
                let cncx_offsets = calculate_cncx_offsets(&selectors);
                let cncx = build_cncx(&selectors);

                self.frag_index = self.records.len() as u32;
                let chunk_records = build_chunk_indx(&chunker_result.chunk_table, &cncx_offsets);
                for record in chunk_records {
                    self.records.push(record);
                }

                if !cncx.is_empty() {
                    self.records.push(cncx);
                }
            }
        }

        // Build NCX index
        if !self.ctx.toc.is_empty()
            && let Some(ref chunker_result) = self.chunker_result
        {
            let ncx_entries = flatten_toc(
                &self.ctx.toc,
                self.text_length as u32,
                &chunker_result.id_map,
                &chunker_result.aid_offset_map,
                &chunker_result.filepos_map,
            );

            if !ncx_entries.is_empty() {
                self.ncx_index = self.records.len() as u32;
                let (ncx_records, ncx_cncx) = build_ncx_indx(&ncx_entries);
                for record in ncx_records {
                    self.records.push(record);
                }
                if !ncx_cncx.is_empty() {
                    self.records.push(ncx_cncx);
                }
            }
        }

        Ok(())
    }

    fn build_fdst_record(&mut self) -> io::Result<()> {
        let num_flows = 1 + self.css_flows.len();

        let mut fdst = Vec::new();
        fdst.extend_from_slice(b"FDST");
        fdst.extend_from_slice(&12u32.to_be_bytes());
        fdst.extend_from_slice(&(num_flows as u32).to_be_bytes());

        // Flow 0: text
        fdst.extend_from_slice(&0u32.to_be_bytes());
        fdst.extend_from_slice(&(self.text_length as u32).to_be_bytes());

        // CSS flows
        let mut offset = self.text_length;
        for css in &self.css_flows {
            let start = offset;
            let end = offset + css.len();
            fdst.extend_from_slice(&(start as u32).to_be_bytes());
            fdst.extend_from_slice(&(end as u32).to_be_bytes());
            offset = end;
        }

        self.records.push(fdst);
        Ok(())
    }

    fn build_flis_fcis_eof(&mut self) -> io::Result<()> {
        // FLIS
        let flis = b"FLIS\0\0\0\x08\0\x41\0\0\0\0\0\0\xff\xff\xff\xff\0\x01\0\x03\0\0\0\x03\0\0\0\x01\xff\xff\xff\xff";
        self.records.push(flis.to_vec());

        // FCIS
        let mut fcis = Vec::new();
        fcis.extend_from_slice(
            b"FCIS\x00\x00\x00\x14\x00\x00\x00\x10\x00\x00\x00\x02\x00\x00\x00\x00",
        );
        fcis.extend_from_slice(&(self.text_length as u32).to_be_bytes());
        fcis.extend_from_slice(b"\x00\x00\x00\x00\x00\x00\x00\x28\x00\x00\x00\x00\x00\x00\x00");
        fcis.extend_from_slice(b"\x28\x00\x00\x00\x08\x00\x01\x00\x01\x00\x00\x00\x00");
        self.records.push(fcis);

        // EOF
        self.records.push(b"\xe9\x8e\r\n".to_vec());

        Ok(())
    }

    fn build_record0(&mut self) -> io::Result<()> {
        let title = &self.ctx.metadata.title;
        let title_bytes = title.as_bytes();

        let exth = self.build_exth();
        let exth_len = exth.len();

        let mobi_header_len: u32 = 264;
        let title_offset = 16 + mobi_header_len + exth_len as u32;
        let full_record_len = title_offset as usize + title_bytes.len() + 2;

        let mut record0 = Vec::with_capacity(full_record_len + 8192);

        // PalmDOC header (16 bytes)
        record0.extend_from_slice(&2u16.to_be_bytes()); // Compression: PalmDOC
        record0.extend_from_slice(&[0, 0]);
        record0.extend_from_slice(&(self.text_length as u32).to_be_bytes());
        record0.extend_from_slice(&self.last_text_record.to_be_bytes());
        record0.extend_from_slice(&(RECORD_SIZE as u16).to_be_bytes());
        record0.extend_from_slice(&0u16.to_be_bytes()); // Encryption
        record0.extend_from_slice(&0u16.to_be_bytes());

        // MOBI header
        record0.extend_from_slice(b"MOBI");
        record0.extend_from_slice(&mobi_header_len.to_be_bytes());
        record0.extend_from_slice(&2u32.to_be_bytes()); // Book type
        record0.extend_from_slice(&65001u32.to_be_bytes()); // UTF-8
        record0.extend_from_slice(&rand_uid().to_be_bytes());
        record0.extend_from_slice(&8u32.to_be_bytes()); // KF8 version

        // Meta indices (40-80)
        for _ in 0..10 {
            record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        }

        // First non-text record
        let first_non_text = (self.last_text_record as u32) + 1;
        record0.extend_from_slice(&first_non_text.to_be_bytes());

        // Title offset and length
        record0.extend_from_slice(&title_offset.to_be_bytes());
        record0.extend_from_slice(&(title_bytes.len() as u32).to_be_bytes());

        // Language
        record0.extend_from_slice(&0x09u32.to_be_bytes());

        // Dictionary in/out
        record0.extend_from_slice(&0u32.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Min version
        record0.extend_from_slice(&8u32.to_be_bytes());

        // First resource record
        record0.extend_from_slice(&self.first_resource_record.to_be_bytes());

        // Huffman records
        for _ in 0..4 {
            record0.extend_from_slice(&0u32.to_be_bytes());
        }

        // EXTH flags
        record0.extend_from_slice(&0x50u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0u8; 32]);

        // Unknown index
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());

        // DRM
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0u8; 8]);

        // FDST
        let fdst_record = (self.records.len() - 4) as u32;
        record0.extend_from_slice(&fdst_record.to_be_bytes());
        let fdst_count = 1 + self.css_flows.len() as u32;
        record0.extend_from_slice(&fdst_count.to_be_bytes());

        // FCIS
        let fcis_record = (self.records.len() - 2) as u32;
        record0.extend_from_slice(&fcis_record.to_be_bytes());
        record0.extend_from_slice(&1u32.to_be_bytes());

        // FLIS
        let flis_record = (self.records.len() - 3) as u32;
        record0.extend_from_slice(&flis_record.to_be_bytes());
        record0.extend_from_slice(&1u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0u8; 8]);

        // SRCS
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0xFF; 8]);

        // Extra data flags
        record0.extend_from_slice(&1u32.to_be_bytes());

        // KF8 indices
        record0.extend_from_slice(&self.ncx_index.to_be_bytes());
        record0.extend_from_slice(&self.frag_index.to_be_bytes());
        record0.extend_from_slice(&self.skel_index.to_be_bytes());
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());

        // Unknown
        record0.extend_from_slice(&[0xFF; 4]);
        record0.extend_from_slice(&[0; 4]);
        record0.extend_from_slice(&[0xFF; 4]);
        record0.extend_from_slice(&[0; 4]);

        // EXTH
        record0.extend_from_slice(&exth);

        // Title
        record0.extend_from_slice(title_bytes);
        record0.extend_from_slice(&[0, 0]);

        // Padding
        while record0.len() < full_record_len + 4096 {
            record0.push(0);
        }

        self.records[0] = record0;
        Ok(())
    }

    fn build_exth(&self) -> Vec<u8> {
        let mut records: Vec<(u32, Vec<u8>)> = Vec::new();

        // Authors
        for author in &self.ctx.metadata.authors {
            records.push((100, author.as_bytes().to_vec()));
        }

        // Publisher
        if let Some(ref publisher) = self.ctx.metadata.publisher {
            records.push((101, publisher.as_bytes().to_vec()));
        }

        // Description
        if let Some(ref desc) = self.ctx.metadata.description {
            records.push((103, desc.as_bytes().to_vec()));
        }

        // Subjects
        for subject in &self.ctx.metadata.subjects {
            records.push((105, subject.as_bytes().to_vec()));
        }

        // Date
        if let Some(ref date) = self.ctx.metadata.date {
            records.push((106, date.as_bytes().to_vec()));
        }

        // Rights
        if let Some(ref rights) = self.ctx.metadata.rights {
            records.push((109, rights.as_bytes().to_vec()));
        }

        // Cover offset - find the index of the cover image in sorted image_hrefs
        if let Some(ref cover_path) = self.ctx.metadata.cover_image {
            if let Some(cover_idx) = self.image_hrefs.iter().position(|h| h == cover_path) {
                records.push((201, (cover_idx as u32).to_be_bytes().to_vec()));
            }
        }

        // Title
        records.push((503, self.ctx.metadata.title.as_bytes().to_vec()));

        // ASIN placeholder
        records.push((113, b"EBOK000000".to_vec()));

        // Document type
        records.push((501, b"EBOK".to_vec()));

        // CDE Type
        records.push((504, b"EBOK".to_vec()));

        // Build EXTH
        let mut exth = Vec::new();
        exth.extend_from_slice(b"EXTH");

        let mut content = Vec::new();
        content.extend_from_slice(&(records.len() as u32).to_be_bytes());
        for (rec_type, data) in &records {
            let rec_len = 8 + data.len() as u32;
            content.extend_from_slice(&rec_type.to_be_bytes());
            content.extend_from_slice(&rec_len.to_be_bytes());
            content.extend_from_slice(data);
        }

        // Pad to 4-byte boundary
        while !content.len().is_multiple_of(4) {
            content.push(0);
        }

        let header_len = 12 + content.len() as u32;
        exth.extend_from_slice(&header_len.to_be_bytes());
        exth.extend_from_slice(&content);

        exth
    }

    fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        // Calculate offsets
        let mut offsets = Vec::new();
        let pdb_header_size = 78 + 8 * self.records.len() + 2;
        let mut offset = pdb_header_size;

        for record in &self.records {
            offsets.push(offset as u32);
            offset += record.len();
        }

        // Write PDB header
        let title = sanitize_title(&self.ctx.metadata.title);
        let mut title_bytes = [0u8; 32];
        let title_slice = title.as_bytes();
        let copy_len = title_slice.len().min(31);
        title_bytes[..copy_len].copy_from_slice(&title_slice[..copy_len]);
        writer.write_all(&title_bytes)?;

        // Timestamps
        let now = crate::util::time_now_secs();
        writer.write_all(&0u16.to_be_bytes())?;
        writer.write_all(&0u16.to_be_bytes())?;
        writer.write_all(&now.to_be_bytes())?;
        writer.write_all(&now.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;

        // Type and Creator
        writer.write_all(b"BOOKMOBI")?;

        // UID seed, next record
        writer.write_all(&((2 * self.records.len() - 1) as u32).to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;

        // Number of records
        writer.write_all(&(self.records.len() as u16).to_be_bytes())?;

        // Record info list
        for (i, &offset) in offsets.iter().enumerate() {
            writer.write_all(&offset.to_be_bytes())?;
            let id_bytes = ((2 * i) as u32).to_be_bytes();
            writer.write_all(&[0, id_bytes[1], id_bytes[2], id_bytes[3]])?;
        }

        // Gap
        writer.write_all(&[0, 0])?;

        // Write records
        for record in &self.records {
            writer.write_all(record)?;
        }

        Ok(())
    }
}

/// Create a FONT record from raw font data.
fn write_font_record(data: &[u8]) -> io::Result<Vec<u8>> {
    let usize_val = data.len() as u32;
    let mut flags: u32 = 0;

    // Compress
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(data)?;
    let mut compressed = encoder.finish()?;
    flags |= 0b01;

    // XOR obfuscation
    let mut xor_key = Vec::new();
    if compressed.len() >= 1040 {
        flags |= 0b10;

        let seed = crate::util::time_seed_nanos();
        xor_key = (0..XOR_KEY_LEN)
            .map(|i| {
                let mut x = seed.wrapping_add(i as u64);
                x = x.wrapping_mul(6364136223846793005);
                x = x.wrapping_add(1442695040888963407);
                (x >> 33) as u8
            })
            .collect();

        for i in 0..1040.min(compressed.len()) {
            compressed[i] ^= xor_key[i % XOR_KEY_LEN];
        }
    }

    let key_start: u32 = 24;
    let data_start: u32 = key_start + xor_key.len() as u32;

    let mut record = Vec::with_capacity(24 + xor_key.len() + compressed.len());

    // Header
    record.extend_from_slice(b"FONT");
    record.extend_from_slice(&usize_val.to_be_bytes());
    record.extend_from_slice(&flags.to_be_bytes());
    record.extend_from_slice(&data_start.to_be_bytes());
    record.extend_from_slice(&(xor_key.len() as u32).to_be_bytes());
    record.extend_from_slice(&key_start.to_be_bytes());

    record.extend_from_slice(&xor_key);
    record.extend_from_slice(&compressed);

    Ok(record)
}

fn rand_uid() -> u32 {
    let seed = crate::util::time_seed_nanos() as u32;
    seed.wrapping_mul(1103515245).wrapping_add(12345)
}

fn sanitize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_' || *c == '-')
        .collect::<String>()
        .replace(' ', "_")
}

/// Flatten hierarchical TOC into linear list.
fn flatten_toc(
    entries: &[TocEntry],
    text_length: u32,
    id_map: &HashMap<(String, String), String>,
    aid_offset_map: &HashMap<String, (usize, usize, usize)>,
    filepos_map: &HashMap<String, Vec<(usize, String)>>,
) -> Vec<NcxBuildEntry> {
    struct TempEntry {
        pos: u32,
        length: u32,
        label: String,
        depth: u32,
        parent: i32,
        children: Vec<usize>,
    }

    let mut result: Vec<TempEntry> = Vec::new();

    fn flatten_recursive(
        entries: &[TocEntry],
        depth: u32,
        parent_idx: i32,
        text_length: u32,
        id_map: &HashMap<(String, String), String>,
        aid_offset_map: &HashMap<String, (usize, usize, usize)>,
        filepos_map: &HashMap<String, Vec<(usize, String)>>,
        result: &mut Vec<TempEntry>,
    ) {
        for entry in entries {
            let current_idx = result.len();

            let (file, fragment) = if let Some(hash_pos) = entry.href.find('#') {
                (
                    entry.href[..hash_pos].to_string(),
                    entry.href[hash_pos + 1..].to_string(),
                )
            } else {
                (entry.href.clone(), String::new())
            };

            // Try to find position for this TOC entry
            let pos = if fragment.starts_with("filepos") {
                // MOBI filepos anchor: parse position and find nearest aid
                resolve_filepos(&file, &fragment, filepos_map, aid_offset_map)
            } else {
                // Standard HTML anchor: lookup by element id
                id_map
                    .get(&(file.clone(), fragment.clone()))
                    .or_else(|| id_map.get(&(file.clone(), String::new())))
                    .and_then(|aid| aid_offset_map.get(aid))
                    .map(|&(_, _, off_text)| off_text as u32)
            }
            .unwrap_or(0);

            result.push(TempEntry {
                pos,
                length: text_length.saturating_sub(pos),
                label: entry.title.clone(),
                depth,
                parent: parent_idx,
                children: Vec::new(),
            });

            if parent_idx >= 0 {
                result[parent_idx as usize].children.push(current_idx);
            }

            flatten_recursive(
                &entry.children,
                depth + 1,
                current_idx as i32,
                text_length,
                id_map,
                aid_offset_map,
                filepos_map,
                result,
            );
        }
    }

    flatten_recursive(
        entries,
        0,
        -1,
        text_length,
        id_map,
        aid_offset_map,
        filepos_map,
        &mut result,
    );

    result
        .into_iter()
        .map(|e| NcxBuildEntry {
            pos: e.pos,
            length: e.length,
            label: e.label,
            depth: e.depth,
            parent: e.parent,
            first_child: e.children.first().map(|&i| i as i32).unwrap_or(-1),
            last_child: e.children.last().map(|&i| i as i32).unwrap_or(-1),
        })
        .collect()
}

/// Resolve MOBI filepos anchor to text position.
///
/// MOBI files use `#fileposNNN` anchors where NNN is a byte position in the
/// original HTML content. We use the filepos_map to find the aid that was
/// closest to that position, then look up the aid's position in the
/// transformed text.
fn resolve_filepos(
    file: &str,
    fragment: &str,
    filepos_map: &HashMap<String, Vec<(usize, String)>>,
    aid_offset_map: &HashMap<String, (usize, usize, usize)>,
) -> Option<u32> {
    // Parse the filepos number
    let filepos_str = fragment.strip_prefix("filepos")?;
    let target_pos: usize = filepos_str.parse().ok()?;

    // Get the position map for this file
    let positions = filepos_map.get(file)?;
    if positions.is_empty() {
        return None;
    }

    // Find the aid at or before target_pos using binary search
    // positions is sorted by original position (ascending)
    let idx = match positions.binary_search_by_key(&target_pos, |(pos, _)| *pos) {
        Ok(i) => i,                    // Exact match
        Err(i) => i.saturating_sub(1), // Use previous entry (largest <= target)
    };

    let (_, aid) = &positions[idx];

    // Look up the aid's position in the transformed text
    aid_offset_map
        .get(aid)
        .map(|&(_, _, off_text)| off_text as u32)
}

/// Resolve MOBI filepos anchor to (fid, offset) for link resolution.
///
/// Similar to resolve_filepos but returns the seq_num and offset_in_chunk
/// needed for kindle:pos:fid:XXXX:off:YYYYYY link format.
fn resolve_filepos_to_offset(
    file: &str,
    fragment: &str,
    filepos_map: &HashMap<String, Vec<(usize, String)>>,
    aid_offset_map: &HashMap<String, (usize, usize, usize)>,
) -> Option<(usize, usize)> {
    // Parse the filepos number
    let filepos_str = fragment.strip_prefix("filepos")?;
    let target_pos: usize = filepos_str.parse().ok()?;

    // Get the position map for this file
    let positions = filepos_map.get(file)?;
    if positions.is_empty() {
        return None;
    }

    // Find the aid at or before target_pos using binary search
    let idx = match positions.binary_search_by_key(&target_pos, |(pos, _)| *pos) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };

    let (_, aid) = &positions[idx];

    // Look up the aid's position - return (seq_num, offset_in_chunk)
    aid_offset_map
        .get(aid)
        .map(|&(seq_num, offset_in_chunk, _)| (seq_num, offset_in_chunk))
}

/// Guess media type from file extension.
fn guess_media_type(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "xhtml" | "html" | "htm" => "application/xhtml+xml".to_string(),
        "css" => "text/css".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "png" => "image/png".to_string(),
        "gif" => "image/gif".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "ttf" => "font/ttf".to_string(),
        "otf" => "font/otf".to_string(),
        "woff" => "font/woff".to_string(),
        "woff2" => "font/woff2".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title() {
        assert_eq!(sanitize_title("Hello World"), "Hello_World");
        assert_eq!(sanitize_title("Test <Book>"), "Test_Book");
    }

    #[test]
    fn test_resolve_filepos_exact_match() {
        let mut filepos_map = HashMap::new();
        filepos_map.insert(
            "content.html".to_string(),
            vec![
                (100, "0001".to_string()),
                (200, "0002".to_string()),
                (300, "0003".to_string()),
            ],
        );

        let mut aid_offset_map = HashMap::new();
        aid_offset_map.insert("0001".to_string(), (0, 50, 100));
        aid_offset_map.insert("0002".to_string(), (0, 150, 200));
        aid_offset_map.insert("0003".to_string(), (0, 250, 300));

        // Exact match at position 200
        let result = resolve_filepos("content.html", "filepos200", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some(200));
    }

    #[test]
    fn test_resolve_filepos_nearest_before() {
        let mut filepos_map = HashMap::new();
        filepos_map.insert(
            "content.html".to_string(),
            vec![
                (100, "0001".to_string()),
                (200, "0002".to_string()),
                (300, "0003".to_string()),
            ],
        );

        let mut aid_offset_map = HashMap::new();
        aid_offset_map.insert("0001".to_string(), (0, 50, 100));
        aid_offset_map.insert("0002".to_string(), (0, 150, 200));
        aid_offset_map.insert("0003".to_string(), (0, 250, 300));

        // Position 250 should resolve to aid at 200 (nearest before)
        let result = resolve_filepos("content.html", "filepos250", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some(200));
    }

    #[test]
    fn test_resolve_filepos_invalid_fragment() {
        let filepos_map = HashMap::new();
        let aid_offset_map = HashMap::new();

        // Invalid fragment (not starting with "filepos")
        let result = resolve_filepos("content.html", "anchor123", &filepos_map, &aid_offset_map);
        assert_eq!(result, None);

        // Invalid fragment (non-numeric after filepos)
        let result = resolve_filepos("content.html", "fileposXYZ", &filepos_map, &aid_offset_map);
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_filepos_to_offset() {
        let mut filepos_map = HashMap::new();
        filepos_map.insert(
            "content.html".to_string(),
            vec![(100, "0001".to_string()), (500, "0002".to_string())],
        );

        let mut aid_offset_map = HashMap::new();
        aid_offset_map.insert("0001".to_string(), (0, 50, 100));
        aid_offset_map.insert("0002".to_string(), (1, 25, 500));

        // Position 450 should resolve to aid at 100 (nearest before)
        let result =
            resolve_filepos_to_offset("content.html", "filepos450", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some((0, 50))); // seq_num=0, offset_in_chunk=50

        // Exact match at 500
        let result =
            resolve_filepos_to_offset("content.html", "filepos500", &filepos_map, &aid_offset_map);
        assert_eq!(result, Some((1, 25))); // seq_num=1, offset_in_chunk=25
    }
}
