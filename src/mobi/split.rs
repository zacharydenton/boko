//! Splitting of monolithic MOBI HTML into per-chapter documents.

use std::collections::HashMap;

/// Result of splitting MOBI HTML into chapters.
pub(crate) struct ChapterSplit {
    /// Split chapter content (complete XHTML documents).
    pub(crate) chapters: Vec<Vec<u8>>,
    /// Chapter file paths.
    pub(crate) chapter_paths: Vec<String>,
    /// Maps "fileposN" → chapter index.
    pub(crate) filepos_to_chapter: HashMap<String, usize>,
}

/// Split transformed MOBI HTML into chapters at `<mbp:pagebreak>` boundaries.
///
/// Falls back to NCX position-based splitting if no pagebreaks are found.
/// Falls back to a single chapter if neither pagebreaks nor NCX positions exist.
pub(crate) fn split_mobi_html(html: &[u8], ncx_positions: Option<&[u32]>) -> ChapterSplit {
    let html_str = String::from_utf8_lossy(html);

    // Extract <head> content and <body> content
    let (head_content, body_content) = extract_head_and_body(&html_str);

    // Find pagebreak positions in the body content
    let pagebreak_positions = find_pagebreaks(body_content.as_bytes());

    // Split body: pagebreaks first, NCX fallback, then single chapter
    let body_chunks = if !pagebreak_positions.is_empty() {
        split_at_pagebreaks(&body_content, &pagebreak_positions)
    } else if let Some(positions) = ncx_positions {
        let ncx_chunks = split_at_ncx_anchors(&body_content, positions);
        if ncx_chunks.len() > 1 {
            ncx_chunks
        } else {
            vec![body_content.to_string()]
        }
    } else {
        vec![body_content.to_string()]
    };

    // Filter out empty chunks
    let body_chunks: Vec<String> = body_chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect();

    // Build chapter documents and filepos map
    let mut chapters = Vec::with_capacity(body_chunks.len());
    let mut chapter_paths = Vec::with_capacity(body_chunks.len());
    let mut filepos_to_chapter: HashMap<String, usize> = HashMap::new();

    for (i, chunk) in body_chunks.iter().enumerate() {
        let chapter_path = format!("chapter_{}.xhtml", i);
        chapter_paths.push(chapter_path);

        // Scan this chunk for filepos anchors and record their chapter
        collect_filepos_anchors(chunk, i, &mut filepos_to_chapter);

        // Wrap chunk as complete XHTML
        let doc = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <!DOCTYPE html>\n\
             <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
             <head>\n{}</head>\n\
             <body>\n{}\n</body>\n\
             </html>",
            head_content, chunk
        );
        chapters.push(doc.into_bytes());
    }

    // Rewrite cross-chapter links
    rewrite_cross_chapter_links(&mut chapters, &filepos_to_chapter, &chapter_paths);

    // Neutralize bare filename links (OEB source references that don't exist in EPUB)
    neutralize_bare_filename_links(&mut chapters);

    // Ensure at least one chapter
    if chapters.is_empty() {
        chapters.push(html.to_vec());
        chapter_paths.push("chapter_0.xhtml".to_string());
    }

    ChapterSplit {
        chapters,
        chapter_paths,
        filepos_to_chapter,
    }
}

/// Split MOBI HTML using only NCX positions, bypassing pagebreak detection.
///
/// Used when pagebreak-based splitting fails to produce multiple chapters
/// but NCX index entries provide valid split points.
pub(crate) fn split_mobi_html_ncx_only(html: &[u8], ncx_positions: &[u32]) -> ChapterSplit {
    let html_str = String::from_utf8_lossy(html);
    let (head_content, body_content) = extract_head_and_body(&html_str);

    let body_chunks = {
        let ncx_chunks = split_at_ncx_anchors(&body_content, ncx_positions);
        if ncx_chunks.len() > 1 {
            ncx_chunks
        } else {
            vec![body_content.to_string()]
        }
    };

    // Filter out empty chunks
    let body_chunks: Vec<String> = body_chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect();

    // Build chapter documents and filepos map
    let mut chapters = Vec::with_capacity(body_chunks.len());
    let mut chapter_paths = Vec::with_capacity(body_chunks.len());
    let mut filepos_to_chapter: HashMap<String, usize> = HashMap::new();

    for (i, chunk) in body_chunks.iter().enumerate() {
        let chapter_path = format!("chapter_{}.xhtml", i);
        chapter_paths.push(chapter_path);
        collect_filepos_anchors(chunk, i, &mut filepos_to_chapter);

        let doc = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <!DOCTYPE html>\n\
             <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
             <head>\n{}</head>\n\
             <body>\n{}\n</body>\n\
             </html>",
            head_content, chunk
        );
        chapters.push(doc.into_bytes());
    }

    rewrite_cross_chapter_links(&mut chapters, &filepos_to_chapter, &chapter_paths);
    neutralize_bare_filename_links(&mut chapters);

    if chapters.is_empty() {
        chapters.push(html.to_vec());
        chapter_paths.push("chapter_0.xhtml".to_string());
    }

    ChapterSplit {
        chapters,
        chapter_paths,
        filepos_to_chapter,
    }
}

/// Extract the content inside `<head>...</head>` and `<body>...</body>`.
///
/// Returns (head_inner, body_inner). If tags aren't found, returns reasonable
/// defaults.
fn extract_head_and_body(html: &str) -> (String, String) {
    let html_lower = html.to_ascii_lowercase();

    // Find <head> content
    let head_content = if let Some(head_start) = html_lower.find("<head") {
        let after_tag = html[head_start..].find('>').map(|p| head_start + p + 1);
        let head_end = html_lower.find("</head>");
        match (after_tag, head_end) {
            (Some(start), Some(end)) if start <= end => html[start..end].to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };

    // Find <body> content
    let body_content = if let Some(body_start) = html_lower.find("<body") {
        let after_tag = html[body_start..].find('>').map(|p| body_start + p + 1);
        let body_end = html_lower.rfind("</body>");
        match (after_tag, body_end) {
            (Some(start), Some(end)) if start <= end => html[start..end].to_string(),
            (Some(start), None) => html[start..].to_string(),
            _ => html.to_string(),
        }
    } else {
        html.to_string()
    };

    (head_content, body_content)
}

/// A pagebreak location: byte range of the `<mbp:pagebreak...>` tag in the body.
struct PagebreakPos {
    /// Start byte offset of the `<` character.
    start: usize,
    /// End byte offset (one past the `>` character).
    end: usize,
}

/// Find all `<mbp:pagebreak...>` tags in body content.
///
/// Matches variants: `<mbp:pagebreak/>`, `<mbp:pagebreak />`,
/// `<mbp:pagebreak>`, with optional attributes, case-insensitive.
fn find_pagebreaks(body: &[u8]) -> Vec<PagebreakPos> {
    let mut results = Vec::new();
    let body_lower: Vec<u8> = body.iter().map(|b| b.to_ascii_lowercase()).collect();
    let needle = b"<mbp:pagebreak";

    let mut pos = 0;
    while pos + needle.len() < body_lower.len() {
        if let Some(rel) = body_lower[pos..]
            .windows(needle.len())
            .position(|w| w == needle)
        {
            let tag_start = pos + rel;
            // Find the closing > for this tag
            if let Some(close_rel) = body[tag_start..].iter().position(|&b| b == b'>') {
                let tag_end = tag_start + close_rel + 1;
                results.push(PagebreakPos {
                    start: tag_start,
                    end: tag_end,
                });
                pos = tag_end;
            } else {
                pos = tag_start + needle.len();
            }
        } else {
            break;
        }
    }

    results
}

/// Split body content at pagebreak positions.
///
/// The pagebreak tags themselves are removed. Content before the first
/// pagebreak becomes the first chunk, etc.
fn split_at_pagebreaks(body: &str, pagebreaks: &[PagebreakPos]) -> Vec<String> {
    let mut chunks = Vec::with_capacity(pagebreaks.len() + 1);
    let mut last_end = 0;

    for pb in pagebreaks {
        chunks.push(body[last_end..pb.start].to_string());
        last_end = pb.end;
    }

    // Content after the last pagebreak
    chunks.push(body[last_end..].to_string());

    chunks
}

/// Scan a chapter chunk for `<a id="fileposN"` anchors and record them.
fn collect_filepos_anchors(chunk: &str, chapter_idx: usize, map: &mut HashMap<String, usize>) {
    let needle = "id=\"filepos";
    let mut search_pos = 0;

    while let Some(rel) = chunk[search_pos..].find(needle) {
        let value_start = search_pos + rel + needle.len();
        // Read digits until closing quote
        let value_end = chunk[value_start..]
            .find('"')
            .map(|p| value_start + p)
            .unwrap_or(value_start);

        if value_end > value_start {
            let filepos_key = format!("filepos{}", &chunk[value_start..value_end]);
            map.insert(filepos_key, chapter_idx);
        }

        search_pos = value_end + 1;
        if search_pos >= chunk.len() {
            break;
        }
    }
}

/// Rewrite `href="#fileposN"` links that point to anchors in other chapters.
///
/// If the target filepos is in a different chapter, rewrites to
/// `href="chapter_M.xhtml#fileposN"`.
fn rewrite_cross_chapter_links(
    chapters: &mut [Vec<u8>],
    filepos_to_chapter: &HashMap<String, usize>,
    chapter_paths: &[String],
) {
    let needle = b"href=\"#filepos";

    for (chapter_idx, chapter) in chapters.iter_mut().enumerate() {
        let mut output = Vec::with_capacity(chapter.len());
        let mut pos = 0;

        while pos < chapter.len() {
            if pos + needle.len() < chapter.len() && chapter[pos..].starts_with(needle) {
                // Found href="#filepos...", extract the filepos key
                let value_start = pos + b"href=\"#".len();
                let quote_end = chapter[value_start..]
                    .iter()
                    .position(|&b| b == b'"')
                    .map(|p| value_start + p);

                if let Some(end) = quote_end {
                    let filepos_key =
                        String::from_utf8_lossy(&chapter[value_start..end]).to_string();
                    let target_chapter = filepos_to_chapter
                        .get(&filepos_key)
                        .copied()
                        .unwrap_or(chapter_idx);

                    if target_chapter != chapter_idx {
                        // Cross-chapter link: rewrite
                        output.extend_from_slice(b"href=\"");
                        output.extend_from_slice(chapter_paths[target_chapter].as_bytes());
                        output.push(b'#');
                        output.extend_from_slice(filepos_key.as_bytes());
                        output.push(b'"');
                    } else {
                        // Same chapter: keep as-is
                        output.extend_from_slice(&chapter[pos..end + 1]);
                    }
                    pos = end + 1;
                    continue;
                }
            }

            output.push(chapter[pos]);
            pos += 1;
        }

        *chapter = output;
    }
}

/// Split body content at NCX anchor positions.
///
/// Finds `id="fileposN"` attributes in the body for each NCX position,
/// locates the enclosing `<a` tag, and splits just before it.
/// Content before the first anchor becomes the first chunk (preamble/front matter).
///
/// Handles both inserted anchors (`<a id="fileposN" />`) and pre-existing
/// anchors where `id` isn't the first attribute (`<a class="c1" id="fileposN">`).
fn split_at_ncx_anchors(body: &str, positions: &[u32]) -> Vec<String> {
    if positions.is_empty() {
        return vec![body.to_string()];
    }

    let body_bytes = body.as_bytes();

    // Find byte offsets of each NCX anchor in the body
    let mut split_offsets = Vec::new();
    for &pos in positions {
        let needle = format!("id=\"filepos{}\"", pos);
        if let Some(id_offset) = body.find(&needle) {
            // Scan backward to find the opening '<' of the enclosing tag
            let tag_start = body_bytes[..id_offset]
                .iter()
                .rposition(|&b| b == b'<')
                .unwrap_or(id_offset);
            if tag_start > 0 {
                split_offsets.push(tag_start);
            }
        }
    }

    split_offsets.sort_unstable();
    split_offsets.dedup();

    if split_offsets.is_empty() {
        return vec![body.to_string()];
    }

    let mut chunks = Vec::with_capacity(split_offsets.len() + 1);
    let mut last_end = 0;

    for &offset in &split_offsets {
        if offset > last_end {
            chunks.push(body[last_end..offset].to_string());
        }
        last_end = offset;
    }

    // Content after the last split point
    if last_end < body.len() {
        chunks.push(body[last_end..].to_string());
    }

    chunks
}

/// Neutralize bare filename links that reference OEB source files.
///
/// Some older MOBI files retain original OEB package filenames as `href` values
/// (e.g. `HREF="cover.htm"`, `HREF="Book_oeb_01_r1.html"`). These use uppercase
/// `HREF` and coexist with a lowercase `href="#fileposN"` on the same tag.
/// Since HTML parsers take the first attribute, the uppercase OEB link wins.
///
/// This function removes the entire `HREF="filename.html"` attribute (case-
/// insensitive) when it points to a bare filename, letting the correct lowercase
/// `href="#fileposN"` take effect. Falls back to replacing with `href="#"` if
/// there's only one href attribute.
fn neutralize_bare_filename_links(chapters: &mut [Vec<u8>]) {
    for chapter in chapters.iter_mut() {
        let mut output = Vec::with_capacity(chapter.len());
        let mut pos = 0;

        while pos < chapter.len() {
            // Case-insensitive match for href=" (handles HREF=", Href=", etc.)
            if pos + 6 <= chapter.len()
                && chapter[pos..pos + 5].eq_ignore_ascii_case(b"href=")
                && chapter[pos + 5] == b'"'
            {
                let value_start = pos + 6;
                if let Some(quote_rel) = chapter[value_start..].iter().position(|&b| b == b'"') {
                    let value = &chapter[value_start..value_start + quote_rel];
                    if is_bare_filename_link(value) {
                        let attr_end = value_start + quote_rel + 1; // past closing "

                        // Check if there's already a lowercase href on this tag
                        // by looking ahead in the same tag for href="# or href="chapter_
                        let remaining_tag = &chapter[attr_end..];
                        let has_correct_href = remaining_tag
                            .windows(6)
                            .take_while(|w| !w.starts_with(b">") && !w.starts_with(b"<"))
                            .any(|w| w == b"href=\"");

                        if has_correct_href {
                            // Remove the OEB HREF attribute entirely (skip it)
                            // Also skip trailing whitespace
                            pos = attr_end;
                            while pos < chapter.len() && chapter[pos] == b' ' {
                                pos += 1;
                            }
                            continue;
                        } else {
                            // No correct href follows — neutralize to href="#"
                            output.extend_from_slice(b"href=\"#\"");
                            pos = attr_end;
                            continue;
                        }
                    }
                }
            }

            output.push(chapter[pos]);
            pos += 1;
        }

        *chapter = output;
    }
}

/// Check if an href value is a bare filename link to an .htm/.html file.
///
/// Returns true for values like `cover.htm`, `Book_oeb_01_r1.html`,
/// `Book_oeb_ftn_r1.html#f1` (with fragment).
/// Returns false for `#filepos123`, `http://...`, `chapter_0.xhtml`, etc.
fn is_bare_filename_link(href: &[u8]) -> bool {
    let href_str = String::from_utf8_lossy(href);
    // Strip fragment for extension check
    let path_part = href_str.split('#').next().unwrap_or(&href_str);
    let path_lower = path_part.to_ascii_lowercase();

    (path_lower.ends_with(".htm") || path_lower.ends_with(".html"))
        && !href_str.starts_with('#')
        && !href_str.contains("://")
        && !path_lower.ends_with(".xhtml")
}

// ============================================================================
// Helpers
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_head_and_body() {
        let html = r#"<html><head><title>Test</title><link rel="stylesheet" href="style.css"/></head><body><p>Hello</p></body></html>"#;
        let (head, body) = extract_head_and_body(html);
        assert!(head.contains("<title>Test</title>"));
        assert!(head.contains("style.css"));
        assert_eq!(body, "<p>Hello</p>");
    }

    #[test]
    fn test_extract_head_and_body_no_tags() {
        let html = "<p>Just content</p>";
        let (head, body) = extract_head_and_body(html);
        assert!(head.is_empty());
        assert_eq!(body, html);
    }

    #[test]
    fn test_find_pagebreaks() {
        let body = b"<p>Ch1</p><mbp:pagebreak/><p>Ch2</p><mbp:pagebreak /><p>Ch3</p>";
        let pbs = find_pagebreaks(body);
        assert_eq!(pbs.len(), 2);
        assert_eq!(&body[pbs[0].start..pbs[0].end], b"<mbp:pagebreak/>");
        assert_eq!(&body[pbs[1].start..pbs[1].end], b"<mbp:pagebreak />");
    }

    #[test]
    fn test_find_pagebreaks_case_insensitive() {
        let body = b"<p>A</p><MBP:PAGEBREAK/><p>B</p>";
        let pbs = find_pagebreaks(body);
        assert_eq!(pbs.len(), 1);
    }

    #[test]
    fn test_find_pagebreaks_with_attributes() {
        let body = b"<p>A</p><mbp:pagebreak kindle:kindlefix=\"true\"/><p>B</p>";
        let pbs = find_pagebreaks(body);
        assert_eq!(pbs.len(), 1);
    }

    #[test]
    fn test_find_pagebreaks_none() {
        let body = b"<p>No breaks here</p>";
        let pbs = find_pagebreaks(body);
        assert!(pbs.is_empty());
    }

    #[test]
    fn test_split_at_pagebreaks() {
        let body = "<p>Ch1</p><mbp:pagebreak/><p>Ch2</p><mbp:pagebreak /><p>Ch3</p>";
        let pbs = find_pagebreaks(body.as_bytes());
        let chunks = split_at_pagebreaks(body, &pbs);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], "<p>Ch1</p>");
        assert_eq!(chunks[1], "<p>Ch2</p>");
        assert_eq!(chunks[2], "<p>Ch3</p>");
    }

    #[test]
    fn test_split_mobi_html_with_pagebreaks() {
        let html = br#"<html><head><title>T</title></head><body>
<h1>Chapter 1</h1><p>Text1</p>
<mbp:pagebreak/>
<h1>Chapter 2</h1><p>Text2</p>
<mbp:pagebreak/>
<h1>Chapter 3</h1><p>Text3</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapter_paths.len(), 3);
        assert_eq!(split.chapter_paths[0], "chapter_0.xhtml");
        assert_eq!(split.chapter_paths[1], "chapter_1.xhtml");
        assert_eq!(split.chapter_paths[2], "chapter_2.xhtml");

        // Each chapter should be a complete XHTML document
        for ch in &split.chapters {
            let s = String::from_utf8_lossy(ch);
            assert!(s.contains("<html"), "Missing <html>: {}", s);
            assert!(s.contains("</html>"), "Missing </html>: {}", s);
            assert!(s.contains("<head>"), "Missing <head>: {}", s);
            assert!(s.contains("<body>"), "Missing <body>: {}", s);
        }

        // Check content
        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        let ch1 = String::from_utf8_lossy(&split.chapters[1]);
        let ch2 = String::from_utf8_lossy(&split.chapters[2]);
        assert!(ch0.contains("Chapter 1"));
        assert!(ch1.contains("Chapter 2"));
        assert!(ch2.contains("Chapter 3"));
    }

    #[test]
    fn test_split_mobi_html_no_pagebreaks() {
        let html = b"<html><head></head><body><p>Single chapter</p></body></html>";
        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 1);
        assert_eq!(split.chapter_paths[0], "chapter_0.xhtml");

        let ch = String::from_utf8_lossy(&split.chapters[0]);
        assert!(ch.contains("Single chapter"));
    }

    #[test]
    fn test_split_mobi_html_empty_chunks_filtered() {
        // Pagebreak at very start → first chunk is empty → filtered out
        let html = b"<html><head></head><body><mbp:pagebreak/><p>Only chapter</p></body></html>";
        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 1);
        let ch = String::from_utf8_lossy(&split.chapters[0]);
        assert!(ch.contains("Only chapter"));
    }

    #[test]
    fn test_collect_filepos_anchors() {
        let chunk = r#"<a id="filepos100" /><p>Text</p><a id="filepos500" />"#;
        let mut map = HashMap::new();
        collect_filepos_anchors(chunk, 2, &mut map);

        assert_eq!(map.get("filepos100"), Some(&2));
        assert_eq!(map.get("filepos500"), Some(&2));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_cross_chapter_link_rewriting() {
        // Chapter 0 has filepos100, Chapter 1 has filepos500
        let ch0 = concat!(
            "<html><body>",
            "<a id=\"filepos100\" />",
            "<a href=\"#filepos100\">self</a>",
            "<a href=\"#filepos500\">cross</a>",
            "</body></html>",
        );
        let ch1 = concat!(
            "<html><body>",
            "<a id=\"filepos500\" />",
            "<p>Ch2</p>",
            "</body></html>",
        );
        let mut chapters = vec![ch0.as_bytes().to_vec(), ch1.as_bytes().to_vec()];

        let mut map = HashMap::new();
        map.insert("filepos100".to_string(), 0);
        map.insert("filepos500".to_string(), 1);

        let paths = vec!["chapter_0.xhtml".to_string(), "chapter_1.xhtml".to_string()];

        rewrite_cross_chapter_links(&mut chapters, &map, &paths);

        let ch0 = String::from_utf8_lossy(&chapters[0]);
        // Same-chapter link should be unchanged
        assert!(
            ch0.contains(r##"href="#filepos100""##),
            "Same-chapter link should be unchanged: {}",
            ch0
        );
        // Cross-chapter link should be rewritten
        assert!(
            ch0.contains(r##"href="chapter_1.xhtml#filepos500""##),
            "Cross-chapter link should be rewritten: {}",
            ch0
        );
    }

    #[test]
    fn test_head_content_shared_across_chapters() {
        let html =
            br#"<html><head><title>Book</title><link rel="stylesheet" href="s.css"/></head><body>
<p>Ch1</p><mbp:pagebreak/><p>Ch2</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        assert_eq!(split.chapters.len(), 2);
        for ch in &split.chapters {
            let s = String::from_utf8_lossy(ch);
            assert!(
                s.contains("<title>Book</title>"),
                "Head should contain title: {}",
                s
            );
            assert!(
                s.contains("s.css"),
                "Head should contain stylesheet link: {}",
                s
            );
        }
    }

    #[test]
    fn test_filepos_to_chapter_mapping() {
        let html = br#"<html><head></head><body>
<a id="filepos10" /><p>Ch1</p>
<mbp:pagebreak/>
<a id="filepos200" /><p>Ch2</p>
<mbp:pagebreak/>
<a id="filepos500" /><p>Ch3</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        assert_eq!(split.filepos_to_chapter.get("filepos10"), Some(&0));
        assert_eq!(split.filepos_to_chapter.get("filepos200"), Some(&1));
        assert_eq!(split.filepos_to_chapter.get("filepos500"), Some(&2));
    }

    #[test]
    fn test_toc_uses_chapter_paths() {
        // Simulate what from_source does: build TOC with chapter paths
        let html = br#"<html><head></head><body>
<a id="filepos0" /><p>Ch1</p>
<mbp:pagebreak/>
<a id="filepos100" /><p>Ch2</p>
</body></html>"#;

        let split = split_mobi_html(html, None);

        // Simulate NCX-based TOC construction
        let filepos0_ch = split
            .filepos_to_chapter
            .get("filepos0")
            .copied()
            .unwrap_or(0);
        let filepos100_ch = split
            .filepos_to_chapter
            .get("filepos100")
            .copied()
            .unwrap_or(0);

        let href0 = format!("{}#filepos0", split.chapter_paths[filepos0_ch]);
        let href1 = format!("{}#filepos100", split.chapter_paths[filepos100_ch]);

        assert_eq!(href0, "chapter_0.xhtml#filepos0");
        assert_eq!(href1, "chapter_1.xhtml#filepos100");
    }

    // ====================================================================
    // NCX fallback splitting tests
    // ====================================================================

    #[test]
    fn test_split_ncx_fallback_basic() {
        // HTML without pagebreaks but with filepos anchors at NCX positions
        let html = br#"<html><head><title>Book</title></head><body>
<a id="filepos0" /><h1>Preamble</h1><p>Front matter</p>
<a id="filepos100" /><h1>Chapter 1</h1><p>Text1</p>
<a id="filepos500" /><h1>Chapter 2</h1><p>Text2</p>
</body></html>"#;

        let ncx_positions = vec![0, 100, 500];
        let split = split_mobi_html(html, Some(&ncx_positions));

        // Should split at filepos100 and filepos500 (filepos0 is at body start, skipped)
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapter_paths[0], "chapter_0.xhtml");
        assert_eq!(split.chapter_paths[1], "chapter_1.xhtml");
        assert_eq!(split.chapter_paths[2], "chapter_2.xhtml");

        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        let ch1 = String::from_utf8_lossy(&split.chapters[1]);
        let ch2 = String::from_utf8_lossy(&split.chapters[2]);
        assert!(
            ch0.contains("Preamble"),
            "Ch0 should have preamble: {}",
            ch0
        );
        assert!(
            ch1.contains("Chapter 1"),
            "Ch1 should have Chapter 1: {}",
            ch1
        );
        assert!(
            ch2.contains("Chapter 2"),
            "Ch2 should have Chapter 2: {}",
            ch2
        );
    }

    #[test]
    fn test_split_ncx_fallback_filepos_to_chapter_map() {
        let html = br#"<html><head></head><body>
<a id="filepos0" /><p>Preamble</p>
<a id="filepos200" /><h1>Ch1</h1><a id="filepos300" /><p>More ch1</p>
<a id="filepos800" /><h1>Ch2</h1>
</body></html>"#;

        // Only split at 200 and 800 (skip sub-position 300)
        let ncx_positions = vec![0, 200, 800];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 3);

        // filepos300 should be in chapter 1 (same chapter as filepos200)
        assert_eq!(split.filepos_to_chapter.get("filepos0"), Some(&0));
        assert_eq!(split.filepos_to_chapter.get("filepos200"), Some(&1));
        assert_eq!(split.filepos_to_chapter.get("filepos300"), Some(&1));
        assert_eq!(split.filepos_to_chapter.get("filepos800"), Some(&2));
    }

    #[test]
    fn test_split_ncx_no_matching_anchors() {
        // NCX positions that don't match any filepos anchors → single chapter
        let html = b"<html><head></head><body><p>No anchors here</p></body></html>";

        let ncx_positions = vec![100, 200, 300];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 1);
    }

    #[test]
    fn test_split_ncx_empty_positions() {
        let html = b"<html><head></head><body><p>Content</p></body></html>";

        let ncx_positions: Vec<u32> = vec![];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 1);
    }

    #[test]
    fn test_pagebreaks_preferred_over_ncx() {
        // When both pagebreaks and NCX positions exist, pagebreaks should be used
        let html = br#"<html><head></head><body>
<a id="filepos0" /><p>Ch1</p>
<mbp:pagebreak/>
<a id="filepos100" /><p>Ch2</p>
<mbp:pagebreak/>
<a id="filepos200" /><p>Ch3</p>
</body></html>"#;

        // Pass NCX positions that would create a different split
        let ncx_positions = vec![0, 200];
        let split = split_mobi_html(html, Some(&ncx_positions));

        // Should get 3 chapters from pagebreaks, not 2 from NCX
        assert_eq!(split.chapters.len(), 3);
    }

    #[test]
    fn test_ncx_cross_chapter_links() {
        // NCX-split chapters should have cross-chapter links rewritten
        let html = br##"<html><head></head><body>
<a id="filepos0" /><a href="#filepos500">Go to Ch2</a><p>Ch1</p>
<a id="filepos500" /><a href="#filepos0">Back to Ch1</a><p>Ch2</p>
</body></html>"##;

        let ncx_positions = vec![0, 500];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 2);

        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        let ch1 = String::from_utf8_lossy(&split.chapters[1]);

        // Cross-chapter links should be rewritten
        assert!(
            ch0.contains(r##"href="chapter_1.xhtml#filepos500""##),
            "Ch0 cross-link should be rewritten: {}",
            ch0
        );
        assert!(
            ch1.contains(r##"href="chapter_0.xhtml#filepos0""##),
            "Ch1 cross-link should be rewritten: {}",
            ch1
        );
    }

    // ====================================================================
    // OEB filename link neutralization tests
    // ====================================================================

    #[test]
    fn test_neutralize_bare_filename_links() {
        let html = br#"<a href="cover.htm">Cover</a> and <a href="Book_oeb_01_r1.html">Ch1</a>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            result.contains(r##"href="#""##),
            "Bare .htm link should be neutralized: {}",
            result
        );
        assert!(
            !result.contains("cover.htm"),
            "Original .htm reference should be removed: {}",
            result
        );
        assert!(
            !result.contains("oeb_01_r1.html"),
            "Original .html reference should be removed: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_preserves_filepos_links() {
        let html =
            br##"<a href="#filepos100">Ch1</a> and <a href="chapter_0.xhtml#filepos200">Ch2</a>"##;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            result.contains(r##"href="#filepos100""##),
            "filepos link should be preserved: {}",
            result
        );
        assert!(
            result.contains("chapter_0.xhtml"),
            "xhtml link should be preserved: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_preserves_xhtml_links() {
        let html = br#"<a href="chapter_1.xhtml">Link</a>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            result.contains("chapter_1.xhtml"),
            "xhtml link should be preserved: {}",
            result
        );
    }

    #[test]
    fn test_is_bare_filename_link_cases() {
        assert!(is_bare_filename_link(b"cover.htm"));
        assert!(is_bare_filename_link(b"Book_oeb_01_r1.html"));
        assert!(is_bare_filename_link(b"Cover.HTML"));
        assert!(is_bare_filename_link(b"file.HTM"));

        assert!(!is_bare_filename_link(b"#filepos100"));
        assert!(!is_bare_filename_link(b"chapter_0.xhtml"));
        assert!(!is_bare_filename_link(b"http://example.com/file.html"));
        assert!(!is_bare_filename_link(b"https://example.com/page.htm"));
        assert!(!is_bare_filename_link(b"#"));
        assert!(!is_bare_filename_link(b"image.jpg"));

        // Fragment handling
        assert!(is_bare_filename_link(b"Book_oeb_ftn_r1.html#f1"));
        assert!(is_bare_filename_link(b"cover.htm#section"));
        assert!(!is_bare_filename_link(b"chapter_0.xhtml#filepos100"));
    }

    #[test]
    fn test_neutralize_uppercase_href() {
        // Real MOBI pattern: uppercase HREF with OEB link + lowercase href with filepos
        let html = br##"<A HREF="Asim_oeb_tp_r1.html"  href="#filepos1129"> Title Page</A>"##;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            !result.contains("oeb_tp_r1.html"),
            "Uppercase HREF OEB link should be removed: {}",
            result
        );
        assert!(
            result.contains(r##"href="#filepos1129""##),
            "Lowercase filepos href should be preserved: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_uppercase_href_no_fallback() {
        // Uppercase HREF without a lowercase href fallback
        let html = br#"<A HREF="cover.htm"> Cover</A>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            !result.contains("cover.htm"),
            "OEB link should be neutralized: {}",
            result
        );
        assert!(
            result.contains(r##"href="#""##),
            "Should have fallback href: {}",
            result
        );
    }

    #[test]
    fn test_neutralize_href_with_fragment() {
        let html = br#"<a href="Book_oeb_ftn_r1.html#f1">Note</a>"#;
        let mut chapters = vec![html.to_vec()];
        neutralize_bare_filename_links(&mut chapters);

        let result = String::from_utf8_lossy(&chapters[0]);
        assert!(
            !result.contains("oeb_ftn_r1.html"),
            "OEB link with fragment should be neutralized: {}",
            result
        );
    }

    #[test]
    fn test_ncx_split_with_oeb_links_neutralized() {
        // Simulate a MOBI with NCX-split chapters and OEB filename links
        let html = br#"<html><head></head><body>
<a id="filepos0" /><a href="cover.htm">Cover</a>
<a href="Book_oeb_01_r1.html">Ch1</a>
<a href="Book_oeb_02_r1.html">Ch2</a>
<p>Preamble content</p>
<a id="filepos500" /><h1>Chapter 1</h1><p>Text1</p>
<a id="filepos1000" /><h1>Chapter 2</h1><p>Text2</p>
</body></html>"#;

        let ncx_positions = vec![0, 500, 1000];
        let split = split_mobi_html(html, Some(&ncx_positions));

        assert_eq!(split.chapters.len(), 3);

        // OEB links in preamble should be neutralized
        let ch0 = String::from_utf8_lossy(&split.chapters[0]);
        assert!(
            !ch0.contains("cover.htm"),
            "OEB links should be neutralized: {}",
            ch0
        );
        assert!(
            !ch0.contains("oeb_01_r1.html"),
            "OEB links should be neutralized: {}",
            ch0
        );

        // Content should still be there
        assert!(ch0.contains("Cover"), "Link text should be preserved");
        assert!(ch0.contains("Ch1"), "Link text should be preserved");
    }
}
