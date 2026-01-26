//! KFX token stream for bidirectional conversion.
//!
//! The token stream is an intermediate representation that abstracts away
//! the nested Ion structure. Both import and export work through tokens:
//!
//! Import: Ion → TokenStream → IR
//! Export: IR → TokenStream → Ion
//!
//! ## Key Design: Generic Semantic Storage
//!
//! Tokens use `HashMap<SemanticTarget, String>` for semantic attributes,
//! not typed fields like `link_target` or `resource`. This keeps the token
//! layer format-agnostic - all format-specific logic lives in the schema.

use crate::ir::Role;
use crate::kfx::schema::SemanticTarget;
use std::collections::HashMap;

/// A token in the KFX content stream.
#[derive(Debug, Clone, PartialEq)]
pub enum KfxToken {
    /// Start of an element (container, paragraph, etc.)
    StartElement(ElementStart),
    /// End of an element
    EndElement,
    /// Text content
    Text(String),
    /// Start of an inline style span
    StartSpan(SpanStart),
    /// End of an inline style span
    EndSpan,
}

/// Information about an element start.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementStart {
    /// The resolved IR role for this element.
    pub role: Role,
    /// KFX element ID (for anchors/links).
    pub id: Option<i64>,
    /// Semantic attributes (generic map, not typed fields).
    pub semantics: HashMap<SemanticTarget, String>,
    /// Content reference (for text lookup).
    pub content_ref: Option<ContentRef>,
    /// Inline style events (spans within text content).
    pub style_events: Vec<SpanStart>,
}

impl ElementStart {
    /// Create a new element start with just a role.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            id: None,
            semantics: HashMap::new(),
            content_ref: None,
            style_events: Vec::new(),
        }
    }

    /// Get a semantic attribute value.
    pub fn get_semantic(&self, target: SemanticTarget) -> Option<&str> {
        self.semantics.get(&target).map(|s| s.as_str())
    }

    /// Set a semantic attribute value.
    pub fn set_semantic(&mut self, target: SemanticTarget, value: String) {
        self.semantics.insert(target, value);
    }
}

/// Reference to text in a content entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentRef {
    pub name: String,
    pub index: usize,
}

/// Information about an inline span start.
///
/// The role and semantics are determined by the schema based on which fields are present.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanStart {
    /// IR Role determined by schema (Link, Inline, etc.)
    pub role: Role,
    /// Semantic attributes (generic map).
    pub semantics: HashMap<SemanticTarget, String>,
    /// Byte offset in parent text (for reconstruction)
    pub offset: usize,
    /// Length in bytes
    pub length: usize,
}

impl SpanStart {
    /// Create a new span start.
    pub fn new(role: Role, offset: usize, length: usize) -> Self {
        Self {
            role,
            semantics: HashMap::new(),
            offset,
            length,
        }
    }

    /// Get a semantic attribute value.
    pub fn get_semantic(&self, target: SemanticTarget) -> Option<&str> {
        self.semantics.get(&target).map(|s| s.as_str())
    }

    /// Set a semantic attribute value.
    pub fn set_semantic(&mut self, target: SemanticTarget, value: String) {
        self.semantics.insert(target, value);
    }
}

/// A stream of KFX tokens with iterator support.
#[derive(Debug, Default)]
pub struct TokenStream {
    tokens: Vec<KfxToken>,
}

impl TokenStream {
    pub fn new() -> Self {
        Self { tokens: Vec::new() }
    }

    pub fn push(&mut self, token: KfxToken) {
        self.tokens.push(token);
    }

    pub fn start_element(&mut self, role: Role) {
        self.tokens
            .push(KfxToken::StartElement(ElementStart::new(role)));
    }

    pub fn start_element_with(
        &mut self,
        role: Role,
        id: Option<i64>,
        semantics: HashMap<SemanticTarget, String>,
        content_ref: Option<ContentRef>,
        style_events: Vec<SpanStart>,
    ) {
        self.tokens.push(KfxToken::StartElement(ElementStart {
            role,
            id,
            semantics,
            content_ref,
            style_events,
        }));
    }

    pub fn end_element(&mut self) {
        self.tokens.push(KfxToken::EndElement);
    }

    pub fn text(&mut self, s: impl Into<String>) {
        self.tokens.push(KfxToken::Text(s.into()));
    }

    pub fn start_span(&mut self, role: Role, semantics: HashMap<SemanticTarget, String>) {
        self.tokens.push(KfxToken::StartSpan(SpanStart {
            role,
            semantics,
            offset: 0,
            length: 0,
        }));
    }

    pub fn end_span(&mut self) {
        self.tokens.push(KfxToken::EndSpan);
    }

    pub fn iter(&self) -> impl Iterator<Item = &KfxToken> {
        self.tokens.iter()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn into_iter(self) -> impl Iterator<Item = KfxToken> {
        self.tokens.into_iter()
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

impl IntoIterator for TokenStream {
    type Item = KfxToken;
    type IntoIter = std::vec::IntoIter<KfxToken>;

    fn into_iter(self) -> Self::IntoIter {
        self.tokens.into_iter()
    }
}

impl<'a> IntoIterator for &'a TokenStream {
    type Item = &'a KfxToken;
    type IntoIter = std::slice::Iter<'a, KfxToken>;

    fn into_iter(self) -> Self::IntoIter {
        self.tokens.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_stream_basic() {
        let mut stream = TokenStream::new();
        stream.start_element(Role::Paragraph);
        stream.text("Hello");
        stream.end_element();

        assert_eq!(stream.len(), 3);
    }

    #[test]
    fn test_token_stream_with_spans() {
        let mut stream = TokenStream::new();
        stream.start_element(Role::Paragraph);
        stream.text("Click ");

        let mut semantics = HashMap::new();
        semantics.insert(SemanticTarget::Href, "http://example.com".to_string());
        stream.start_span(Role::Link, semantics);
        stream.text("here");
        stream.end_span();
        stream.end_element();

        assert_eq!(stream.len(), 6);
    }

    #[test]
    fn test_element_semantics() {
        let mut elem = ElementStart::new(Role::Image);
        elem.set_semantic(SemanticTarget::Src, "cover.jpg".to_string());
        elem.set_semantic(SemanticTarget::Alt, "Cover image".to_string());

        assert_eq!(elem.get_semantic(SemanticTarget::Src), Some("cover.jpg"));
        assert_eq!(elem.get_semantic(SemanticTarget::Alt), Some("Cover image"));
        assert_eq!(elem.get_semantic(SemanticTarget::Href), None);
    }

    #[test]
    fn test_span_semantics() {
        let mut span = SpanStart::new(Role::Link, 10, 5);
        span.set_semantic(SemanticTarget::Href, "chapter2".to_string());

        assert_eq!(span.get_semantic(SemanticTarget::Href), Some("chapter2"));
        assert_eq!(span.offset, 10);
        assert_eq!(span.length, 5);
    }
}
