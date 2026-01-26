//! KFX storyline parsing and IR building.
//!
//! This module handles bidirectional conversion between KFX storyline
//! structures and boko's IR, using a schema-driven approach:
//!
//! Import: Ion → TokenStream → IR
//! Export: IR → TokenStream → Ion (TODO)
//!
//! ## Key Design: Generic Interpreter
//!
//! The interpreter is completely generic - it knows nothing about KFX semantics.
//! All mapping logic is driven by the schema:
//!
//! 1. Read element type symbol ID
//! 2. Fetch Strategy from schema
//! 3. Execute Strategy to determine role
//! 4. Extract ALL attributes using schema's AttrRules
//! 5. Apply transformers to convert values

use crate::ir::{IRChapter, Node, NodeId};
use crate::kfx::container::get_field;
use crate::kfx::ion::IonValue;
use crate::kfx::schema::{schema, SemanticTarget};
use crate::kfx::symbols::KfxSymbol;
use crate::kfx::tokens::{ContentRef, ElementStart, KfxToken, SpanStart, TokenStream};
use crate::kfx::transforms::ImportContext;
use std::collections::HashMap;

/// Context for tokenization including anchor resolution.
struct TokenizeContext<'a> {
    doc_symbols: &'a [String],
    anchors: Option<&'a HashMap<String, String>>,
}

/// Shorthand for getting a KfxSymbol as u32.
macro_rules! sym {
    ($variant:ident) => {
        KfxSymbol::$variant as u32
    };
}

// ============================================================================
// IMPORT: Ion → TokenStream → IR
// ============================================================================

/// Tokenize a KFX storyline into a token stream.
///
/// This is the first stage of import: converting the nested Ion structure
/// into a flat stream of tokens that can be processed by the stack builder.
///
/// The `anchors` map is used to resolve external links (anchor_name → uri).
pub fn tokenize_storyline(
    storyline: &IonValue,
    doc_symbols: &[String],
    anchors: Option<&HashMap<String, String>>,
) -> TokenStream {
    let mut stream = TokenStream::new();

    let fields = match storyline.as_struct() {
        Some(f) => f,
        None => return stream,
    };

    let content_list = match get_field(fields, sym!(ContentList)) {
        Some(list) => list,
        None => return stream,
    };

    let ctx = TokenizeContext { doc_symbols, anchors };
    tokenize_content_list(content_list, &ctx, &mut stream);
    stream
}

/// Tokenize a content_list array.
fn tokenize_content_list(list: &IonValue, ctx: &TokenizeContext, stream: &mut TokenStream) {
    let items = match list.as_list() {
        Some(l) => l,
        None => return,
    };

    for item in items {
        tokenize_content_item(item, ctx, stream);
    }
}

/// Tokenize a single content item.
///
/// This is a **generic schema-driven interpreter** that:
/// 1. Reads the element's type symbol
/// 2. Looks up the strategy from the schema
/// 3. Executes the strategy to determine role
/// 4. Extracts ALL attributes using schema rules (no hardcoded targets)
/// 5. Applies transformers to values
fn tokenize_content_item(item: &IonValue, ctx: &TokenizeContext, stream: &mut TokenStream) {
    // Unwrap annotation if present
    let inner = item.unwrap_annotated();
    let fields = match inner.as_struct() {
        Some(f) => f,
        None => return,
    };

    // Get element type symbol ID
    let kfx_type_id = get_field(fields, sym!(Type))
        .and_then(|v| v.as_symbol())
        .unwrap_or(sym!(Container));

    // Use schema to resolve role with attribute lookup closure
    let role = schema().resolve_element_role(kfx_type_id, |symbol| {
        get_field(fields, symbol as u32).and_then(|v| v.as_int())
    });

    // Get element ID
    let id = get_field(fields, sym!(Id)).and_then(|v| v.as_int());

    // Extract ALL semantic attributes using schema rules (GENERIC!)
    let semantics = extract_all_element_attrs(fields, kfx_type_id, ctx);

    // Get content reference (for text)
    let content_ref = get_field(fields, sym!(Content))
        .and_then(|v| v.as_struct())
        .and_then(|content_fields| {
            let name = get_field(content_fields, sym!(Name))
                .and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))?;
            let index = get_field(content_fields, sym!(Index))
                .and_then(|v| v.as_int())
                .map(|n| n as usize)?;
            Some(ContentRef { name, index })
        });

    // Get style events (inline spans) - fully schema-driven
    let style_events = get_field(fields, sym!(StyleEvents))
        .and_then(|v| v.as_list())
        .map(|events| parse_style_events(events, ctx))
        .unwrap_or_default();

    // Get nested children
    let has_children = get_field(fields, sym!(ContentList)).is_some();

    // Emit StartElement token
    stream.push(KfxToken::StartElement(ElementStart {
        role,
        id,
        semantics,
        content_ref,
        style_events,
    }));

    // Recurse into children
    if has_children {
        if let Some(children) = get_field(fields, sym!(ContentList)) {
            tokenize_content_list(children, ctx, stream);
        }
    }

    // Emit EndElement token
    stream.push(KfxToken::EndElement);
}

/// Extract ALL semantic attributes for an element using schema rules.
///
/// This is **fully generic** - it iterates all AttrRules from the schema
/// and applies their transformers. No hardcoded SemanticTarget checks.
fn extract_all_element_attrs(
    fields: &[(u32, IonValue)],
    kfx_type_id: u32,
    ctx: &TokenizeContext,
) -> HashMap<SemanticTarget, String> {
    let mut result = HashMap::new();
    let import_ctx = ImportContext {
        doc_symbols: ctx.doc_symbols,
        chapter_id: None,
        anchors: ctx.anchors,
    };

    for rule in schema().element_attr_rules(kfx_type_id) {
        if let Some(raw_value) =
            get_field(fields, rule.kfx_field as u32).and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))
        {
            // Apply the transformer to convert the raw value
            let parsed = rule.transform.import(&raw_value, &import_ctx);

            // Convert ParsedAttribute to string for storage
            let final_value = match parsed {
                crate::kfx::transforms::ParsedAttribute::String(s) => s,
                crate::kfx::transforms::ParsedAttribute::Link(link) => link.to_href(),
                crate::kfx::transforms::ParsedAttribute::Anchor(id) => id,
            };

            result.insert(rule.target, final_value);
        }
    }

    result
}

/// Parse style events from Ion using schema-driven interpretation.
fn parse_style_events(events: &[IonValue], ctx: &TokenizeContext) -> Vec<SpanStart> {
    events
        .iter()
        .filter_map(|event| {
            let fields = event.as_struct()?;
            let offset = get_field(fields, sym!(Offset))
                .and_then(|v| v.as_int())
                .map(|n| n as usize)?;
            let length = get_field(fields, sym!(Length))
                .and_then(|v| v.as_int())
                .map(|n| n as usize)?;

            // Create closure to check which fields are present
            let has_field = |symbol: KfxSymbol| get_field(fields, symbol as u32).is_some();

            // Use schema to determine role
            let role = schema().resolve_span_role(&has_field);

            // Extract ALL semantic attributes using schema rules (GENERIC!)
            let semantics = extract_all_span_attrs(fields, &has_field, ctx);

            Some(SpanStart {
                role,
                semantics,
                offset,
                length,
            })
        })
        .collect()
}

/// Extract ALL semantic attributes for a span using schema rules.
///
/// This is **fully generic** - no hardcoded SemanticTarget checks.
fn extract_all_span_attrs<F>(
    fields: &[(u32, IonValue)],
    has_field: F,
    ctx: &TokenizeContext,
) -> HashMap<SemanticTarget, String>
where
    F: Fn(KfxSymbol) -> bool,
{
    let mut result = HashMap::new();
    let import_ctx = ImportContext {
        doc_symbols: ctx.doc_symbols,
        chapter_id: None,
        anchors: ctx.anchors,
    };

    for rule in schema().span_attr_rules(&has_field) {
        if let Some(raw_value) =
            get_field(fields, rule.kfx_field as u32).and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))
        {
            // Apply the transformer to convert the raw value
            let parsed = rule.transform.import(&raw_value, &import_ctx);

            // Convert ParsedAttribute to string for storage
            let final_value = match parsed {
                crate::kfx::transforms::ParsedAttribute::String(s) => s,
                crate::kfx::transforms::ParsedAttribute::Link(link) => link.to_href(),
                crate::kfx::transforms::ParsedAttribute::Anchor(id) => id,
            };

            result.insert(rule.target, final_value);
        }
    }

    result
}

// ============================================================================
// Token Stream → IR (Stack-based builder)
// ============================================================================

/// Build an IR chapter from a token stream.
///
/// Uses a stack-based approach to handle nested elements.
/// Applies semantics **generically** from the token's semantics map.
pub fn build_ir_from_tokens<F>(tokens: &TokenStream, mut content_lookup: F) -> IRChapter
where
    F: FnMut(&str, usize) -> Option<String>,
{
    let mut chapter = IRChapter::new();
    let mut stack: Vec<NodeId> = vec![chapter.root()];

    for token in tokens {
        match token {
            KfxToken::StartElement(elem) => {
                let parent = *stack.last().unwrap_or(&chapter.root());

                // Create the node
                let node = Node::new(elem.role);
                let node_id = chapter.alloc_node(node);
                chapter.append_child(parent, node_id);

                // Apply ALL semantic attributes from the generic map
                apply_semantics_to_node(&mut chapter, node_id, &elem.semantics);

                // Handle text content with style events
                if let Some(ref content_ref) = elem.content_ref {
                    if let Some(text) = content_lookup(&content_ref.name, content_ref.index) {
                        if elem.style_events.is_empty() {
                            // Simple case: no inline styles
                            let range = chapter.append_text(&text);
                            let text_node = chapter.alloc_node(Node::text(range));
                            chapter.append_child(node_id, text_node);
                        } else {
                            // Complex case: apply style events as spans
                            build_text_with_spans(&mut chapter, node_id, &text, &elem.style_events);
                        }
                    }
                }

                stack.push(node_id);
            }

            KfxToken::EndElement => {
                stack.pop();
            }

            KfxToken::Text(text) => {
                let parent = *stack.last().unwrap_or(&chapter.root());
                let range = chapter.append_text(text);
                let text_node = chapter.alloc_node(Node::text(range));
                chapter.append_child(parent, text_node);
            }

            KfxToken::StartSpan(_) | KfxToken::EndSpan => {
                // Style events are handled via ElementStart.style_events
            }
        }
    }

    chapter
}

/// Apply semantic attributes to a node from a generic map.
///
/// This is the **only place** that knows about SemanticTarget → IR mapping.
/// It's a simple dispatcher, not format-specific logic.
fn apply_semantics_to_node(
    chapter: &mut IRChapter,
    node_id: NodeId,
    semantics: &HashMap<SemanticTarget, String>,
) {
    for (target, value) in semantics {
        match target {
            SemanticTarget::Src => chapter.semantics.set_src(node_id, value.clone()),
            SemanticTarget::Href => {
                chapter.semantics.set_href(node_id, value.clone());
            }
            SemanticTarget::Alt => chapter.semantics.set_alt(node_id, value.clone()),
            SemanticTarget::Id => chapter.semantics.set_id(node_id, value.clone()),
        }
    }
}

/// Build text nodes with inline spans applied.
fn build_text_with_spans(
    chapter: &mut IRChapter,
    parent: NodeId,
    text: &str,
    spans: &[SpanStart],
) {
    // Sort spans by offset
    let mut sorted_spans: Vec<_> = spans.iter().collect();
    sorted_spans.sort_by_key(|s| s.offset);

    let mut pos = 0;

    for span in sorted_spans {
        // Snap byte offsets to valid UTF-8 char boundaries
        let span_start = snap_to_char_boundary(text, span.offset);
        let span_end = snap_to_char_boundary(text, span.offset + span.length);

        // Add text before this span
        if span_start > pos {
            let before = &text[pos..span_start];
            if !before.is_empty() {
                let range = chapter.append_text(before);
                let text_node = chapter.alloc_node(Node::text(range));
                chapter.append_child(parent, text_node);
            }
        }

        // Add the span - role is already determined by schema
        if span_end > span_start {
            let span_text = &text[span_start..span_end];

            let span_node = chapter.alloc_node(Node::new(span.role));
            chapter.append_child(parent, span_node);

            // Apply ALL semantic attributes from the generic map
            apply_semantics_to_node(chapter, span_node, &span.semantics);

            let range = chapter.append_text(span_text);
            let text_node = chapter.alloc_node(Node::text(range));
            chapter.append_child(span_node, text_node);
        }

        pos = span_end;
    }

    // Add remaining text after last span
    if pos < text.len() {
        let after = &text[pos..];
        if !after.is_empty() {
            let range = chapter.append_text(after);
            let text_node = chapter.alloc_node(Node::text(range));
            chapter.append_child(parent, text_node);
        }
    }
}

/// Snap a byte offset to the nearest valid UTF-8 char boundary.
fn snap_to_char_boundary(text: &str, byte_offset: usize) -> usize {
    if byte_offset >= text.len() {
        return text.len();
    }

    if text.is_char_boundary(byte_offset) {
        return byte_offset;
    }

    // Search backwards for a valid boundary
    let mut pos = byte_offset;
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

// ============================================================================
// Helper functions
// ============================================================================

/// Resolve a symbol ID to its string representation.
fn resolve_symbol(id: u32, doc_symbols: &[String]) -> Option<&str> {
    crate::kfx::container::resolve_symbol(id, doc_symbols)
}

/// Resolve a value that could be either a symbol or string.
fn resolve_symbol_or_string(value: &IonValue, doc_symbols: &[String]) -> Option<String> {
    match value {
        IonValue::String(s) => Some(s.clone()),
        IonValue::Symbol(id) => resolve_symbol(*id, doc_symbols).map(|s| s.to_string()),
        _ => None,
    }
}

// ============================================================================
// High-level API (used by KfxImporter)
// ============================================================================

/// Parse a storyline and build IR in one step.
///
/// This is the main entry point for KFX import.
///
/// The `anchors` map is used to resolve external links (anchor_name → uri).
pub fn parse_storyline_to_ir<F>(
    storyline: &IonValue,
    doc_symbols: &[String],
    anchors: Option<&HashMap<String, String>>,
    content_lookup: F,
) -> IRChapter
where
    F: FnMut(&str, usize) -> Option<String>,
{
    let tokens = tokenize_storyline(storyline, doc_symbols, anchors);
    build_ir_from_tokens(&tokens, content_lookup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Role;

    #[test]
    fn test_tokenize_creates_proper_structure() {
        // Test that tokenization produces expected token sequence
        let mut stream = TokenStream::new();
        stream.start_element(Role::Paragraph);
        stream.text("Hello");
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, |_, _| None);
        assert_eq!(chapter.node_count(), 3); // root + para + text
    }

    #[test]
    fn test_build_ir_with_image() {
        let mut stream = TokenStream::new();
        let mut semantics = HashMap::new();
        semantics.insert(SemanticTarget::Src, "cover.jpg".to_string());

        stream.push(KfxToken::StartElement(ElementStart {
            role: Role::Image,
            id: Some(123),
            semantics,
            content_ref: None,
            style_events: Vec::new(),
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, |_, _| None);

        let children: Vec<_> = chapter.children(chapter.root()).collect();
        assert_eq!(children.len(), 1);

        let image_node = chapter.node(children[0]).unwrap();
        assert_eq!(image_node.role, Role::Image);
        assert_eq!(chapter.semantics.src(children[0]), Some("cover.jpg"));
    }

    #[test]
    fn test_build_ir_with_text_content() {
        let mut stream = TokenStream::new();
        stream.push(KfxToken::StartElement(ElementStart {
            role: Role::Paragraph,
            id: None,
            semantics: HashMap::new(),
            content_ref: Some(ContentRef {
                name: "content_1".to_string(),
                index: 0,
            }),
            style_events: Vec::new(),
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, |name, idx| {
            if name == "content_1" && idx == 0 {
                Some("Hello, world!".to_string())
            } else {
                None
            }
        });

        assert_eq!(chapter.node_count(), 3); // root + para + text
        let para_id = chapter.children(chapter.root()).next().unwrap();
        let text_id = chapter.children(para_id).next().unwrap();
        let text_node = chapter.node(text_id).unwrap();
        assert_eq!(chapter.text(text_node.text), "Hello, world!");
    }

    #[test]
    fn test_build_ir_with_heading() {
        let mut stream = TokenStream::new();
        stream.push(KfxToken::StartElement(ElementStart {
            role: Role::Heading(2),
            id: None,
            semantics: HashMap::new(),
            content_ref: Some(ContentRef {
                name: "content_1".to_string(),
                index: 0,
            }),
            style_events: Vec::new(),
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, |_, _| Some("Chapter 1".to_string()));

        let heading_id = chapter.children(chapter.root()).next().unwrap();
        let heading = chapter.node(heading_id).unwrap();
        assert_eq!(heading.role, Role::Heading(2));
    }

    #[test]
    fn test_build_ir_with_link_span() {
        let mut stream = TokenStream::new();
        let mut span_semantics = HashMap::new();
        span_semantics.insert(SemanticTarget::Href, "chapter2".to_string());

        stream.push(KfxToken::StartElement(ElementStart {
            role: Role::Paragraph,
            id: None,
            semantics: HashMap::new(),
            content_ref: Some(ContentRef {
                name: "content_1".to_string(),
                index: 0,
            }),
            style_events: vec![SpanStart {
                role: Role::Link,
                semantics: span_semantics,
                offset: 7,
                length: 5,
            }],
        }));
        stream.end_element();

        // Text is "Hello, world!" - span at offset 7, length 5 = "world"
        let chapter = build_ir_from_tokens(&stream, |_, _| Some("Hello, world!".to_string()));

        // Should have: root -> para -> [text("Hello, "), link("world"), text("!")]
        let para_id = chapter.children(chapter.root()).next().unwrap();
        let children: Vec<_> = chapter.children(para_id).collect();
        assert_eq!(children.len(), 3);

        // First: plain text "Hello, "
        let first = chapter.node(children[0]).unwrap();
        assert_eq!(first.role, Role::Text);
        assert_eq!(chapter.text(first.text), "Hello, ");

        // Second: link containing "world"
        let link = chapter.node(children[1]).unwrap();
        assert_eq!(link.role, Role::Link);
        assert_eq!(chapter.semantics.href(children[1]), Some("chapter2"));

        // Third: plain text "!"
        let last = chapter.node(children[2]).unwrap();
        assert_eq!(last.role, Role::Text);
        assert_eq!(chapter.text(last.text), "!");
    }

    #[test]
    fn test_snap_to_char_boundary() {
        let text = "Hello ὑπόληψις world";

        // Valid boundary
        assert_eq!(snap_to_char_boundary(text, 0), 0);
        assert_eq!(snap_to_char_boundary(text, 5), 5);

        // Past end
        assert_eq!(snap_to_char_boundary(text, 100), text.len());

        // Inside multi-byte char - should snap back
        let greek_start = text.find('ὑ').unwrap();
        assert_eq!(snap_to_char_boundary(text, greek_start + 1), greek_start);
    }

    #[test]
    fn test_apply_semantics_generic() {
        let mut chapter = IRChapter::new();
        let node = Node::new(Role::Image);
        let node_id = chapter.alloc_node(node);

        let mut semantics = HashMap::new();
        semantics.insert(SemanticTarget::Src, "image.jpg".to_string());
        semantics.insert(SemanticTarget::Alt, "An image".to_string());

        apply_semantics_to_node(&mut chapter, node_id, &semantics);

        assert_eq!(chapter.semantics.src(node_id), Some("image.jpg"));
        assert_eq!(chapter.semantics.alt(node_id), Some("An image"));
    }
}
