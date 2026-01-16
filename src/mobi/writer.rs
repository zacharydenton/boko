//! MOBI/AZW3 Writer
//!
//! Creates KF8 (MOBI version 8) files from Book structures.

use std::io::{self, Write};
use std::path::Path;

use crate::book::Book;
use crate::error::Result;

use super::palmdoc;
use super::skeleton::{Chunker, ChunkerResult};

// Constants
const RECORD_SIZE: usize = 4096;
const NULL_INDEX: u32 = 0xFFFF_FFFF;

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
    chunker_result: Option<ChunkerResult>,
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
            chunker_result: None,
        };

        builder.build_text_records()?;
        builder.build_resource_records()?;
        builder.build_kf8_indices()?;
        builder.build_fdst_record()?;
        builder.build_flis_fcis_eof()?;
        builder.build_record0()?;

        Ok(builder)
    }

    fn build_text_records(&mut self) -> Result<()> {
        // Collect HTML files from spine
        let mut html_files: Vec<(String, Vec<u8>)> = Vec::new();
        for spine_item in &self.book.spine {
            if let Some(resource) = self.book.resources.get(&spine_item.href) {
                if resource.media_type == "application/xhtml+xml" {
                    html_files.push((spine_item.href.clone(), resource.data.clone()));
                }
            }
        }

        // Process with chunker (adds aids, prepares for KF8)
        let mut chunker = Chunker::new();
        let chunker_result = chunker.process(&html_files);
        self.text_length = chunker_result.text.len();

        // Split into records and compress
        let mut pos = 0;
        while pos < chunker_result.text.len() {
            let end = (pos + RECORD_SIZE).min(chunker_result.text.len());
            let chunk = &chunker_result.text[pos..end];

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
        Ok(())
    }

    fn build_kf8_indices(&mut self) -> Result<()> {
        // Build SKEL and Fragment INDX records
        // For now, just set indices - proper INDX generation is complex
        if self.chunker_result.is_some() {
            // Will be set when we add actual INDX record generation
            // self.skel_index = self.records.len() as u32;
            // self.records.push(build_skel_indx(...));
            // self.frag_index = self.records.len() as u32;
            // self.records.push(build_frag_indx(...));
        }
        Ok(())
    }

    fn build_resource_records(&mut self) -> Result<()> {
        // Add images
        let mut image_hrefs: Vec<_> = self
            .book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type.starts_with("image/"))
            .map(|(href, _)| href.clone())
            .collect();
        image_hrefs.sort();

        if !image_hrefs.is_empty() {
            self.first_resource_record = self.records.len() as u32;
        }

        for href in image_hrefs {
            if let Some(resource) = self.book.resources.get(&href) {
                self.records.push(resource.data.clone());
            }
        }

        Ok(())
    }

    fn build_fdst_record(&mut self) -> Result<()> {
        // FDST (Flow Descriptor Table) - single flow for our simple output
        let mut fdst = Vec::new();
        fdst.extend_from_slice(b"FDST");
        fdst.extend_from_slice(&12u32.to_be_bytes()); // Offset to flow table
        fdst.extend_from_slice(&1u32.to_be_bytes()); // Number of flows (1)
        fdst.extend_from_slice(&0u32.to_be_bytes()); // Flow 0 start
        fdst.extend_from_slice(&(self.text_length as u32).to_be_bytes()); // Flow 0 end

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
        record0.extend_from_slice(&1u32.to_be_bytes()); // FDST count

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
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes()); // NCX index
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes()); // Chunk index
        record0.extend_from_slice(&NULL_INDEX.to_be_bytes()); // Skel index
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title() {
        assert_eq!(sanitize_title("Hello World"), "Hello_World");
        assert_eq!(sanitize_title("Test <Book>"), "Test_Book");
    }
}
