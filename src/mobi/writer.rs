//! MOBI/AZW3 Writer
//!
//! Creates KF8 (MOBI version 8) files from Book structures.

use std::io::{self, Write};
use std::path::Path;

use crate::book::Book;
use crate::error::Result;

use super::index::{build_skel_indx, build_chunk_indx, build_cncx, calculate_cncx_offsets, build_ncx_indx, NcxBuildEntry};
use super::palmdoc;
use super::skeleton::{Chunker, ChunkerResult};

use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::collections::HashMap;

// Constants
const RECORD_SIZE: usize = 4096;
const NULL_INDEX: u32 = 0xFFFF_FFFF;
const XOR_KEY_LEN: usize = 20;

/// Create a FONT record from raw font data
/// Format: 24-byte header + XOR key (20 bytes) + compressed/obfuscated data
fn write_font_record(data: &[u8]) -> Vec<u8> {
    use std::io::Write as IoWrite;

    let usize_val = data.len() as u32;
    let mut flags: u32 = 0;

    // Step 1: Zlib compress the data
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).unwrap();
    let mut compressed = encoder.finish().unwrap();
    flags |= 0b01; // Compression flag

    // Step 2: XOR obfuscation (only if data >= 1040 bytes)
    let mut xor_key = Vec::new();
    if compressed.len() >= 1040 {
        flags |= 0b10; // XOR obfuscation flag

        // Generate random XOR key (use timestamp-based pseudo-random)
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(12345);
        xor_key = (0..XOR_KEY_LEN)
            .map(|i| {
                let mut x = seed.wrapping_add(i as u64);
                x = x.wrapping_mul(6364136223846793005);
                x = x.wrapping_add(1442695040888963407);
                (x >> 33) as u8
            })
            .collect();

        // XOR first 1040 bytes
        for i in 0..1040.min(compressed.len()) {
            compressed[i] ^= xor_key[i % XOR_KEY_LEN];
        }
    }

    // Step 3: Build the FONT record
    let key_start: u32 = 24; // Header is 24 bytes
    let data_start: u32 = key_start + xor_key.len() as u32;

    let mut record = Vec::with_capacity(24 + xor_key.len() + compressed.len());

    // Header: FONT + 5 big-endian u32s
    record.extend_from_slice(b"FONT");
    record.extend_from_slice(&usize_val.to_be_bytes());
    record.extend_from_slice(&flags.to_be_bytes());
    record.extend_from_slice(&data_start.to_be_bytes());
    record.extend_from_slice(&(xor_key.len() as u32).to_be_bytes());
    record.extend_from_slice(&key_start.to_be_bytes());

    // XOR key (if present)
    record.extend_from_slice(&xor_key);

    // Compressed (and possibly obfuscated) data
    record.extend_from_slice(&compressed);

    record
}

/// Convert a number to base32 with minimum 4 digits (0-9A-V)
fn to_base32(mut num: usize) -> String {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
    let mut result = Vec::new();

    if num == 0 {
        result.push(b'0');
    } else {
        while num > 0 {
            result.push(DIGITS[num % 32]);
            num /= 32;
        }
    }

    // Pad to 4 digits minimum
    while result.len() < 4 {
        result.push(b'0');
    }

    result.reverse();
    String::from_utf8(result).unwrap()
}

/// Write a Book to a MOBI/AZW3 file
pub fn write_mobi<P: AsRef<Path>>(book: &Book, path: P) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut writer = io::BufWriter::new(file);
    write_mobi_to_writer(book, &mut writer)
}

/// Write a Book to any Write destination
pub fn write_mobi_to_writer<W: Write>(book: &Book, writer: &mut W) -> Result<()> {
    let mobi = MobiBuilder::new(book)?;
    mobi.write(writer)
}

struct MobiBuilder<'a> {
    book: &'a Book,
    records: Vec<Vec<u8>>,
    text_length: usize,
    last_text_record: u16,
    first_resource_record: u32,
    skel_index: u32,
    frag_index: u32,
    ncx_index: u32,
    chunker_result: Option<ChunkerResult>,
    /// Maps resource href to 1-indexed resource record number (for kindle:embed references)
    resource_map: HashMap<String, usize>,
    /// CSS flows (flow 0 is text, flows 1+ are CSS)
    css_flows: Vec<String>,
    /// Total flows length (text + CSS)
    flows_length: usize,
    /// Ordered list of image hrefs (for writing after text)
    image_hrefs: Vec<String>,
    /// Ordered list of font hrefs (for writing after images)
    font_hrefs: Vec<String>,
}

impl<'a> MobiBuilder<'a> {
    fn new(book: &'a Book) -> Result<Self> {
        let mut builder = Self {
            book,
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
        };

        builder.collect_resources()?;      // Build resource_map (no records yet)
        builder.build_text_records()?;     // Text records 1-N (uses resource_map)
        builder.write_resource_records()?; // Resource records after text
        builder.build_kf8_indices()?;
        builder.build_fdst_record()?;
        builder.build_flis_fcis_eof()?;
        builder.build_record0()?;

        Ok(builder)
    }

    fn build_text_records(&mut self) -> Result<()> {
        // Build CSS href -> flow index map (flow 0 is text, CSS starts at 1)
        // MUST be sorted to match the order in collect_resources() / css_flows
        let mut css_hrefs: Vec<_> = self
            .book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .map(|(href, _)| href.clone())
            .collect();
        css_hrefs.sort();

        let mut css_flow_map: HashMap<String, usize> = HashMap::new();
        for (i, href) in css_hrefs.iter().enumerate() {
            css_flow_map.insert(href.clone(), i + 1); // Flows are 1-indexed (0 is text)
        }

        // Collect and process HTML files from spine
        let mut html_files: Vec<(String, Vec<u8>)> = Vec::new();
        for spine_item in &self.book.spine {
            if let Some(resource) = self.book.resources.get(&spine_item.href) {
                if resource.media_type == "application/xhtml+xml" {
                    // Rewrite HTML to use kindle: references
                    let html = String::from_utf8_lossy(&resource.data);
                    let rewritten = self.rewrite_html_references(&html, &spine_item.href, &css_flow_map);
                    html_files.push((spine_item.href.clone(), rewritten.into_bytes()));
                }
            }
        }

        // Rewrite CSS to use kindle:embed references for fonts/images
        let rewritten_css: Vec<String> = self
            .css_flows
            .iter()
            .map(|css| self.rewrite_css_references(css))
            .collect();

        // Process HTML with chunker (adds aids, prepares for KF8)
        let mut chunker = Chunker::new();
        let chunker_result = chunker.process(&html_files);
        self.text_length = chunker_result.text.len();

        // Build combined flow data: text (flow 0) + CSS flows (1+)
        let mut all_flows = chunker_result.text.clone();
        for css in &rewritten_css {
            all_flows.extend_from_slice(css.as_bytes());
        }
        self.flows_length = all_flows.len();

        // Split into records and compress
        let mut pos = 0;
        while pos < all_flows.len() {
            let end = (pos + RECORD_SIZE).min(all_flows.len());
            let chunk = &all_flows[pos..end];

            // Compress with PalmDOC
            let compressed = palmdoc::compress(chunk);

            // Add trailing byte (overlap byte for multibyte chars)
            let mut record = compressed;
            record.push(0);

            self.records.push(record);
            pos = end;
        }

        self.last_text_record = (self.records.len() - 1) as u16;
        self.chunker_result = Some(chunker_result);

        // Store flow boundaries for FDST
        // Flow 0: text (0 to text_length)
        // Flow 1+: CSS flows
        self.css_flows = rewritten_css;

        Ok(())
    }

    /// Rewrite HTML references to use kindle: URLs
    fn rewrite_html_references(
        &self,
        html: &str,
        html_href: &str,
        css_flow_map: &HashMap<String, usize>,
    ) -> String {
        use regex_lite::Regex;

        let mut result = html.to_string();

        // Get the directory of this HTML file for resolving relative paths
        let base_dir = std::path::Path::new(html_href)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Rewrite <link href="..."> to kindle:flow references
        let link_re = Regex::new(r#"<link\s+[^>]*href\s*=\s*["']([^"']+)["'][^>]*>"#).unwrap();
        result = link_re
            .replace_all(&result, |caps: &regex_lite::Captures| {
                let href = &caps[1];
                let full_tag = &caps[0];

                // Resolve relative path
                let resolved = resolve_href(&base_dir, href);

                if let Some(&flow_idx) = css_flow_map.get(&resolved) {
                    let flow_ref = to_base32(flow_idx);
                    full_tag.replace(href, &format!("kindle:flow:{}?mime=text/css", flow_ref))
                } else {
                    full_tag.to_string()
                }
            })
            .to_string();

        // Rewrite <img src="..."> to kindle:embed references
        let img_re = Regex::new(r#"<img\s+[^>]*src\s*=\s*["']([^"']+)["']"#).unwrap();
        result = img_re
            .replace_all(&result, |caps: &regex_lite::Captures| {
                let src = &caps[1];
                let full_match = &caps[0];

                // Resolve relative path
                let resolved = resolve_href(&base_dir, src);

                if let Some(&res_idx) = self.resource_map.get(&resolved) {
                    let embed_ref = to_base32(res_idx);
                    // Get MIME type
                    let mime = self
                        .book
                        .resources
                        .get(&resolved)
                        .map(|r| r.media_type.as_str())
                        .unwrap_or("image/jpeg");
                    full_match.replace(src, &format!("kindle:embed:{}?mime={}", embed_ref, mime))
                } else {
                    full_match.to_string()
                }
            })
            .to_string();

        // Rewrite <a href="..."> internal links to fragment-only references
        // In KF8, all content is merged, so file.xhtml#anchor becomes #anchor
        let anchor_re = Regex::new(r#"<a\s+([^>]*)href\s*=\s*["']([^"']+)["']([^>]*)>"#).unwrap();
        result = anchor_re
            .replace_all(&result, |caps: &regex_lite::Captures| {
                let before = &caps[1];
                let href = &caps[2];
                let after = &caps[3];

                // Skip external links and already-fragment links
                if href.starts_with("http") || href.starts_with("mailto:") || href.starts_with("#") {
                    return caps[0].to_string();
                }

                // Extract fragment if present (file.xhtml#anchor -> #anchor)
                let new_href = if let Some(hash_pos) = href.find('#') {
                    &href[hash_pos..] // Keep just #anchor
                } else {
                    // No fragment - link to file start. Use file name as anchor.
                    // The skeleton/chunker should have added filepos markers
                    "#"
                };

                format!("<a {}href=\"{}\"{}>", before, new_href, after)
            })
            .to_string();

        result
    }

    /// Rewrite CSS url() references to kindle:embed
    fn rewrite_css_references(&self, css: &str) -> String {
        use regex_lite::Regex;

        let mut result = css.to_string();

        // Rewrite url(...) references
        let url_re = Regex::new(r#"url\s*\(\s*["']?([^"')]+)["']?\s*\)"#).unwrap();
        result = url_re
            .replace_all(&result, |caps: &regex_lite::Captures| {
                let url = &caps[1];

                // Skip data: URLs and external URLs
                if url.starts_with("data:") || url.starts_with("http") {
                    return caps[0].to_string();
                }

                // Try to find the resource
                // CSS files might reference ../fonts/foo.ttf or similar
                let normalized = url.trim_start_matches("../").trim_start_matches("./");

                // Try different path patterns
                for href in self.resource_map.keys() {
                    if href.ends_with(normalized) || href == normalized {
                        let res_idx = self.resource_map[href];
                        let embed_ref = to_base32(res_idx);
                        return format!("url(kindle:embed:{})", embed_ref);
                    }
                }

                // Not found, keep original
                caps[0].to_string()
            })
            .to_string();

        result
    }

    fn build_kf8_indices(&mut self) -> Result<()> {
        // Build SKEL and Fragment INDX records
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
                // Build CNCX for chunk selectors
                let selectors: Vec<String> = chunker_result.chunk_table
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

                // Add CNCX record after chunk index records
                if !cncx.is_empty() {
                    self.records.push(cncx);
                }
            }
        }

        // Build NCX index for table of contents
        if !self.book.toc.is_empty() {
            // Flatten TOC entries (including children) into a list with depth
            let ncx_entries = flatten_toc(&self.book.toc, 0, self.text_length as u32);

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

    /// Phase 1: Collect resources and build resource_map (before text records)
    /// This populates resource_map for kindle:embed reference rewriting
    fn collect_resources(&mut self) -> Result<()> {
        // Collect images
        self.image_hrefs = self
            .book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type.starts_with("image/"))
            .map(|(href, _)| href.clone())
            .collect();
        self.image_hrefs.sort();

        // Collect fonts
        self.font_hrefs = self
            .book
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

        // Collect CSS (for flow tracking - actual CSS goes in text flows)
        let mut css_hrefs: Vec<_> = self
            .book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .map(|(href, _)| href.clone())
            .collect();
        css_hrefs.sort();

        // Store CSS flows (will be appended to text)
        for href in &css_hrefs {
            if let Some(resource) = self.book.resources.get(href) {
                let css = String::from_utf8_lossy(&resource.data).to_string();
                self.css_flows.push(css);
            }
        }

        // Build resource_map with indices (1-indexed for kindle:embed)
        // Resources will be written after text records, but we need indices now
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

    /// Phase 2: Write resource records (after text records)
    fn write_resource_records(&mut self) -> Result<()> {
        // Set first resource record (now that text records are written)
        if !self.image_hrefs.is_empty() || !self.font_hrefs.is_empty() {
            self.first_resource_record = self.records.len() as u32;
        }

        // Write images as raw data
        for href in &self.image_hrefs.clone() {
            if let Some(resource) = self.book.resources.get(href) {
                self.records.push(resource.data.clone());
            }
        }

        // Write fonts as FONT records
        for href in &self.font_hrefs.clone() {
            if let Some(resource) = self.book.resources.get(href) {
                let font_record = write_font_record(&resource.data);
                self.records.push(font_record);
            }
        }

        Ok(())
    }

    fn build_fdst_record(&mut self) -> Result<()> {
        // FDST (Flow Descriptor Table) - supports multiple flows
        // Flow 0: text content
        // Flows 1+: CSS stylesheets

        let num_flows = 1 + self.css_flows.len(); // text + CSS flows

        let mut fdst = Vec::new();
        fdst.extend_from_slice(b"FDST");
        fdst.extend_from_slice(&12u32.to_be_bytes()); // Offset to flow table
        fdst.extend_from_slice(&(num_flows as u32).to_be_bytes());

        // Flow 0: text (0 to text_length)
        fdst.extend_from_slice(&0u32.to_be_bytes());
        fdst.extend_from_slice(&(self.text_length as u32).to_be_bytes());

        // Flow 1+: CSS flows
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

    fn build_flis_fcis_eof(&mut self) -> Result<()> {
        // FLIS record
        let flis = b"FLIS\0\0\0\x08\0\x41\0\0\0\0\0\0\xff\xff\xff\xff\0\x01\0\x03\0\0\0\x03\0\0\0\x01\xff\xff\xff\xff";
        self.records.push(flis.to_vec());

        // FCIS record
        let mut fcis = Vec::new();
        fcis.extend_from_slice(b"FCIS\x00\x00\x00\x14\x00\x00\x00\x10\x00\x00\x00\x02\x00\x00\x00\x00");
        fcis.extend_from_slice(&(self.text_length as u32).to_be_bytes());
        fcis.extend_from_slice(b"\x00\x00\x00\x00\x00\x00\x00\x28\x00\x00\x00\x00\x00\x00\x00");
        fcis.extend_from_slice(b"\x28\x00\x00\x00\x08\x00\x01\x00\x01\x00\x00\x00\x00");
        self.records.push(fcis);

        // EOF record
        self.records.push(b"\xe9\x8e\r\n".to_vec());

        Ok(())
    }

    fn build_record0(&mut self) -> Result<()> {
        let title = &self.book.metadata.title;
        let title_bytes = title.as_bytes();

        // Build EXTH header
        let exth = self.build_exth();
        let exth_len = exth.len();

        // Calculate offsets
        let mobi_header_len: u32 = 264;
        let title_offset = 16 + mobi_header_len + exth_len as u32;
        let full_record_len = title_offset as usize + title_bytes.len() + 2; // +2 for null padding

        let mut record0 = Vec::with_capacity(full_record_len + 8192); // Include padding

        // PalmDOC header (16 bytes)
        record0.extend_from_slice(&2u16.to_be_bytes()); // Compression: PalmDOC
        record0.extend_from_slice(&[0, 0]); // Unused
        record0.extend_from_slice(&(self.text_length as u32).to_be_bytes());
        record0.extend_from_slice(&self.last_text_record.to_be_bytes());
        record0.extend_from_slice(&(RECORD_SIZE as u16).to_be_bytes());
        record0.extend_from_slice(&0u16.to_be_bytes()); // Encryption: none
        record0.extend_from_slice(&0u16.to_be_bytes()); // Unused

        // MOBI header
        record0.extend_from_slice(b"MOBI");
        record0.extend_from_slice(&mobi_header_len.to_be_bytes()); // Header length
        record0.extend_from_slice(&2u32.to_be_bytes()); // Book type
        record0.extend_from_slice(&65001u32.to_be_bytes()); // UTF-8 encoding
        record0.extend_from_slice(&rand_uid().to_be_bytes()); // UID
        record0.extend_from_slice(&8u32.to_be_bytes()); // File version (KF8)

        // Meta indices (40-80)
        for _ in 0..10 {
            record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        }

        // First non-text record (80)
        let first_non_text = (self.last_text_record as u32) + 1;
        record0.extend_from_slice(&first_non_text.to_be_bytes());

        // Title offset (84)
        record0.extend_from_slice(&title_offset.to_be_bytes());

        // Title length (88)
        record0.extend_from_slice(&(title_bytes.len() as u32).to_be_bytes());

        // Language code (92) - English
        record0.extend_from_slice(&0x09u32.to_be_bytes());

        // Dictionary in/out lang (96-104)
        record0.extend_from_slice(&0u32.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Min version (104)
        record0.extend_from_slice(&8u32.to_be_bytes());

        // First resource record (108)
        record0.extend_from_slice(&self.first_resource_record.to_be_bytes());

        // Huffman records (112-128)
        for _ in 0..4 {
            record0.extend_from_slice(&0u32.to_be_bytes());
        }

        // EXTH flags (128)
        record0.extend_from_slice(&0x50u32.to_be_bytes()); // Has EXTH

        // Unknown (132-164)
        record0.extend_from_slice(&[0u8; 32]);

        // Unknown index (164)
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());

        // DRM (168-184)
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes()); // DRM offset
        record0.extend_from_slice(&0u32.to_be_bytes()); // DRM count
        record0.extend_from_slice(&0u32.to_be_bytes()); // DRM size
        record0.extend_from_slice(&0u32.to_be_bytes()); // DRM flags

        // Unknown (184-192)
        record0.extend_from_slice(&[0u8; 8]);

        // FDST (192-200)
        let fdst_record = (self.records.len() - 4) as u32; // FDST is 4 before end
        record0.extend_from_slice(&fdst_record.to_be_bytes());
        let fdst_count = 1 + self.css_flows.len() as u32; // 1 text flow + N CSS flows
        record0.extend_from_slice(&fdst_count.to_be_bytes());

        // FCIS (200-208)
        let fcis_record = (self.records.len() - 2) as u32;
        record0.extend_from_slice(&fcis_record.to_be_bytes());
        record0.extend_from_slice(&1u32.to_be_bytes());

        // FLIS (208-216)
        let flis_record = (self.records.len() - 3) as u32;
        record0.extend_from_slice(&flis_record.to_be_bytes());
        record0.extend_from_slice(&1u32.to_be_bytes());

        // Unknown (216-224)
        record0.extend_from_slice(&[0u8; 8]);

        // SRCS (224-232)
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes());
        record0.extend_from_slice(&0u32.to_be_bytes());

        // Unknown (232-240)
        record0.extend_from_slice(&[0xFF; 8]);

        // Extra data flags (240)
        record0.extend_from_slice(&1u32.to_be_bytes()); // Multibyte overlap

        // KF8 indices (244-264)
        record0.extend_from_slice(&self.ncx_index.to_be_bytes()); // NCX index
        record0.extend_from_slice(&self.frag_index.to_be_bytes()); // Chunk/Fragment index
        record0.extend_from_slice(&self.skel_index.to_be_bytes()); // Skel index
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes()); // DATP index
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes()); // Guide index

        // Unknown (264-280)
        record0.extend_from_slice(&[0xFF; 4]);
        record0.extend_from_slice(&[0; 4]);
        record0.extend_from_slice(&[0xFF; 4]);
        record0.extend_from_slice(&[0; 4]);

        // EXTH header
        record0.extend_from_slice(&exth);

        // Full title
        record0.extend_from_slice(title_bytes);
        record0.extend_from_slice(&[0, 0]); // Null terminator + padding

        // Padding (for Amazon's DTP service)
        while record0.len() < full_record_len + 4096 {
            record0.push(0);
        }

        self.records[0] = record0;
        Ok(())
    }

    fn build_exth(&self) -> Vec<u8> {
        let mut exth = Vec::new();
        let mut records: Vec<(u32, Vec<u8>)> = Vec::new();

        // Authors (100)
        for author in &self.book.metadata.authors {
            records.push((100, author.as_bytes().to_vec()));
        }

        // Publisher (101)
        if let Some(ref publisher) = self.book.metadata.publisher {
            records.push((101, publisher.as_bytes().to_vec()));
        }

        // Description (103)
        if let Some(ref desc) = self.book.metadata.description {
            records.push((103, desc.as_bytes().to_vec()));
        }

        // Subjects (105)
        for subject in &self.book.metadata.subjects {
            records.push((105, subject.as_bytes().to_vec()));
        }

        // Publication date (106)
        if let Some(ref date) = self.book.metadata.date {
            records.push((106, date.as_bytes().to_vec()));
        }

        // Rights (109)
        if let Some(ref rights) = self.book.metadata.rights {
            records.push((109, rights.as_bytes().to_vec()));
        }

        // Cover offset (201) - if we have a cover
        if self.book.metadata.cover_image.is_some() {
            records.push((201, 0u32.to_be_bytes().to_vec()));
        }

        // Updated title (503)
        records.push((503, self.book.metadata.title.as_bytes().to_vec()));

        // ASIN placeholder (113)
        records.push((113, b"EBOK000000".to_vec()));

        // Document type (501)
        records.push((501, b"EBOK".to_vec()));

        // CDE Type (504)
        records.push((504, b"EBOK".to_vec()));

        // Build EXTH
        exth.extend_from_slice(b"EXTH");

        // Calculate header length
        let mut content = Vec::new();
        content.extend_from_slice(&(records.len() as u32).to_be_bytes());
        for (rec_type, data) in &records {
            let rec_len = 8 + data.len() as u32;
            content.extend_from_slice(&rec_type.to_be_bytes());
            content.extend_from_slice(&rec_len.to_be_bytes());
            content.extend_from_slice(data);
        }

        // Pad to 4-byte boundary
        while content.len() % 4 != 0 {
            content.push(0);
        }

        let header_len = 12 + content.len() as u32;
        exth.extend_from_slice(&header_len.to_be_bytes());
        exth.extend_from_slice(&content);

        exth
    }

    fn generate_html(&self) -> String {
        let mut html = String::new();

        html.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        html.push_str("<!DOCTYPE html>\n");
        html.push_str("<html xmlns=\"http://www.w3.org/1999/xhtml\">\n");
        html.push_str("<head>\n");
        html.push_str(&format!(
            "<title>{}</title>\n",
            escape_xml(&self.book.metadata.title)
        ));
        html.push_str("<style type=\"text/css\">\n");
        html.push_str("body { font-family: serif; }\n");
        html.push_str("</style>\n");
        html.push_str("</head>\n");
        html.push_str("<body>\n");

        // Add each spine item's content
        for spine_item in &self.book.spine {
            if let Some(resource) = self.book.resources.get(&spine_item.href) {
                if resource.media_type == "application/xhtml+xml" {
                    let content = String::from_utf8_lossy(&resource.data);
                    // Extract body content
                    if let Some(body_start) = content.find("<body") {
                        if let Some(body_tag_end) = content[body_start..].find('>') {
                            let after_body = body_start + body_tag_end + 1;
                            if let Some(body_end) = content[after_body..].rfind("</body>") {
                                let body_content = &content[after_body..after_body + body_end];
                                html.push_str(body_content);
                                html.push_str("\n");
                            }
                        }
                    }
                }
            }
        }

        html.push_str("</body>\n");
        html.push_str("</html>\n");

        html
    }

    fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Calculate record offsets
        let mut offsets = Vec::new();
        let pdb_header_size = 78 + 8 * self.records.len() + 2;
        let mut offset = pdb_header_size;

        for record in &self.records {
            offsets.push(offset as u32);
            offset += record.len();
        }

        // Write PDB header
        let title = sanitize_title(&self.book.metadata.title);
        let mut title_bytes = [0u8; 32];
        let title_slice = title.as_bytes();
        let copy_len = title_slice.len().min(31);
        title_bytes[..copy_len].copy_from_slice(&title_slice[..copy_len]);
        writer.write_all(&title_bytes)?;

        // Attributes, version, creation/modification dates
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        writer.write_all(&0u16.to_be_bytes())?; // Attributes
        writer.write_all(&0u16.to_be_bytes())?; // Version
        writer.write_all(&now.to_be_bytes())?; // Creation
        writer.write_all(&now.to_be_bytes())?; // Modification
        writer.write_all(&0u32.to_be_bytes())?; // Last backup
        writer.write_all(&0u32.to_be_bytes())?; // Modification number
        writer.write_all(&0u32.to_be_bytes())?; // App info
        writer.write_all(&0u32.to_be_bytes())?; // Sort info

        // Type and Creator
        writer.write_all(b"BOOKMOBI")?;

        // Unique ID seed, next record list ID
        writer.write_all(&((2 * self.records.len() - 1) as u32).to_be_bytes())?;
        writer.write_all(&0u32.to_be_bytes())?;

        // Number of records
        writer.write_all(&(self.records.len() as u16).to_be_bytes())?;

        // Record info list
        for (i, &offset) in offsets.iter().enumerate() {
            writer.write_all(&offset.to_be_bytes())?;
            // Record attributes (unique ID)
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

fn rand_uid() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(12345);
    // Simple LCG
    seed.wrapping_mul(1103515245).wrapping_add(12345)
}

fn sanitize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_' || *c == '-')
        .collect::<String>()
        .replace(' ', "_")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Resolve a relative href against a base directory
fn resolve_href(base_dir: &str, href: &str) -> String {
    // Handle absolute paths
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_string();
    }

    // Handle ../ paths
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };

    for segment in href.split('/') {
        match segment {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }

    parts.join("/")
}

/// Flatten a hierarchical TOC into a linear list with depth info
fn flatten_toc(entries: &[crate::book::TocEntry], depth: u32, text_length: u32) -> Vec<NcxBuildEntry> {
    let mut result = Vec::new();

    for entry in entries {
        result.push(NcxBuildEntry {
            pos: 0, // Simplified: all entries point to start
            length: text_length,
            label: entry.title.clone(),
            depth,
        });

        // Recursively add children
        result.extend(flatten_toc(&entry.children, depth + 1, text_length));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title() {
        assert_eq!(sanitize_title("Hello World"), "Hello_World");
        assert_eq!(sanitize_title("Test <Book>"), "Test_Book");
    }
}
