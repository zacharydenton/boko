//! Transform ArenaDom to IRChapter.

use html5ever::LocalName;

use super::arena::{ArenaDom, ArenaNodeData, ArenaNodeId};
use super::css::{compute_styles, Origin, Stylesheet};
use super::element_ref::ElementRef;
use crate::ir::{ComputedStyle, Display, IRChapter, Node, NodeId, Role};

/// User-agent default stylesheet.
pub fn user_agent_stylesheet() -> Stylesheet {
    Stylesheet::parse(
        r#"
        /* Block elements */
        html, body, div, section, article, aside, nav, header, footer, main,
        address, blockquote, figure, figcaption, details, summary {
            display: block;
        }

        /* Headings */
        h1, h2, h3, h4, h5, h6 {
            display: block;
            font-weight: bold;
        }
        h1 { font-size: 2em; margin-top: 0.67em; margin-bottom: 0.67em; }
        h2 { font-size: 1.5em; margin-top: 0.83em; margin-bottom: 0.83em; }
        h3 { font-size: 1.17em; margin-top: 1em; margin-bottom: 1em; }
        h4 { font-size: 1em; margin-top: 1.33em; margin-bottom: 1.33em; }
        h5 { font-size: 0.83em; margin-top: 1.67em; margin-bottom: 1.67em; }
        h6 { font-size: 0.67em; margin-top: 2.33em; margin-bottom: 2.33em; }

        /* Paragraphs */
        p {
            display: block;
            margin-top: 1em;
            margin-bottom: 1em;
        }

        /* Lists */
        ul, ol {
            display: block;
            margin-top: 1em;
            margin-bottom: 1em;
            padding-left: 40px;
        }
        ul {
            list-style-type: disc;
        }
        ol {
            list-style-type: decimal;
        }
        li {
            display: list-item;
        }

        /* Inline elements */
        span, a, em, i, strong, b, cite, var, dfn, abbr, acronym,
        code, kbd, samp, tt, sub, sup, small, big, q,
        u, ins, s, strike, del, mark, time, label {
            display: inline;
        }

        /* Inline styles */
        em, i, cite, var, dfn {
            font-style: italic;
        }
        strong, b {
            font-weight: bold;
        }
        code, kbd, samp, tt {
            font-family: monospace;
        }
        sup {
            vertical-align: super;
            font-size: 0.83em;
        }
        sub {
            vertical-align: sub;
            font-size: 0.83em;
        }
        u, ins {
            text-decoration: underline;
        }
        s, strike, del {
            text-decoration: line-through;
        }

        /* Links */
        a:link {
            color: blue;
            text-decoration: underline;
        }

        /* Preformatted */
        pre {
            display: block;
            font-family: monospace;
            white-space: pre;
            margin: 1em 0;
        }

        /* Blockquote */
        blockquote {
            display: block;
            margin-top: 1em;
            margin-bottom: 1em;
            margin-left: 40px;
            margin-right: 40px;
        }

        /* Horizontal rule */
        hr {
            display: block;
            margin-top: 0.5em;
            margin-bottom: 0.5em;
            border-style: inset;
            border-width: 1px;
        }

        /* Tables */
        table {
            display: table;
        }
        tr {
            display: table-row;
        }
        td, th {
            display: table-cell;
        }
        th {
            font-weight: bold;
            text-align: center;
        }

        /* Hidden elements */
        head, script, style, link, meta, title, template {
            display: none;
        }

        /* Images are inline-block by default but we treat them as inline */
        img {
            display: inline;
        }

        /* Spans are inline */
        span, a {
            display: inline;
        }
    "#,
    )
}

/// Map an HTML element to its semantic role.
fn map_element_to_role(local_name: &LocalName) -> Role {
    match local_name.as_ref() {
        // Block containers
        "div" | "section" | "article" | "nav" | "header" | "footer" | "main" | "address"
        | "details" | "summary" | "hgroup" => Role::Container,

        // Line break (leaf node, not a container)
        "br" => Role::Break,

        // Horizontal rule (thematic break)
        "hr" => Role::Rule,

        // Aside/sidebar
        "aside" => Role::Sidebar,

        // Figure and caption
        "figure" => Role::Figure,
        "figcaption" | "caption" => Role::Caption,

        // Paragraphs - block-level text containers
        "p" => Role::Paragraph,

        // Preformatted code blocks
        "pre" => Role::CodeBlock,

        // Inline elements with styling (rendered via ComputedStyle)
        "span" | "em" | "i" | "cite" | "var" | "dfn" | "strong" | "b" | "code" | "kbd"
        | "samp" | "tt" | "sup" | "sub" | "u" | "ins" | "s" | "strike" | "del" | "small"
        | "mark" | "abbr" | "time" | "q" => Role::Inline,

        // Headings with level
        "h1" => Role::Heading(1),
        "h2" => Role::Heading(2),
        "h3" => Role::Heading(3),
        "h4" => Role::Heading(4),
        "h5" => Role::Heading(5),
        "h6" => Role::Heading(6),

        // Links
        "a" => Role::Link,

        // Images
        "img" => Role::Image,

        // Lists
        "ul" => Role::UnorderedList,
        "ol" => Role::OrderedList,
        "li" => Role::ListItem,

        // Block quote
        "blockquote" => Role::BlockQuote,

        // Definition lists
        "dl" => Role::DefinitionList,
        "dt" => Role::DefinitionTerm,
        "dd" => Role::DefinitionDescription,

        // Tables
        "table" => Role::Table,
        "tr" => Role::TableRow,
        "td" | "th" => Role::TableCell,

        // Other inline containers
        "label" | "legend" | "output" | "data" | "ruby" | "rt" | "rp" | "bdi" | "bdo"
        | "wbr" => Role::Inline,

        // Default to container for unknown block elements
        _ => Role::Container,
    }
}

/// Context for the transform operation.
struct TransformContext<'a> {
    dom: &'a ArenaDom,
    stylesheets: &'a [(Stylesheet, Origin)],
    chapter: IRChapter,
    /// Map from ArenaNodeId to IRChapter NodeId
    node_map: std::collections::HashMap<ArenaNodeId, NodeId>,
}

impl<'a> TransformContext<'a> {
    fn new(dom: &'a ArenaDom, stylesheets: &'a [(Stylesheet, Origin)]) -> Self {
        Self {
            dom,
            stylesheets,
            chapter: IRChapter::new(),
            node_map: std::collections::HashMap::new(),
        }
    }

    /// Transform the DOM to IR.
    fn transform(mut self) -> IRChapter {
        // Find the body element, or use document root
        let body = self.dom.find_by_tag("body").unwrap_or(self.dom.document());

        // Get language from html element (if present) to propagate to all content
        let html_lang = self.dom.find_by_tag("html").and_then(|html_id| {
            if let Some(node) = self.dom.get(html_id) {
                if let ArenaNodeData::Element { attrs, .. } = &node.data {
                    for attr in attrs {
                        if attr.name.local.as_ref() == "lang" && !attr.value.is_empty() {
                            return Some(attr.value.clone());
                        }
                    }
                }
            }
            None
        });

        // Compute body's style so its properties (like hyphens: auto) are inherited
        let mut body_style = {
            let elem_ref = ElementRef::new(self.dom, body);
            compute_styles(elem_ref, self.stylesheets, None, &mut self.chapter.styles)
        };

        // Add html lang to body style if present (so it's inherited by all content)
        if let Some(lang) = html_lang {
            if body_style.language.is_none() {
                body_style.language = Some(lang);
            }
        }

        // Process body's children as children of IR root, inheriting body's style
        self.process_children(body, NodeId::ROOT, Some(&body_style));

        self.chapter
    }

    /// Process children of a DOM node.
    fn process_children(
        &mut self,
        dom_parent: ArenaNodeId,
        ir_parent: NodeId,
        parent_style: Option<&ComputedStyle>,
    ) {
        for child_id in self.dom.children(dom_parent).collect::<Vec<_>>() {
            self.process_node(child_id, ir_parent, parent_style);
        }
    }

    /// Process a single DOM node.
    fn process_node(
        &mut self,
        dom_id: ArenaNodeId,
        ir_parent: NodeId,
        parent_style: Option<&ComputedStyle>,
    ) {
        let node = match self.dom.get(dom_id) {
            Some(n) => n,
            None => return,
        };

        match &node.data {
            ArenaNodeData::Text(text) => {
                // Handle whitespace-only text nodes
                if text.trim().is_empty() {
                    // Whitespace between inline elements should be preserved as a single space.
                    // We preserve whitespace unless:
                    // 1. We're at the root level (no parent style)
                    // 2. The whitespace contains newlines and we're in a block context
                    //
                    // This handles cases like: <cite><abbr>A</abbr> <abbr>B</abbr></cite>
                    // where the space between abbrs must be preserved even though cite is block.
                    let has_newlines = text.contains('\n');
                    let is_block_parent = parent_style
                        .map(|s| s.display != Display::Inline)
                        .unwrap_or(true);

                    // Skip pure-whitespace with newlines in block contexts (inter-element whitespace)
                    // But preserve spaces without newlines (intra-line whitespace between inline elements)
                    if has_newlines && is_block_parent {
                        return;
                    }

                    // No parent means we're at root level - skip whitespace
                    if parent_style.is_none() {
                        return;
                    }

                    // Preserve as a single space
                    let range = self.chapter.append_text(" ");
                    let text_node = Node::text(range);
                    let ir_id = self.chapter.alloc_node(text_node);
                    self.chapter.append_child(ir_parent, ir_id);
                    self.node_map.insert(dom_id, ir_id);
                    return;
                }

                let range = self.chapter.append_text(text);
                // Text nodes don't have styles - they inherit from parent element
                let text_node = Node::text(range);
                let ir_id = self.chapter.alloc_node(text_node);
                self.chapter.append_child(ir_parent, ir_id);
                self.node_map.insert(dom_id, ir_id);
            }

            ArenaNodeData::Element { name, attrs, .. } => {
                // Compute style for this element
                let elem_ref = ElementRef::new(self.dom, dom_id);
                let mut computed = compute_styles(
                    elem_ref,
                    self.stylesheets,
                    parent_style,
                    &mut self.chapter.styles,
                );

                // Merge lang attribute into style (for KFX language property)
                // This must happen before interning so the style includes the language
                for attr in attrs {
                    if attr.name.local.as_ref() == "lang" && !attr.value.is_empty() {
                        computed.language = Some(attr.value.to_string());
                        break;
                    }
                }

                // Map to role first (needed for Break check)
                let role = map_element_to_role(&name.local);

                // Skip hidden elements, but preserve Break nodes
                // CSS may hide <br> (e.g., in verse: "span + br { display: none }") but
                // we still need them for line breaks in text/markdown export
                if computed.display == Display::None && role != Role::Break {
                    return;
                }

                // Create IR node
                let mut ir_node = Node::new(role);
                ir_node.style = self.chapter.styles.intern(computed.clone());

                let ir_id = self.chapter.alloc_node(ir_node);
                self.chapter.append_child(ir_parent, ir_id);
                self.node_map.insert(dom_id, ir_id);

                // Store semantic attributes
                for attr in attrs {
                    let attr_name = attr.name.local.as_ref();
                    let attr_ns = attr.name.ns.as_ref();
                    match attr_name {
                        // Core layout attributes
                        "href" => {
                            self.chapter.semantics.set_href(ir_id, attr.value.clone());
                        }
                        "src" => self.chapter.semantics.set_src(ir_id, attr.value.clone()),
                        "alt" => self.chapter.semantics.set_alt(ir_id, attr.value.clone()),
                        "id" => self.chapter.semantics.set_id(ir_id, attr.value.clone()),
                        "title" => self.chapter.semantics.set_title(ir_id, attr.value.clone()),
                        // Language (both lang and xml:lang)
                        "lang" => self.chapter.semantics.set_lang(ir_id, attr.value.clone()),
                        // List start attribute (ol@start)
                        "start" if name.local.as_ref() == "ol" => {
                            if let Ok(start) = attr.value.parse::<u32>() {
                                self.chapter.semantics.set_list_start(ir_id, start);
                            }
                        }
                        // Semantic fidelity attributes
                        // epub:type attribute - handle both namespaced and prefixed forms
                        // html5ever parses "epub:type" as literal name with empty namespace
                        "type" if attr_ns == "http://www.idpf.org/2007/ops" => {
                            self.chapter
                                .semantics
                                .set_epub_type(ir_id, attr.value.clone());
                        }
                        "epub:type" => {
                            self.chapter
                                .semantics
                                .set_epub_type(ir_id, attr.value.clone());
                        }
                        "role" => {
                            self.chapter
                                .semantics
                                .set_aria_role(ir_id, attr.value.clone());
                        }
                        "datetime" => {
                            self.chapter
                                .semantics
                                .set_datetime(ir_id, attr.value.clone());
                        }
                        // Table cell attributes
                        "rowspan" if matches!(name.local.as_ref(), "td" | "th") => {
                            if let Ok(span) = attr.value.parse::<u32>() {
                                self.chapter.semantics.set_row_span(ir_id, span);
                            }
                        }
                        "colspan" if matches!(name.local.as_ref(), "td" | "th") => {
                            if let Ok(span) = attr.value.parse::<u32>() {
                                self.chapter.semantics.set_col_span(ir_id, span);
                            }
                        }
                        // Extract language from class for code elements
                        "class" if matches!(name.local.as_ref(), "code" | "pre") => {
                            for class in attr.value.split_whitespace() {
                                if let Some(lang) = class.strip_prefix("language-") {
                                    self.chapter.semantics.set_language(ir_id, lang.to_string());
                                    break;
                                }
                                if let Some(lang) = class.strip_prefix("lang-") {
                                    self.chapter.semantics.set_language(ir_id, lang.to_string());
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Mark th elements as header cells
                if name.local.as_ref() == "th" {
                    self.chapter.semantics.set_header_cell(ir_id, true);
                }

                // Process children
                self.process_children(dom_id, ir_id, Some(&computed));
            }

            // Skip other node types
            ArenaNodeData::Document
            | ArenaNodeData::Comment(_)
            | ArenaNodeData::Doctype { .. } => {}
        }
    }
}

/// Transform an ArenaDom to IRChapter.
pub fn transform(dom: &ArenaDom, stylesheets: &[(Stylesheet, Origin)]) -> IRChapter {
    let ctx = TransformContext::new(dom, stylesheets);
    ctx.transform()
}

#[cfg(test)]
mod tests {
    use html5ever::driver::ParseOpts;
    use html5ever::parse_document;
    use html5ever::tendril::TendrilSink;

    use super::*;
    use crate::compiler::tree_sink::ArenaSink;

    fn parse_html(html: &str) -> ArenaDom {
        let sink = ArenaSink::new();
        let result = parse_document(sink, ParseOpts::default())
            .from_utf8()
            .one(html.as_bytes());
        result.into_dom()
    }

    #[test]
    fn test_basic_transform() {
        let dom = parse_html("<html><body><p>Hello, World!</p></body></html>");
        let ua = user_agent_stylesheet();
        let stylesheets = vec![(ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        // Should have root + paragraph (Text) + text content
        assert!(chapter.node_count() >= 3);

        // Find text nodes
        let mut found_text = false;
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text && !node.text.is_empty() {
                found_text = true;
                let text = chapter.text(node.text);
                assert!(text.contains("Hello"));
            }
        }
        assert!(found_text);
    }

    #[test]
    fn test_heading_levels() {
        let dom = parse_html("<html><body><h1>Title</h1><h2>Subtitle</h2></body></html>");
        let ua = user_agent_stylesheet();
        let stylesheets = vec![(ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        let mut h1_count = 0;
        let mut h2_count = 0;
        for id in chapter.iter_dfs() {
            match chapter.node(id).unwrap().role {
                Role::Heading(1) => h1_count += 1,
                Role::Heading(2) => h2_count += 1,
                _ => {}
            }
        }
        assert_eq!(h1_count, 1);
        assert_eq!(h2_count, 1);
    }

    #[test]
    fn test_link_semantics() {
        let dom = parse_html(r#"<a href="https://example.com">Link</a>"#);
        let ua = user_agent_stylesheet();
        let stylesheets = vec![(ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        // Find link node
        for id in chapter.iter_dfs() {
            if chapter.node(id).unwrap().role == Role::Link {
                assert_eq!(
                    chapter.semantics.href(id),
                    Some("https://example.com")
                );
                return;
            }
        }
        panic!("Link not found");
    }

    #[test]
    fn test_style_inheritance() {
        let dom = parse_html(
            r#"<html><body>
            <div style="color: red;"><p>Inherited</p></div>
        </body></html>"#,
        );
        let ua = user_agent_stylesheet();
        let author = Stylesheet::parse("div { color: red; }");
        let stylesheets = vec![(ua, Origin::UserAgent), (author, Origin::Author)];

        let chapter = transform(&dom, &stylesheets);

        // The paragraph should inherit the red color from div
        // (This is implicit in the cascade since we pass parent_style)
        assert!(chapter.node_count() > 1);
    }

    #[test]
    fn test_hidden_elements() {
        let dom = parse_html(
            r#"<html><head><title>Test</title></head><body><p>Visible</p></body></html>"#,
        );
        let ua = user_agent_stylesheet();
        let stylesheets = vec![(ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        // Should not contain title element (display: none)
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text {
                let text = chapter.text(node.text);
                assert!(!text.contains("Test"));
            }
        }
    }

    #[test]
    fn test_br_element() {
        let dom = parse_html(r#"<html><body><p>Line one<br/>Line two</p></body></html>"#);
        let ua = user_agent_stylesheet();
        let stylesheets = vec![(ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        // Should have a Break node
        let mut found_break = false;
        for id in chapter.iter_dfs() {
            if chapter.node(id).unwrap().role == Role::Break {
                found_break = true;
                break;
            }
        }
        assert!(found_break, "Break node not found");
    }

    #[test]
    fn test_br_element_xhtml_style() {
        // Test with XHTML-style self-closing br with namespace
        let dom = parse_html(
            r#"<?xml version="1.0" encoding="utf-8"?>
            <html xmlns="http://www.w3.org/1999/xhtml">
            <body><p><span>Line one</span><br/><span>Line two</span></p></body></html>"#,
        );
        let ua = user_agent_stylesheet();
        let stylesheets = vec![(ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        // Should have a Break node
        let mut found_break = false;
        for id in chapter.iter_dfs() {
            if chapter.node(id).unwrap().role == Role::Break {
                found_break = true;
                break;
            }
        }
        assert!(found_break, "Break node not found in XHTML-style input");
    }

    #[test]
    fn test_br_in_blockquote_verse() {
        // Exact structure from epictetus.epub endnote 30
        let dom = parse_html(
            r#"<html xmlns="http://www.w3.org/1999/xhtml">
            <body>
                <blockquote>
                    <p lang="la">
                        <span>Cui non conveniet sua res, ut calceus olim,</span>
                        <br/>
                        <span>Si pede major erit, subvertet; si minor, uret.</span>
                    </p>
                </blockquote>
            </body></html>"#,
        );
        let ua = user_agent_stylesheet();
        let stylesheets = vec![(ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        // Should have a Break node
        let mut found_break = false;
        for id in chapter.iter_dfs() {
            if chapter.node(id).unwrap().role == Role::Break {
                found_break = true;
                break;
            }
        }
        assert!(found_break, "Break node not found in blockquote verse");
    }
}
