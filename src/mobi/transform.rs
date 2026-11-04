//! High-performance HTML transformation for MOBI/KF8 processing.
//!
//! Handles kindle: reference transformation and attribute stripping.

use std::collections::HashSet;

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
            let (file_num, anchor) =
                resolve_pos_fid(*elem_idx, *offset, elems, raw_text, file_starts);
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

/// Resolve a `kindle:pos:fid:XXXX:off:YYYY` target to its file number and
/// nearest anchor id.
fn resolve_pos_fid(
    elem_idx: usize,
    offset: usize,
    elems: &[DivElement],
    raw_text: &[u8],
    file_starts: &[(u32, u32)],
) -> (usize, Option<String>) {
    // Both operands are attacker-controlled (insert_pos parses from
    // the div-index entry name, offset from base32 in the book's own
    // HTML); an unchecked add aborts in release builds.
    let (file_num, target_pos) = if let Some(elem) = elems.get(elem_idx) {
        (
            elem.file_number as usize,
            elem.insert_pos.saturating_add(offset as u32),
        )
    } else {
        (0, 0)
    };

    let anchor = find_nearest_id_fast(raw_text, target_pos as usize, file_num, file_starts);
    (file_num, anchor)
}

/// Collect the `aid` values that `kindle:pos:fid` links in `raw_text` resolve
/// to (as `aid-{value}` anchors).
///
/// [`strip_kindle_attributes_fast`] keeps exactly these as `id="aid-{value}"`
/// so the links have a target, without giving every block element an id.
pub fn collect_linked_aids(
    raw_text: &[u8],
    elems: &[DivElement],
    file_starts: &[(u32, u32)],
) -> HashSet<String> {
    let mut aids = HashSet::new();
    for start in memmem::find_iter(raw_text, b"kindle:pos:fid:") {
        let Some(kindle_ref) = parse_kindle_ref(&raw_text[start..]) else {
            continue;
        };
        if let RefKind::PosFid { elem_idx, offset } = kindle_ref.kind
            && let (_, Some(id)) = resolve_pos_fid(elem_idx, offset, elems, raw_text, file_starts)
            && let Some(aid) = id.strip_prefix("aid-")
        {
            aids.insert(aid.to_string());
        }
    }
    aids
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

    // Search forward from pos to find the closest anchor (within reasonable distance)
    let end_pos = (pos + 2000).min(file_end);
    if pos < end_pos {
        let search_window = &raw_text[pos..end_pos];

        // Find the closest attribute of any type
        let id_match = find_attr_with_pos(search_window, &id_finder, &id_finder_single, 4);
        let name_match = find_attr_with_pos(search_window, &name_finder, &name_finder_single, 6);
        let aid_match = find_attr_with_pos(search_window, &aid_finder, &aid_finder_single, 5);

        // Collect all matches with their positions and whether they're aid
        let mut candidates: Vec<(usize, String, bool)> = Vec::new();
        if let Some((p, v)) = id_match {
            candidates.push((p, v, false));
        }
        if let Some((p, v)) = name_match {
            candidates.push((p, v, false));
        }
        if let Some((p, v)) = aid_match {
            candidates.push((p, v, true));
        }

        // Pick the closest one
        if let Some((p, val, is_aid)) = candidates.into_iter().min_by_key(|(p, _, _)| *p) {
            if is_aid {
                return Some(aid_anchor(raw_text, pos + p, &val));
            } else {
                return Some(val);
            }
        }
    }

    // Search backwards from pos
    let start_pos = pos.saturating_sub(2000).max(file_start);
    if start_pos < pos {
        let back_window = &raw_text[start_pos..pos];

        // Find the last occurrence of each attribute type
        let last_id = find_last_attr_in_window(back_window, &id_finder, &id_finder_single, 4);
        let last_name = find_last_attr_in_window(back_window, &name_finder, &name_finder_single, 6);
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
                aid_anchor(raw_text, start_pos + opt_pos, &opt_val)
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

/// Anchor for an `aid` attribute found at `attr_pos` in `raw_text`.
///
/// Normally `aid-{value}` (the id [`strip_kindle_attributes_fast`] gives the
/// element), but a tag with its own `id` keeps that one, so use it instead.
fn aid_anchor(raw_text: &[u8], attr_pos: usize, aid: &str) -> String {
    let tag_start = memchr::memrchr(b'<', &raw_text[..attr_pos]);
    let tag_end = memchr::memchr(b'>', &raw_text[attr_pos..]).map(|e| attr_pos + e);
    if let (Some(start), Some(end)) = (tag_start, tag_end)
        && let Some(id) = tag_id_value(&raw_text[start..end])
    {
        return id;
    }
    format!("aid-{aid}")
}

/// The quoted `id` value of a tag, as-is (ids like `idまえがき` are valid).
fn tag_id_value(tag: &[u8]) -> Option<String> {
    let attr = [b" id=", b"\tid=", b"\nid="]
        .iter()
        .find_map(|pat| memmem::find(tag, *pat))?;
    let quote = *tag.get(attr + 4)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let value = &tag[attr + 5..];
    let value = &value[..memchr::memchr(quote, value)?];
    let value = std::str::from_utf8(value).ok()?;
    (!value.is_empty()).then(|| value.to_string())
}

/// Find attribute value in a search window (forward search), returning position and value.
fn find_attr_with_pos(
    window: &[u8],
    finder_double: &memmem::Finder,
    finder_single: &memmem::Finder,
    attr_len: usize, // length of " id=" or " name=" etc
) -> Option<(usize, String)> {
    let pos = finder_double
        .find(window)
        .or_else(|| finder_single.find(window))?;

    let quote_char = window[pos + attr_len];
    let value_start = pos + attr_len + 1;

    if let Some(value_end) = window[value_start..].iter().position(|&b| b == quote_char) {
        let id_bytes = &window[value_start..value_start + value_end];
        if id_bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.')
        {
            return Some((pos, String::from_utf8_lossy(id_bytes).into_owned()));
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
///
/// An `aid` listed in `linked_aids` is rewritten to `id="aid-{value}"`
/// instead (when the tag has no `id` of its own), since resolved
/// `kindle:pos:fid` links and TOC entries point at that anchor.
pub fn strip_kindle_attributes_fast(html: &[u8], linked_aids: &HashSet<String>) -> Vec<u8> {
    let mut output = Vec::with_capacity(html.len());
    let mut pos = 0;

    while pos < html.len() {
        if let Some(tag_start) = memchr::memchr(b'<', &html[pos..]) {
            let abs_tag_start = pos + tag_start;
            output.extend_from_slice(&html[pos..abs_tag_start]);

            if let Some(tag_end) = memchr::memchr(b'>', &html[abs_tag_start..]) {
                let abs_tag_end = abs_tag_start + tag_end + 1;
                let tag = &html[abs_tag_start..abs_tag_end];

                let cleaned = clean_tag(tag, linked_aids);
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
fn clean_tag(tag: &[u8], linked_aids: &HashSet<String>) -> Vec<u8> {
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
    let has_id = tag_has_id(tag);

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
        while i < tag.len() && tag[i] != b'=' && tag[i] != b' ' && tag[i] != b'>' && tag[i] != b'/'
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
            let mut value = &tag[i..i];
            if i < tag.len() && tag[i] == b'=' {
                i += 1;
                if i < tag.len() && (tag[i] == b'"' || tag[i] == b'\'') {
                    let quote = tag[i];
                    i += 1;
                    let value_start = i;
                    while i < tag.len() && tag[i] != quote {
                        i += 1;
                    }
                    value = &tag[value_start..i];
                    if i < tag.len() {
                        i += 1;
                    }
                } else {
                    let value_start = i;
                    while i < tag.len() && tag[i] != b' ' && tag[i] != b'>' {
                        i += 1;
                    }
                    value = &tag[value_start..i];
                }
            }

            // Keep a linked aid as the anchor its links resolved to
            if attr_name == b"aid"
                && !has_id
                && let Ok(value) = std::str::from_utf8(value)
                && linked_aids.contains(value)
            {
                result.extend_from_slice(b"id=\"aid-");
                result.extend_from_slice(value.as_bytes());
                result.push(b'"');
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

/// Drop stylesheet `<link>`s whose local href isn't a served asset.
///
/// The KF8 writer only turns primary `rel="stylesheet"` links into
/// `kindle:flow` references and embeds their CSS; `rel="alternate
/// stylesheet"` links (e.g. the horizontal variant of a vertical book) keep
/// their original EPUB path and their CSS isn't in the book at all.
pub fn drop_dangling_stylesheet_links(html: &[u8], assets: &[String]) -> Vec<u8> {
    let mut output = Vec::with_capacity(html.len());
    let mut pos = 0;

    while let Some(rel_start) = memchr::memchr(b'<', &html[pos..]) {
        let start = pos + rel_start;
        let Some(rel_end) = memchr::memchr(b'>', &html[start..]) else {
            break;
        };
        let end = start + rel_end + 1;
        let tag = &html[start..end];

        output.extend_from_slice(&html[pos..start]);
        if !is_dangling_stylesheet_link(tag, assets) {
            output.extend_from_slice(tag);
        }
        pos = end;
    }

    output.extend_from_slice(&html[pos..]);
    output
}

fn is_dangling_stylesheet_link(tag: &[u8], assets: &[String]) -> bool {
    if tag.len() < 6 || !tag[..6].eq_ignore_ascii_case(b"<link ") {
        return false;
    }
    let is_stylesheet = attr_value(tag, b"rel").is_some_and(|rel| {
        rel.to_ascii_lowercase()
            .split_str(" ")
            .any(|t| t == b"stylesheet")
    });
    let Some(href) = attr_value(tag, b"href") else {
        return false;
    };
    is_stylesheet && !href.contains_str(":") && !assets.iter().any(|a| a.as_bytes() == href)
}

/// The quoted value of attribute `name` (lowercase) in a tag, matched
/// case-insensitively.
fn attr_value<'a>(tag: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = memmem::find(&lower[from..], name) {
        let at = from + found;
        from = at + 1;
        let preceded = at > 0 && lower[at - 1].is_ascii_whitespace();
        let rest = &tag[at + name.len()..];
        if !preceded || !rest.starts_with(b"=") {
            continue;
        }
        let quote = *rest.get(1)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let value = &rest[2..];
        return Some(&value[..memchr::memchr(quote, value)?]);
    }
    None
}

/// Whether a tag carries its own `id` attribute.
fn tag_has_id(tag: &[u8]) -> bool {
    [b" id=", b"\tid=", b"\nid="]
        .iter()
        .any(|pat| memmem::find(tag, *pat).is_some())
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
        let output = strip_kindle_attributes_fast(input, &HashSet::new());
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
    fn test_linked_aid_becomes_id() {
        let linked = HashSet::from(["UGKE".to_string()]);
        let output = strip_kindle_attributes_fast(
            b"<h1 class=\"s13\" aid=\"UGKE\">A</h1><p aid=\"UGKF\">B</p>",
            &linked,
        );
        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("<h1 class=\"s13\" id=\"aid-UGKE\">A</h1>"));
        assert!(
            !output_str.contains("UGKF"),
            "unlinked aid kept: {output_str}"
        );
    }

    #[test]
    fn test_linked_aid_does_not_override_id() {
        let linked = HashSet::from(["UGKE".to_string()]);
        let output = strip_kindle_attributes_fast(b"<h1 aid=\"UGKE\" id=\"ch1\">A</h1>", &linked);
        let output_str = String::from_utf8_lossy(&output);
        assert!(!output_str.contains("aid"), "got: {output_str}");
        assert!(output_str.contains("id=\"ch1\""));
    }

    #[test]
    fn test_collect_linked_aids() {
        // A pos:fid link into an element whose nearest anchor is an aid
        let raw = b"<body><a href=\"kindle:pos:fid:0000:off:0000000000\">x</a><h1 aid=\"UGKE\">T</h1><p aid=\"UGKF\">B</p></body>";
        let target = raw.find(b"<h1").unwrap() as u32;
        let elems = [DivElement {
            insert_pos: target,
            toc_text: None,
            file_number: 0,
            sequence_number: 0,
            start_pos: 0,
            length: 0,
        }];
        let aids = collect_linked_aids(raw, &elems, &[(0, 0)]);
        assert_eq!(aids, HashSet::from(["UGKE".to_string()]));

        // The rewritten link and the kept id agree
        let linked = transform_kindle_refs(raw, &elems, raw, &[(0, 0)]);
        let cleaned = strip_kindle_attributes_fast(&linked, &aids);
        let cleaned = String::from_utf8_lossy(&cleaned);
        assert!(
            cleaned.contains("href=\"part0000.html#aid-UGKE\""),
            "{cleaned}"
        );
        assert!(cleaned.contains("<h1 id=\"aid-UGKE\">"), "{cleaned}");
    }

    #[test]
    fn test_aid_on_tag_with_id_resolves_to_id() {
        let raw = b"<body><div id=\"_idContainer002\" aid=\"1T142\"><h4>T</h4></div></body>";
        let pos = raw.find(b"<div").unwrap();
        let file_starts = [(0, 0)];
        // Forward search lands on the aid (no other anchor follows)
        assert_eq!(
            find_nearest_id_fast(raw, pos + 6, 0, &file_starts).as_deref(),
            Some("_idContainer002")
        );
        // Backward search from inside the div does too
        let inner = raw.find(b"<h4").unwrap();
        assert_eq!(
            find_nearest_id_fast(raw, inner, 0, &file_starts).as_deref(),
            Some("_idContainer002")
        );

        // Non-ASCII ids are kept as-is
        let raw = "<body><span aid=\"UGI2\" id=\"idまえがき\">T</span></body>".as_bytes();
        let pos = raw.find(b"<span").unwrap();
        assert_eq!(
            find_nearest_id_fast(raw, pos, 0, &file_starts).as_deref(),
            Some("idまえがき")
        );
    }

    #[test]
    fn test_drop_dangling_stylesheet_links() {
        let assets = vec!["styles/style0000.css".to_string()];
        let html =
            b"<head><link rel=\"stylesheet\" href=\"styles/style0000.css\" type=\"text/css\"/>\n\
<link rel=\"alternate stylesheet\" href=\"../Styles/TextHori.css\" class=\"horizontal\"/>\n\
<LINK class=\"horizontal\" REL=\"Alternate Stylesheet\" href='../styles/x_h.css'/>\n\
<link rel=\"stylesheet\" href=\"https://example.com/a.css\"/>\n\
<link rel=\"icon\" href=\"missing.png\"/></head>";
        let out = drop_dangling_stylesheet_links(html, &assets);
        let out = String::from_utf8_lossy(&out);
        assert!(out.contains("styles/style0000.css"), "{out}");
        assert!(!out.contains("TextHori"), "{out}");
        assert!(!out.contains("x_h.css"), "{out}");
        // External sheets and non-stylesheet links are not ours to judge
        assert!(out.contains("https://example.com/a.css"), "{out}");
        assert!(out.contains("missing.png"), "{out}");
    }

    #[test]
    fn test_img_alt() {
        let input = b"<img src=\"test.jpg\"/>";
        let output = ensure_img_alt(input);
        assert!(output.contains_str("alt=\"\""));
    }
}
