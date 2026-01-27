//! HTML to IR compiler pipeline.
//!
//! This module transforms HTML content with CSS stylesheets into the
//! normalized IR (Intermediate Representation) format.
//!
//! # Example
//!
//! ```
//! use boko::compiler::{compile_html, Stylesheet, Origin};
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
mod css;
mod element_ref;
mod optimizer;
mod transform;
mod tree_sink;

pub use arena::{ArenaDom, ArenaNode, ArenaNodeData, ArenaNodeId};
pub use css::{Declaration, Origin, PropertyValue, Specificity, Stylesheet};
pub use element_ref::{BokoSelectors, ElementRef};
pub use transform::user_agent_stylesheet;

use html5ever::driver::ParseOpts;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;

use crate::ir::IRChapter;
use tree_sink::ArenaSink;

/// Compile HTML content to IR.
///
/// This is the main entry point for the compiler pipeline.
///
/// # Arguments
///
/// * `html` - The HTML content to parse
/// * `stylesheets` - Author stylesheets with their origins (user-agent stylesheet is added automatically)
///
/// # Returns
///
/// An `IRChapter` containing the normalized content tree.
///
/// # Example
///
/// ```
/// use boko::compiler::{compile_html, Stylesheet, Origin};
///
/// let html = "<p class='intro'>Welcome!</p>";
/// let css = ".intro { font-weight: bold; }";
///
/// let author = Stylesheet::parse(css);
/// let chapter = compile_html(html, &[(author, Origin::Author)]);
/// ```
pub fn compile_html(html: &str, author_stylesheets: &[(Stylesheet, Origin)]) -> IRChapter {
    // Parse HTML to ArenaDom
    let sink = ArenaSink::new();
    let result = parse_document(sink, ParseOpts::default())
        .from_utf8()
        .one(html.as_bytes());
    let dom = result.into_dom();

    // Build complete stylesheet list with UA defaults
    let ua = transform::user_agent_stylesheet();
    let mut all_stylesheets: Vec<(Stylesheet, Origin)> = vec![(ua, Origin::UserAgent)];
    for (sheet, origin) in author_stylesheets {
        all_stylesheets.push((sheet.clone(), *origin));
    }

    // Transform to IR
    let mut chapter = transform::transform(&dom, &all_stylesheets);

    // Optimize: merge adjacent text nodes with identical styles
    optimizer::optimize(&mut chapter);

    chapter
}

/// Compile HTML bytes to IR.
///
/// Convenience wrapper that handles byte-to-string conversion with proper
/// encoding detection. Supports UTF-8, Windows-1252, and other encodings
/// via the XML declaration.
pub fn compile_html_bytes(html: &[u8], author_stylesheets: &[(Stylesheet, Origin)]) -> IRChapter {
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
pub fn extract_stylesheets(html: &str) -> (Vec<String>, Vec<String>) {
    let sink = ArenaSink::new();
    let result = parse_document(sink, ParseOpts::default())
        .from_utf8()
        .one(html.as_bytes());
    let dom = result.into_dom();

    let mut linked = Vec::new();
    let mut inline = Vec::new();

    // Find all link[rel=stylesheet] and style elements
    let mut stack = vec![dom.document()];
    while let Some(id) = stack.pop() {
        if let Some(node) = dom.get(id) {
            if let ArenaNodeData::Element { name, attrs, .. } = &node.data {
                match name.local.as_ref() {
                    "link" => {
                        let is_stylesheet = attrs
                            .iter()
                            .any(|a| a.name.local.as_ref() == "rel" && a.value == "stylesheet");
                        if is_stylesheet {
                            if let Some(href) = attrs
                                .iter()
                                .find(|a| a.name.local.as_ref() == "href")
                                .map(|a| a.value.clone())
                            {
                                linked.push(href);
                            }
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
/// ```
/// use boko::compiler::resolve_path;
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
    use crate::ir::Role;

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
                if style.font_weight == crate::ir::FontWeight::BOLD {
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
}
