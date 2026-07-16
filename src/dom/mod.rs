//! HTML to IR compiler pipeline.
//!
//! This module transforms HTML content with CSS stylesheets into the
//! normalized IR (Intermediate Representation) format.
//!
//! # Example
//!
//! ```
//! use boko::{compile_html, Stylesheet, Origin};
//!
//! let html = "<html><body><p>Hello, World!</p></body></html>";
//! let css = "p { color: blue; }";
//!
//! let author_css = Stylesheet::parse(css);
//! let chapter = compile_html(html, &[(author_css, Origin::Author)]);
//!
//! // The chapter now contains normalized IR nodes
//! assert!(chapter.node_count() > 1);
//! ```

mod arena;
pub mod element_ref;
pub mod optimize;
mod role_map;
mod transform;
mod tree_sink;

pub use arena::{ArenaDom, ArenaNodeData};

// Re-export style types for convenience
pub use crate::style::{Origin, Stylesheet};

use html5ever::driver::ParseOpts;
use html5ever::tendril::TendrilSink;

use crate::model::Chapter;
use tree_sink::ArenaSink;

/// Check if content looks like XHTML/XML based on the first ~500 bytes.
///
/// Checks for XML declaration (`<?xml`) or XHTML namespace (`xmlns=`).
fn looks_like_xhtml(html: &str) -> bool {
    let end = html.floor_char_boundary(500);
    let prefix = &html[..end];
    prefix.contains("<?xml") || prefix.contains("xmlns=")
}

/// Parse HTML/XHTML into an ArenaDom.
///
/// Uses xml5ever for XHTML content (detected by `<?xml` or `xmlns=` in the
/// first 500 bytes), falling back to html5ever for plain HTML. This correctly
/// handles self-closing tags like `<script/>` which are valid in XHTML but
/// cause content loss with HTML5 parsing.
pub(crate) fn parse_dom(html: &str) -> ArenaDom {
    if looks_like_xhtml(html) {
        let sink = ArenaSink::new();
        let result =
            xml5ever::driver::parse_document(sink, xml5ever::driver::XmlParseOpts::default())
                .from_utf8()
                .one(html.as_bytes());
        let dom = result.into_dom();

        // Verify xml5ever produced a usable tree (has a body with children).
        // Fall through to html5ever if not.
        if let Some(body) = dom.find_by_tag("body")
            && dom.children(body).next().is_some()
        {
            return dom;
        }
    }

    // Fallback: html5ever (permissive HTML5 parser)
    let sink = ArenaSink::new();
    let result = html5ever::parse_document(sink, ParseOpts::default())
        .from_utf8()
        .one(html.as_bytes());
    result.into_dom()
}

/// Compile HTML content to IR.
///
/// This is the main entry point for the compiler pipeline.
/// Automatically detects XHTML and uses the appropriate parser.
///
/// # Arguments
///
/// * `html` - The HTML content to parse
/// * `stylesheets` - Author stylesheets with their origins (user-agent stylesheet is added automatically)
///
/// # Returns
///
/// A `Chapter` containing the normalized content tree.
///
/// # Example
///
/// ```
/// use boko::{compile_html, Stylesheet, Origin};
///
/// let html = "<p class='intro'>Welcome!</p>";
/// let css = ".intro { font-weight: bold; }";
///
/// let author = Stylesheet::parse(css);
/// let chapter = compile_html(html, &[(author, Origin::Author)]);
/// ```
pub fn compile_html(html: &str, author_stylesheets: &[(Stylesheet, Origin)]) -> Chapter {
    let dom = parse_dom(html);
    let refs: Vec<(&Stylesheet, Origin)> =
        author_stylesheets.iter().map(|(s, o)| (s, *o)).collect();
    compile_dom(&dom, &refs)
}

/// Compile an already-parsed DOM to IR with borrowed stylesheets.
///
/// Internal hot path shared by [`compile_html`] and the importers: no
/// stylesheet is cloned — the UA sheet is shared per thread and author
/// sheets are borrowed (typically from `Arc<Stylesheet>` caches).
pub(crate) fn compile_dom(dom: &ArenaDom, author_stylesheets: &[(&Stylesheet, Origin)]) -> Chapter {
    // Build complete stylesheet list with UA defaults
    let ua = transform::user_agent_stylesheet_arc();
    let mut all_stylesheets: Vec<(&Stylesheet, Origin)> =
        Vec::with_capacity(author_stylesheets.len() + 1);
    all_stylesheets.push((ua.as_ref(), Origin::UserAgent));
    all_stylesheets.extend_from_slice(author_stylesheets);

    // Transform to IR
    let mut chapter = transform::transform(dom, &all_stylesheets);

    // Optimize: merge adjacent text nodes with identical styles
    optimize::optimize(&mut chapter);

    chapter
}

/// Compile HTML bytes to IR.
///
/// Convenience wrapper that handles byte-to-string conversion with proper
/// encoding detection. Supports UTF-8, Windows-1252, and other encodings
/// via the XML declaration.
#[cfg(test)]
pub(crate) fn compile_html_bytes(
    html: &[u8],
    author_stylesheets: &[(Stylesheet, Origin)],
) -> Chapter {
    // Extract encoding from XML declaration if present
    let hint_encoding = crate::util::extract_xml_encoding(html);

    // Decode with proper encoding support
    let html_str = crate::util::decode_text(html, hint_encoding);

    compile_html(&html_str, author_stylesheets)
}

/// Extract stylesheet links and inline styles from HTML.
///
/// Returns a list of (href, media) tuples for linked stylesheets,
/// and a list of inline CSS content.
#[cfg(test)]
pub(crate) fn extract_stylesheets(html: &str) -> (Vec<String>, Vec<String>) {
    extract_stylesheets_from_dom(&parse_dom(html))
}

/// Extract stylesheet references from an already-parsed DOM.
///
/// Internal importer hot path: lets `load_chapter` parse each chapter's HTML
/// once and reuse the DOM for both stylesheet discovery and IR compilation.
pub(crate) fn extract_stylesheets_from_dom(dom: &ArenaDom) -> (Vec<String>, Vec<String>) {
    let mut linked = Vec::new();
    let mut inline = Vec::new();

    // Find all link[rel=stylesheet] and style elements
    let mut stack = vec![dom.document()];
    while let Some(id) = stack.pop() {
        if let Some(node) = dom.get(id)
            && let ArenaNodeData::Element { name, attrs, .. } = &node.data
        {
            match name.local.as_ref() {
                "link" => {
                    let is_stylesheet = attrs
                        .iter()
                        .any(|a| a.name.local.as_ref() == "rel" && a.value == "stylesheet");
                    if is_stylesheet
                        && let Some(href) = attrs
                            .iter()
                            .find(|a| a.name.local.as_ref() == "href")
                            .map(|a| a.value.clone())
                    {
                        linked.push(href);
                    }
                }
                "style" => {
                    // Collect text content
                    let mut text = String::new();
                    for child in dom.children(id) {
                        if let Some(t) = dom.text_content(child) {
                            text.push_str(t);
                        }
                    }
                    if !text.trim().is_empty() {
                        inline.push(text);
                    }
                }
                _ => {}
            }
        }

        // Add children to stack (reverse for left-to-right order)
        let children: Vec<_> = dom.children(id).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }

    (linked, inline)
}

/// Resolve a relative path against a base path logically (no filesystem access).
///
/// This is used to canonicalize paths like `../images/photo.jpg` relative to
/// a chapter file like `OEBPS/text/ch1.html` into an absolute archive path
/// like `OEBPS/images/photo.jpg`.
///
/// # Arguments
///
/// * `base` - The base file path (e.g., `OEBPS/text/ch1.html`)
/// * `rel` - The relative path to resolve (e.g., `../images/photo.jpg`)
///
/// # Returns
///
/// The resolved path as a string, normalized with forward slashes.
///
/// # Examples
///
/// ```ignore (crate-internal; exercised by unit tests below)
/// use crate::dom::resolve_path;
///
/// assert_eq!(
///     resolve_path("OEBPS/text/ch1.html", "../images/logo.png"),
///     "OEBPS/images/logo.png"
/// );
/// assert_eq!(
///     resolve_path("OEBPS/content.html", "images/photo.jpg"),
///     "OEBPS/images/photo.jpg"
/// );
/// assert_eq!(
///     resolve_path("ch1.html", "/images/absolute.png"),
///     "images/absolute.png"
/// );
/// ```
pub fn resolve_path(base: &str, rel: &str) -> String {
    use std::path::{Component, Path};

    let rel_path = Path::new(rel);

    // If absolute (starts with /), treat as archive root
    if rel_path.has_root() {
        return rel.trim_start_matches('/').to_string();
    }

    // If it's a URL (http://, https://, data:, etc.), return as-is
    if rel.contains("://") || rel.starts_with("data:") {
        return rel.to_string();
    }

    // Pop the filename from base to get the directory
    let base_path = Path::new(base);
    let mut stack: Vec<&str> = base_path
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .filter_map(|c| {
            if let Component::Normal(s) = c {
                s.to_str()
            } else {
                None
            }
        })
        .collect();

    // Process relative path components
    for component in rel_path.components() {
        match component {
            Component::ParentDir => {
                stack.pop(); // Handle ".."
            }
            Component::Normal(c) => {
                if let Some(s) = c.to_str() {
                    stack.push(s);
                }
            }
            Component::CurDir => {} // Handle "." (no-op)
            _ => {}
        }
    }

    // Join with forward slashes for ZIP compatibility
    stack.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Role;

    #[test]
    fn deeply_nested_html_does_not_overflow_stack() {
        // Deeply nested elements overflowed the stack in compile_html before the
        // transform gained a depth cap. Run on a small (1 MiB) stack so a modest
        // nesting depth is enough to blow it if the cap ever regresses — this
        // keeps the test both sensitive and fast (html5ever's parse is O(depth²),
        // so we deliberately avoid huge depths).
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let depth = 3000;
                let mut html = String::from("<html><body>");
                html.push_str(&"<div>".repeat(depth));
                html.push_str("deep");
                html.push_str(&"</div>".repeat(depth));
                html.push_str("</body></html>");
                compile_html(&html, &[]).node_count()
            })
            .unwrap();
        assert!(handle.join().unwrap() > 0);
    }

    /// Concatenate every text node in document order.
    fn full_text(chapter: &Chapter) -> String {
        let mut out = String::new();
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text && !node.text.is_empty() {
                out.push_str(chapter.text(node.text));
            }
        }
        out
    }

    #[test]
    fn pre_preserves_whitespace_only_text_nodes() {
        // The whitespace-only node between the spans carries all of the line
        // structure — the standard shape of syntax-highlighted code. It used
        // to be dropped by the whitespace heuristics before the white-space
        // check ran.
        let html =
            "<html><body><pre><span>fn a()</span>\n    <span>fn b()</span></pre></body></html>";
        let chapter = compile_html(html, &[]);
        assert_eq!(full_text(&chapter), "fn a()\n    fn b()");
    }

    #[test]
    fn whitespace_between_inline_siblings_in_div_is_kept() {
        // Browsers render both of these as "A B"; the space/newline between
        // the <i> elements is a word separator, not indentation.
        let chapter = compile_html(
            "<html><body><div><i>A</i> <i>B</i></div></body></html>",
            &[],
        );
        assert_eq!(full_text(&chapter), "A B");

        let chapter = compile_html(
            "<html><body><div><i>A</i>\n<i>B</i></div></body></html>",
            &[],
        );
        assert_eq!(full_text(&chapter), "A B");
    }

    #[test]
    fn indentation_between_blocks_is_still_dropped() {
        let html = "<html><body><div>\n  <p>One</p>\n  <p>Two</p>\n</div></body></html>";
        let chapter = compile_html(html, &[]);
        assert_eq!(full_text(&chapter), "OneTwo");
    }

    #[test]
    fn inline_style_attribute_applies() {
        let chapter = compile_html(
            r#"<html><body><p style="font-weight: bold">x</p></body></html>"#,
            &[],
        );
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Paragraph {
                let style = chapter.styles.get(node.style).unwrap();
                assert_eq!(style.font_weight, crate::style::FontWeight::BOLD);
                return;
            }
        }
        panic!("paragraph not found");
    }

    #[test]
    fn inline_style_beats_selector_specificity_but_not_important() {
        let css = "p.x { color: #00ff00; } p.y { color: #0000ff !important; }";
        let author = Stylesheet::parse(css);

        // Inline normal beats any selector specificity...
        let chapter = compile_html(
            r#"<html><body><p class="x" style="color: #ff0000">x</p></body></html>"#,
            &[(author.clone(), Origin::Author)],
        );
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Paragraph {
                let style = chapter.styles.get(node.style).unwrap();
                assert_eq!(style.color, Some(crate::style::Color::rgb(255, 0, 0)));
            }
        }

        // ...but loses to a stylesheet !important.
        let chapter = compile_html(
            r#"<html><body><p class="y" style="color: #ff0000">x</p></body></html>"#,
            &[(author, Origin::Author)],
        );
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Paragraph {
                let style = chapter.styles.get(node.style).unwrap();
                assert_eq!(style.color, Some(crate::style::Color::rgb(0, 0, 255)));
            }
        }
    }

    #[test]
    fn html_element_styles_inherit_into_body() {
        let author = Stylesheet::parse("html { color: #123456; }");
        let chapter = compile_html(
            "<html><body><p>t</p></body></html>",
            &[(author, Origin::Author)],
        );
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Paragraph {
                let style = chapter.styles.get(node.style).unwrap();
                assert_eq!(
                    style.color,
                    Some(crate::style::Color::rgb(0x12, 0x34, 0x56))
                );
                return;
            }
        }
        panic!("paragraph not found");
    }

    #[test]
    fn test_compile_simple_html() {
        let html = "<html><body><p>Test paragraph</p></body></html>";
        let chapter = compile_html(html, &[]);

        // Should have at least root + p (Text) + text content
        assert!(chapter.node_count() >= 3);

        // Verify there's at least one Text node
        let mut found_text = false;
        for id in chapter.iter_dfs() {
            if chapter.node(id).unwrap().role == Role::Text {
                found_text = true;
            }
        }
        assert!(found_text);
    }

    #[test]
    fn test_compile_with_css() {
        let html = "<p class='highlight'>Styled</p>";
        let css = ".highlight { font-weight: bold; }";

        let author = Stylesheet::parse(css);
        let chapter = compile_html(html, &[(author, Origin::Author)]);

        // Find a styled Paragraph node and check its style
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Paragraph {
                let style = chapter.styles.get(node.style).unwrap();
                if style.font_weight == crate::style::FontWeight::BOLD {
                    return; // Found the styled paragraph
                }
            }
        }
        panic!("Styled paragraph not found");
    }

    #[test]
    fn test_extract_stylesheets() {
        let html = r#"
            <html>
            <head>
                <link rel="stylesheet" href="styles.css">
                <link rel="stylesheet" href="theme.css">
                <style>p { color: red; }</style>
            </head>
            <body><p>Content</p></body>
            </html>
        "#;

        let (linked, inline) = extract_stylesheets(html);

        assert_eq!(linked.len(), 2);
        assert!(linked.contains(&"styles.css".to_string()));
        assert!(linked.contains(&"theme.css".to_string()));

        assert_eq!(inline.len(), 1);
        assert!(inline[0].contains("color: red"));
    }

    #[test]
    fn test_compile_html_bytes() {
        let html = b"<p>Bytes test</p>";
        let chapter = compile_html_bytes(html, &[]);

        assert!(chapter.node_count() > 1);
    }

    #[test]
    fn test_resolve_path_parent_dir() {
        assert_eq!(
            resolve_path("OEBPS/text/ch1.html", "../images/logo.png"),
            "OEBPS/images/logo.png"
        );
    }

    #[test]
    fn test_resolve_path_same_dir() {
        assert_eq!(
            resolve_path("OEBPS/content.html", "images/photo.jpg"),
            "OEBPS/images/photo.jpg"
        );
    }

    #[test]
    fn test_resolve_path_absolute() {
        assert_eq!(
            resolve_path("ch1.html", "/images/absolute.png"),
            "images/absolute.png"
        );
    }

    #[test]
    fn test_resolve_path_multiple_parent() {
        assert_eq!(
            resolve_path("a/b/c/file.html", "../../images/test.png"),
            "a/images/test.png"
        );
    }

    #[test]
    fn test_resolve_path_current_dir() {
        assert_eq!(
            resolve_path("OEBPS/ch1.html", "./images/test.png"),
            "OEBPS/images/test.png"
        );
    }

    #[test]
    fn test_optimizer_merges_sibling_text_nodes() {
        // The optimizer merges adjacent sibling Text nodes with the same style.
        // Note: <b>A</b><b>B</b> creates separate Inline containers, so those
        // Text nodes are NOT siblings and won't be merged. This tests the case
        // where Text nodes are actual siblings (e.g., from text interspersed
        // with inline elements that get stripped).

        // Direct test of the optimizer unit tests cover the merge logic.
        // This integration test verifies the optimizer runs without corrupting
        // the tree structure.
        let html = r#"
            <html><body>
                <p>Hello, <b>World</b>!</p>
            </body></html>
        "#;
        let chapter = compile_html(html, &[]);

        // Collect all text content
        let mut text_content = String::new();
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text && !node.text.is_empty() {
                text_content.push_str(chapter.text(node.text));
            }
        }

        // All text should be preserved
        assert!(
            text_content.contains("Hello"),
            "Missing 'Hello' in: {}",
            text_content
        );
        assert!(
            text_content.contains("World"),
            "Missing 'World' in: {}",
            text_content
        );
    }

    #[test]
    fn test_optimizer_preserves_tree_structure() {
        // The optimizer should not corrupt the tree structure
        let html = r#"
            <html><body>
                <p>First paragraph</p>
                <p>Second paragraph</p>
            </body></html>
        "#;
        let chapter = compile_html(html, &[]);

        // Collect all text content via DFS traversal
        let mut text_content = String::new();
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text && !node.text.is_empty() {
                text_content.push_str(chapter.text(node.text));
            }
        }

        // Both paragraphs should be present
        assert!(
            text_content.contains("First paragraph"),
            "Missing 'First paragraph' in: {}",
            text_content
        );
        assert!(
            text_content.contains("Second paragraph"),
            "Missing 'Second paragraph' in: {}",
            text_content
        );
    }

    #[test]
    fn test_resolve_path_url_passthrough() {
        assert_eq!(
            resolve_path("ch1.html", "https://example.com/image.png"),
            "https://example.com/image.png"
        );
        assert_eq!(
            resolve_path("ch1.html", "data:image/png;base64,abc"),
            "data:image/png;base64,abc"
        );
    }

    #[test]
    fn test_br_survives_optimizer() {
        // Verify Break nodes survive the full compile_html pipeline (including optimizer)
        let chapter = compile_html(
            r#"<html xmlns="http://www.w3.org/1999/xhtml">
            <body>
                <blockquote>
                    <p>
                        <span>Line 1</span>
                        <br/>
                        <span>Line 2</span>
                    </p>
                </blockquote>
            </body></html>"#,
            &[],
        );

        // Should have a Break node
        let mut found_break = false;
        for id in chapter.iter_dfs() {
            if chapter.node(id).unwrap().role == Role::Break {
                found_break = true;
                break;
            }
        }
        assert!(found_break, "Break node lost during optimization");
    }

    #[test]
    fn test_xhtml_self_closing_script_preserves_content() {
        // EPUB XHTML files often have self-closing <script/> tags.
        // In HTML5 parsing, <script/> swallows everything after it.
        // xml5ever handles this correctly.
        let html = r#"<html xmlns="http://www.w3.org/1999/xhtml">
            <head>
                <script src="book.js"/>
            </head>
            <body><p>Hello World</p></body>
        </html>"#;
        let chapter = compile_html(html, &[]);

        let mut found_text = false;
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text && !node.text.is_empty() {
                let text = chapter.text(node.text);
                if text.contains("Hello World") {
                    found_text = true;
                }
            }
        }
        assert!(
            found_text,
            "Self-closing <script/> in XHTML swallowed body content"
        );
    }

    #[test]
    fn test_looks_like_xhtml() {
        assert!(looks_like_xhtml(
            r#"<?xml version="1.0"?><html><body>Hi</body></html>"#
        ));
        assert!(looks_like_xhtml(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>Hi</body></html>"#
        ));
        assert!(!looks_like_xhtml(
            "<html><body><p>Plain HTML</p></body></html>"
        ));
    }

    #[test]
    fn test_plain_html_still_works() {
        // Plain HTML without xmlns should use html5ever and still work fine
        let html = "<html><body><p>Plain HTML</p></body></html>";
        let chapter = compile_html(html, &[]);

        let mut found_text = false;
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text && !node.text.is_empty() {
                let text = chapter.text(node.text);
                if text.contains("Plain HTML") {
                    found_text = true;
                }
            }
        }
        assert!(found_text, "Plain HTML content should be preserved");
    }

    #[test]
    fn test_xhtml_extract_stylesheets() {
        // Stylesheet extraction should also work with XHTML
        let html = r#"<html xmlns="http://www.w3.org/1999/xhtml">
            <head>
                <link rel="stylesheet" href="style.css"/>
                <script src="book.js"/>
                <style>p { color: red; }</style>
            </head>
            <body><p>Content</p></body>
        </html>"#;

        let (linked, inline) = extract_stylesheets(html);
        assert_eq!(linked.len(), 1);
        assert!(linked.contains(&"style.css".to_string()));
        assert_eq!(inline.len(), 1);
        assert!(inline[0].contains("color: red"));
    }
}
