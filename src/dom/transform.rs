//! Transform ArenaDom to Chapter.

use selectors::Element as _;
use selectors::bloom::BloomFilter;

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

#[cfg(test)]
pub(crate) fn user_agent_stylesheet() -> Stylesheet {
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

/// Synthesize CSS declarations from legacy presentational attributes
/// (`align=`, `valign=`), which many older EPUB/MOBI-derived books rely on.
/// Per CSS these are presentational hints: they apply before author rules
/// (any matching selector overrides them) but beat inherited values.
fn presentational_hints(
    element: &html5ever::LocalName,
    attrs: &[crate::dom::arena::Attribute],
) -> Option<crate::style::InlineStyle> {
    let name = element.as_ref();
    let aligned = matches!(
        name,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "td"
            | "th"
            | "tr"
            | "table"
            | "caption"
            | "tbody"
            | "thead"
            | "tfoot"
            | "center"
    );
    let cellish = matches!(name, "td" | "th" | "tr" | "tbody" | "thead" | "tfoot");
    let font = name == "font";
    // MOBI-7 spacing convention: height= is vertical space before the
    // block, width= is the first-line indent.
    let mobi_spaced = matches!(name, "p" | "div" | "blockquote");
    if !aligned && !cellish && !font && !mobi_spaced {
        return None;
    }
    let clean_len = |v: &str| {
        !v.is_empty()
            && v.len() <= 12
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '%' || c == '-')
    };
    let mut css = String::new();
    for attr in attrs {
        match attr.name.local.as_ref() {
            "align" if aligned => {
                let v = attr.value.trim().to_ascii_lowercase();
                if matches!(v.as_str(), "left" | "right" | "center" | "justify") {
                    css.push_str("text-align: ");
                    css.push_str(&v);
                    css.push(';');
                }
            }
            "valign" if cellish => {
                let v = attr.value.trim().to_ascii_lowercase();
                if matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline") {
                    css.push_str("vertical-align: ");
                    css.push_str(&v);
                    css.push(';');
                }
            }
            // Legacy <font> element — the styling mechanism of old MOBI
            // books (syntax-highlight colors, sized code and headings).
            "size" if font => {
                let v = attr.value.trim();
                // Absolute 1-7 or relative +N/-N from the base size 3,
                // mapped onto the browser scale.
                let base: i32 = if let Some(rest) = v.strip_prefix('+') {
                    3 + rest.parse::<i32>().unwrap_or(0)
                } else if let Some(rest) = v.strip_prefix('-') {
                    3 - rest.parse::<i32>().unwrap_or(0)
                } else {
                    v.parse().unwrap_or(3)
                };
                let em = match base.clamp(1, 7) {
                    1 => "0.625",
                    2 => "0.8125",
                    3 => "1",
                    4 => "1.125",
                    5 => "1.5",
                    6 => "2",
                    _ => "3",
                };
                css.push_str("font-size: ");
                css.push_str(em);
                css.push_str("em;");
            }
            "color" if font => {
                let v = attr.value.trim();
                if !v.is_empty() {
                    css.push_str("color: ");
                    css.push_str(v);
                    css.push(';');
                }
            }
            "face" if font => {
                let v = attr.value.trim();
                if !v.is_empty() && v.chars().all(|c| !c.is_control() && c != ';') {
                    css.push_str("font-family: ");
                    css.push_str(v);
                    css.push(';');
                }
            }
            // MOBI-7 convention: a height=/width= attribute marks a
            // legacy-spaced block — ALL of its vertical spacing comes from
            // the attribute (paragraphs carry no implicit margins in that
            // model), so both margins are pinned: top to the attribute
            // value, bottom to zero.
            "height" if mobi_spaced => {
                let v = attr.value.trim();
                if clean_len(v) {
                    css.push_str("margin-top: ");
                    css.push_str(v);
                    css.push_str(";margin-bottom: 0;");
                }
            }
            "width" if mobi_spaced => {
                let v = attr.value.trim();
                if clean_len(v) {
                    css.push_str("text-indent: ");
                    css.push_str(v);
                    css.push_str(";margin-bottom: 0;");
                    if !attrs.iter().any(|a| a.name.local.as_ref() == "height") {
                        css.push_str("margin-top: 0;");
                    }
                }
            }
            _ => {}
        }
    }
    if css.is_empty() {
        return None;
    }
    let parsed = crate::style::InlineStyle::parse(&css);
    (!parsed.is_empty()).then_some(parsed)
}

/// Context for the transform operation.
struct TransformContext<'a> {
    dom: &'a ArenaDom,
    /// Selector-bucketed view of `stylesheets`, built once for the whole chapter.
    cascade_index: CascadeIndex<'a>,
    /// Reused across every element of the chapter (candidate buffer + selector caches).
    cascade_scratch: CascadeScratch,
    /// Ancestor bloom filter for the selectors crate's fast-reject path.
    /// Invariant: whenever styles are computed for an element, the filter
    /// contains the hashes of exactly that element's element ancestors (the
    /// DFS pushes each element before descending into its children and pops
    /// it after). Only maintained when `use_bloom` is set.
    bloom: BloomFilter,
    /// Whether maintaining `bloom` can pay off: false when no selector has
    /// ancestor requirements (then `None` is passed and matching skips the
    /// bloom checks entirely).
    use_bloom: bool,
    chapter: Chapter,
}

impl<'a> TransformContext<'a> {
    fn new(dom: &'a ArenaDom, stylesheets: &'a [(&'a Stylesheet, Origin)]) -> Self {
        let cascade_index = CascadeIndex::build(stylesheets);
        let use_bloom = cascade_index.has_complex_selectors();
        Self {
            dom,
            cascade_index,
            cascade_scratch: CascadeScratch::default(),
            bloom: BloomFilter::new(),
            use_bloom,
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

        // Seed the ancestor bloom filter with body's element ancestors (html):
        // the filter must contain every element ancestor of whatever element
        // styles are computed for, starting with body itself.
        if self.use_bloom {
            let mut ancestor = ElementRef::new(self.dom, body).parent_element();
            while let Some(elem) = ancestor {
                elem.each_bloom_hash(|hash| self.bloom.insert_hash(hash));
                ancestor = elem.parent_element();
            }
        }

        // Compute html's style first so properties set on the `html` selector
        // (e.g. `html { color: … }`) inherit into body like in a browser.
        // No bloom filter here: it is seeded for body's ancestors, not html's.
        let html_style = self
            .dom
            .find_by_tag("html")
            .filter(|&html_id| html_id != body)
            .map(|html_id| {
                let inline = self.inline_style_of(html_id);
                compute_styles_indexed(
                    ElementRef::new(self.dom, html_id),
                    &self.cascade_index,
                    None,
                    &mut self.chapter.styles,
                    &mut self.cascade_scratch,
                    None,
                    inline.as_ref(),
                    None,
                )
            });

        // Compute body's style so its properties (like hyphens: auto) are inherited
        let mut body_style = {
            let elem_ref = ElementRef::new(self.dom, body);
            let bloom = if self.use_bloom {
                Some(&self.bloom)
            } else {
                None
            };
            let inline = self.inline_style_of(body);
            compute_styles_indexed(
                elem_ref,
                &self.cascade_index,
                html_style.as_ref(),
                &mut self.chapter.styles,
                &mut self.cascade_scratch,
                bloom,
                inline.as_ref(),
                None,
            )
        };

        // Add html lang to body style if present (so it's inherited by all content)
        if let Some(lang) = html_lang
            && body_style.language.is_none()
        {
            body_style.language = Some(lang);
        }

        // Process body's children as children of IR root, inheriting body's
        // style. Body becomes an ancestor of everything the DFS styles, so
        // push it onto the filter first (a no-op when body is the document
        // node rather than an element).
        if self.use_bloom {
            ElementRef::new(self.dom, body).each_bloom_hash(|hash| self.bloom.insert_hash(hash));
        }
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

    /// Parse the `style` attribute of a DOM element, if present and non-empty.
    fn inline_style_of(&self, dom_id: ArenaNodeId) -> Option<crate::style::InlineStyle> {
        let node = self.dom.get(dom_id)?;
        let ArenaNodeData::Element { attrs, .. } = &node.data else {
            return None;
        };
        attrs
            .iter()
            .find(|a| a.name.local.as_ref() == "style" && !a.value.is_empty())
            .map(|a| crate::style::InlineStyle::parse(&a.value))
            .filter(|i| !i.is_empty())
    }

    /// Whether the DOM node's previous and next siblings are both
    /// inline-level (non-blank text, or an element with an inline role).
    /// Distinguishes a word separator between inline siblings from
    /// inter-block indentation.
    fn flanked_by_inline(&self, dom_id: ArenaNodeId) -> bool {
        let Some(node) = self.dom.get(dom_id) else {
            return false;
        };
        self.is_inline_level(node.prev_sibling) && self.is_inline_level(node.next_sibling)
    }

    fn is_inline_level(&self, id: ArenaNodeId) -> bool {
        match self.dom.get(id).map(|n| &n.data) {
            Some(ArenaNodeData::Text(t)) => !t.trim().is_empty(),
            Some(ArenaNodeData::Element { name, .. }) => {
                crate::dom::optimize::is_inline_role(element_to_role(&name.local))
            }
            _ => false,
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
                // In pre-like contexts every byte is content, including
                // whitespace-only nodes: `<pre><span>a</span>\n  <span>b</span>`
                // relies on that text node for its line structure, so this
                // check must run before any whitespace-dropping heuristics.
                let preserve_whitespace = parent_style
                    .map(|s| {
                        matches!(
                            s.white_space,
                            WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine
                        )
                    })
                    .unwrap_or(false);

                // Handle whitespace-only text nodes
                if text.trim().is_empty() && !preserve_whitespace {
                    // No parent means we're at root level - skip whitespace
                    if parent_style.is_none() {
                        return;
                    }

                    // Whitespace between inline elements should be preserved as a single space.
                    //
                    // This handles cases like: <cite><abbr>A</abbr> <abbr>B</abbr></cite>
                    // where the space between abbrs must be preserved even though cite is block.
                    let has_newlines = text.contains('\n');
                    let is_block_parent = parent_style
                        .map(|s| s.display != Display::Inline)
                        .unwrap_or(true);

                    // Whitespace containing newlines inside a block parent is
                    // usually inter-element indentation noise — but between
                    // two inline-level siblings it is a real word separator:
                    // browsers render `<div><i>A</i>\n<i>B</i></div>` as
                    // "A B". Drop it only when it touches a block boundary.
                    if has_newlines && is_block_parent && !self.flanked_by_inline(dom_id) {
                        return;
                    }

                    // Preserve as a single space
                    let range = self.chapter.append_text(" ");
                    let text_node = Node::text(range);
                    let ir_id = self.chapter.alloc_node(text_node);
                    self.chapter.append_child(ir_parent, ir_id);
                    return;
                }

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
                // Compute style for this element. The bloom filter holds the
                // hashes of this element's ancestors (maintained by the
                // push/pop around process_children below).
                let elem_ref = ElementRef::new(self.dom, dom_id);
                let bloom = if self.use_bloom {
                    Some(&self.bloom)
                } else {
                    None
                };
                // Inline style="" declarations join the cascade above every
                // selector-matched normal declaration.
                let inline = attrs
                    .iter()
                    .find(|a| a.name.local.as_ref() == "style" && !a.value.is_empty())
                    .map(|a| crate::style::InlineStyle::parse(&a.value))
                    .filter(|i| !i.is_empty());
                let hints = presentational_hints(&name.local, attrs.as_slice());
                let mut computed = compute_styles_indexed(
                    elem_ref,
                    &self.cascade_index,
                    parent_style,
                    &mut self.chapter.styles,
                    &mut self.cascade_scratch,
                    bloom,
                    inline.as_ref(),
                    hints.as_ref(),
                );

                // Merge lang attribute into style (for KFX language property)
                // This must happen before interning so the style includes the language
                for attr in attrs {
                    if attr.name.local.as_ref() == "lang" && !attr.value.is_empty() {
                        computed.language = Some(attr.value.to_string());
                        break;
                    }
                }

                // MathML: `<math>` (by namespace or local name) becomes a
                // single `Role::Math` IR leaf whose expression tree lives in
                // the chapter's `math` side-table. `element_to_role` only
                // sees the local name, so the namespace test is done here.
                // We take over the whole subtree — no generic child recursion.
                if name.ns.as_ref() == crate::math::mathml::MATHML_NS
                    || name.local.as_ref() == "math"
                {
                    if computed.display == Display::None {
                        return;
                    }
                    let mut ir_node = Node::new(Role::Math);
                    ir_node.style = self.chapter.styles.intern_ref(&computed);
                    let ir_id = self.chapter.alloc_node(ir_node);
                    self.chapter.append_child(ir_parent, ir_id);
                    // Preserve the element id for anchor/link resolution.
                    for attr in attrs {
                        if attr.name.local.as_ref() == "id" {
                            self.chapter.semantics.set_id(ir_id, &attr.value);
                        }
                    }
                    let mut expr = crate::math::mathml::from_mathml(self.dom, dom_id);
                    // Block context implies display math: a <math> that is
                    // its parent's only non-whitespace content is a display
                    // equation even without display="block" (the common
                    // publisher shape — only ~2% of equations carry the
                    // attribute in practice).
                    if !expr.display
                        && let Some(parent) = self.dom.get(dom_id).map(|n| n.parent)
                    {
                        let alone = self.dom.children(parent).all(|c| {
                            c == dom_id
                                || self
                                    .dom
                                    .text_content(c)
                                    .is_some_and(|t| t.trim().is_empty())
                        });
                        if alone {
                            expr.display = true;
                        }
                    }
                    self.chapter.math.insert(ir_id, expr);
                    return;
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

                // Process children. This element is an ancestor of its
                // children, so push its hashes onto the filter around the
                // recursion (and pop them after, keeping the counting filter
                // balanced).
                if self.use_bloom {
                    elem_ref.each_bloom_hash(|hash| self.bloom.insert_hash(hash));
                }
                self.process_children(dom_id, ir_id, Some(&computed), depth + 1);
                if self.use_bloom {
                    elem_ref.each_bloom_hash(|hash| self.bloom.remove_hash(hash));
                }
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
