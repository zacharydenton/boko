//! HTML Synthesizer - converts IR back to valid XHTML.
//!
//! This module walks the IR tree and emits XHTML tags, using the generated
//! CSS class names for styling. It tracks asset references (images) so the
//! exporter knows which files to bundle.
//!
//! # Example
//!
//! ```
//! use boko::model::Chapter;
//! use boko::export::{generate_css, synthesize_html};
//!
//! let chapter = Chapter::new();
//! let used_styles = vec![];
//! let css = generate_css(&chapter.styles, &used_styles);
//! let result = synthesize_html(&chapter, &css.class_map);
//!
//! // result.body contains the XHTML body content
//! // result.assets contains referenced image paths
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::model::{Chapter, NodeId, Role};
use crate::style::StyleId;

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
pub fn synthesize_html(ir: &Chapter, style_map: &HashMap<StyleId, String>) -> SynthesisResult {
    let resolver = HashMapResolver { map: style_map };
    synthesize_html_with_resolver(ir, &resolver, MathForm::MathMl)
}

/// How math nodes serialize, per target format's native capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathForm {
    /// Re-emit `<math>` MathML — native for EPUB 3.
    MathMl,
    /// Emit the Unicode linearization — for KF8/MOBI, whose renderers show
    /// raw MathML as one stacked token per line.
    Text,
}

/// Synthesize an XHTML body like [`synthesize_html`], but resolving class
/// names from a slice indexed by `StyleId` (`None` = no class) instead of
/// a hash map.
pub fn synthesize_html_with_class_list(
    ir: &Chapter,
    class_list: &[Option<&str>],
) -> SynthesisResult {
    let resolver = ClassListResolver { list: class_list };
    synthesize_html_with_resolver(ir, &resolver, MathForm::MathMl)
}

/// [`synthesize_html_with_class_list`] with an explicit math serialization
/// form (KF8 targets pass [`MathForm::Text`]).
pub fn synthesize_html_with_class_list_math(
    ir: &Chapter,
    class_list: &[Option<&str>],
    math_form: MathForm,
) -> SynthesisResult {
    let resolver = ClassListResolver { list: class_list };
    synthesize_html_with_resolver(ir, &resolver, math_form)
}

fn synthesize_html_with_resolver<R: StyleResolver>(
    ir: &Chapter,
    resolver: &R,
    math_form: MathForm,
) -> SynthesisResult {
    let mut ctx = SynthesisContext {
        out: String::new(),
        assets: HashSet::new(),
        ir,
        resolver,
        indent_level: 0,
        math_form,
    };

    // Walk children of root (skip the root node itself)
    for child_id in ir.children(NodeId::ROOT) {
        walk_node(child_id, &mut ctx, 0);
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
    ir: &Chapter,
    style_map: &HashMap<StyleId, String>,
    title: &str,
    stylesheet_href: Option<&str>,
) -> SynthesisResult {
    let body_result = synthesize_html(ir, style_map);
    synthesize_xhtml_from_body(body_result, title, stylesheet_href)
}

/// Synthesize a complete XHTML document like [`synthesize_xhtml_document`],
/// but resolving class names from a slice indexed by `StyleId`.
pub fn synthesize_xhtml_document_with_class_list(
    ir: &Chapter,
    class_list: &[Option<&str>],
    title: &str,
    stylesheet_href: Option<&str>,
) -> SynthesisResult {
    synthesize_xhtml_document_with_class_list_math(
        ir,
        class_list,
        title,
        stylesheet_href,
        MathForm::MathMl,
    )
}

/// [`synthesize_xhtml_document_with_class_list`] with an explicit math form.
pub fn synthesize_xhtml_document_with_class_list_math(
    ir: &Chapter,
    class_list: &[Option<&str>],
    title: &str,
    stylesheet_href: Option<&str>,
    math_form: MathForm,
) -> SynthesisResult {
    let body_result = synthesize_html_with_class_list_math(ir, class_list, math_form);
    synthesize_xhtml_from_body(body_result, title, stylesheet_href)
}

fn synthesize_xhtml_from_body(
    body_result: SynthesisResult,
    title: &str,
    stylesheet_href: Option<&str>,
) -> SynthesisResult {
    let mut doc = String::new();

    // EPUB 3 content documents use the HTML5 DOCTYPE and a UTF-8 charset meta;
    // the XHTML 1.1 DOCTYPE and application/xhtml+xml content-type are rejected
    // by epubcheck (HTM-004 / RSC-005).
    doc.push_str(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
  <meta charset="utf-8"/>
  <title>"#,
    );
    escape_xml_into(&mut doc, title);
    doc.push_str("</title>\n");

    if let Some(href) = stylesheet_href {
        doc.push_str("  <link rel=\"stylesheet\" type=\"text/css\" href=\"");
        escape_xml_into(&mut doc, href);
        doc.push_str("\"/>\n");
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
struct SynthesisContext<'a, R: StyleResolver> {
    out: String,
    assets: HashSet<String>,
    ir: &'a Chapter,
    resolver: &'a R,
    indent_level: usize,
    math_form: MathForm,
}

impl<R: StyleResolver> SynthesisContext<'_, R> {
    fn indent(&mut self) {
        for _ in 0..self.indent_level {
            self.out.push_str("  ");
        }
    }
}

/// Walk a node and emit its HTML.
fn walk_node<R: StyleResolver>(id: NodeId, ctx: &mut SynthesisContext<'_, R>, depth: usize) {
    if depth > crate::util::MAX_TREE_DEPTH {
        return;
    }
    let Some(node) = ctx.ir.node(id) else {
        return;
    };

    let role = node.role;
    let style_id = node.style;

    // Math serializes in the target's native form: MathML for EPUB, the
    // Unicode linearization for KF8/MOBI (whose renderers stack raw MathML
    // one token per line). An anchor id on it stays addressable.
    if role == Role::Math {
        if let Some(math) = ctx.ir.math.get(&id) {
            let rendered = match ctx.math_form {
                MathForm::MathMl => crate::math::mathml::to_mathml(math),
                MathForm::Text => escape_xml(&math.to_text()),
            };
            if let Some(anchor) = ctx.ir.semantics.id(id) {
                ctx.out.push_str("<span id=\"");
                ctx.out.push_str(&escape_xml(anchor));
                ctx.out.push_str("\">");
                ctx.out.push_str(&rendered);
                ctx.out.push_str("</span>");
            } else {
                ctx.out.push_str(&rendered);
            }
        }
        return;
    }

    // Handle leaf text nodes (Text role with text content, no children)
    if role == Role::Text && !node.text.is_empty() && node.first_child.is_none() {
        let text = ctx.ir.text(node.text);
        // A text node carrying an anchor id must stay addressable: KFX
        // anchors land on text nodes, and emitting bare text would leave
        // every internal link to them (`chapter_5.xhtml#a19F`) dangling.
        let anchor = ctx.ir.semantics.id(id);
        if let Some(anchor) = anchor {
            ctx.out.push_str("<span id=\"");
            ctx.out.push_str(&escape_xml(anchor));
            ctx.out.push_str("\">");
        }
        // KFX uses \n in text content for forced line breaks — emit as <br/>.
        // escape_xml_into writes straight into the output buffer, avoiding a
        // temporary String per text node (this is the hottest synth loop).
        if text.contains('\n') {
            for (i, segment) in text.split('\n').enumerate() {
                if i > 0 {
                    ctx.out.push_str("<br/>");
                }
                escape_xml_into(&mut ctx.out, segment);
            }
        } else {
            escape_xml_into(&mut ctx.out, text);
        }
        if anchor.is_some() {
            ctx.out.push_str("</span>");
        }
        return;
    }

    // Map role to tag
    let (mut tag, is_void, is_block) = role_to_tag(role);

    // For table cells, use th for header cells
    if role == Role::TableCell && ctx.ir.semantics.is_header_cell(id) {
        tag = "th";
    }

    // <p> and headings cannot legally contain block-level children in
    // (X)HTML; KFX containers routinely import as Paragraph with nested
    // headings/paragraphs. Demote to <div> — presentation comes from the
    // class anyway, and browsers would otherwise auto-close the <p> and
    // mangle the structure.
    if matches!(tag, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
        let has_block_child = ctx.ir.children(id).any(|child_id| {
            ctx.ir
                .node(child_id)
                .is_some_and(|child| role_to_tag(child.role).2)
        });
        if has_block_child {
            tag = "div";
        }
    }

    // Build attributes
    let mut attrs = String::new();

    // Class attribute (from style)
    if let Some(class) = ctx.resolver.class_for(style_id) {
        write!(attrs, " class=\"{}\"", class).unwrap();
    }

    // Semantic attributes
    if let Some(elem_id) = ctx.ir.semantics.id(id) {
        attrs.push_str(" id=\"");
        escape_xml_into(&mut attrs, elem_id);
        attrs.push('"');
    }
    if let Some(href) = ctx.ir.semantics.href(id) {
        attrs.push_str(" href=\"");
        escape_xml_into(&mut attrs, href);
        attrs.push('"');
    }
    if let Some(src) = ctx.ir.semantics.src(id) {
        attrs.push_str(" src=\"");
        escape_xml_into(&mut attrs, src);
        attrs.push('"');
        // Track as asset
        ctx.assets.insert(src.to_string());
    }
    if let Some(alt) = ctx.ir.semantics.alt(id) {
        attrs.push_str(" alt=\"");
        escape_xml_into(&mut attrs, alt);
        attrs.push('"');
    }
    if let Some(title) = ctx.ir.semantics.title(id) {
        attrs.push_str(" title=\"");
        escape_xml_into(&mut attrs, title);
        attrs.push('"');
    }
    if let Some(lang) = ctx.ir.semantics.lang(id) {
        attrs.push_str(" xml:lang=\"");
        escape_xml_into(&mut attrs, lang);
        attrs.push('"');
    }
    // Emit start attribute for ordered lists
    if role == Role::OrderedList
        && let Some(start) = ctx.ir.semantics.list_start(id)
    {
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

    // Preserve semantic markers captured on import: epub:type (footnote /
    // noteref / pagebreak — drives reader footnote popups and page lists),
    // ARIA role, and <time datetime>. Without these the round trip demotes
    // EPUB3 footnotes to plain links and loses pagebreak semantics.
    if let Some(epub_type) = ctx.ir.semantics.epub_type(id) {
        attrs.push_str(" epub:type=\"");
        escape_xml_into(&mut attrs, epub_type);
        attrs.push('"');
    }
    if let Some(aria_role) = ctx.ir.semantics.aria_role(id) {
        attrs.push_str(" role=\"");
        escape_xml_into(&mut attrs, aria_role);
        attrs.push('"');
    }
    // datetime is only ever set on <time> elements, so emitting it wherever
    // present reproduces it on the right element without a role check.
    if let Some(datetime) = ctx.ir.semantics.datetime(id) {
        attrs.push_str(" datetime=\"");
        escape_xml_into(&mut attrs, datetime);
        attrs.push('"');
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
        walk_node(child_id, ctx, depth + 1);
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

trait StyleResolver {
    fn class_for(&self, id: StyleId) -> Option<&str>;
}

struct HashMapResolver<'a> {
    map: &'a HashMap<StyleId, String>,
}

impl StyleResolver for HashMapResolver<'_> {
    fn class_for(&self, id: StyleId) -> Option<&str> {
        self.map.get(&id).map(|s| s.as_str())
    }
}

struct ClassListResolver<'a> {
    list: &'a [Option<&'a str>],
}

impl StyleResolver for ClassListResolver<'_> {
    fn class_for(&self, id: StyleId) -> Option<&str> {
        self.list.get(id.0 as usize).copied().flatten()
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
        Role::TableHead => ("thead", false, true),
        Role::TableBody => ("tbody", false, true),
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

        // Math is re-serialized verbatim in walk_node; this arm exists only
        // for exhaustiveness. Inline by default (most math is inline).
        Role::Math => ("math", false, false),
    }
}

/// Escape special XML/HTML characters.
pub fn escape_xml(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    escape_xml_into(&mut result, s);
    result
}

/// Escape special XML/HTML characters into an existing buffer.
///
/// Copies unescaped runs in bulk; text with nothing to escape (the common
/// case) is a single `push_str`.
pub fn escape_xml_into(out: &mut String, s: &str) {
    let mut rest = s;
    while let Some(i) = rest.find(['&', '<', '>', '"', '\'']) {
        out.push_str(&rest[..i]);
        out.push_str(match rest.as_bytes()[i] {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            _ => "&#39;",
        });
        rest = &rest[i + 1..];
    }
    out.push_str(rest);
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::model::Node;
    use crate::style::{ComputedStyle, FontWeight};

    fn make_test_chapter() -> Chapter {
        let mut chapter = Chapter::new();

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
    fn epub_type_and_role_are_preserved() {
        // Semantic markers captured on import must survive synthesis, or
        // EPUB3 footnote/pagebreak semantics are lost on every →EPUB export.
        let mut chapter = Chapter::new();
        let a = chapter.alloc_node(Node::new(Role::Link));
        chapter.append_child(NodeId::ROOT, a);
        chapter.semantics.set_href(a, "ch2.xhtml#n1");
        chapter.semantics.set_epub_type(a, "noteref");
        chapter.semantics.set_aria_role(a, "doc-noteref");

        let result = synthesize_html(&chapter, &HashMap::new());
        assert!(
            result.body.contains("epub:type=\"noteref\""),
            "{}",
            result.body
        );
        assert!(
            result.body.contains("role=\"doc-noteref\""),
            "{}",
            result.body
        );

        // And the document root must declare the epub namespace.
        let doc = synthesize_xhtml_document(&chapter, &HashMap::new(), "T", None);
        assert!(
            doc.body
                .contains("xmlns:epub=\"http://www.idpf.org/2007/ops\"")
        );
    }

    #[test]
    fn test_synthesize_with_style_class() {
        let mut chapter = Chapter::new();

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
    fn test_synthesize_with_class_list() {
        let mut chapter = Chapter::new();

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

        let mut class_list = vec![None; chapter.styles.len()];
        class_list[bold_id.0 as usize] = Some("c1");

        let result = synthesize_html_with_class_list(&chapter, &class_list);

        assert!(result.body.contains(r#"<p class="c1">"#));
    }

    #[test]
    fn test_synthesize_link() {
        let mut chapter = Chapter::new();

        let link = chapter.alloc_node(Node::new(Role::Link));
        chapter.append_child(NodeId::ROOT, link);
        chapter.semantics.set_href(link, "https://example.com");

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
        let mut chapter = Chapter::new();

        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter.semantics.set_src(img, "images/photo.jpg");
        chapter.semantics.set_alt(img, "A photo");

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
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

        // Image (void element)
        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter.semantics.set_src(img, "test.png");

        let result = synthesize_html(&chapter, &HashMap::new());

        // XHTML self-closing tag
        assert!(result.body.contains("<img"));
        assert!(result.body.contains("/>"));
        // Should NOT have closing tag
        assert!(!result.body.contains("</img>"));
    }

    #[test]
    fn test_heading_levels() {
        let mut chapter = Chapter::new();

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

    #[test]
    fn test_text_newlines_become_br() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        // Text with embedded newline (KFX forced line break)
        let text_range = chapter.append_text("Interface Culture:\nHow New Technology");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(para, text_node);

        let result = synthesize_html(&chapter, &HashMap::new());

        assert!(
            result
                .body
                .contains("Interface Culture:<br/>How New Technology"),
            "Newlines in text content should become <br/> tags, got: {}",
            result.body
        );
        // Should NOT contain a bare newline between the segments
        assert!(
            !result.body.contains("Culture:\nHow"),
            "Raw newline should not appear in HTML output"
        );
    }

    #[test]
    fn test_text_without_newlines_unchanged() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let text_range = chapter.append_text("Normal text without breaks");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(para, text_node);

        let result = synthesize_html(&chapter, &HashMap::new());

        assert!(result.body.contains("Normal text without breaks"));
        assert!(!result.body.contains("<br/>"));
    }
}
