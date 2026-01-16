//! KF8 Skeleton Chunking
//!
//! Implements the algorithm for breaking HTML content into skeleton + chunks
//! as used by Amazon's KF8/AZW3 format.
//!
//! The skeleton is the HTML structure with large content removed.
//! Chunks are the removed content pieces, each with an insert position.

use std::collections::HashMap;

/// Maximum size of a chunk in bytes
pub const CHUNK_SIZE: usize = 8192;

/// Tags that can receive aid attributes (based on Calibre's list)
pub const AID_ABLE_TAGS: &[&str] = &[
    "a", "abbr", "address", "article", "aside", "audio", "b", "bdo", "blockquote",
    "body", "button", "cite", "code", "dd", "del", "details", "dfn", "div", "dl",
    "dt", "em", "fieldset", "figcaption", "figure", "footer", "h1", "h2", "h3",
    "h4", "h5", "h6", "header", "hgroup", "i", "ins", "kbd", "label", "legend",
    "li", "map", "mark", "meter", "nav", "ol", "output", "p", "pre", "progress",
    "q", "rp", "rt", "samp", "section", "select", "small", "span", "strong",
    "sub", "summary", "sup", "textarea", "time", "ul", "var", "video",
];

/// A chunk of content extracted from the skeleton
#[derive(Debug, Clone)]
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
    /// Position of <body> tag in skeleton
    pub body_offset: usize,
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

    /// Rebuild the original HTML by inserting chunks at their positions
    pub fn rebuild(&self) -> Vec<u8> {
        let mut result = self.skeleton.clone();
        // Insert chunks in reverse order to preserve positions
        for chunk in self.chunks.iter().rev() {
            let pos = chunk.insert_pos.saturating_sub(self.start_pos);
            if pos <= result.len() {
                result.splice(pos..pos, chunk.raw.iter().cloned());
            }
        }
        result
    }
}

/// SKEL table entry (for INDX record)
#[derive(Debug, Clone)]
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
pub struct ChunkerResult {
    pub skeletons: Vec<Skeleton>,
    pub skel_table: Vec<SkelEntry>,
    pub chunk_table: Vec<ChunkEntry>,
    pub text: Vec<u8>,
}

/// Chunker - breaks HTML files into skeletons and chunks
pub struct Chunker {
    aid_counter: u32,
}

impl Chunker {
    pub fn new() -> Self {
        Self { aid_counter: 0 }
    }

    /// Generate a unique aid value
    fn next_aid(&mut self) -> String {
        let aid = to_base32(self.aid_counter);
        self.aid_counter += 1;
        aid
    }

    /// Process multiple HTML files into skeletons and chunks
    pub fn process(&mut self, html_files: &[(String, Vec<u8>)]) -> ChunkerResult {
        let mut skeletons = Vec::new();
        let mut start_pos = 0;

        for (i, (_filename, html)) in html_files.iter().enumerate() {
            let skeleton = self.process_file(i, html, start_pos);
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

        let mut chunk_table = Vec::new();
        let mut seq_num = 0;
        for skel in &skeletons {
            for chunk in &skel.chunks {
                chunk_table.push(ChunkEntry {
                    insert_pos: chunk.insert_pos,
                    selector: chunk.selector.clone(),
                    file_number: skel.file_number,
                    sequence_number: seq_num,
                    start_pos: chunk.start_pos,
                    length: chunk.raw.len(),
                });
                seq_num += 1;
            }
        }

        // Combine all text
        let text: Vec<u8> = skeletons.iter().flat_map(|s| s.raw_text()).collect();

        ChunkerResult {
            skeletons,
            skel_table,
            chunk_table,
            text,
        }
    }

    /// Process a single HTML file
    fn process_file(&mut self, file_number: usize, html: &[u8], start_pos: usize) -> Skeleton {
        // For now, simple implementation: add aids and don't chunk
        // A full implementation would parse HTML, add aids to aidable tags,
        // and break large content into chunks

        let html_str = String::from_utf8_lossy(html);
        let mut result = self.add_aid_attributes(&html_str);

        // Find body offset
        let body_offset = result.find("<body").unwrap_or(0);

        // For simple case: no chunking, entire body content is one chunk
        let skeleton_bytes = result.as_bytes().to_vec();

        Skeleton {
            file_number,
            skeleton: skeleton_bytes,
            chunks: Vec::new(), // No chunking for now
            start_pos,
            body_offset,
        }
    }

    /// Add aid attributes to aidable tags
    fn add_aid_attributes(&mut self, html: &str) -> String {
        use regex_lite::Regex;

        // Pattern to find opening tags of aidable elements
        let tag_pattern = format!(
            r"<({})\b([^>]*)>",
            AID_ABLE_TAGS.join("|")
        );
        let re = Regex::new(&tag_pattern).unwrap();

        re.replace_all(html, |caps: &regex_lite::Captures| {
            let tag = &caps[1];
            let attrs = &caps[2];

            // Skip if already has aid
            if attrs.contains("aid=") {
                return format!("<{}{}>", tag, attrs);
            }

            let aid = self.next_aid();
            if attrs.is_empty() {
                format!("<{} aid=\"{}\">", tag, aid)
            } else {
                format!("<{}{} aid=\"{}\">", tag, attrs, aid)
            }
        }).to_string()
    }
}

/// Convert number to base32 (using Kindle's encoding)
fn to_base32(mut n: u32) -> String {
    const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
    if n == 0 {
        return "0000".to_string();
    }

    let mut result = Vec::new();
    while n > 0 {
        result.push(CHARS[(n % 32) as usize]);
        n /= 32;
    }

    // Pad to at least 4 characters
    while result.len() < 4 {
        result.push(b'0');
    }

    result.reverse();
    String::from_utf8(result).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_base32() {
        assert_eq!(to_base32(0), "0000");
        assert_eq!(to_base32(1), "0001");
        assert_eq!(to_base32(31), "000V");
        assert_eq!(to_base32(32), "0010");
    }

    #[test]
    fn test_add_aids() {
        let mut chunker = Chunker::new();
        let html = "<html><body><p>Hello</p><div>World</div></body></html>";
        let result = chunker.add_aid_attributes(html);
        assert!(result.contains("aid=\"0000\""));
        assert!(result.contains("aid=\"0001\""));
    }
}
