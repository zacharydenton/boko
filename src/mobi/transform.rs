//! High-performance HTML transformation for MOBI processing.
//!
//! Uses single-pass processing and SIMD-accelerated byte operations
//! to efficiently transform kindle: references and strip attributes.

use bstr::ByteSlice;
use memchr::memmem;

use super::index::DivElement;
use super::parse_base32;

/// Result of finding a kindle reference in the input
struct KindleRef {
    /// End position (after closing quote/paren)
    end: usize,
    /// Type of reference
    kind: RefKind,
}

enum RefKind {
    /// kindle:flow:XXXX?mime=...
    Flow { flow_num: usize },
    /// kindle:pos:fid:XXXX:off:YYYY
    PosFid { elem_idx: usize, offset: usize },
    /// kindle:pos:fid:XXXX (old format)
    PosFidOld { elem_idx: usize },
    /// kindle:embed:XXXX?mime=...
    Embed { img_idx: usize, ext: &'static str },
    /// Malformed reference to remove
    Malformed,
}

/// Single-pass transformation of kindle: references in HTML.
///
/// This is much faster than the naive approach because:
/// 1. Uses SIMD-accelerated search (memchr/memmem)
/// 2. Single pass through the input
/// 3. Builds result incrementally without intermediate strings
/// 4. Pre-allocates output buffer
pub fn transform_kindle_refs(
    html: &[u8],
    elems: &[DivElement],
    raw_text: &[u8],
    file_starts: &[(u32, u32)],
) -> Vec<u8> {
    // Pre-allocate output (usually slightly smaller than input due to shorter paths)
    let mut output = Vec::with_capacity(html.len());
    let mut pos = 0;

    // Use memmem finder for SIMD-accelerated search
    let finder = memmem::Finder::new(b"kindle:");

    while let Some(rel_start) = finder.find(&html[pos..]) {
        let start = pos + rel_start;

        // Copy everything before this reference
        output.extend_from_slice(&html[pos..start]);

        // Parse the reference
        if let Some(kindle_ref) = parse_kindle_ref(&html[start..]) {
            let replacement = generate_replacement(&kindle_ref, elems, raw_text, file_starts);
            output.extend_from_slice(&replacement);
            pos = start + kindle_ref.end;
        } else {
            // Couldn't parse, skip the "kindle:" prefix and continue
            output.extend_from_slice(b"kindle:");
            pos = start + 7;
        }
    }

    // Copy remaining content
    output.extend_from_slice(&html[pos..]);
    output
}

/// Parse a kindle: reference starting at the given position
fn parse_kindle_ref(data: &[u8]) -> Option<KindleRef> {
    if !data.starts_with(b"kindle:") {
        return None;
    }

    // Find the end of the reference (quote or paren)
    let end_pos = data[7..]
        .iter()
        .position(|&b| b == b'"' || b == b'\'' || b == b')')?;
    let end = 7 + end_pos;
    let content = &data[7..end];

    let kind = if content.starts_with(b"flow:") {
        // kindle:flow:XXXX?mime=...
        let id_end = content[5..].find_byte(b'?').unwrap_or(content.len() - 5);
        let flow_num = parse_base32(&content[5..5 + id_end]);
        RefKind::Flow { flow_num }
    } else if content.starts_with(b"pos:fid:") {
        // kindle:pos:fid:XXXX:off:YYYY or kindle:pos:fid:XXXX
        parse_pos_fid(content)
    } else if content.starts_with(b"embed:") {
        // kindle:embed:XXXX?mime=...
        let id_end = content[6..].find_byte(b'?').unwrap_or(content.len() - 6);
        let img_num = parse_base32(&content[6..6 + id_end]);
        let img_idx = img_num.saturating_sub(1); // 1-indexed to 0-indexed

        // Determine extension from mime type
        let ext = if content.find(b"image/png").is_some() {
            "png"
        } else if content.find(b"image/gif").is_some() {
            "gif"
        } else {
            "jpg"
        };

        RefKind::Embed { img_idx, ext }
    } else {
        RefKind::Malformed
    };

    Some(KindleRef { end, kind })
}

/// Parse kindle:pos:fid:... format
fn parse_pos_fid(content: &[u8]) -> RefKind {
    // Format: pos:fid:XXXX:off:YYYY or pos:fid:XXXX
    // content starts after "kindle:"
    let rest = &content[8..]; // After "pos:fid:"

    // Find the fid value (ends at ':' or end of string)
    let fid_end = rest.find_byte(b':').unwrap_or(rest.len());
    let elem_idx = parse_base32(&rest[..fid_end]);

    // Check for :off:YYYY
    if fid_end < rest.len() && rest[fid_end..].starts_with(b":off:") {
        let off_start = fid_end + 5;
        let offset = parse_base32(&rest[off_start..]);
        RefKind::PosFid { elem_idx, offset }
    } else {
        RefKind::PosFidOld { elem_idx }
    }
}

/// Generate replacement text for a kindle reference
fn generate_replacement(
    kindle_ref: &KindleRef,
    elems: &[DivElement],
    raw_text: &[u8],
    file_starts: &[(u32, u32)],
) -> Vec<u8> {
    match &kindle_ref.kind {
        RefKind::Flow { flow_num } => {
            let css_idx = flow_num.saturating_sub(1);
            format!("styles/style{:04}.css", css_idx).into_bytes()
        }
        RefKind::PosFid { elem_idx, offset } => {
            let (file_num, target_pos) = if let Some(elem) = elems.get(*elem_idx) {
                (elem.file_number as usize, elem.insert_pos + *offset as u32)
            } else {
                (0, 0)
            };

            let anchor = find_nearest_id_fast(raw_text, target_pos as usize, file_num, file_starts);
            if let Some(id) = anchor {
                format!("part{:04}.html#{}", file_num, id).into_bytes()
            } else {
                format!("part{:04}.html", file_num).into_bytes()
            }
        }
        RefKind::PosFidOld { elem_idx } => {
            let file_num = elems
                .get(*elem_idx)
                .map(|e| e.file_number as usize)
                .unwrap_or(0);
            format!("part{:04}.html", file_num).into_bytes()
        }
        RefKind::Embed { img_idx, ext } => {
            format!("images/image_{:04}.{}", img_idx, ext).into_bytes()
        }
        RefKind::Malformed => {
            // Return empty to remove malformed reference
            Vec::new()
        }
    }
}

/// Fast ID lookup using memchr, constrained to file bounds
fn find_nearest_id_fast(
    raw_text: &[u8],
    pos: usize,
    file_num: usize,
    file_starts: &[(u32, u32)],
) -> Option<String> {
    // Calculate file bounds from file_starts
    // file_starts contains (start_pos, file_number) tuples
    let (file_start, file_end) = {
        let mut start = 0usize;
        let mut end = raw_text.len();

        // Find the start and end of our target file
        for (i, &(start_pos, fnum)) in file_starts.iter().enumerate() {
            if fnum as usize == file_num {
                start = start_pos as usize;
                // End is the start of the next file, or end of raw_text
                if let Some(&(next_start, _)) = file_starts.get(i + 1) {
                    end = next_start as usize;
                }
                break;
            }
        }
        (start, end)
    };

    // Constrain pos to file bounds
    let pos = pos.clamp(file_start, file_end);

    // Use memchr to find potential id= patterns (with space prefix to avoid matching aid=)
    let id_finder = memmem::Finder::new(b" id=\"");
    let id_finder_single = memmem::Finder::new(b" id='");

    // Search forward from pos to find the next id= attribute, but stay within file
    let end_pos = (pos + 2000).min(file_end);
    if pos < end_pos {
        let search_window = &raw_text[pos..end_pos];

        let id_pos = id_finder
            .find(search_window)
            .or_else(|| id_finder_single.find(search_window));

        if let Some(rel_pos) = id_pos {
            let quote_char = search_window[rel_pos + 4];
            let value_start = rel_pos + 5;
            if let Some(value_end) = search_window[value_start..].find_byte(quote_char) {
                let id_bytes = &search_window[value_start..value_start + value_end];
                // Validate it's ASCII alphanumeric with allowed punctuation
                if id_bytes.iter().all(|&b| {
                    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.'
                }) {
                    return Some(String::from_utf8_lossy(id_bytes).into_owned());
                }
            }
        }
    }

    // Fallback: search backwards within file bounds
    let start_pos = pos.saturating_sub(500).max(file_start);
    if start_pos < pos {
        let back_window = &raw_text[start_pos..pos];

        // Search for last id= in backwards window
        let mut last_id = None;
        let mut search_pos = 0;
        while let Some(rel_pos) = id_finder.find(&back_window[search_pos..]) {
            let abs_pos = search_pos + rel_pos;
            let quote_char = back_window.get(abs_pos + 4).copied().unwrap_or(b'"');
            let value_start = abs_pos + 5;
            if let Some(value_end) = back_window[value_start..].find_byte(quote_char) {
                let id_bytes = &back_window[value_start..value_start + value_end];
                if id_bytes.iter().all(|&b| {
                    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.'
                }) {
                    last_id = Some(String::from_utf8_lossy(id_bytes).into_owned());
                }
            }
            search_pos = abs_pos + 1;
        }

        if last_id.is_some() {
            return last_id;
        }
    }

    None
}

/// Strip Amazon-specific attributes from HTML in a single pass.
///
/// Removes:
/// - aid="..." attributes
/// - data-AmznRemoved... attributes
/// - data-AmznPageBreak="..." attributes
///
/// Also fixes img tags without alt attributes.
pub fn strip_kindle_attributes_fast(html: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(html.len());
    let mut pos = 0;

    // Patterns to remove (we'll handle these during tag processing)
    let aid_finder = memmem::Finder::new(b" aid=");
    let amzn_removed_finder = memmem::Finder::new(b" data-AmznRemoved");
    let amzn_page_finder = memmem::Finder::new(b" data-AmznPageBreak");

    while pos < html.len() {
        // Find next tag start
        if let Some(tag_start) = memchr::memchr(b'<', &html[pos..]) {
            let abs_tag_start = pos + tag_start;

            // Copy content before tag
            output.extend_from_slice(&html[pos..abs_tag_start]);

            // Find tag end
            if let Some(tag_end) = memchr::memchr(b'>', &html[abs_tag_start..]) {
                let abs_tag_end = abs_tag_start + tag_end + 1;
                let tag = &html[abs_tag_start..abs_tag_end];

                // Process the tag
                let processed =
                    process_tag(tag, &aid_finder, &amzn_removed_finder, &amzn_page_finder);
                output.extend_from_slice(&processed);

                pos = abs_tag_end;
            } else {
                // No closing >, copy rest and break
                output.extend_from_slice(&html[abs_tag_start..]);
                break;
            }
        } else {
            // No more tags, copy rest
            output.extend_from_slice(&html[pos..]);
            break;
        }
    }

    output
}

/// Process a single HTML tag, removing unwanted attributes
fn process_tag(
    tag: &[u8],
    aid_finder: &memmem::Finder<'_>,
    amzn_removed_finder: &memmem::Finder<'_>,
    amzn_page_finder: &memmem::Finder<'_>,
) -> Vec<u8> {
    // Quick check: if no attributes to remove, return as-is
    let has_aid = aid_finder.find(tag).is_some();
    let has_amzn_removed = amzn_removed_finder.find(tag).is_some();
    let has_amzn_page = amzn_page_finder.find(tag).is_some();

    if !has_aid && !has_amzn_removed && !has_amzn_page {
        // Check if this is an img tag needing alt attribute
        if tag.starts_with(b"<img ") || tag.starts_with(b"<IMG ") {
            return ensure_img_alt(tag);
        }
        return tag.to_vec();
    }

    // Need to filter attributes
    let mut result = Vec::with_capacity(tag.len());
    let mut i = 0;

    // Copy tag name
    while i < tag.len() && tag[i] != b' ' && tag[i] != b'>' && tag[i] != b'/' {
        result.push(tag[i]);
        i += 1;
    }

    // Process attributes
    while i < tag.len() {
        // Skip whitespace
        while i < tag.len() && tag[i] == b' ' {
            i += 1;
        }

        if i >= tag.len() || tag[i] == b'>' {
            break;
        }

        // Handle stray "/" - skip it but continue processing (handles malformed tags)
        if tag[i] == b'/' {
            // Check if this is just the self-closing "/" at the end
            if i + 1 < tag.len() && tag[i + 1] == b'>' {
                break;
            }
            // Stray "/" in middle of attributes - skip it
            i += 1;
            continue;
        }

        // Find attribute name end
        let attr_start = i;
        while i < tag.len() && tag[i] != b'=' && tag[i] != b' ' && tag[i] != b'>' && tag[i] != b'/'
        {
            i += 1;
        }
        let attr_name = &tag[attr_start..i];

        // Check if this attribute should be removed
        let should_remove = attr_name == b"aid"
            || attr_name.starts_with(b"data-AmznRemoved")
            || attr_name == b"data-AmznPageBreak";

        if should_remove {
            // Skip attribute value if present
            if i < tag.len() && tag[i] == b'=' {
                i += 1; // Skip '='
                if i < tag.len() {
                    let quote = tag[i];
                    if quote == b'"' || quote == b'\'' {
                        i += 1; // Skip opening quote
                        while i < tag.len() && tag[i] != quote {
                            i += 1;
                        }
                        if i < tag.len() {
                            i += 1; // Skip closing quote
                        }
                    } else {
                        // Unquoted value - skip until space or >
                        while i < tag.len() && tag[i] != b' ' && tag[i] != b'>' {
                            i += 1;
                        }
                    }
                }
            }
        } else {
            // Keep this attribute
            result.push(b' ');
            result.extend_from_slice(attr_name);

            // Copy value if present
            if i < tag.len() && tag[i] == b'=' {
                result.push(b'=');
                i += 1;
                if i < tag.len() {
                    let quote = tag[i];
                    if quote == b'"' || quote == b'\'' {
                        result.push(quote);
                        i += 1;
                        let value_start = i;
                        while i < tag.len() && tag[i] != quote {
                            i += 1;
                        }
                        result.extend_from_slice(&tag[value_start..i]);
                        if i < tag.len() {
                            result.push(quote);
                            i += 1;
                        }
                    } else {
                        // Unquoted value
                        let value_start = i;
                        while i < tag.len() && tag[i] != b' ' && tag[i] != b'>' {
                            i += 1;
                        }
                        result.extend_from_slice(&tag[value_start..i]);
                    }
                }
            }
        }
    }

    // Copy closing
    while i < tag.len() {
        result.push(tag[i]);
        i += 1;
    }

    // Handle img alt attribute
    if result.starts_with(b"<img ") || result.starts_with(b"<IMG ") {
        return ensure_img_alt(&result);
    }

    result
}

/// Ensure img tag has alt attribute
fn ensure_img_alt(tag: &[u8]) -> Vec<u8> {
    if memmem::find(tag, b"alt=").is_some() {
        return tag.to_vec();
    }

    // Find position before closing
    let mut result = Vec::with_capacity(tag.len() + 7);

    // Find the position to insert alt=""
    if let Some(close_pos) = tag.iter().rposition(|&b| b == b'/' || b == b'>') {
        result.extend_from_slice(&tag[..close_pos]);
        if !result.ends_with(b" ") {
            result.push(b' ');
        }
        result.extend_from_slice(b"alt=\"\"");
        result.extend_from_slice(&tag[close_pos..]);
    } else {
        result.extend_from_slice(tag);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_base32() {
        assert_eq!(parse_base32(b"0000"), 0);
        assert_eq!(parse_base32(b"0001"), 1);
        assert_eq!(parse_base32(b"000V"), 31);
        assert_eq!(parse_base32(b"0010"), 32);
    }

    #[test]
    fn test_strip_aid_attribute() {
        let input = b"<p aid=\"0001\">Hello</p>";
        let output = strip_kindle_attributes_fast(input);
        assert!(!output.contains_str("aid="));
        assert!(output.contains_str("<p>Hello</p>"));
    }

    #[test]
    fn test_img_alt() {
        let input = b"<img src=\"test.jpg\"/>";
        let output = ensure_img_alt(input);
        assert!(output.contains_str("alt=\"\""));
    }

    #[test]
    fn test_strip_malformed_aid() {
        // Test malformed tag with stray "/" before aid
        let input = b"<a id=\"tp\"/ aid=\"006F\"/>";
        let output = strip_kindle_attributes_fast(input);
        assert!(!output.contains_str("aid="), "aid should be stripped");
        assert!(
            output.contains_str("<a id=\"tp\""),
            "id should be preserved"
        );
    }

    #[test]
    fn test_strip_self_closing_aid() {
        // Test properly formed self-closing tag with aid
        let input = b"<a id=\"tp\" aid=\"006F\"/>";
        let output = strip_kindle_attributes_fast(input);
        assert!(!output.contains_str("aid="));
        assert!(output.contains_str("<a id=\"tp\""));
    }
}
