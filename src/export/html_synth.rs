//! HTML Synthesizer - converts IR back to valid XHTML.
//!
//! This module walks the IR tree and emits XHTML tags, using the generated
//! CSS class names for styling. It tracks asset references (images) so the
//! exporter knows which files to bundle.
//!
//! # Example
//!
//! ```
//! use boko::ir::IRChapter;
//! use boko::export::{generate_css, synthesize_html};
//!
//! let chapter = IRChapter::new();
//! let used_styles = vec![];
//! let css = generate_css(&chapter.styles, &used_styles);
//! let result = synthesize_html(&chapter, &css.class_map);
//!
//! // result.body contains the XHTML body content
//! // result.assets contains referenced image paths
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::ir::{IRChapter, NodeId, Role, StyleId};

/// Result of HTML synthesis.
#[derive(Debug, Clone)]
pub struct SynthesisResult {
    /// The generated XHTML body content.
    pub body: String,
    /// Set of asset paths referenced in the content (images, etc.).
    pub assets: HashSet<String>,
}

/// Synthesize XHTML from an IR chapter.
///
/// # Arguments
///
/// * `ir` - The IR chapter to convert
/// * `style_map` - Mapping from StyleId to CSS class name (from `generate_css`)
///
/// # Returns
///
/// A `SynthesisResult` containing the XHTML body and referenced assets.
pub fn synthesize_html(ir: &IRChapter, style_map: &HashMap<StyleId, String>) -> SynthesisResult {
    let mut ctx = SynthesisContext {
        out: String::new(),
        assets: HashSet::new(),
        ir,
        style_map,
        indent_level: 0,
    };

    // Walk children of root (skip the root node itself)
    for child_id in ir.children(NodeId::ROOT) {
        walk_node(child_id, &mut ctx);
    }

    SynthesisResult {
        body: ctx.out,
        assets: ctx.assets,
    }
}

/// Synthesize a complete XHTML document (with DOCTYPE, html, head, body).
///
/// # Arguments
///
/// * `ir` - The IR chapter to convert
/// * `style_map` - Mapping from StyleId to CSS class name
/// * `title` - Document title
/// * `stylesheet_href` - Optional href to external stylesheet
///
/// # Returns
///
/// A complete XHTML document string.
pub fn synthesize_xhtml_document(
    ir: &IRChapter,
    style_map: &HashMap<StyleId, String>,
    title: &str,
    stylesheet_href: Option<&str>,
) -> SynthesisResult {
    let body_result = synthesize_html(ir, style_map);

    let mut doc = String::new();

    // XHTML 1.1 DOCTYPE (compatible with EPUB)
    doc.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <meta http-equiv="Content-Type" content="application/xhtml+xml; charset=utf-8"/>
  <title>"#,
    );
    doc.push_str(&escape_xml(title));
    doc.push_str("</title>\n");

    if let Some(href) = stylesheet_href {
        writeln!(
            doc,
            "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>",
            escape_xml(href)
        )
        .unwrap();
    }

    doc.push_str("</head>\n<body>\n");
    doc.push_str(&body_result.body);
    doc.push_str("</body>\n</html>\n");

    SynthesisResult {
        body: doc,
        assets: body_result.assets,
    }
}

/// Context for the synthesis walk.
struct SynthesisContext<'a> {
    out: String,
    assets: HashSet<String>,
    ir: &'a IRChapter,
    style_map: &'a HashMap<StyleId, String>,
    indent_level: usize,
}

impl SynthesisContext<'_> {
    fn indent(&mut self) {
        for _ in 0..self.indent_level {
            self.out.push_str("  ");
        }
    }
}

/// Walk a node and emit its HTML.
fn walk_node(id: NodeId, ctx: &mut SynthesisContext) {
    let Some(node) = ctx.ir.node(id) else {
        return;
    };

    let role = node.role;
    let style_id = node.style;

    // Handle leaf text nodes (Text role with text content, no children)
    if role == Role::Text && !node.text.is_empty() && node.first_child.is_none() {
        let text = ctx.ir.text(node.text);
        ctx.out.push_str(&escape_xml(text));
        return;
    }

    // Map role to tag
    let (mut tag, is_void, is_block) = role_to_tag(role);

    // For table cells, use th for header cells
    if role == Role::TableCell && ctx.ir.semantics.is_header_cell(id) {
        tag = "th";
    }

    // Build attributes
    let mut attrs = String::new();

    // Class attribute (from style)
    if let Some(class) = ctx.style_map.get(&style_id) {
        write!(attrs, " class=\"{}\"", class).unwrap();
    }

    // Semantic attributes
    if let Some(elem_id) = ctx.ir.semantics.id(id) {
        write!(attrs, " id=\"{}\"", escape_xml(elem_id)).unwrap();
    }
    if let Some(href) = ctx.ir.semantics.href(id) {
        write!(attrs, " href=\"{}\"", escape_xml(href)).unwrap();
    }
    if let Some(src) = ctx.ir.semantics.src(id) {
        write!(attrs, " src=\"{}\"", escape_xml(src)).unwrap();
        // Track as asset
        ctx.assets.insert(src.to_string());
    }
    if let Some(alt) = ctx.ir.semantics.alt(id) {
        write!(attrs, " alt=\"{}\"", escape_xml(alt)).unwrap();
    }
    if let Some(title) = ctx.ir.semantics.title(id) {
        write!(attrs, " title=\"{}\"", escape_xml(title)).unwrap();
    }
    if let Some(lang) = ctx.ir.semantics.lang(id) {
        write!(attrs, " xml:lang=\"{}\"", escape_xml(lang)).unwrap();
    }
    // Emit start attribute for ordered lists
    if role == Role::OrderedList
        && let Some(start) = ctx.ir.semantics.list_start(id) {
            write!(attrs, " start=\"{}\"", start).unwrap();
        }
    // Emit rowspan/colspan for table cells
    if role == Role::TableCell {
        if let Some(rowspan) = ctx.ir.semantics.row_span(id) {
            write!(attrs, " rowspan=\"{}\"", rowspan).unwrap();
        }
        if let Some(colspan) = ctx.ir.semantics.col_span(id) {
            write!(attrs, " colspan=\"{}\"", colspan).unwrap();
        }
    }

    // Emit opening tag
    if is_block {
        ctx.indent();
    }

    if is_void {
        // Self-closing tag for XHTML (img, br, hr)
        write!(ctx.out, "<{}{}/>", tag, attrs).unwrap();
        if is_block {
            ctx.out.push('\n');
        }
        return;
    }

    write!(ctx.out, "<{}{}>", tag, attrs).unwrap();

    // Check if we have any children
    let has_children = ctx.ir.children(id).next().is_some();

    if is_block && has_children {
        ctx.out.push('\n');
        ctx.indent_level += 1;
    }

    // Emit children
    for child_id in ctx.ir.children(id) {
        walk_node(child_id, ctx);
    }

    // Emit closing tag
    if is_block && has_children {
        ctx.indent_level -= 1;
        ctx.indent();
    }
    write!(ctx.out, "</{}>", tag).unwrap();

    if is_block {
        ctx.out.push('\n');
    }
}

/// Map an IR Role to an HTML tag name.
///
/// Returns (tag_name, is_void_element, is_block_element).
fn role_to_tag(role: Role) -> (&'static str, bool, bool) {
    match role {
        // Root and containers
        Role::Root => ("div", false, true),
        Role::Container => ("div", false, true),

        // Paragraphs (block-level text containers)
        Role::Paragraph => ("p", false, true),

        // Text nodes are leaf content - handled specially in render
        // This fallback shouldn't normally be used
        Role::Text => ("span", false, false),

        // Headings with level
        Role::Heading(1) => ("h1", false, true),
        Role::Heading(2) => ("h2", false, true),
        Role::Heading(3) => ("h3", false, true),
        Role::Heading(4) => ("h4", false, true),
        Role::Heading(5) => ("h5", false, true),
        Role::Heading(6) => ("h6", false, true),
        Role::Heading(_) => ("h6", false, true), // Fallback

        // Block elements
        Role::BlockQuote => ("blockquote", false, true),
        Role::OrderedList => ("ol", false, true),
        Role::UnorderedList => ("ul", false, true),
        Role::ListItem => ("li", false, true),
        Role::DefinitionList => ("dl", false, true),
        Role::DefinitionTerm => ("dt", false, true),
        Role::DefinitionDescription => ("dd", false, true),
        Role::CodeBlock => ("pre", false, true),
        // Default to figcaption; context-aware logic could choose caption for tables
        Role::Caption => ("figcaption", false, true),
        Role::Table => ("table", false, true),
        Role::TableRow => ("tr", false, true),
        Role::TableCell => ("td", false, true),
        Role::Figure => ("figure", false, true),
        Role::Sidebar => ("aside", false, true),
        Role::Footnote => ("aside", false, true), // Footnotes as aside

        // Void elements (self-closing in XHTML)
        Role::Image => ("img", true, false),
        Role::Break => ("br", true, false),
        Role::Rule => ("hr", true, true),

        // Inline elements
        Role::Inline => ("span", false, false),
        Role::Link => ("a", false, false),
    }
}

/// Escape special XML/HTML characters.
pub fn escape_xml(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#39;"),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::ir::{ComputedStyle, FontWeight, Node};

    fn make_test_chapter() -> IRChapter {
        let mut chapter = IRChapter::new();

        // Create a paragraph with text content
        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let text_range = chapter.append_text("Hello, World!");
        let text_node = Node::text(text_range);
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(para, text_id);

        chapter
    }

    #[test]
    fn test_synthesize_simple_paragraph() {
        let chapter = make_test_chapter();
        let style_map = HashMap::new();

        let result = synthesize_html(&chapter, &style_map);

        assert!(result.body.contains("<p>"));
        assert!(result.body.contains("Hello, World!"));
        assert!(result.body.contains("</p>"));
    }

    #[test]
    fn test_synthesize_with_style_class() {
        let mut chapter = IRChapter::new();

        // Create a bold style
        let mut bold = ComputedStyle::default();
        bold.font_weight = FontWeight::BOLD;
        let bold_id = chapter.styles.intern(bold);

        // Create paragraph with bold style
        let mut para_node = Node::new(Role::Paragraph);
        para_node.style = bold_id;
        let para = chapter.alloc_node(para_node);
        chapter.append_child(NodeId::ROOT, para);

        let text_range = chapter.append_text("Bold text");
        let text_node = Node::text(text_range);
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(para, text_id);

        // Create style map
        let mut style_map = HashMap::new();
        style_map.insert(bold_id, "c1".to_string());

        let result = synthesize_html(&chapter, &style_map);

        assert!(result.body.contains(r#"<p class="c1">"#));
    }

    #[test]
    fn test_synthesize_link() {
        let mut chapter = IRChapter::new();

        let link = chapter.alloc_node(Node::new(Role::Link));
        chapter.append_child(NodeId::ROOT, link);
        chapter
            .semantics
            .set_href(link, "https://example.com".to_string());

        let text_range = chapter.append_text("Click me");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(link, text_node);

        let result = synthesize_html(&chapter, &HashMap::new());

        assert!(result.body.contains(r#"<a href="https://example.com">"#));
        assert!(result.body.contains("Click me"));
        assert!(result.body.contains("</a>"));
    }

    #[test]
    fn test_synthesize_image_tracks_assets() {
        let mut chapter = IRChapter::new();

        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter
            .semantics
            .set_src(img, "images/photo.jpg".to_string());
        chapter.semantics.set_alt(img, "A photo".to_string());

        let result = synthesize_html(&chapter, &HashMap::new());

        assert!(
            result
                .body
                .contains(r#"<img src="images/photo.jpg" alt="A photo"/>"#)
        );
        assert!(result.assets.contains("images/photo.jpg"));
    }

    #[test]
    fn test_synthesize_nested_structure() {
        let mut chapter = IRChapter::new();

        // Create: <ul><li>Item 1</li><li>Item 2</li></ul>
        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);

        let li1 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul, li1);
        let text1_range = chapter.append_text("Item 1");
        let text1_id = chapter.alloc_node(Node::text(text1_range));
        chapter.append_child(li1, text1_id);

        let li2 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul, li2);
        let text2_range = chapter.append_text("Item 2");
        let text2_id = chapter.alloc_node(Node::text(text2_range));
        chapter.append_child(li2, text2_id);

        let result = synthesize_html(&chapter, &HashMap::new());

        assert!(result.body.contains("<ul>"));
        assert!(result.body.contains("<li>"));
        assert!(result.body.contains("Item 1"));
        assert!(result.body.contains("Item 2"));
        assert!(result.body.contains("</li>"));
        assert!(result.body.contains("</ul>"));
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("Hello"), "Hello");
        assert_eq!(escape_xml("<script>"), "&lt;script&gt;");
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml(r#"Say "hi""#), "Say &quot;hi&quot;");
        assert_eq!(escape_xml("it's"), "it&#39;s");
    }

    #[test]
    fn test_synthesize_xhtml_document() {
        let chapter = make_test_chapter();
        let style_map = HashMap::new();

        let result =
            synthesize_xhtml_document(&chapter, &style_map, "Test Chapter", Some("style.css"));

        assert!(result.body.contains("<?xml version"));
        assert!(result.body.contains("<!DOCTYPE html"));
        assert!(result.body.contains("<title>Test Chapter</title>"));
        assert!(result.body.contains(r#"href="style.css""#));
        assert!(result.body.contains("<body>"));
        assert!(result.body.contains("Hello, World!"));
        assert!(result.body.contains("</body>"));
    }

    #[test]
    fn test_void_elements() {
        let mut chapter = IRChapter::new();

        // Image (void element)
        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter.semantics.set_src(img, "test.png".to_string());

        let result = synthesize_html(&chapter, &HashMap::new());

        // XHTML self-closing tag
        assert!(result.body.contains("<img"));
        assert!(result.body.contains("/>"));
        // Should NOT have closing tag
        assert!(!result.body.contains("</img>"));
    }

    #[test]
    fn test_heading_levels() {
        let mut chapter = IRChapter::new();

        for level in 1u8..=6 {
            let h = chapter.alloc_node(Node::new(Role::Heading(level)));
            chapter.append_child(NodeId::ROOT, h);

            let text_range = chapter.append_text(&format!("Heading {}", level));
            let text_id = chapter.alloc_node(Node::text(text_range));
            chapter.append_child(h, text_id);
        }

        let result = synthesize_html(&chapter, &HashMap::new());

        assert!(result.body.contains("<h1>"));
        assert!(result.body.contains("<h2>"));
        assert!(result.body.contains("<h3>"));
        assert!(result.body.contains("<h4>"));
        assert!(result.body.contains("<h5>"));
        assert!(result.body.contains("<h6>"));
    }
}
