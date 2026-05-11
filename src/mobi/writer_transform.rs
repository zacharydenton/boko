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
    book_resources: &HashMap<String, crate::model::Resource>,
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

    // No matching CSS flow. Don't pass the original `<link href="...css">`
    // through — its href would be a relative path Kindle can't resolve,
    // and stale stylesheet references can stall the layout engine waiting
    // for the resource. Calibre's pipeline never emits unresolved `<link>`
    // tags into the rawml. Drop it.
    let _ = tag;
}

/// Process <img> tag, rewriting src to kindle:embed
fn process_img_tag(
    tag: &[u8],
    base_dir: &str,
    resource_map: &HashMap<String, usize>,
    book_resources: &HashMap<String, crate::model::Resource>,
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
    // Strip CSS at-rules that Kindle's parser doesn't support: `@charset`,
    // `@namespace`. Calibre normalises these away in its OEB transform
    // pipeline; leaving them in is correlated with the renderer freezing
    // on otherwise-valid books.
    let css = strip_unsupported_css_at_rules(css);

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

/// Strip `@charset "..."` and `@namespace ...` at-rules from CSS.
///
/// Kindle's CSS parser (KF8 rendering engine) doesn't implement CSS
/// namespaces and gets unhappy with `@charset` directives; calibre's
/// `mobiml.py` removes both as part of its normalisation pass before
/// shipping CSS into the KF8 stream.
fn strip_unsupported_css_at_rules(css: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(css.len());
    let mut i = 0;
    while i < css.len() {
        if css[i] == b'@' {
            let rest = &css[i..];
            let matches_charset = rest.len() >= 8
                && rest[..8].eq_ignore_ascii_case(b"@charset");
            let matches_namespace = rest.len() >= 10
                && rest[..10].eq_ignore_ascii_case(b"@namespace");

            if matches_charset || matches_namespace {
                // Drop the rule up to and including the next `;`. (Both
                // `@charset` and `@namespace` are simple at-rules
                // terminated by `;`.)
                let mut j = i + if matches_charset { 8 } else { 10 };
                while j < css.len() && css[j] != b';' {
                    j += 1;
                }
                if j < css.len() {
                    j += 1; // consume the `;`
                }
                // Also swallow any trailing whitespace so we don't leave
                // a gap of blank lines where the rule was.
                while j < css.len() && matches!(css[j], b' ' | b'\t' | b'\r' | b'\n') {
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        output.push(css[i]);
        i += 1;
    }
    output
}

/// Result of aid attribute insertion
pub struct AidInsertResult {
    /// The HTML with aid attributes added
    pub html: Vec<u8>,
    /// Mapping of original byte position -> aid for filepos resolution
    pub position_map: Vec<(usize, String)>,
}

/// Strip XML namespace declarations and namespaced attributes that confuse
/// the Kindle HTML5 parser.
///
/// Calibre's KF8 writer explicitly does this (see `writer8/skeleton.py:remove_namespaces`)
/// because Kindle firmware refuses to render documents with EPUB3-style
/// prefixed attributes such as `xmlns:epub`, `epub:prefix`, `epub:type`, or
/// `xml:lang`. Leaving them in produces files that pass libmobi parsing but
/// still get "Unable to Open Item" on the device.
///
/// We strip:
///   - Any `xmlns:NAME="..."` namespace declaration except the default xhtml one.
///   - Any attribute with a prefix in {`epub`, `xml`, `opf`, `dc`, `dcterms`,
///     `xsi`}, e.g. `epub:type="..."`, `xml:lang="..."`.
///
/// The default xhtml `xmlns="..."` is preserved.
pub fn strip_xml_namespaces(html: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(html.len());
    let mut pos = 0;

    while pos < html.len() {
        let Some(rel) = memchr::memchr(b'<', &html[pos..]) else {
            out.extend_from_slice(&html[pos..]);
            break;
        };
        let tag_start = pos + rel;
        out.extend_from_slice(&html[pos..tag_start]);

        // Closing tags, comments, processing instructions: copy unchanged.
        // DOCTYPE declarations: drop entirely — Kindle's HTML5 parser refuses
        // to load external DTDs (e.g. XHTML 1.1's
        // `http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd`) and calibre's KF8
        // writer never emits a DOCTYPE for the same reason.
        if matches!(html.get(tag_start + 1), Some(&b'/') | Some(&b'!') | Some(&b'?')) {
            let Some(end_rel) = memchr::memchr(b'>', &html[tag_start..]) else {
                out.extend_from_slice(&html[tag_start..]);
                break;
            };
            let tag_end = tag_start + end_rel + 1;
            let is_doctype = html
                .get(tag_start + 2..tag_start + 9)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"DOCTYPE"));
            if !is_doctype {
                out.extend_from_slice(&html[tag_start..tag_end]);
            }
            pos = tag_end;
            continue;
        }

        // Find tag end. Honor quoted attribute values so a `>` inside a value
        // doesn't terminate early.
        let mut i = tag_start + 1;
        let mut in_quote: Option<u8> = None;
        while i < html.len() {
            let c = html[i];
            match in_quote {
                Some(q) if c == q => in_quote = None,
                None => match c {
                    b'"' | b'\'' => in_quote = Some(c),
                    b'>' => break,
                    _ => {}
                },
                _ => {}
            }
            i += 1;
        }
        if i >= html.len() {
            out.extend_from_slice(&html[tag_start..]);
            break;
        }
        let tag_end = i + 1;

        out.push(b'<');
        let mut p = tag_start + 1;

        // Copy tag name (up to first whitespace, `/`, or `>`).
        while p < tag_end - 1
            && !matches!(
                html[p],
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>'
            )
        {
            out.push(html[p]);
            p += 1;
        }

        // Walk attributes. For each, decide keep vs drop based on its name.
        while p < tag_end - 1 {
            // Copy whitespace.
            let ws_start = p;
            while p < tag_end - 1
                && matches!(html[p], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
            {
                p += 1;
            }
            if p >= tag_end - 1 {
                out.extend_from_slice(&html[ws_start..p]);
                break;
            }
            // Self-close or end of tag.
            if html[p] == b'/' || html[p] == b'>' {
                out.extend_from_slice(&html[ws_start..p]);
                break;
            }

            // Parse attribute name.
            let name_start = p;
            while p < tag_end - 1
                && !matches!(
                    html[p],
                    b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'=' | b'/' | b'>'
                )
            {
                p += 1;
            }
            let name = &html[name_start..p];

            // Parse optional value.
            let value_end;
            if p < tag_end - 1 && html[p] == b'=' {
                p += 1;
                // Skip whitespace before value.
                while p < tag_end - 1
                    && matches!(html[p], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
                {
                    p += 1;
                }
                if p < tag_end - 1 && (html[p] == b'"' || html[p] == b'\'') {
                    let q = html[p];
                    p += 1;
                    while p < tag_end - 1 && html[p] != q {
                        p += 1;
                    }
                    if p < tag_end - 1 {
                        p += 1; // consume closing quote
                    }
                } else {
                    // Unquoted value: read until whitespace, `/`, or `>`.
                    while p < tag_end - 1
                        && !matches!(
                            html[p],
                            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>'
                        )
                    {
                        p += 1;
                    }
                }
                value_end = p;
            } else {
                value_end = p;
            }

            // Decide whether to drop this attribute.
            let drop = should_drop_attr(name);

            if !drop {
                out.extend_from_slice(&html[ws_start..value_end]);
            }
        }

        // Copy whatever's left of the tag (self-close marker, `>`).
        out.extend_from_slice(&html[p..tag_end]);
        pos = tag_end;
    }

    out
}

fn should_drop_attr(name: &[u8]) -> bool {
    // Drop `xmlns:NAME` (but keep bare `xmlns`).
    if name.len() > 6 && name[..6].eq_ignore_ascii_case(b"xmlns:") {
        return true;
    }
    // Drop attributes with these prefixes.
    const DROP_PREFIXES: &[&[u8]] = &[
        b"epub:", b"opf:", b"dc:", b"dcterms:", b"xsi:", b"xml:",
        // ARIA — Kindle's KF8 parser doesn't implement WAI-ARIA, and
        // ARIA-DPUB role values (`doc-noteref`, `doc-chapter`, etc.) come
        // from the same EPUB3 vocabulary family as `epub:type`. Standard
        // Ebooks files emit them on every link/section. Stripping them
        // here matches the same hypothesis that motivated stripping
        // `epub:type`: same firmware sensitivity class.
        b"aria-",
    ];
    for prefix in DROP_PREFIXES {
        if name.len() > prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return true;
        }
    }
    // Bare `role="..."` (no prefix). Drop too — same reasoning as above.
    if name.eq_ignore_ascii_case(b"role") {
        return true;
    }
    false
}

/// Optimized aid attribute insertion using byte operations
///
/// Returns the transformed HTML and a position map for filepos resolution.
pub fn add_aid_attributes_fast(
    html: &[u8],
    file_href: &str,
    aid_counter: &mut u32,
    id_map: &mut HashMap<(String, String), String>,
) -> AidInsertResult {
    use super::skeleton::AID_ABLE_TAGS;

    let mut output = Vec::with_capacity(html.len() + html.len() / 5); // ~20% overhead for aids
    let mut position_map = Vec::new();
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
                && html[name_end] != b'\t'
                && html[name_end] != b'\n'
                && html[name_end] != b'\r'
                && html[name_end] != b'\x0c'
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

                if is_aidable && tag.find(b"aid=").is_none() {
                    // Generate aid
                    let mut aid_buf = [0u8; 4];
                    write_base32_4(*aid_counter as usize, &mut aid_buf);
                    let aid_str = std::str::from_utf8(&aid_buf).unwrap().to_string();
                    *aid_counter += 1;

                    // Record original position -> aid mapping for filepos resolution
                    position_map.push((tag_start, aid_str.clone()));

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

                    // Check if there are attributes or whitespace after tag name
                    // tag_end points to the character AFTER '>', so tag_end - 1 is '>'
                    let is_self_closing = tag.ends_with(b"/>");
                    // For self-closing tags, exclude the trailing "/" from attributes
                    let attr_end = if is_self_closing {
                        tag_end - 2 // Before "/>"
                    } else {
                        tag_end - 1 // Before ">"
                    };

                    if name_end < attr_end {
                        // Copy existing attributes/whitespace, preserving format
                        // This handles <div class="..."> and <div\nclass="...">
                        output.extend_from_slice(&html[name_end..attr_end]);
                        output.extend_from_slice(b" aid=\"");
                    } else {
                        // No attributes, just add aid
                        output.extend_from_slice(b" aid=\"");
                    }
                    output.extend_from_slice(&aid_buf);
                    output.push(b'"');

                    // Copy closing
                    if is_self_closing {
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

    AidInsertResult {
        html: output,
        position_map,
    }
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

    #[test]
    fn test_add_aid_self_closing() {
        use bstr::ByteSlice;

        let html = b"<a id=\"tp\"/>";
        let mut aid_counter = 0u32;
        let mut id_map = HashMap::new();
        let result = add_aid_attributes_fast(html, "test.html", &mut aid_counter, &mut id_map);
        let result_str = String::from_utf8_lossy(&result.html);

        // Should produce <a id="tp" aid="0000"/> not <a id="tp"/ aid="0000"/>
        assert!(
            result_str.contains("id=\"tp\" aid="),
            "aid should come after id, not after /: {result_str}"
        );
        assert!(
            !result_str.contains("\"/ aid"),
            "should not have stray / before aid: {result_str}"
        );
        assert!(
            result.html.ends_with_str(b"/>"),
            "should end with />: {result_str}"
        );
    }
}
