//! High-performance HTML/CSS transformation for MOBI writing.
//!
//! Uses single-pass processing and avoids allocations where possible.

use bstr::ByteSlice;
use memchr::memmem;
use std::collections::HashMap;

/// Fixed-size base32 encoding (no allocation)
/// Writes 4 digits to the provided slice, returns the slice
#[inline]
pub fn write_base32_4(num: usize, buf: &mut [u8; 4]) {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
    buf[3] = DIGITS[num % 32];
    buf[2] = DIGITS[(num / 32) % 32];
    buf[1] = DIGITS[(num / 1024) % 32];
    buf[0] = DIGITS[(num / 32768) % 32];
}

/// Fixed-size base32 encoding for 10 digits (link offsets)
#[inline]
pub fn write_base32_10(num: usize, buf: &mut [u8; 10]) {
    const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
    let mut n = num;
    for i in (0..10).rev() {
        buf[i] = DIGITS[n % 32];
        n /= 32;
    }
}

/// Information about a collected link for later resolution
pub struct CollectedLink {
    pub target_file: String,
    pub fragment: String,
}

/// Result of HTML rewriting
pub struct RewriteResult {
    pub html: Vec<u8>,
    pub links: Vec<CollectedLink>,
}

/// Single-pass HTML rewriting for kindle: references.
///
/// Rewrites:
/// - `<link href="...css">` → `kindle:flow:XXXX?mime=text/css`
/// - `<img src="...">` → `kindle:embed:XXXX?mime=...`
/// - `<a href="...">` (internal) → placeholder for later resolution
pub fn rewrite_html_references_fast(
    html: &[u8],
    html_href: &str,
    css_flow_map: &HashMap<String, usize>,
    resource_map: &HashMap<String, usize>,
    spine_hrefs: &std::collections::HashSet<&str>,
    book_resources: &HashMap<String, crate::book::Resource>,
    link_counter_start: usize,
) -> RewriteResult {
    let mut output = Vec::with_capacity(html.len() + html.len() / 10); // ~10% overhead
    let mut links = Vec::new();
    let mut link_counter = link_counter_start;
    let mut pos = 0;

    // Pre-compute base directory
    let base_dir = std::path::Path::new(html_href)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // Finders for tag detection
    let link_finder = memmem::Finder::new(b"<link ");
    let img_finder = memmem::Finder::new(b"<img ");
    let a_finder = memmem::Finder::new(b"<a ");

    while pos < html.len() {
        // Find next tag of interest
        let next_link = link_finder.find(&html[pos..]).map(|p| p + pos);
        let next_img = img_finder.find(&html[pos..]).map(|p| p + pos);
        let next_a = a_finder.find(&html[pos..]).map(|p| p + pos);

        // Find the earliest match
        let next_match = [next_link, next_img, next_a].into_iter().flatten().min();

        if let Some(tag_start) = next_match {
            // Copy content before this tag
            output.extend_from_slice(&html[pos..tag_start]);

            // Find tag end
            if let Some(tag_end_rel) = memchr::memchr(b'>', &html[tag_start..]) {
                let tag_end = tag_start + tag_end_rel + 1;
                let tag = &html[tag_start..tag_end];

                // Determine tag type and process
                if tag.starts_with(b"<link ") {
                    process_link_tag(tag, &base_dir, css_flow_map, &mut output);
                } else if tag.starts_with(b"<img ") {
                    process_img_tag(tag, &base_dir, resource_map, book_resources, &mut output);
                } else if tag.starts_with(b"<a ") {
                    process_anchor_tag(
                        tag,
                        html_href,
                        &base_dir,
                        spine_hrefs,
                        &mut output,
                        &mut links,
                        &mut link_counter,
                    );
                } else {
                    output.extend_from_slice(tag);
                }

                pos = tag_end;
            } else {
                // No closing >, copy rest
                output.extend_from_slice(&html[pos..]);
                break;
            }
        } else {
            // No more tags of interest
            output.extend_from_slice(&html[pos..]);
            break;
        }
    }

    RewriteResult {
        html: output,
        links,
    }
}

/// Process <link> tag, rewriting CSS href to kindle:flow
fn process_link_tag(
    tag: &[u8],
    base_dir: &str,
    css_flow_map: &HashMap<String, usize>,
    output: &mut Vec<u8>,
) {
    // Find href attribute
    if let Some(href_value) = extract_attribute_value(tag, b"href") {
        let href_str = String::from_utf8_lossy(href_value);
        let resolved = resolve_href(base_dir, &href_str);

        if let Some(&flow_idx) = css_flow_map.get(&resolved) {
            // Build new tag with kindle:flow reference
            let mut base32_buf = [0u8; 4];
            write_base32_4(flow_idx, &mut base32_buf);

            // Write tag with replaced href
            output.extend_from_slice(b"<link ");
            let mut wrote_href = false;

            // Copy attributes, replacing href
            for (name, value) in AttributeIter::new(tag) {
                if name == b"href" {
                    output.extend_from_slice(b"href=\"kindle:flow:");
                    output.extend_from_slice(&base32_buf);
                    output.extend_from_slice(b"?mime=text/css\" ");
                    wrote_href = true;
                } else {
                    output.extend_from_slice(name);
                    output.extend_from_slice(b"=\"");
                    output.extend_from_slice(value);
                    output.extend_from_slice(b"\" ");
                }
            }

            if !wrote_href {
                output.extend_from_slice(b"href=\"kindle:flow:");
                output.extend_from_slice(&base32_buf);
                output.extend_from_slice(b"?mime=text/css\" ");
            }

            // Close tag
            if tag.ends_with(b"/>") {
                output.extend_from_slice(b"/>");
            } else {
                output.push(b'>');
            }
            return;
        }
    }

    // No rewrite needed, copy as-is
    output.extend_from_slice(tag);
}

/// Process <img> tag, rewriting src to kindle:embed
fn process_img_tag(
    tag: &[u8],
    base_dir: &str,
    resource_map: &HashMap<String, usize>,
    book_resources: &HashMap<String, crate::book::Resource>,
    output: &mut Vec<u8>,
) {
    if let Some(src_value) = extract_attribute_value(tag, b"src") {
        let src_str = String::from_utf8_lossy(src_value);
        let resolved = resolve_href(base_dir, &src_str);

        if let Some(&res_idx) = resource_map.get(&resolved) {
            let mime = book_resources
                .get(&resolved)
                .map(|r| r.media_type.as_str())
                .unwrap_or("image/jpeg");

            let mut base32_buf = [0u8; 4];
            write_base32_4(res_idx, &mut base32_buf);

            // Build new tag
            output.extend_from_slice(b"<img ");

            for (name, value) in AttributeIter::new(tag) {
                if name == b"src" {
                    output.extend_from_slice(b"src=\"kindle:embed:");
                    output.extend_from_slice(&base32_buf);
                    output.extend_from_slice(b"?mime=");
                    output.extend_from_slice(mime.as_bytes());
                    output.extend_from_slice(b"\" ");
                } else {
                    output.extend_from_slice(name);
                    output.extend_from_slice(b"=\"");
                    output.extend_from_slice(value);
                    output.extend_from_slice(b"\" ");
                }
            }

            if tag.ends_with(b"/>") {
                output.extend_from_slice(b"/>");
            } else {
                output.push(b'>');
            }
            return;
        }
    }

    output.extend_from_slice(tag);
}

/// Process <a> tag, creating placeholder for internal links
fn process_anchor_tag(
    tag: &[u8],
    html_href: &str,
    base_dir: &str,
    spine_hrefs: &std::collections::HashSet<&str>,
    output: &mut Vec<u8>,
    links: &mut Vec<CollectedLink>,
    link_counter: &mut usize,
) {
    if let Some(href_value) = extract_attribute_value(tag, b"href") {
        let href_str = String::from_utf8_lossy(href_value);

        // Skip external links
        if href_str.starts_with("http")
            || href_str.starts_with("mailto:")
            || href_str.starts_with("kindle:")
        {
            output.extend_from_slice(tag);
            return;
        }

        // Parse target file and fragment
        let (target_file, fragment) = if let Some(hash_pos) = href_str.find('#') {
            let file_part = &href_str[..hash_pos];
            let frag_part = &href_str[hash_pos + 1..];
            if file_part.is_empty() {
                (html_href.to_string(), frag_part.to_string())
            } else {
                (resolve_href(base_dir, file_part), frag_part.to_string())
            }
        } else {
            (resolve_href(base_dir, &href_str), String::new())
        };

        // Check if internal link
        if spine_hrefs.contains(target_file.as_str()) {
            *link_counter += 1;

            // Create placeholder
            let mut base32_buf = [0u8; 10];
            write_base32_10(*link_counter, &mut base32_buf);

            // Build new tag
            output.extend_from_slice(b"<a ");

            for (name, value) in AttributeIter::new(tag) {
                if name == b"href" {
                    output.extend_from_slice(b"href=\"kindle:pos:fid:0000:off:");
                    output.extend_from_slice(&base32_buf);
                    output.extend_from_slice(b"\" ");
                } else {
                    output.extend_from_slice(name);
                    output.extend_from_slice(b"=\"");
                    output.extend_from_slice(value);
                    output.extend_from_slice(b"\" ");
                }
            }

            output.push(b'>');

            links.push(CollectedLink {
                target_file,
                fragment,
            });
            return;
        }
    }

    output.extend_from_slice(tag);
}

/// Extract attribute value from a tag
fn extract_attribute_value<'a>(tag: &'a [u8], attr_name: &[u8]) -> Option<&'a [u8]> {
    for (name, value) in AttributeIter::new(tag) {
        if name.eq_ignore_ascii_case(attr_name) {
            return Some(value);
        }
    }
    None
}

/// Iterator over attributes in a tag
struct AttributeIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> AttributeIter<'a> {
    fn new(tag: &'a [u8]) -> Self {
        // Skip tag name
        let mut pos = 0;
        while pos < tag.len() && tag[pos] != b' ' && tag[pos] != b'>' && tag[pos] != b'/' {
            pos += 1;
        }
        Self { data: tag, pos }
    }
}

impl<'a> Iterator for AttributeIter<'a> {
    type Item = (&'a [u8], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        // Skip whitespace
        while self.pos < self.data.len() && self.data[self.pos] == b' ' {
            self.pos += 1;
        }

        if self.pos >= self.data.len() || self.data[self.pos] == b'>' || self.data[self.pos] == b'/'
        {
            return None;
        }

        // Find attribute name end
        let name_start = self.pos;
        while self.pos < self.data.len()
            && self.data[self.pos] != b'='
            && self.data[self.pos] != b' '
            && self.data[self.pos] != b'>'
            && self.data[self.pos] != b'/'
        {
            self.pos += 1;
        }
        let name = &self.data[name_start..self.pos];

        // Skip to value
        if self.pos < self.data.len() && self.data[self.pos] == b'=' {
            self.pos += 1; // Skip '='

            // Get quote character
            if self.pos < self.data.len()
                && (self.data[self.pos] == b'"' || self.data[self.pos] == b'\'')
            {
                let quote = self.data[self.pos];
                self.pos += 1;
                let value_start = self.pos;

                // Find closing quote
                while self.pos < self.data.len() && self.data[self.pos] != quote {
                    self.pos += 1;
                }
                let value = &self.data[value_start..self.pos];

                if self.pos < self.data.len() {
                    self.pos += 1; // Skip closing quote
                }

                return Some((name, value));
            }
        }

        // Attribute without value
        Some((name, &[]))
    }
}

/// Resolve relative href against base directory
fn resolve_href(base_dir: &str, href: &str) -> String {
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_string();
    }

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

/// Single-pass CSS url() rewriting
pub fn rewrite_css_references_fast(css: &[u8], resource_map: &HashMap<String, usize>) -> Vec<u8> {
    let mut output = Vec::with_capacity(css.len());
    let mut pos = 0;

    let url_finder = memmem::Finder::new(b"url(");

    while let Some(url_start) = url_finder.find(&css[pos..]) {
        let abs_start = pos + url_start;

        // Copy content before url(
        output.extend_from_slice(&css[pos..abs_start]);

        // Find the closing )
        let content_start = abs_start + 4;
        if let Some(paren_end) = css[content_start..].find_byte(b')') {
            let url_content = &css[content_start..content_start + paren_end];

            // Strip quotes if present
            let url = url_content.trim_with(|c| c == '"' || c == '\'' || c == ' ');

            // Skip data: and http: URLs
            if url.starts_with(b"data:") || url.starts_with(b"http") {
                output.extend_from_slice(&css[abs_start..content_start + paren_end + 1]);
            } else {
                // Try to find resource
                let url_str = String::from_utf8_lossy(url);
                let normalized = url_str.trim_start_matches("../").trim_start_matches("./");

                let mut found = false;
                for (href, &res_idx) in resource_map {
                    if href.ends_with(normalized) || href == normalized {
                        let mut base32_buf = [0u8; 4];
                        write_base32_4(res_idx, &mut base32_buf);

                        output.extend_from_slice(b"url(kindle:embed:");
                        output.extend_from_slice(&base32_buf);
                        output.push(b')');
                        found = true;
                        break;
                    }
                }

                if !found {
                    output.extend_from_slice(&css[abs_start..content_start + paren_end + 1]);
                }
            }

            pos = content_start + paren_end + 1;
        } else {
            // No closing paren, copy as-is
            output.extend_from_slice(&css[pos..]);
            break;
        }
    }

    // Copy remaining content
    output.extend_from_slice(&css[pos..]);
    output
}

/// Optimized aid attribute insertion using byte operations
#[allow(dead_code)]
pub fn add_aid_attributes_fast(
    html: &[u8],
    file_href: &str,
    aid_counter: &mut u32,
    id_map: &mut HashMap<(String, String), String>,
) -> Vec<u8> {
    use super::skeleton::AID_ABLE_TAGS;

    let mut output = Vec::with_capacity(html.len() + html.len() / 5); // ~20% overhead for aids
    let mut pos = 0;

    while pos < html.len() {
        // Find next '<'
        if let Some(tag_start_rel) = memchr::memchr(b'<', &html[pos..]) {
            let tag_start = pos + tag_start_rel;

            // Copy content before tag
            output.extend_from_slice(&html[pos..tag_start]);

            // Check if this is a closing tag or comment
            if html.get(tag_start + 1) == Some(&b'/')
                || html.get(tag_start + 1) == Some(&b'!')
                || html.get(tag_start + 1) == Some(&b'?')
            {
                // Find end and copy as-is
                if let Some(tag_end_rel) = memchr::memchr(b'>', &html[tag_start..]) {
                    let tag_end = tag_start + tag_end_rel + 1;
                    output.extend_from_slice(&html[tag_start..tag_end]);
                    pos = tag_end;
                } else {
                    output.extend_from_slice(&html[tag_start..]);
                    break;
                }
                continue;
            }

            // Find tag name end
            let mut name_end = tag_start + 1;
            while name_end < html.len()
                && html[name_end] != b' '
                && html[name_end] != b'>'
                && html[name_end] != b'/'
            {
                name_end += 1;
            }

            let tag_name = &html[tag_start + 1..name_end];

            // Check if this is an aidable tag
            let tag_name_lower = tag_name.to_ascii_lowercase();
            let is_aidable = AID_ABLE_TAGS
                .iter()
                .any(|&t| t.as_bytes() == tag_name_lower.as_slice());

            // Find tag end
            if let Some(tag_end_rel) = memchr::memchr(b'>', &html[tag_start..]) {
                let tag_end = tag_start + tag_end_rel + 1;
                let tag = &html[tag_start..tag_end];

                if is_aidable && !tag.find(b"aid=").is_some() {
                    // Generate aid
                    let mut aid_buf = [0u8; 4];
                    write_base32_4(*aid_counter as usize, &mut aid_buf);
                    let aid_str = std::str::from_utf8(&aid_buf).unwrap().to_string();
                    *aid_counter += 1;

                    // Extract id if present
                    if let Some(id_value) = extract_attribute_value(tag, b"id") {
                        let id_str = String::from_utf8_lossy(id_value).to_string();
                        id_map.insert((file_href.to_string(), id_str), aid_str.clone());
                    }

                    // For body tag, also map empty fragment
                    if tag_name_lower == b"body" {
                        id_map.insert((file_href.to_string(), String::new()), aid_str.clone());
                    }

                    // Write tag with aid
                    output.push(b'<');
                    output.extend_from_slice(tag_name);

                    // Check if there's a space after tag name
                    if name_end < tag_end - 1 && html[name_end] == b' ' {
                        output.extend_from_slice(&html[name_end..tag_end - 1]);
                        output.extend_from_slice(b" aid=\"");
                    } else {
                        output.extend_from_slice(b" aid=\"");
                    }
                    output.extend_from_slice(&aid_buf);
                    output.push(b'"');

                    // Copy closing
                    if tag.ends_with(b"/>") {
                        output.extend_from_slice(b"/>");
                    } else {
                        output.push(b'>');
                    }
                } else {
                    output.extend_from_slice(tag);
                }

                pos = tag_end;
            } else {
                output.extend_from_slice(&html[tag_start..]);
                break;
            }
        } else {
            // No more tags
            output.extend_from_slice(&html[pos..]);
            break;
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_base32_4() {
        let mut buf = [0u8; 4];
        write_base32_4(0, &mut buf);
        assert_eq!(&buf, b"0000");

        write_base32_4(1, &mut buf);
        assert_eq!(&buf, b"0001");

        write_base32_4(32, &mut buf);
        assert_eq!(&buf, b"0010");
    }

    #[test]
    fn test_attribute_iter() {
        let tag = b"<img src=\"test.jpg\" alt=\"hello\" />";
        let attrs: Vec<_> = AttributeIter::new(tag).collect();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].0, b"src");
        assert_eq!(attrs[0].1, b"test.jpg");
    }

    #[test]
    fn test_resolve_href() {
        assert_eq!(
            resolve_href("chapter", "../images/test.jpg"),
            "images/test.jpg"
        );
        assert_eq!(resolve_href("", "test.html"), "test.html");
    }
}
