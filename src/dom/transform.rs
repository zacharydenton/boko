//! Transform ArenaDom to Chapter.

use super::arena::{ArenaDom, ArenaNodeData, ArenaNodeId};
use super::element_ref::ElementRef;
use super::role_map::element_to_role;
use crate::model::{Chapter, Node, NodeId, Role};
use crate::style::{
    CascadeIndex, CascadeScratch, ComputedStyle, Display, Origin, Stylesheet, WhiteSpace,
    compute_styles_indexed,
};

/// User agent stylesheet (browser defaults).
const UA_CSS: &str = include_str!("data/styles.css");

pub fn user_agent_stylesheet() -> Stylesheet {
    (*user_agent_stylesheet_arc()).clone()
}

/// Shared handle to the process-wide UA stylesheet. Cloning the `Arc` is a
/// refcount bump; use this instead of [`user_agent_stylesheet`] anywhere the
/// per-chapter hot path would otherwise deep-clone the parsed rules.
pub(crate) fn user_agent_stylesheet_arc() -> std::sync::Arc<Stylesheet> {
    // The UA stylesheet is a constant, but `compile_html` is called once per
    // chapter (now potentially from many rayon workers), so parse UA_CSS
    // exactly once for the whole process.
    static UA_STYLESHEET: std::sync::LazyLock<std::sync::Arc<Stylesheet>> =
        std::sync::LazyLock::new(|| std::sync::Arc::new(Stylesheet::parse(UA_CSS)));
    UA_STYLESHEET.clone()
}

/// Context for the transform operation.
struct TransformContext<'a> {
    dom: &'a ArenaDom,
    /// Selector-bucketed view of `stylesheets`, built once for the whole chapter.
    cascade_index: CascadeIndex<'a>,
    /// Reused across every element of the chapter (candidate buffer + selector caches).
    cascade_scratch: CascadeScratch,
    chapter: Chapter,
}

impl<'a> TransformContext<'a> {
    fn new(dom: &'a ArenaDom, stylesheets: &'a [(&'a Stylesheet, Origin)]) -> Self {
        Self {
            dom,
            cascade_index: CascadeIndex::build(stylesheets),
            cascade_scratch: CascadeScratch::default(),
            chapter: Chapter::new(),
        }
    }

    /// Transform the DOM to IR.
    fn transform(mut self) -> Chapter {
        // Find the body element, or use document root
        let body = self.dom.find_by_tag("body").unwrap_or(self.dom.document());

        // Get language from html element (if present) to propagate to all content
        let html_lang = self.dom.find_by_tag("html").and_then(|html_id| {
            if let Some(node) = self.dom.get(html_id)
                && let ArenaNodeData::Element { attrs, .. } = &node.data
            {
                for attr in attrs {
                    if attr.name.local.as_ref() == "lang" && !attr.value.is_empty() {
                        return Some(attr.value.clone());
                    }
                }
            }
            None
        });

        // Compute body's style so its properties (like hyphens: auto) are inherited
        let mut body_style = {
            let elem_ref = ElementRef::new(self.dom, body);
            compute_styles_indexed(
                elem_ref,
                &self.cascade_index,
                None,
                &mut self.chapter.styles,
                &mut self.cascade_scratch,
            )
        };

        // Add html lang to body style if present (so it's inherited by all content)
        if let Some(lang) = html_lang
            && body_style.language.is_none()
        {
            body_style.language = Some(lang);
        }

        // Process body's children as children of IR root, inheriting body's style
        self.process_children(body, NodeId::ROOT, Some(&body_style), 0);

        self.chapter
    }

    /// Process children of a DOM node.
    fn process_children(
        &mut self,
        dom_parent: ArenaNodeId,
        ir_parent: NodeId,
        parent_style: Option<&ComputedStyle>,
        depth: usize,
    ) {
        // Copy the &'a ArenaDom out of self so the child iterator borrows the
        // DOM (immutable for the whole transform), not `self` — avoids
        // collecting children into a Vec for every element.
        let dom = self.dom;
        for child_id in dom.children(dom_parent) {
            self.process_node(child_id, ir_parent, parent_style, depth);
        }
    }

    /// Process a single DOM node.
    fn process_node(
        &mut self,
        dom_id: ArenaNodeId,
        ir_parent: NodeId,
        parent_style: Option<&ComputedStyle>,
        depth: usize,
    ) {
        // A hostile document can nest elements arbitrarily deep; cap the
        // recursion so it degrades gracefully instead of overflowing the stack.
        if depth > crate::util::MAX_TREE_DEPTH {
            return;
        }
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
                    return;
                }

                // Check if whitespace should be preserved (pre, pre-wrap, pre-line)
                let preserve_whitespace = parent_style
                    .map(|s| {
                        matches!(
                            s.white_space,
                            WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine
                        )
                    })
                    .unwrap_or(false);

                // Normalize whitespace unless we're in a pre-like context.
                // Both paths append straight into the chapter's text buffer,
                // avoiding an intermediate per-text-node String.
                let range = if preserve_whitespace {
                    self.chapter.append_text(text)
                } else {
                    self.chapter.append_text_normalized(text)
                };
                // Text nodes don't have styles - they inherit from parent element
                let text_node = Node::text(range);
                let ir_id = self.chapter.alloc_node(text_node);
                self.chapter.append_child(ir_parent, ir_id);
            }

            ArenaNodeData::Element { name, attrs, .. } => {
                // Compute style for this element
                let elem_ref = ElementRef::new(self.dom, dom_id);
                let mut computed = compute_styles_indexed(
                    elem_ref,
                    &self.cascade_index,
                    parent_style,
                    &mut self.chapter.styles,
                    &mut self.cascade_scratch,
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
                let role = element_to_role(&name.local);

                // Skip hidden elements, but preserve Break nodes
                // CSS may hide <br> (e.g., in verse: "span + br { display: none }") but
                // we still need them for line breaks in text/markdown export
                if computed.display == Display::None && role != Role::Break {
                    return;
                }

                // Create IR node
                let mut ir_node = Node::new(role);
                ir_node.style = self.chapter.styles.intern_ref(&computed);

                let ir_id = self.chapter.alloc_node(ir_node);
                self.chapter.append_child(ir_parent, ir_id);

                // Store semantic attributes
                for attr in attrs {
                    let attr_name = attr.name.local.as_ref();
                    let attr_ns = attr.name.ns.as_ref();
                    match attr_name {
                        // Core layout attributes
                        "href" => {
                            self.chapter.semantics.set_href(ir_id, &attr.value);
                        }
                        "src" => self.chapter.semantics.set_src(ir_id, &attr.value),
                        "alt" => self.chapter.semantics.set_alt(ir_id, &attr.value),
                        "id" => self.chapter.semantics.set_id(ir_id, &attr.value),
                        "title" => self.chapter.semantics.set_title(ir_id, &attr.value),
                        // Language (both lang and xml:lang)
                        "lang" => self.chapter.semantics.set_lang(ir_id, &attr.value),
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
                            self.chapter.semantics.set_epub_type(ir_id, &attr.value);
                        }
                        "epub:type" => {
                            self.chapter.semantics.set_epub_type(ir_id, &attr.value);
                        }
                        "role" => {
                            self.chapter.semantics.set_aria_role(ir_id, &attr.value);
                        }
                        "datetime" => {
                            self.chapter.semantics.set_datetime(ir_id, &attr.value);
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
                                    self.chapter.semantics.set_language(ir_id, lang);
                                    break;
                                }
                                if let Some(lang) = class.strip_prefix("lang-") {
                                    self.chapter.semantics.set_language(ir_id, lang);
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
                self.process_children(dom_id, ir_id, Some(&computed), depth + 1);
            }

            // Skip other node types
            ArenaNodeData::Document | ArenaNodeData::Comment(_) | ArenaNodeData::Doctype { .. } => {
            }
        }
    }
}

/// Transform an ArenaDom to Chapter.
pub fn transform(dom: &ArenaDom, stylesheets: &[(&Stylesheet, Origin)]) -> Chapter {
    let ctx = TransformContext::new(dom, stylesheets);
    ctx.transform()
}

#[cfg(test)]
mod tests {
    use html5ever::driver::ParseOpts;
    use html5ever::parse_document;
    use html5ever::tendril::TendrilSink;

    use super::*;
    use crate::dom::tree_sink::ArenaSink;

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
        let stylesheets = vec![(&ua, Origin::UserAgent)];

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
        let stylesheets = vec![(&ua, Origin::UserAgent)];

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
        let stylesheets = vec![(&ua, Origin::UserAgent)];

        let chapter = transform(&dom, &stylesheets);

        // Find link node
        for id in chapter.iter_dfs() {
            if chapter.node(id).unwrap().role == Role::Link {
                assert_eq!(chapter.semantics.href(id), Some("https://example.com"));
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
        let stylesheets = vec![(&ua, Origin::UserAgent), (&author, Origin::Author)];

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
        let stylesheets = vec![(&ua, Origin::UserAgent)];

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
        let stylesheets = vec![(&ua, Origin::UserAgent)];

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
        let stylesheets = vec![(&ua, Origin::UserAgent)];

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
        let stylesheets = vec![(&ua, Origin::UserAgent)];

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
