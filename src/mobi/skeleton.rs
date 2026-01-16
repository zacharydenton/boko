//! KF8 Skeleton Chunking
//!
//! Implements the algorithm for breaking HTML content into skeleton + chunks
//! as used by Amazon's KF8/AZW3 format.
//!
//! The skeleton is the HTML structure with large content removed.
//! Chunks are the removed content pieces, each with an insert position.

use std::collections::HashMap;

/// Tags that can receive aid attributes (based on Calibre's list)
pub const AID_ABLE_TAGS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "article",
    "aside",
    "audio",
    "b",
    "bdo",
    "blockquote",
    "body",
    "button",
    "cite",
    "code",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "i",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "map",
    "mark",
    "meter",
    "nav",
    "ol",
    "output",
    "p",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "samp",
    "section",
    "select",
    "small",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "textarea",
    "time",
    "ul",
    "var",
    "video",
];

/// A chunk of content extracted from the skeleton
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields prepared for full chunking implementation
pub struct Chunk {
    /// The raw bytes of this chunk
    pub raw: Vec<u8>,
    /// Insert position in the skeleton (absolute byte offset)
    pub insert_pos: usize,
    /// Tags that start in this chunk (their aid values)
    pub starts_tags: Vec<String>,
    /// Tags that end in this chunk (their aid values)
    pub ends_tags: Vec<String>,
    /// Selector for this chunk (e.g., "P-//*[@aid='xxx']")
    pub selector: String,
    /// File number this chunk belongs to
    pub file_number: usize,
    /// Sequence number within all chunks
    pub sequence_number: usize,
    /// Start position within the file's chunks
    pub start_pos: usize,
}

/// A skeleton file with its associated chunks
#[derive(Debug)]
pub struct Skeleton {
    /// File number (index in spine)
    pub file_number: usize,
    /// The skeleton HTML (structure with content removed)
    pub skeleton: Vec<u8>,
    /// Chunks extracted from this skeleton
    pub chunks: Vec<Chunk>,
    /// Start position of this skeleton in the combined text
    pub start_pos: usize,
}

impl Skeleton {
    /// Total length of skeleton + all chunks
    pub fn len(&self) -> usize {
        self.skeleton.len() + self.chunks.iter().map(|c| c.raw.len()).sum::<usize>()
    }

    /// Get the raw text (skeleton + chunks concatenated)
    pub fn raw_text(&self) -> Vec<u8> {
        let mut result = self.skeleton.clone();
        for chunk in &self.chunks {
            result.extend_from_slice(&chunk.raw);
        }
        result
    }
}

/// SKEL table entry (for INDX record)
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields prepared for full chunking implementation
pub struct SkelEntry {
    pub file_number: usize,
    pub name: String,
    pub chunk_count: usize,
    pub start_pos: usize,
    pub length: usize,
}

/// Chunk table entry (for INDX record)
#[derive(Debug, Clone)]
pub struct ChunkEntry {
    pub insert_pos: usize,
    pub selector: String,
    pub file_number: usize,
    pub sequence_number: usize,
    pub start_pos: usize,
    pub length: usize,
}

/// Result of chunking operation
#[allow(dead_code)] // skeletons field prepared for full chunking implementation
pub struct ChunkerResult {
    pub skeletons: Vec<Skeleton>,
    pub skel_table: Vec<SkelEntry>,
    pub chunk_table: Vec<ChunkEntry>,
    pub text: Vec<u8>,
    /// Maps (file_href, anchor_id) -> aid for link resolution
    pub id_map: HashMap<(String, String), String>,
    /// Maps aid -> (chunk_sequence_number, offset_in_chunk, offset_in_text)
    pub aid_offset_map: HashMap<String, (usize, usize, usize)>,
}

/// Chunker - breaks HTML files into skeletons and chunks
pub struct Chunker {
    aid_counter: u32,
    /// Mapping of (file, id) -> aid built during processing
    id_map: HashMap<(String, String), String>,
}

impl Chunker {
    pub fn new() -> Self {
        Self {
            aid_counter: 0,
            id_map: HashMap::new(),
        }
    }

    /// Process multiple HTML files into skeletons and chunks
    pub fn process(&mut self, html_files: &[(String, Vec<u8>)]) -> ChunkerResult {
        let mut skeletons = Vec::new();
        let mut start_pos = 0;

        for (i, (file_href, html)) in html_files.iter().enumerate() {
            let skeleton = self.process_file(i, file_href, html, start_pos);
            start_pos += skeleton.len();
            skeletons.push(skeleton);
        }

        // Create tables
        let skel_table: Vec<SkelEntry> = skeletons
            .iter()
            .map(|s| SkelEntry {
                file_number: s.file_number,
                name: format!("SKEL{:010}", s.file_number),
                chunk_count: s.chunks.len(),
                start_pos: s.start_pos,
                length: s.skeleton.len(),
            })
            .collect();

        // Create virtual chunk entries to cover the entire text
        // Each chunk covers CHUNK_SIZE bytes for link resolution
        let mut chunk_table = Vec::new();
        let mut text_offset = 0usize;
        let mut seq_num = 0usize;

        for skel in &skeletons {
            // Create one chunk entry per skeleton file covering its content
            let skel_len = skel.skeleton.len();
            if skel_len > 0 {
                chunk_table.push(ChunkEntry {
                    insert_pos: text_offset,
                    selector: "P-//*[@aid='0000']".to_string(),
                    file_number: skel.file_number,
                    sequence_number: seq_num,
                    start_pos: 0,
                    length: skel_len,
                });
                seq_num += 1;
            }
            text_offset += skel_len;
        }

        // Combine all text
        let text: Vec<u8> = skeletons.iter().flat_map(|s| s.raw_text()).collect();

        // Build aid_offset_map by finding all aid attributes in the text
        let aid_offset_map = self.build_aid_offset_map(&text, &chunk_table);

        ChunkerResult {
            skeletons,
            skel_table,
            chunk_table,
            text,
            id_map: std::mem::take(&mut self.id_map),
            aid_offset_map,
        }
    }

    /// Build map of aid -> (chunk_sequence_number, offset_in_chunk, offset_in_text)
    fn build_aid_offset_map(
        &self,
        text: &[u8],
        chunk_table: &[ChunkEntry],
    ) -> HashMap<String, (usize, usize, usize)> {
        use memchr::memmem;

        let mut aid_offset_map = HashMap::new();
        let finder = memmem::Finder::new(b" aid=\"");
        let mut search_pos = 0;

        while let Some(rel_pos) = finder.find(&text[search_pos..]) {
            let offset = search_pos + rel_pos;
            let val_start = offset + 6; // len(" aid=\"") is 6

            // Validate we have enough bytes for 4-char ID + quote
            if val_start + 5 <= text.len() {
                // Extract 4-byte aid
                let aid_bytes = &text[val_start..val_start + 4];
                let quote = text[val_start + 4];

                if quote == b'"' {
                    let aid = String::from_utf8_lossy(aid_bytes).to_string();

                    // Find which chunk this offset is in
                    // Since we don't do real chunking yet, use a simple approach:
                    // sequence_number = 0 for first skeleton, offset_in_chunk = offset
                    let (seq_num, offset_in_chunk) = if chunk_table.is_empty() {
                        // No chunks, treat whole text as one chunk
                        (0usize, offset)
                    } else {
                        // Find the chunk containing this offset
                        let mut found_seq = 0usize;
                        let mut found_offset = offset;
                        for chunk in chunk_table {
                            let chunk_start = chunk.insert_pos;
                            let chunk_end = chunk_start + chunk.length;
                            if offset >= chunk_start && offset < chunk_end {
                                found_seq = chunk.sequence_number;
                                found_offset = offset - chunk_start;
                                break;
                            }
                        }
                        (found_seq, found_offset)
                    };

                    aid_offset_map.insert(aid, (seq_num, offset_in_chunk, offset));
                }
            }
            search_pos = val_start;
        }

        aid_offset_map
    }

    /// Process a single HTML file
    fn process_file(
        &mut self,
        file_number: usize,
        file_href: &str,
        html: &[u8],
        start_pos: usize,
    ) -> Skeleton {
        // Simple implementation: add aids, no actual chunking
        // Full HTML goes to skeleton, chunks are empty (content stays in skeleton)

        // Use fast path from writer_transform
        let result = super::writer_transform::add_aid_attributes_fast(
            html,
            file_href,
            &mut self.aid_counter,
            &mut self.id_map,
        );

        Skeleton {
            file_number,
            skeleton: result,
            chunks: Vec::new(), // No chunking - content stays in skeleton
            start_pos,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse Kindle base32 encoding back to number
    /// Kindle uses custom base32: 0-9 (0-9), A-V (10-31)
    pub(super) fn from_base32(s: &str) -> u32 {
        let mut result = 0u32;
        for c in s.chars() {
            result = result.saturating_mul(32);
            let val = match c {
                '0'..='9' => c as u32 - '0' as u32,
                'A'..='V' => c as u32 - 'A' as u32 + 10,
                'a'..='v' => c as u32 - 'a' as u32 + 10,
                _ => continue,
            };
            result = result.saturating_add(val);
        }
        result
    }

    #[test]
    fn test_from_base32() {
        assert_eq!(from_base32("0000"), 0);
        assert_eq!(from_base32("0001"), 1);
        assert_eq!(from_base32("000V"), 31);
        assert_eq!(from_base32("0010"), 32);
    }

    #[test]
    fn test_add_aids() {
        use crate::mobi::writer_transform::add_aid_attributes_fast;
        let mut chunker = Chunker::new();
        let html = b"<html><body><p>Hello</p><div>World</div></body></html>";
        let result = add_aid_attributes_fast(html, "test.xhtml", &mut chunker.aid_counter, &mut chunker.id_map);
        let result_str = String::from_utf8_lossy(&result);
        assert!(result_str.contains("aid=\"0000\""));
        assert!(result_str.contains("aid=\"0001\""));
    }
}


