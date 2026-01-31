//! High-performance HTML transformation for MOBI/KF8 processing.
//!
//! Handles kindle: reference transformation and attribute stripping.

use bstr::ByteSlice;
use memchr::memmem;

use super::parser::DivElement;

/// Parse Kindle base32 encoding (0-9A-V) to number.
#[inline]
pub fn parse_base32(s: &[u8]) -> usize {
    let mut result = 0usize;
    for &b in s {
        result = result.wrapping_mul(32);
        let val = match b {
            b'0'..=b'9' => (b - b'0') as usize,
            b'A'..=b'V' => (b - b'A') as usize + 10,
            b'a'..=b'v' => (b - b'a') as usize + 10,
            _ => continue,
        };
        result = result.wrapping_add(val);
    }
    result
}

/// Result of finding a kindle reference in the input.
struct KindleRef {
    /// End position (after closing quote/paren).
    end: usize,
    /// Type of reference.
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
    /// Malformed reference to remove.
    Malformed,
}

/// Transform kindle: references in HTML to standard EPUB-style paths.
///
/// Converts:
/// - `kindle:flow:XXXX` → `styles/styleNNNN.css`
/// - `kindle:pos:fid:XXXX:off:YYYY` → `partNNNN.html#id` or `partNNNN.html`
/// - `kindle:embed:XXXX` → `images/image_NNNN.ext`
pub fn transform_kindle_refs(
    html: &[u8],
    elems: &[DivElement],
    raw_text: &[u8],
    file_starts: &[(u32, u32)],
) -> Vec<u8> {
    let mut output = Vec::with_capacity(html.len());
    let mut pos = 0;

    let finder = memmem::Finder::new(b"kindle:");

    while let Some(rel_start) = finder.find(&html[pos..]) {
        let start = pos + rel_start;
        output.extend_from_slice(&html[pos..start]);

        if let Some(kindle_ref) = parse_kindle_ref(&html[start..]) {
            let replacement = generate_replacement(&kindle_ref, elems, raw_text, file_starts);
            output.extend_from_slice(&replacement);
            pos = start + kindle_ref.end;
        } else {
            output.extend_from_slice(b"kindle:");
            pos = start + 7;
        }
    }

    output.extend_from_slice(&html[pos..]);
    output
}

/// Parse a kindle: reference starting at the given position.
fn parse_kindle_ref(data: &[u8]) -> Option<KindleRef> {
    if !data.starts_with(b"kindle:") {
        return None;
    }

    let end_pos = data[7..]
        .iter()
        .position(|&b| b == b'"' || b == b'\'' || b == b')')?;
    let end = 7 + end_pos;
    let content = &data[7..end];

    let kind = if content.starts_with(b"flow:") {
        let id_end = content[5..].find_byte(b'?').unwrap_or(content.len() - 5);
        let flow_num = parse_base32(&content[5..5 + id_end]);
        RefKind::Flow { flow_num }
    } else if content.starts_with(b"pos:fid:") {
        parse_pos_fid(content)
    } else if content.starts_with(b"embed:") {
        let id_end = content[6..].find_byte(b'?').unwrap_or(content.len() - 6);
        let img_num = parse_base32(&content[6..6 + id_end]);
        let img_idx = img_num.saturating_sub(1);

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

/// Parse kindle:pos:fid:... format.
fn parse_pos_fid(content: &[u8]) -> RefKind {
    let rest = &content[8..]; // After "pos:fid:"

    let fid_end = rest.find_byte(b':').unwrap_or(rest.len());
    let elem_idx = parse_base32(&rest[..fid_end]);

    if fid_end < rest.len() && rest[fid_end..].starts_with(b":off:") {
        let off_start = fid_end + 5;
        let offset = parse_base32(&rest[off_start..]);
        RefKind::PosFid { elem_idx, offset }
    } else {
        RefKind::PosFidOld { elem_idx }
    }
}

/// Generate replacement text for a kindle reference.
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
        RefKind::Malformed => Vec::new(),
    }
}

/// Find nearest id/name/aid attribute in raw text near the target position.
///
/// Matches KindleUnpack's getIDTag behavior:
/// 1. Search backward from position for id=, name=, or aid= attributes
/// 2. If aid= is found, return "aid-{value}"
/// 3. Stop at <body tag (return empty = link to top of file)
///
/// This is used to resolve kindle:pos:fid:XXXX:off:YYYY links to anchors.
pub fn find_nearest_id_fast(
    raw_text: &[u8],
    pos: usize,
    file_num: usize,
    file_starts: &[(u32, u32)],
) -> Option<String> {
    // Calculate file bounds
    let (file_start, file_end) = {
        let mut start = 0usize;
        let mut end = raw_text.len();

        for (i, &(start_pos, fnum)) in file_starts.iter().enumerate() {
            if fnum as usize == file_num {
                start = start_pos as usize;
                if let Some(&(next_start, _)) = file_starts.get(i + 1) {
                    end = next_start as usize;
                }
                break;
            }
        }
        (start, end)
    };

    let pos = pos.clamp(file_start, file_end);

    // Finders for id=, name=, aid= patterns (with space prefix to avoid matching aid= when looking for id=)
    let id_finder = memmem::Finder::new(b" id=\"");
    let id_finder_single = memmem::Finder::new(b" id='");
    let name_finder = memmem::Finder::new(b" name=\"");
    let name_finder_single = memmem::Finder::new(b" name='");
    let aid_finder = memmem::Finder::new(b" aid=\"");
    let aid_finder_single = memmem::Finder::new(b" aid='");

    // Search forward from pos to find the next anchor (within reasonable distance)
    let end_pos = (pos + 2000).min(file_end);
    if pos < end_pos {
        let search_window = &raw_text[pos..end_pos];

        // Try id= first (highest priority)
        if let Some(val) = find_attr_in_window(search_window, &id_finder, &id_finder_single, 4) {
            return Some(val);
        }

        // Try name=
        if let Some(val) = find_attr_in_window(search_window, &name_finder, &name_finder_single, 6)
        {
            return Some(val);
        }

        // Try aid= (convert to aid-{value})
        if let Some(val) = find_attr_in_window(search_window, &aid_finder, &aid_finder_single, 5) {
            return Some(format!("aid-{}", val));
        }
    }

    // Search backwards from pos
    let start_pos = pos.saturating_sub(2000).max(file_start);
    if start_pos < pos {
        let back_window = &raw_text[start_pos..pos];

        // Find the last occurrence of each attribute type
        let last_id = find_last_attr_in_window(back_window, &id_finder, &id_finder_single, 4);
        let last_name =
            find_last_attr_in_window(back_window, &name_finder, &name_finder_single, 6);
        let last_aid = find_last_attr_in_window(back_window, &aid_finder, &aid_finder_single, 5);

        // Check if we hit a <body tag (stop searching)
        let body_pos = memmem::find(back_window, b"<body ");

        // Find the closest one that's after any <body tag
        let mut best: Option<(usize, String)> = None;

        for (opt_pos, opt_val, is_aid) in [(last_id, false), (last_name, false), (last_aid, true)]
            .into_iter()
            .filter_map(|(opt, is_aid)| opt.map(|(p, v)| (p, v, is_aid)))
        {
            // Skip if before <body
            if let Some(bp) = body_pos
                && opt_pos < bp
            {
                continue;
            }

            let val = if is_aid {
                format!("aid-{}", opt_val)
            } else {
                opt_val
            };

            match &best {
                None => best = Some((opt_pos, val)),
                Some((best_pos, _)) if opt_pos > *best_pos => best = Some((opt_pos, val)),
                _ => {}
            }
        }

        if let Some((_, val)) = best {
            return Some(val);
        }
    }

    None
}

/// Find attribute value in a search window (forward search).
fn find_attr_in_window(
    window: &[u8],
    finder_double: &memmem::Finder,
    finder_single: &memmem::Finder,
    attr_len: usize, // length of " id=" or " name=" etc
) -> Option<String> {
    let pos = finder_double
        .find(window)
        .or_else(|| finder_single.find(window))?;

    let quote_char = window[pos + attr_len];
    let value_start = pos + attr_len + 1;

    if let Some(value_end) = window[value_start..].iter().position(|&b| b == quote_char) {
        let id_bytes = &window[value_start..value_start + value_end];
        if id_bytes.iter().all(|&b| {
            b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.'
        }) {
            return Some(String::from_utf8_lossy(id_bytes).into_owned());
        }
    }

    None
}

/// Find the last occurrence of an attribute in a window (backward search).
/// Returns (position, value) if found.
fn find_last_attr_in_window(
    window: &[u8],
    finder_double: &memmem::Finder,
    finder_single: &memmem::Finder,
    attr_len: usize,
) -> Option<(usize, String)> {
    let mut last: Option<(usize, String)> = None;
    let mut search_pos = 0;

    while search_pos < window.len() {
        let next = finder_double
            .find(&window[search_pos..])
            .or_else(|| finder_single.find(&window[search_pos..]));

        if let Some(rel_pos) = next {
            let abs_pos = search_pos + rel_pos;
            let quote_char = window.get(abs_pos + attr_len).copied().unwrap_or(b'"');
            let value_start = abs_pos + attr_len + 1;

            if let Some(value_end) = window[value_start..].iter().position(|&b| b == quote_char) {
                let id_bytes = &window[value_start..value_start + value_end];
                if id_bytes.iter().all(|&b| {
                    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.'
                }) {
                    last = Some((abs_pos, String::from_utf8_lossy(id_bytes).into_owned()));
                }
            }
            search_pos = abs_pos + 1;
        } else {
            break;
        }
    }

    last
}

/// Strip Amazon-specific attributes from HTML.
///
/// Removes: aid="...", data-AmznRemoved..., data-AmznPageBreak="..."
pub fn strip_kindle_attributes_fast(html: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(html.len());
    let mut pos = 0;

    while pos < html.len() {
        if let Some(tag_start) = memchr::memchr(b'<', &html[pos..]) {
            let abs_tag_start = pos + tag_start;
            output.extend_from_slice(&html[pos..abs_tag_start]);

            if let Some(tag_end) = memchr::memchr(b'>', &html[abs_tag_start..]) {
                let abs_tag_end = abs_tag_start + tag_end + 1;
                let tag = &html[abs_tag_start..abs_tag_end];

                let cleaned = clean_tag(tag);
                output.extend_from_slice(&cleaned);

                pos = abs_tag_end;
            } else {
                output.extend_from_slice(&html[abs_tag_start..]);
                break;
            }
        } else {
            output.extend_from_slice(&html[pos..]);
            break;
        }
    }

    output
}

/// Clean a single tag by removing Amazon-specific attributes.
fn clean_tag(tag: &[u8]) -> Vec<u8> {
    // Skip comments and special tags
    if tag.starts_with(b"<!--")
        || tag.starts_with(b"<!DOCTYPE")
        || tag.starts_with(b"<?")
        || tag.starts_with(b"</")
    {
        return tag.to_vec();
    }

    let mut result = Vec::with_capacity(tag.len());
    let mut i = 0;

    // Copy tag name
    result.push(b'<');
    i += 1;

    while i < tag.len() && tag[i] != b' ' && tag[i] != b'>' && tag[i] != b'/' {
        result.push(tag[i]);
        i += 1;
    }

    // Process attributes
    while i < tag.len() {
        // Skip whitespace
        while i < tag.len() && (tag[i] == b' ' || tag[i] == b'\t' || tag[i] == b'\n') {
            result.push(tag[i]);
            i += 1;
        }

        if i >= tag.len() || tag[i] == b'>' || tag[i] == b'/' {
            break;
        }

        // Get attribute name
        let attr_start = i;
        while i < tag.len()
            && tag[i] != b'='
            && tag[i] != b' '
            && tag[i] != b'>'
            && tag[i] != b'/'
        {
            i += 1;
        }
        let attr_name = &tag[attr_start..i];

        // Check if this is an attribute to strip
        let should_strip = attr_name == b"aid"
            || attr_name.starts_with(b"data-Amzn")
            || attr_name.starts_with(b"data-amzn");

        if should_strip {
            // Skip the attribute value
            if i < tag.len() && tag[i] == b'=' {
                i += 1;
                if i < tag.len() && (tag[i] == b'"' || tag[i] == b'\'') {
                    let quote = tag[i];
                    i += 1;
                    while i < tag.len() && tag[i] != quote {
                        i += 1;
                    }
                    if i < tag.len() {
                        i += 1;
                    }
                } else {
                    while i < tag.len() && tag[i] != b' ' && tag[i] != b'>' {
                        i += 1;
                    }
                }
            }
        } else {
            // Keep this attribute
            result.extend_from_slice(attr_name);
            if i < tag.len() && tag[i] == b'=' {
                result.push(b'=');
                i += 1;
                if i < tag.len() && (tag[i] == b'"' || tag[i] == b'\'') {
                    let quote = tag[i];
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
                    let value_start = i;
                    while i < tag.len() && tag[i] != b' ' && tag[i] != b'>' {
                        i += 1;
                    }
                    result.extend_from_slice(&tag[value_start..i]);
                }
            }
        }
    }

    // Copy closing
    while i < tag.len() {
        result.push(tag[i]);
        i += 1;
    }

    // Ensure img tags have alt attribute
    if result.starts_with(b"<img ") || result.starts_with(b"<IMG ") {
        return ensure_img_alt(&result);
    }

    result
}

/// Ensure img tag has alt attribute.
fn ensure_img_alt(tag: &[u8]) -> Vec<u8> {
    if memmem::find(tag, b"alt=").is_some() {
        return tag.to_vec();
    }

    let mut result = Vec::with_capacity(tag.len() + 7);

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
        let output_str = String::from_utf8_lossy(&output);
        eprintln!("Output: {:?}", output_str);
        assert!(!output.contains_str("aid="));
        // After stripping aid, there may be trailing whitespace before >
        assert!(
            output_str.starts_with("<p") && output_str.contains(">Hello</p>"),
            "Expected <p...>Hello</p>, got: {}",
            output_str
        );
    }

    #[test]
    fn test_img_alt() {
        let input = b"<img src=\"test.jpg\"/>";
        let output = ensure_img_alt(input);
        assert!(output.contains_str("alt=\"\""));
    }
}
