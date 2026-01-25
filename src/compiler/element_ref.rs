//! selectors crate Element implementation for ArenaDom.
//!
//! This enables CSS selector matching against our arena DOM.

use std::fmt;

use html5ever::{LocalName, Namespace};
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::context::MatchingContext;
use selectors::matching::ElementSelectorFlags;
use selectors::parser::SelectorParseErrorKind;
use selectors::{OpaqueElement, SelectorImpl};

use super::arena::{ArenaDom, ArenaNodeData, ArenaNodeId};

/// Our selector implementation for the selectors crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BokoSelectors;

/// Identifier string type.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct IdentStr(pub String);

impl precomputed_hash::PrecomputedHash for IdentStr {
    fn precomputed_hash(&self) -> u32 {
        // Simple hash based on string content
        let mut h: u32 = 0;
        for byte in self.0.bytes() {
            h = h.wrapping_mul(31).wrapping_add(byte as u32);
        }
        h
    }
}

/// Wrapper type for LocalName that implements ToCss.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CssLocalName(pub LocalName);

impl precomputed_hash::PrecomputedHash for CssLocalName {
    fn precomputed_hash(&self) -> u32 {
        self.0.precomputed_hash()
    }
}

impl cssparser::ToCss for CssLocalName {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(self.0.as_ref())
    }
}

impl From<String> for CssLocalName {
    fn from(s: String) -> Self {
        Self(LocalName::from(s))
    }
}

impl<'a> From<&'a str> for CssLocalName {
    fn from(s: &'a str) -> Self {
        Self(LocalName::from(s))
    }
}

impl AsRef<str> for CssLocalName {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

/// Wrapper type for Namespace that implements ToCss.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CssNamespace(pub Namespace);

impl precomputed_hash::PrecomputedHash for CssNamespace {
    fn precomputed_hash(&self) -> u32 {
        self.0.precomputed_hash()
    }
}

impl cssparser::ToCss for CssNamespace {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(self.0.as_ref())
    }
}

impl From<String> for CssNamespace {
    fn from(s: String) -> Self {
        Self(Namespace::from(s))
    }
}

impl<'a> From<&'a str> for CssNamespace {
    fn from(s: &'a str) -> Self {
        Self(Namespace::from(s))
    }
}

impl<'i> selectors::parser::Parser<'i> for BokoSelectors {
    type Impl = BokoSelectors;
    type Error = SelectorParseErrorKind<'i>;
}

impl AsRef<str> for IdentStr {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for IdentStr {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl<'a> From<&'a str> for IdentStr {
    fn from(s: &'a str) -> Self {
        Self(s.to_string())
    }
}

impl cssparser::ToCss for IdentStr {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(&self.0)
    }
}

/// Pseudo-element type (not used but required by trait).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PseudoElement {}

impl cssparser::ToCss for PseudoElement {
    fn to_css<W: fmt::Write>(&self, _dest: &mut W) -> fmt::Result {
        match *self {}
    }
}

impl selectors::parser::PseudoElement for PseudoElement {
    type Impl = BokoSelectors;

    fn accepts_state_pseudo_classes(&self) -> bool {
        false
    }

    fn valid_after_slotted(&self) -> bool {
        false
    }
}

/// Non-TS pseudo-class type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NonTSPseudoClass {
    Link,
    Visited,
    Hover,
    Active,
    Focus,
}

impl selectors::parser::NonTSPseudoClass for NonTSPseudoClass {
    type Impl = BokoSelectors;

    fn is_active_or_hover(&self) -> bool {
        matches!(self, Self::Hover | Self::Active)
    }

    fn is_user_action_state(&self) -> bool {
        matches!(self, Self::Hover | Self::Active | Self::Focus)
    }
}

impl cssparser::ToCss for NonTSPseudoClass {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        match self {
            Self::Link => dest.write_str(":link"),
            Self::Visited => dest.write_str(":visited"),
            Self::Hover => dest.write_str(":hover"),
            Self::Active => dest.write_str(":active"),
            Self::Focus => dest.write_str(":focus"),
        }
    }
}

impl SelectorImpl for BokoSelectors {
    type ExtraMatchingData<'a> = ();
    type AttrValue = IdentStr;
    type Identifier = IdentStr;
    type LocalName = CssLocalName;
    type NamespaceUrl = CssNamespace;
    type NamespacePrefix = IdentStr;
    type BorrowedLocalName = CssLocalName;
    type BorrowedNamespaceUrl = CssNamespace;
    type NonTSPseudoClass = NonTSPseudoClass;
    type PseudoElement = PseudoElement;
}

/// Reference to an element in the ArenaDom for selector matching.
#[derive(Clone, Copy)]
pub struct ElementRef<'a> {
    pub dom: &'a ArenaDom,
    pub id: ArenaNodeId,
}

impl<'a> ElementRef<'a> {
    pub fn new(dom: &'a ArenaDom, id: ArenaNodeId) -> Self {
        Self { dom, id }
    }
}

impl fmt::Debug for ElementRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElementRef")
            .field("id", &self.id)
            .field("name", &self.dom.element_name(self.id))
            .finish()
    }
}

impl<'a> selectors::Element for ElementRef<'a> {
    type Impl = BokoSelectors;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self)
    }

    fn parent_element(&self) -> Option<Self> {
        let node = self.dom.get(self.id)?;
        if node.parent.is_none() {
            return None;
        }
        // Only return if parent is an element
        if self.dom.is_element(node.parent) {
            Some(Self::new(self.dom, node.parent))
        } else {
            None
        }
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let node = self.dom.get(self.id)?;
        let mut current = node.prev_sibling;
        while current.is_some() {
            if self.dom.is_element(current) {
                return Some(Self::new(self.dom, current));
            }
            current = self.dom.get(current)?.prev_sibling;
        }
        None
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let node = self.dom.get(self.id)?;
        let mut current = node.next_sibling;
        while current.is_some() {
            if self.dom.is_element(current) {
                return Some(Self::new(self.dom, current));
            }
            current = self.dom.get(current)?.next_sibling;
        }
        None
    }

    fn first_element_child(&self) -> Option<Self> {
        for child in self.dom.children(self.id) {
            if self.dom.is_element(child) {
                return Some(Self::new(self.dom, child));
            }
        }
        None
    }

    fn is_html_element_in_html_document(&self) -> bool {
        // Assume HTML document
        true
    }

    fn has_local_name(&self, name: &CssLocalName) -> bool {
        self.dom
            .element_name(self.id)
            .is_some_and(|n| n == &name.0)
    }

    fn has_namespace(&self, ns: &CssNamespace) -> bool {
        self.dom
            .element_namespace(self.id)
            .is_some_and(|n| n == &ns.0)
    }

    fn is_same_type(&self, other: &Self) -> bool {
        let self_name = self.dom.element_name(self.id);
        let other_name = other.dom.element_name(other.id);
        self_name == other_name
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&CssNamespace>,
        local_name: &CssLocalName,
        operation: &AttrSelectorOperation<&IdentStr>,
    ) -> bool {
        let node = match self.dom.get(self.id) {
            Some(n) => n,
            None => return false,
        };

        let attrs = match &node.data {
            ArenaNodeData::Element { attrs, .. } => attrs,
            _ => return false,
        };

        for attr in attrs {
            // Check namespace
            let ns_match = match ns {
                NamespaceConstraint::Any => true,
                NamespaceConstraint::Specific(ns) => attr.name.ns == ns.0,
            };
            if !ns_match {
                continue;
            }

            // Check local name
            if attr.name.local != local_name.0 {
                continue;
            }

            // Check value operation
            return operation.eval_str(&attr.value);
        }
        false
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &NonTSPseudoClass,
        _context: &mut MatchingContext<'_, Self::Impl>,
    ) -> bool {
        match pc {
            NonTSPseudoClass::Link => {
                // Check if this is an <a> with href
                let is_anchor = self
                    .dom
                    .element_name(self.id)
                    .is_some_and(|n| n.as_ref() == "a");
                is_anchor && self.dom.get_attr(self.id, "href").is_some()
            }
            // Other pseudo-classes don't apply in static context
            _ => false,
        }
    }

    fn match_pseudo_element(
        &self,
        _pe: &PseudoElement,
        _context: &mut MatchingContext<'_, Self::Impl>,
    ) -> bool {
        false
    }

    fn is_link(&self) -> bool {
        let is_anchor = self
            .dom
            .element_name(self.id)
            .is_some_and(|n| n.as_ref() == "a");
        is_anchor && self.dom.get_attr(self.id, "href").is_some()
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, id: &IdentStr, case_sensitivity: CaseSensitivity) -> bool {
        let elem_id = match self.dom.element_id(self.id) {
            Some(i) => i,
            None => return false,
        };
        case_sensitivity.eq(elem_id.as_bytes(), id.0.as_bytes())
    }

    fn has_class(&self, name: &IdentStr, case_sensitivity: CaseSensitivity) -> bool {
        let classes = self.dom.element_classes(self.id);
        classes
            .iter()
            .any(|c| case_sensitivity.eq(c.as_bytes(), name.0.as_bytes()))
    }

    fn imported_part(&self, _name: &IdentStr) -> Option<IdentStr> {
        None
    }

    fn is_part(&self, _name: &IdentStr) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        for child in self.dom.children(self.id) {
            let node = match self.dom.get(child) {
                Some(n) => n,
                None => continue,
            };
            match &node.data {
                ArenaNodeData::Element { .. } => return false,
                ArenaNodeData::Text(t) if !t.trim().is_empty() => return false,
                _ => {}
            }
        }
        true
    }

    fn is_root(&self) -> bool {
        // Root is the html element (child of document)
        let parent = self.dom.get(self.id).map(|n| n.parent);
        if let Some(parent) = parent {
            if let Some(parent_node) = self.dom.get(parent) {
                return matches!(parent_node.data, ArenaNodeData::Document);
            }
        }
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {
        // We don't need to track selector flags for our use case
    }

    fn add_element_unique_hashes(&self, _filter: &mut selectors::bloom::BloomFilter) -> bool {
        // No bloom filter support needed
        false
    }

    fn has_custom_state(&self, _name: &IdentStr) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use selectors::context::SelectorCaches;

    use super::*;
    use crate::compiler::tree_sink::ArenaSink;
    use html5ever::driver::ParseOpts;
    use html5ever::parse_document;
    use html5ever::tendril::TendrilSink;

    fn parse_html(html: &str) -> ArenaDom {
        let sink = ArenaSink::new();
        let result = parse_document(sink, ParseOpts::default())
            .from_utf8()
            .one(html.as_bytes());
        result.into_dom()
    }

    fn parse_selector(
        s: &str,
    ) -> Result<
        selectors::parser::Selector<BokoSelectors>,
        cssparser::ParseError<'_, SelectorParseErrorKind<'_>>,
    > {
        let mut parser_input = cssparser::ParserInput::new(s);
        let mut parser = cssparser::Parser::new(&mut parser_input);
        selectors::parser::Selector::parse(&BokoSelectors, &mut parser)
    }

    fn matches_selector(
        elem: ElementRef<'_>,
        selector: &selectors::parser::Selector<BokoSelectors>,
    ) -> bool {
        let mut caches = SelectorCaches::default();
        let mut context = MatchingContext::new(
            selectors::matching::MatchingMode::Normal,
            None,
            &mut caches,
            selectors::context::QuirksMode::NoQuirks,
            selectors::matching::NeedsSelectorFlags::No,
            selectors::matching::MatchingForInvalidation::No,
        );
        selectors::matching::matches_selector(selector, 0, None, &elem, &mut context)
    }

    #[test]
    fn test_tag_selector() {
        let dom = parse_html("<div><p>Hello</p></div>");
        let p = dom.find_by_tag("p").unwrap();
        let elem = ElementRef::new(&dom, p);

        let selector = parse_selector("p").unwrap();
        assert!(matches_selector(elem, &selector));

        let selector = parse_selector("div").unwrap();
        assert!(!matches_selector(elem, &selector));
    }

    #[test]
    fn test_class_selector() {
        let dom = parse_html(r#"<p class="intro highlight">Hello</p>"#);
        let p = dom.find_by_tag("p").unwrap();
        let elem = ElementRef::new(&dom, p);

        assert!(matches_selector(elem, &parse_selector(".intro").unwrap()));
        assert!(matches_selector(
            elem,
            &parse_selector(".highlight").unwrap()
        ));
        assert!(matches_selector(elem, &parse_selector("p.intro").unwrap()));
        assert!(!matches_selector(
            elem,
            &parse_selector(".missing").unwrap()
        ));
    }

    #[test]
    fn test_id_selector() {
        let dom = parse_html(r#"<p id="main">Hello</p>"#);
        let p = dom.find_by_tag("p").unwrap();
        let elem = ElementRef::new(&dom, p);

        assert!(matches_selector(elem, &parse_selector("#main").unwrap()));
        assert!(matches_selector(elem, &parse_selector("p#main").unwrap()));
        assert!(!matches_selector(elem, &parse_selector("#other").unwrap()));
    }

    #[test]
    fn test_descendant_selector() {
        let dom = parse_html("<div><span><p>Hello</p></span></div>");
        let p = dom.find_by_tag("p").unwrap();
        let elem = ElementRef::new(&dom, p);

        assert!(matches_selector(elem, &parse_selector("div p").unwrap()));
        assert!(matches_selector(
            elem,
            &parse_selector("div span p").unwrap()
        ));
        assert!(matches_selector(elem, &parse_selector("span p").unwrap()));
    }

    #[test]
    fn test_child_selector() {
        let dom = parse_html("<div><p>Direct</p></div>");
        let p = dom.find_by_tag("p").unwrap();
        let elem = ElementRef::new(&dom, p);

        assert!(matches_selector(elem, &parse_selector("div > p").unwrap()));

        let dom2 = parse_html("<div><span><p>Nested</p></span></div>");
        let p2 = dom2.find_by_tag("p").unwrap();
        let elem2 = ElementRef::new(&dom2, p2);

        assert!(!matches_selector(
            elem2,
            &parse_selector("div > p").unwrap()
        ));
        assert!(matches_selector(
            elem2,
            &parse_selector("span > p").unwrap()
        ));
    }
}
