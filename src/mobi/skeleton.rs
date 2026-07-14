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
pub struct Chunk {
    /// The raw bytes of this chunk
    pub raw: Vec<u8>,
    /// Insert position in the skeleton (absolute byte offset)
    pub insert_pos: usize,
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

    /// Get the raw text (skeleton + chunks concatenated). This is the
    /// on-disk layout that ends up in rawML records.
    pub fn raw_text(&self) -> Vec<u8> {
        let mut result = self.skeleton.clone();
        for chunk in &self.chunks {
            result.extend_from_slice(&chunk.raw);
        }
        result
    }

    /// Get the reassembled file content (chunks inserted into the skeleton
    /// at their `insert_pos`). This is what Kindle materialises when
    /// rendering a part; `chunk_table.insert_pos` and `pos_fid` offsets are
    /// in *this* coordinate space, not in the rawML layout.
    pub fn rebuild(&self) -> Vec<u8> {
        // Sort chunks by their position-within-skel so multiple chunks per
        // skel insert in order. (In the current writer there's always one
        // chunk per skel, but the math is the same.)
        let mut chunks: Vec<&Chunk> = self.chunks.iter().collect();
        chunks.sort_by_key(|c| c.insert_pos);

        let mut result = Vec::with_capacity(
            self.skeleton.len() + chunks.iter().map(|c| c.raw.len()).sum::<usize>(),
        );
        let mut skel_cursor = 0;
        for chunk in chunks {
            let rel_pos = chunk.insert_pos.saturating_sub(self.start_pos);
            let rel_pos = rel_pos.min(self.skeleton.len());
            result.extend_from_slice(&self.skeleton[skel_cursor..rel_pos]);
            result.extend_from_slice(&chunk.raw);
            skel_cursor = rel_pos;
        }
        result.extend_from_slice(&self.skeleton[skel_cursor..]);
        result
    }
}

/// SKEL table entry (for INDX record)
#[derive(Debug, Clone)]
pub struct SkelEntry {
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
    pub skel_table: Vec<SkelEntry>,
    pub chunk_table: Vec<ChunkEntry>,
    pub text: Vec<u8>,
    /// Maps (file_href, anchor_id) -> aid for link resolution
    pub id_map: HashMap<(String, String), String>,
    /// Maps aid -> (chunk_sequence_number, offset_in_chunk, offset_in_text)
    pub aid_offset_map: HashMap<String, (usize, usize, usize)>,
    /// Maps file_href -> [(original_position, aid)] for filepos resolution
    pub filepos_map: HashMap<String, Vec<(usize, String)>>,
}

/// Chunker - breaks HTML files into skeletons and chunks
pub struct Chunker {
    aid_counter: u32,
    /// Mapping of (file, id) -> aid built during processing
    id_map: HashMap<(String, String), String>,
    /// Mapping of file_href -> [(original_position, aid)] for filepos resolution
    filepos_map: HashMap<String, Vec<(usize, String)>>,
}

impl Chunker {
    pub fn new() -> Self {
        Self {
            aid_counter: 0,
            id_map: HashMap::new(),
            filepos_map: HashMap::new(),
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

        // Build SKEL and Chunk tables directly from the per-file chunks
        // produced by process_file. SKEL.length is the skeleton's bytes (0
        // in this writer); chunks' insert_pos and length describe the
        // content slice that lives in the rawML right after the skeleton.
        let mut chunk_table: Vec<ChunkEntry> = Vec::new();
        let mut seq_num = 0usize;
        for skel in &mut skeletons {
            for chunk in &mut skel.chunks {
                chunk.sequence_number = seq_num;
                chunk_table.push(ChunkEntry {
                    insert_pos: chunk.insert_pos,
                    selector: chunk.selector.clone(),
                    file_number: chunk.file_number,
                    sequence_number: seq_num,
                    start_pos: chunk.start_pos,
                    length: chunk.raw.len(),
                });
                seq_num += 1;
            }
        }

        let skel_table: Vec<SkelEntry> = skeletons
            .iter()
            .map(|s| SkelEntry {
                name: format!("SKEL{:010}", s.file_number),
                chunk_count: s.chunks.len(),
                start_pos: s.start_pos,
                length: s.skeleton.len(),
            })
            .collect();

        // `text` is the on-disk rawML layout: per skel, the skeleton bytes
        // followed by its chunk bytes.
        let text: Vec<u8> = skeletons.iter().flat_map(|s| s.raw_text()).collect();

        // Build aid_offset_map by scanning the *reassembled* book — chunks
        // inserted into their skeletons — because `chunk_table.insert_pos`
        // is in reassembled coordinates. Calibre does the same in
        // `Skeleton.set_internal_links` (it iterates `rebuilt_text`, not
        // the rawML).
        let rebuilt: Vec<u8> = skeletons.iter().flat_map(|s| s.rebuild()).collect();
        let aid_offset_map = self.build_aid_offset_map(&rebuilt, &chunk_table);

        ChunkerResult {
            skel_table,
            chunk_table,
            text,
            id_map: std::mem::take(&mut self.id_map),
            aid_offset_map,
            filepos_map: std::mem::take(&mut self.filepos_map),
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
        // Aids are discovered in ascending offset order and chunks are sorted
        // by insert_pos, so a persistent cursor resolves each aid without
        // rescanning the chunk table (previously O(aids × chunks)).
        let mut chunk_cursor = 0usize;

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

                    // `offset` is in reassembled coordinates. Find the chunk
                    // whose [insert_pos, insert_pos+length) range contains
                    // it. Calibre falls back to "the chunk immediately
                    // after" when the aid is in the skeleton (e.g. on
                    // `<body aid="0000">`), with an in-chunk offset of 0.
                    let (seq_num, offset_in_chunk) = if chunk_table.is_empty() {
                        (0usize, offset)
                    } else {
                        // Advance to the first chunk that ends after `offset`.
                        while chunk_cursor < chunk_table.len() {
                            let c = &chunk_table[chunk_cursor];
                            if c.insert_pos + c.length > offset {
                                break;
                            }
                            chunk_cursor += 1;
                        }
                        match chunk_table.get(chunk_cursor) {
                            // Chunk contains the aid.
                            Some(chunk) if offset >= chunk.insert_pos => {
                                (chunk.sequence_number, offset - chunk.insert_pos)
                            }
                            // Aid is in a skeleton before this chunk — use
                            // this chunk with in-chunk offset 0.
                            Some(chunk) => (chunk.sequence_number, 0),
                            // Past every chunk: point to the last chunk's end.
                            None => {
                                let last = chunk_table.last().expect("chunk_table non-empty here");
                                (last.sequence_number, last.length.saturating_sub(1))
                            }
                        }
                    };

                    aid_offset_map.insert(aid, (seq_num, offset_in_chunk, offset));
                }
            }
            search_pos = val_start;
        }

        aid_offset_map
    }

    /// Process a single HTML file into a skeleton + chunk.
    ///
    /// Layout mirrors calibre's approach: the skeleton holds the HTML
    /// scaffolding (everything up to and including the `<body...>` opening
    /// tag, plus `</body></html>` at the end), while the chunk holds the
    /// body content. When Kindle reassembles, it reads `skel.length` bytes
    /// of scaffold from the rawML, then inserts each chunk at its
    /// `insert_pos` into that scaffold.
    ///
    /// Earlier versions packed the whole file into a single chunk and used
    /// an empty skeleton, which freezes the device's renderer: Kindle's
    /// layout engine seems to require real HTML scaffolding bytes per skel.
    fn process_file(
        &mut self,
        file_number: usize,
        file_href: &str,
        html: &[u8],
        start_pos: usize,
    ) -> Skeleton {
        // Kindle's HTML5 parser chokes on EPUB3 namespace decorations
        // (`xmlns:epub`, `epub:type`, `epub:prefix`, `xml:lang`, etc.) so
        // strip them before aid annotation.
        let cleaned = super::writer_transform::strip_xml_namespaces(html);
        let result = super::writer_transform::add_aid_attributes_fast(
            &cleaned,
            file_href,
            &mut self.aid_counter,
            &mut self.id_map,
        );

        if !result.position_map.is_empty() {
            self.filepos_map
                .insert(file_href.to_string(), result.position_map);
        }

        // Split the (aid-annotated) HTML into [head + body-open][body
        // content][body-close + html-close]. The chunk holds the body
        // content; the skeleton is the surrounding scaffold.
        let (skel_prefix, body_content, skel_suffix) = split_body(&result.html);

        let mut skeleton_bytes = Vec::with_capacity(skel_prefix.len() + skel_suffix.len());
        skeleton_bytes.extend_from_slice(skel_prefix);
        skeleton_bytes.extend_from_slice(skel_suffix);

        // Split body content into ~CHUNK_SIZE pieces at `<` tag boundaries.
        // Kindle's renderer freezes on chunks larger than this — calibre's
        // writer enforces the same limit. Splitting at `<` is safe because
        // chunks are simply concatenated when Kindle reassembles the file;
        // no chunk needs to be valid HTML on its own.
        let body_chunks = split_body_into_chunks(body_content, CHUNK_SIZE);

        // Build the chunk selector from this file's body aid. Kindle's
        // renderer uses chunk selectors to map a chunk's content back to
        // its enclosing DOM element; without per-file uniqueness it
        // conflates positions across files when laying out and locks up.
        let body_aid = extract_body_aid(skel_prefix).unwrap_or_else(|| "0000".to_string());
        let selector = format!("P-//*[@aid='{body_aid}']");

        // Each chunk's `insert_pos` is the absolute rawML position where its
        // bytes go in the reassembled file. The first chunk starts just
        // after the prefix scaffold; subsequent chunks follow contiguously.
        let mut chunks = Vec::with_capacity(body_chunks.len());
        let mut cumulative = 0usize;
        let base = start_pos + skel_prefix.len();
        for raw in body_chunks {
            let len = raw.len();
            chunks.push(Chunk {
                raw,
                insert_pos: base + cumulative,
                selector: selector.clone(),
                file_number,
                sequence_number: 0, // assigned by Chunker::process
                start_pos: cumulative,
            });
            cumulative += len;
        }

        Skeleton {
            file_number,
            skeleton: skeleton_bytes,
            chunks,
            start_pos,
        }
    }
}

const CHUNK_SIZE: usize = 8192;

/// Extract the `aid="…"` value from the `<body…>` tag inside the scaffold
/// prefix. Calibre's chunker tracks this naturally via DOM walk; in our
/// byte-level chunker we recover it by scanning the prefix bytes for the
/// already-inserted aid attribute on the body opening tag.
fn extract_body_aid(skel_prefix: &[u8]) -> Option<String> {
    use memchr::memmem;
    let body_pos = memmem::find(skel_prefix, b"<body")?;
    let tag_end_rel = memchr::memchr(b'>', &skel_prefix[body_pos..])?;
    let body_open = &skel_prefix[body_pos..body_pos + tag_end_rel];
    let aid_pos = memmem::find(body_open, b" aid=\"")?;
    let val_start = aid_pos + 6;
    let val_end = memchr::memchr(b'"', &body_open[val_start..])?;
    Some(
        std::str::from_utf8(&body_open[val_start..val_start + val_end])
            .ok()?
            .to_string(),
    )
}

/// Split body-content bytes into chunks of at most ~`max_size` bytes each,
/// always cutting at HTML element boundaries (after a closing tag).
///
/// We walk the bytes tracking tag nesting depth. After each closing tag or
/// self-closing tag we're at a safe boundary between sibling elements. Cut
/// there once the in-progress chunk has reached `max_size`, preferring the
/// *shallowest* such boundary so each chunk ends with a complete (possibly
/// nested) element rather than mid-way through some deeply-nested run.
///
/// In practice this gives chunks that always contain whole `<p>` /
/// `<li>` / `<section>` etc. units. Comments, processing instructions,
/// CDATA, and doctypes don't affect depth. A single element larger than
/// `max_size` becomes one oversized chunk (rare; would need to recurse
/// inside it to split further, which we don't here).
fn split_body_into_chunks(body: &[u8], max_size: usize) -> Vec<Vec<u8>> {
    if body.is_empty() {
        return vec![Vec::new()];
    }

    let mut chunks = Vec::new();
    let mut chunk_start = 0;
    let mut depth: i32 = 0;
    let mut i = 0;

    while i < body.len() {
        if body[i] != b'<' {
            i += 1;
            continue;
        }

        let tag_start = i;
        let next = body.get(i + 1).copied();

        if matches!(next, Some(b'!') | Some(b'?')) {
            let close = memchr::memchr(b'>', &body[tag_start..])
                .map(|r| tag_start + r + 1)
                .unwrap_or(body.len());
            i = close;
            continue;
        }

        let is_close = next == Some(b'/');

        let mut j = tag_start + 1;
        let mut in_quote: Option<u8> = None;
        while j < body.len() {
            let c = body[j];
            match in_quote {
                Some(q) if c == q => in_quote = None,
                None => match c {
                    b'"' | b'\'' => in_quote = Some(c),
                    b'>' => break,
                    _ => {}
                },
                _ => {}
            }
            j += 1;
        }
        if j >= body.len() {
            break;
        }
        let tag_end = j + 1;

        let self_closing = j > 0 && body[j - 1] == b'/';

        if is_close {
            depth = depth.saturating_sub(1);
        } else if !self_closing {
            depth += 1;
        }

        i = tag_end;

        // Cut at the first element-closing boundary once we've reached the
        // target size — first-fit rather than "prefer shallowest". A deep
        // run that doesn't surface within target shouldn't be allowed to
        // grow past the limit, which is exactly what Kindle's renderer
        // can't handle.
        if (is_close || self_closing) && (i - chunk_start) >= max_size {
            chunks.push(body[chunk_start..i].to_vec());
            chunk_start = i;
        }
    }

    // Flush trailing bytes. If they're tiny, fold them into the previous
    // chunk rather than emitting a sliver — Kindle accepts very small
    // trailing fragments but they're a footgun for fragment indexing.
    if chunk_start < body.len() {
        let tail = &body[chunk_start..];
        if let Some(last) = chunks.last_mut()
            && tail.len() < max_size / 16
        {
            last.extend_from_slice(tail);
        } else {
            chunks.push(tail.to_vec());
        }
    } else if chunks.is_empty() {
        chunks.push(Vec::new());
    }

    chunks
}

/// Split an HTML document into `(scaffold_before_body_content, body_content,
/// scaffold_after_body_content)`. The split point is just past the `<body…>`
/// opening tag and just before the matching `</body>`. If either tag isn't
/// found, returns the whole document as scaffold with empty body content (so
/// minimal files don't break the writer).
fn split_body(html: &[u8]) -> (&[u8], &[u8], &[u8]) {
    use memchr::memmem;

    // Find `<body` case-insensitively. memmem is case-sensitive, but body
    // tags in serialised XHTML are reliably lowercase, so a direct search
    // is fine; fall back to a manual scan if needed.
    let body_open_start = match memmem::find(html, b"<body") {
        Some(i) => i,
        None => return (html, &[], &[]),
    };

    // Find the `>` that closes the opening tag, respecting quoted attribute
    // values.
    let mut p = body_open_start + 5;
    let mut in_quote: Option<u8> = None;
    while p < html.len() {
        let c = html[p];
        match in_quote {
            Some(q) if c == q => in_quote = None,
            None => match c {
                b'"' | b'\'' => in_quote = Some(c),
                b'>' => break,
                _ => {}
            },
            _ => {}
        }
        p += 1;
    }
    if p >= html.len() {
        return (html, &[], &[]);
    }
    let body_open_end = p + 1;

    let body_close_start = match memmem::rfind(html, b"</body>") {
        Some(i) if i >= body_open_end => i,
        _ => return (html, &[], &[]),
    };

    (
        &html[..body_open_end],
        &html[body_open_end..body_close_start],
        &html[body_close_start..],
    )
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
        let result = add_aid_attributes_fast(
            html,
            "test.xhtml",
            &mut chunker.aid_counter,
            &mut chunker.id_map,
        );
        let result_str = String::from_utf8_lossy(&result.html);
        assert!(result_str.contains("aid=\"0000\""));
        assert!(result_str.contains("aid=\"0001\""));
    }
}

#[cfg(test)]
mod chunker_tests {
    use super::*;

    #[test]
    fn chunker_preserves_all_bytes() {
        // Pathological-ish HTML similar to a real chapter.
        let body =
            b"<section><h1>Title</h1><p>One.</p><p>Two.</p><div><p>A</p><p>B</p></div></section>"
                .repeat(200);
        let chunks = split_body_into_chunks(&body, 8192);
        let recon: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(
            recon, body,
            "chunks must concatenate back to the original body"
        );
    }

    #[test]
    fn chunker_under_target_yields_single_chunk() {
        let body = b"<p>tiny</p>".repeat(10);
        let chunks = split_body_into_chunks(&body, 8192);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].as_slice(), body.as_slice());
    }

    #[test]
    fn chunker_empty_body() {
        let chunks = split_body_into_chunks(b"", 8192);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_empty());
    }

    #[test]
    fn chunk_selectors_are_per_file_unique() {
        // Regression test for issue #10: every spine file's chunks must
        // reference *that file's* body aid in their selector, not a single
        // shared aid. With shared selectors Kindle conflates positions
        // across files during layout and locks up.
        let html = |n| {
            format!(
                "<html><head><title>F{n}</title></head><body>\
                 <section><p>hello from file {n}</p></section>\
                 </body></html>"
            )
            .into_bytes()
        };
        let files = vec![
            ("a.xhtml".to_string(), html(0)),
            ("b.xhtml".to_string(), html(1)),
            ("c.xhtml".to_string(), html(2)),
        ];
        let result = Chunker::new().process(&files);

        let selectors_per_file: std::collections::HashMap<usize, std::collections::HashSet<&str>> =
            result
                .chunk_table
                .iter()
                .fold(std::collections::HashMap::new(), |mut acc, c| {
                    acc.entry(c.file_number)
                        .or_default()
                        .insert(c.selector.as_str());
                    acc
                });

        let all_selectors: std::collections::HashSet<&str> = result
            .chunk_table
            .iter()
            .map(|c| c.selector.as_str())
            .collect();

        assert_eq!(
            all_selectors.len(),
            selectors_per_file.len(),
            "each spine file must have a distinct chunk selector; \
             got {:?}",
            all_selectors,
        );
    }
}
