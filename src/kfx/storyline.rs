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
use crate::kfx::schema::{SemanticTarget, schema};
use crate::kfx::symbols::KfxSymbol;
use crate::kfx::tokens::{ContentRef, ElementStart, KfxToken, SpanStart, TokenStream};
use crate::kfx::transforms::ImportContext;
use std::collections::HashMap;

/// Context for tokenization including anchor resolution.
struct TokenizeContext<'a> {
    doc_symbols: &'a [String],
    anchors: Option<&'a HashMap<String, String>>,
}

/// Shorthand for getting a KfxSymbol as u64.
macro_rules! sym {
    ($variant:ident) => {
        KfxSymbol::$variant as u64
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
/// The `styles` map is passed through for the IR building phase.
pub fn tokenize_storyline(
    storyline: &IonValue,
    doc_symbols: &[String],
    anchors: Option<&HashMap<String, String>>,
    _styles: Option<&HashMap<String, Vec<(u64, IonValue)>>>,
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

    let ctx = TokenizeContext {
        doc_symbols,
        anchors,
    };
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

    // Get element type symbol ID (u64 from IonValue, cast to u32 for schema)
    let kfx_type_id = get_field(fields, sym!(Type))
        .and_then(|v| v.as_symbol())
        .unwrap_or(sym!(Container)) as u32;

    // Use schema to resolve role with attribute lookup closure
    // Return int values directly, or symbol IDs cast to i64 for symbol-based attributes
    let mut role = schema().resolve_element_role(kfx_type_id, |symbol| {
        get_field(fields, symbol as u64)
            .and_then(|v| v.as_int().or_else(|| v.as_symbol().map(|s| s as i64)))
    });

    // Check for semantic type annotation (yj.semantics.type) which uses local symbols.
    // The schema's StructureWithSemanticType strategies define what values map to what roles.
    if let Some(semantic_type) = get_semantic_type_annotation(fields, ctx.doc_symbols)
        && let Some(mapped_role) = schema().role_for_semantic_type(&semantic_type)
    {
        role = mapped_role;
    }

    // Check for layout_hints to detect Figure and Caption elements (schema-driven).
    if let Some(layout_hints) = get_field(fields, sym!(LayoutHints))
        && let Some(hints_list) = layout_hints.as_list()
    {
        for hint in hints_list {
            if let Some(hint_id) = hint.as_symbol()
                && let Some(mapped_role) = schema().role_for_layout_hint(hint_id as u32)
            {
                role = mapped_role;
                break;
            }
        }
    }

    // Check for span indicators on elements (e.g., link_to → Link)
    // This enables standalone Link elements to be recognized
    if let Some(override_role) =
        schema().check_span_role_override(|sym| get_field(fields, sym as u64).is_some())
    {
        role = override_role;
    }

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

    // Get style reference (symbol ID or name) for later lookup
    let style_name =
        get_field(fields, sym!(Style)).and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols));

    // Emit StartElement token
    stream.push(KfxToken::StartElement(ElementStart {
        role,
        id,
        semantics,
        content_ref,
        style_events,
        kfx_attrs: Vec::new(),
        style_symbol: None,             // Symbol ID (for export)
        style_name,                     // Style name (for import lookup)
        needs_container_wrapper: false, // Only used during export
    }));

    // Recurse into children
    if has_children && let Some(children) = get_field(fields, sym!(ContentList)) {
        tokenize_content_list(children, ctx, stream);
    }

    // Emit EndElement token
    stream.push(KfxToken::EndElement);
}

/// Extract the semantic type annotation (yj.semantics.type) if present.
///
/// This looks for a field named "yj.semantics.type" (local symbol) and returns
/// its value as a string. Used for bidirectional BlockQuote mapping.
fn get_semantic_type_annotation(
    fields: &[(u64, IonValue)],
    doc_symbols: &[String],
) -> Option<String> {
    // Find the field ID for "yj.semantics.type" in local symbols.
    // Local symbol IDs are offset by the base symbol table size.
    let doc_idx = doc_symbols.iter().position(|s| s == "yj.semantics.type")?;
    let field_id = crate::kfx::symbols::KFX_SYMBOL_TABLE_SIZE + doc_idx;

    // Get the value and resolve it to a string
    get_field(fields, field_id as u64).and_then(|v| resolve_symbol_or_string(v, doc_symbols))
}

/// Extract ALL semantic attributes for an element using schema rules.
///
/// This is **fully generic** - it iterates all AttrRules from the schema
/// and applies their transformers. Also checks span rules for attributes
/// like link_to that may appear on standalone elements.
fn extract_all_element_attrs(
    fields: &[(u64, IonValue)],
    kfx_type_id: u32,
    ctx: &TokenizeContext,
) -> HashMap<SemanticTarget, String> {
    let mut result = HashMap::new();
    let import_ctx = ImportContext {
        doc_symbols: ctx.doc_symbols,
        chapter_id: None,
        anchors: ctx.anchors,
    };

    // Extract using element attr rules
    for rule in schema().element_attr_rules(kfx_type_id) {
        if let Some(raw_value) = get_field(fields, rule.kfx_field as u64)
            .and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))
        {
            let parsed = rule.transform.import(&raw_value, &import_ctx);
            let final_value = match parsed {
                crate::kfx::transforms::ParsedAttribute::String(s) => s,
                crate::kfx::transforms::ParsedAttribute::Link(link) => link.to_href(),
                crate::kfx::transforms::ParsedAttribute::Anchor(id) => id,
            };
            result.insert(rule.target, final_value);
        }
    }

    // Also extract using span rules (for attributes like link_to on standalone elements)
    let has_field = |symbol: KfxSymbol| get_field(fields, symbol as u64).is_some();
    for rule in schema().span_attr_rules(has_field) {
        // Skip if we already have this attribute
        if result.contains_key(&rule.target) {
            continue;
        }
        if let Some(raw_value) = get_field(fields, rule.kfx_field as u64)
            .and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))
        {
            let parsed = rule.transform.import(&raw_value, &import_ctx);
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

            // Get style symbol ID for later lookup
            let style_symbol = get_field(fields, sym!(Style)).and_then(|v| v.as_symbol());

            // Create closure to check which fields are present
            let has_field = |symbol: KfxSymbol| get_field(fields, symbol as u64).is_some();

            // Use schema to determine role
            let role = schema().resolve_span_role(has_field);

            // Extract ALL semantic attributes using schema rules (GENERIC!)
            let semantics = extract_all_span_attrs(fields, has_field, ctx);

            Some(SpanStart {
                role,
                semantics,
                offset,
                length,
                style_symbol,
                kfx_attrs: Vec::new(),
            })
        })
        .collect()
}

/// Extract ALL semantic attributes for a span using schema rules.
///
/// This is **fully generic** - no hardcoded SemanticTarget checks.
fn extract_all_span_attrs<F>(
    fields: &[(u64, IonValue)],
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
        if let Some(raw_value) = get_field(fields, rule.kfx_field as u64)
            .and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))
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
/// The `styles` map is used to look up style definitions by name.
/// The `doc_symbols` are used to resolve style symbol IDs to names.
pub fn build_ir_from_tokens<F>(
    tokens: &TokenStream,
    doc_symbols: &[String],
    styles: Option<&HashMap<String, Vec<(u64, IonValue)>>>,
    mut content_lookup: F,
) -> IRChapter
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

                // Apply style from the styles map (if present)
                if let Some(style_name) = &elem.style_name
                    && let Some(styles_map) = styles
                    && let Some(kfx_props) = styles_map.get(style_name)
                {
                    let ir_style = kfx_style_to_ir(kfx_props);
                    let style_id = chapter.styles.intern(ir_style);
                    if let Some(node) = chapter.node_mut(node_id) {
                        node.style = style_id;
                    }
                }

                // Apply ALL semantic attributes from the generic map
                apply_semantics_to_node(&mut chapter, node_id, &elem.semantics);

                // Handle text content with style events
                if let Some(ref content_ref) = elem.content_ref
                    && let Some(text) = content_lookup(&content_ref.name, content_ref.index)
                {
                    if elem.style_events.is_empty() {
                        // Simple case: no inline styles
                        let range = chapter.append_text(&text);
                        let text_node = chapter.alloc_node(Node::text(range));
                        chapter.append_child(node_id, text_node);
                    } else {
                        // Complex case: apply style events as spans
                        build_text_with_spans(
                            &mut chapter,
                            node_id,
                            &text,
                            &elem.style_events,
                            doc_symbols,
                            styles,
                        );
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
            SemanticTarget::EpubType => chapter.semantics.set_epub_type(node_id, value.clone()),
        }
    }
}

/// Convert KFX style properties to an IR ComputedStyle using the schema.
///
/// This is schema-driven: iterates schema rules with KFX symbol mappings,
/// applies inverse transforms to convert KFX values back to IR values.
fn kfx_style_to_ir(props: &[(u64, IonValue)]) -> crate::ir::ComputedStyle {
    use crate::kfx::style_schema::{StyleSchema, import_kfx_style};

    let schema = StyleSchema::standard();
    import_kfx_style(schema, props)
}

/// Build text nodes with inline spans applied.
///
/// The `doc_symbols` and `styles` parameters are used to resolve span styles:
/// - `doc_symbols`: resolves style symbol IDs to style names
/// - `styles`: maps style names to KFX style properties
fn build_text_with_spans(
    chapter: &mut IRChapter,
    parent: NodeId,
    text: &str,
    spans: &[SpanStart],
    doc_symbols: &[String],
    styles: Option<&HashMap<String, Vec<(u64, IonValue)>>>,
) {
    // Sort spans by offset, then by length (shorter first for same offset)
    let mut sorted_spans: Vec<_> = spans.iter().collect();
    sorted_spans.sort_by_key(|s| (s.offset, s.length));

    // Filter out spans that are completely contained within larger spans at the same offset.
    // This handles the case of nested inlines where both outer and inner emit spans.
    // We keep the shorter (more specific) spans and discard the longer encompassing ones.
    // Build a proper nested span tree to handle overlapping/nested spans.
    // KFX style_events can have nested spans (e.g., Link containing Inline).
    // Sort by offset, then by length DESCENDING (enclosing spans first).
    let mut sorted_spans: Vec<_> = spans.iter().collect();
    sorted_spans.sort_by(|a, b| {
        a.offset
            .cmp(&b.offset)
            .then_with(|| b.length.cmp(&a.length)) // Larger spans first at same offset
    });

    // Helper to create a span node with style and semantics applied
    let create_span_node = |chapter: &mut IRChapter, span: &SpanStart| -> NodeId {
        let span_node = chapter.alloc_node(Node::new(span.role));

        // Apply style from the styles map (if present)
        if let Some(style_sym) = span.style_symbol
            && let Some(style_name) = resolve_symbol(style_sym, doc_symbols)
            && let Some(styles_map) = styles
            && let Some(kfx_props) = styles_map.get(style_name)
        {
            let ir_style = kfx_style_to_ir(kfx_props);
            let style_id = chapter.styles.intern(ir_style);
            if let Some(node) = chapter.node_mut(span_node) {
                node.style = style_id;
            }
        }

        // Apply ALL semantic attributes from the generic map
        apply_semantics_to_node(chapter, span_node, &span.semantics);

        span_node
    };

    // Stack of (node_id, char_end_offset) for active spans
    let mut span_stack: Vec<(NodeId, usize)> = vec![(parent, usize::MAX)];
    let mut char_pos: usize = 0; // Current position in char offsets

    for span in sorted_spans {
        let span_start = span.offset;
        let span_end = span.offset + span.length;

        // Pop any spans that have ended before this span starts
        while span_stack.len() > 1 {
            let (_, stack_end) = span_stack.last().unwrap();
            if *stack_end <= span_start {
                // This span has ended - add any remaining text to it first
                if char_pos < *stack_end {
                    let byte_start = char_to_byte_offset(text, char_pos);
                    let byte_end = char_to_byte_offset(text, *stack_end);
                    if byte_end > byte_start {
                        let segment = &text[byte_start..byte_end];
                        let range = chapter.append_text(segment);
                        let text_node = chapter.alloc_node(Node::text(range));
                        let (parent_id, _) = span_stack.last().unwrap();
                        chapter.append_child(*parent_id, text_node);
                    }
                    char_pos = *stack_end;
                }
                span_stack.pop();
            } else {
                break;
            }
        }

        // Add text between char_pos and span_start to current parent
        if char_pos < span_start {
            let byte_start = char_to_byte_offset(text, char_pos);
            let byte_end = char_to_byte_offset(text, span_start);
            if byte_end > byte_start {
                let before = &text[byte_start..byte_end];
                let range = chapter.append_text(before);
                let text_node = chapter.alloc_node(Node::text(range));
                let (current_parent, _) = span_stack.last().unwrap();
                chapter.append_child(*current_parent, text_node);
            }
            char_pos = span_start;
        }

        // Create this span and push onto stack
        if span_end > span_start {
            let span_node = create_span_node(chapter, span);
            let (current_parent, _) = span_stack.last().unwrap();
            chapter.append_child(*current_parent, span_node);
            span_stack.push((span_node, span_end));
        }
    }

    // Close all remaining spans and add trailing text
    while let Some((node_id, end_offset)) = span_stack.pop() {
        let actual_end = end_offset.min(text.chars().count());
        if char_pos < actual_end {
            let byte_start = char_to_byte_offset(text, char_pos);
            let byte_end = char_to_byte_offset(text, actual_end);
            if byte_end > byte_start {
                let segment = &text[byte_start..byte_end];
                let range = chapter.append_text(segment);
                let text_node = chapter.alloc_node(Node::text(range));
                chapter.append_child(node_id, text_node);
            }
            char_pos = actual_end;
        }
    }
}

/// Convert a character (code point) offset to a byte offset.
///
/// KFX style_events use character offsets, not byte offsets. This function
/// converts the char offset to a byte offset for string slicing.
fn char_to_byte_offset(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(text.len())
}

// ============================================================================
// Helper functions
// ============================================================================

/// Resolve a symbol ID to its string representation.
fn resolve_symbol(id: u64, doc_symbols: &[String]) -> Option<&str> {
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
/// The `styles` map is used to resolve style references (style_name → properties).
pub fn parse_storyline_to_ir<F>(
    storyline: &IonValue,
    doc_symbols: &[String],
    anchors: Option<&HashMap<String, String>>,
    styles: Option<&HashMap<String, Vec<(u64, IonValue)>>>,
    content_lookup: F,
) -> IRChapter
where
    F: FnMut(&str, usize) -> Option<String>,
{
    let tokens = tokenize_storyline(storyline, doc_symbols, anchors, styles);
    build_ir_from_tokens(&tokens, doc_symbols, styles, content_lookup)
}

// ============================================================================
// EXPORT: IR → TokenStream → Ion
// ============================================================================

use crate::ir::{BorderStyle, ComputedStyle, Length, Role};
use crate::kfx::context::ExportContext;

/// Check if a style has borders that require container wrapping in KFX.
///
/// KFX requires block elements with borders to be wrapped in a `type: container`
/// with nested `type: text` for the content. Without this wrapper, borders don't
/// render on Kindle devices.
fn needs_container_wrapper(style: &ComputedStyle) -> bool {
    let has_top = style.border_style_top != BorderStyle::None
        && !matches!(style.border_width_top, Length::Auto | Length::Px(0.0));
    let has_bottom = style.border_style_bottom != BorderStyle::None
        && !matches!(style.border_width_bottom, Length::Auto | Length::Px(0.0));
    let has_left = style.border_style_left != BorderStyle::None
        && !matches!(style.border_width_left, Length::Auto | Length::Px(0.0));
    let has_right = style.border_style_right != BorderStyle::None
        && !matches!(style.border_width_right, Length::Auto | Length::Px(0.0));
    has_top || has_bottom || has_left || has_right
}

/// Convert an IR chapter to a TokenStream.
///
/// This is the first stage of export: walking the IR tree and emitting tokens.
pub fn ir_to_tokens(chapter: &IRChapter, ctx: &mut ExportContext) -> TokenStream {
    let sch = schema();
    let mut stream = TokenStream::new();

    walk_node_for_export(chapter, chapter.root(), sch, ctx, &mut stream);
    stream
}

/// Walk a node and emit tokens for export.
///
/// Uses schema-driven attribute export (FIX for Issue #2: Attribute Hardcoding).
/// Inline roles (Link, Inline) are emitted as StartSpan/EndSpan instead of
/// StartElement/EndElement, enabling proper style_events generation.
fn walk_node_for_export(
    chapter: &IRChapter,
    node_id: NodeId,
    sch: &crate::kfx::schema::KfxSchema,
    ctx: &mut ExportContext,
    stream: &mut TokenStream,
) {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return,
    };

    // Root node: just walk children
    if node.role == Role::Root {
        for child in chapter.children(node_id) {
            walk_node_for_export(chapter, child, sch, ctx, stream);
        }
        return;
    }

    // Text nodes: emit just the text, not a container
    // Text nodes are leaf nodes that contain the actual string data
    if node.role == Role::Text {
        if !node.text.is_empty() {
            let text = chapter.text(node.text);
            if !text.is_empty() {
                stream.push(KfxToken::Text(text.to_string()));
            }
        }
        return;
    }

    // Break nodes: emit a newline character
    // KFX represents <br> as \n within text content, not as separate elements
    if node.role == Role::Break {
        stream.push(KfxToken::Text("\n".to_string()));
        return;
    }

    // Definition lists: group dt+dd pairs into wrapper elements
    // HTML has dt/dd as flat siblings, but KFX needs them grouped for float to work
    if node.role == Role::DefinitionList {
        emit_definition_list(chapter, node_id, sch, ctx, stream);
        return;
    }

    // Inline elements (Link, Inline): use the flattening algorithm.
    // This produces non-overlapping style_events where each text segment
    // carries the accumulated state from all ancestors.
    if node.role == Role::Link || node.role == Role::Inline {
        emit_inline_content_flat(chapter, node_id, sch, ctx, stream);
        return;
    }

    // Get KFX type from schema (will be used in tokens_to_ion)
    let _kfx_type = sch.kfx_type_for_role(node.role);

    // Build element start with semantics
    let mut elem = ElementStart::new(node.role);

    // Register the node's style and get a KFX style symbol
    // This converts IR ComputedStyle → KFX style and deduplicates
    let style_symbol = ctx.register_style_id(node.style, &chapter.styles);
    elem.style_symbol = Some(style_symbol);

    // Check if this element needs container wrapping for borders to render
    // KFX requires type: container with nested type: text for borders
    elem.needs_container_wrapper = chapter
        .styles
        .get(node.style)
        .map(needs_container_wrapper)
        .unwrap_or(false);

    // SCHEMA-DRIVEN attribute export
    // Create a closure to get semantic values by target
    let export_ctx = crate::kfx::transforms::ExportContext {
        spine_map: None,
        resource_registry: Some(&ctx.resource_registry),
    };
    let mut kfx_attrs = sch.export_attributes(
        node.role,
        |target| match target {
            SemanticTarget::Href => chapter.semantics.href(node_id).map(|s| s.to_string()),
            SemanticTarget::Src => chapter.semantics.src(node_id).map(|s| s.to_string()),
            SemanticTarget::Alt => chapter.semantics.alt(node_id).map(|s| s.to_string()),
            SemanticTarget::Id => chapter.semantics.id(node_id).map(|s| s.to_string()),
            SemanticTarget::EpubType => chapter.semantics.epub_type(node_id).map(|s| s.to_string()),
        },
        &export_ctx,
    );

    // Convert link_to values to anchor symbols via the AnchorRegistry
    for (field_id, value) in &mut kfx_attrs {
        if *field_id == sym!(LinkTo) {
            let anchor_symbol = ctx.anchor_registry.register_link_target(value);
            *value = anchor_symbol;
        }
    }

    // Store the transformed KFX attributes for tokens_to_ion
    elem.kfx_attrs = kfx_attrs;

    // Also populate the semantic map for backward compatibility with IR operations
    // (This is redundant but ensures the element has all info needed)
    if let Some(href) = chapter.semantics.href(node_id) {
        elem.set_semantic(SemanticTarget::Href, href.to_string());
    }
    if let Some(src) = chapter.semantics.src(node_id) {
        elem.set_semantic(SemanticTarget::Src, src.to_string());
        // Intern any referenced resources (already done in Pass 1, but safe to repeat)
        ctx.resource_registry.register(src, &mut ctx.symbols);
    }
    if let Some(alt) = chapter.semantics.alt(node_id) {
        elem.set_semantic(SemanticTarget::Alt, alt.to_string());
    }
    if let Some(id) = chapter.semantics.id(node_id) {
        elem.set_semantic(SemanticTarget::Id, id.to_string());
    }
    if let Some(epub_type) = chapter.semantics.epub_type(node_id) {
        elem.set_semantic(SemanticTarget::EpubType, epub_type.to_string());
    }

    stream.push(KfxToken::StartElement(elem));

    // Emit text content if present
    if !node.text.is_empty() {
        let text = chapter.text(node.text);
        if !text.is_empty() {
            stream.push(KfxToken::Text(text.to_string()));
        }
    }

    // Walk children
    for child in chapter.children(node_id) {
        walk_node_for_export(chapter, child, sch, ctx, stream);
    }

    stream.push(KfxToken::EndElement);
}

// ============================================================================
// Inline Content Flattening ("Push Down, Emit at Bottom")
// ============================================================================
//
// This implements the correct algorithm for converting nested inline elements
// (Link, Inline, Text) into flat, non-overlapping KFX style_events.
//
// Problem: HTML has nested inline elements like <a><span>1.</span>Easy...</a>
// KFX needs flat style_events where each event covers a disjoint text range
// and carries ALL applicable attributes from ancestors.
//
// Solution: Depth-first traversal with a context stack. We only emit events
// at TEXT LEAVES, carrying the accumulated state from all ancestors.

/// Active state during inline flattening - accumulated from ancestors.
#[derive(Clone, Default)]
struct InlineState {
    /// Active link target (from Link ancestor), as anchor symbol string
    link_to: Option<String>,
    /// Active style (innermost wins)
    style: Option<crate::ir::StyleId>,
    /// Active epub:type for noteref detection
    epub_type: Option<String>,
    /// Active element ID (for anchor creation)
    element_id: Option<String>,
}

/// A flattened text segment with its computed state.
struct FlatSegment {
    text: String,
    state: InlineState,
}

/// Flatten inline content into segments with computed state.
///
/// This is the "Push Down, Emit at Bottom" algorithm:
/// - Traverse the tree depth-first
/// - Accumulate state (link_to, style) as we go down
/// - Only emit segments when we hit Text leaves
fn flatten_inline_content(
    chapter: &IRChapter,
    node_id: NodeId,
    state: InlineState,
    segments: &mut Vec<FlatSegment>,
) {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return,
    };

    // MERGE STATE: Calculate effective state for this node
    let effective_state = InlineState {
        // Links: propagate down (newest wins if nested)
        link_to: chapter
            .semantics
            .href(node_id)
            .map(|s| s.to_string())
            .or(state.link_to),
        // Styles: innermost wins (child overrides parent)
        style: if node.role == Role::Inline || node.role == Role::Link {
            Some(node.style)
        } else {
            state.style
        },
        // epub:type: propagate for noteref detection
        epub_type: chapter
            .semantics
            .epub_type(node_id)
            .map(|s| s.to_string())
            .or(state.epub_type),
        // Element ID: for anchor creation
        element_id: chapter
            .semantics
            .id(node_id)
            .map(|s| s.to_string())
            .or(state.element_id),
    };

    match node.role {
        // TEXT LEAVES: Emit segment with accumulated state
        Role::Text => {
            if !node.text.is_empty() {
                let text = chapter.text(node.text);
                if !text.is_empty() {
                    segments.push(FlatSegment {
                        text: text.to_string(),
                        state: effective_state,
                    });
                }
            }
        }
        // BREAK: Emit newline as text
        Role::Break => {
            segments.push(FlatSegment {
                text: "\n".to_string(),
                state: effective_state,
            });
        }
        // CONTAINERS (Link, Inline, etc.): Recurse with accumulated state
        _ => {
            for child_id in chapter.children(node_id) {
                flatten_inline_content(chapter, child_id, effective_state.clone(), segments);
            }
        }
    }
}

/// Convert flattened segments into KfxTokens (Text + style info for style_events).
///
/// This emits the text and creates SpanStart markers that will become style_events.
fn emit_flattened_segments(
    segments: Vec<FlatSegment>,
    chapter: &IRChapter,
    _sch: &crate::kfx::schema::KfxSchema,
    ctx: &mut ExportContext,
    stream: &mut TokenStream,
) {
    for segment in segments {
        let needs_style_event = segment.state.link_to.is_some() || segment.state.style.is_some();

        if needs_style_event {
            // Build span with accumulated state
            let mut span = SpanStart::new(
                if segment.state.link_to.is_some() {
                    Role::Link
                } else {
                    Role::Inline
                },
                0,
                0,
            );

            // Set style (innermost)
            if let Some(style_id) = segment.state.style {
                let style_symbol = ctx.register_style_id(style_id, &chapter.styles);
                span.style_symbol = Some(style_symbol);
            }

            // Build KFX attributes
            let mut kfx_attrs = Vec::new();

            // Add link_to if present
            if let Some(ref href) = segment.state.link_to {
                let href_value = href.clone();
                let anchor_symbol = ctx.anchor_registry.register_link_target(&href_value);
                kfx_attrs.push((sym!(LinkTo), anchor_symbol));
            }

            // Add yj.display for noterefs
            if let Some(ref epub_type) = segment.state.epub_type
                && epub_type.split_whitespace().any(|t| t == "noteref")
            {
                // YjNote = 617
                kfx_attrs.push((sym!(YjDisplay), "617".to_string()));
            }

            span.kfx_attrs = kfx_attrs;

            // Store element ID for anchor creation
            if let Some(ref id) = segment.state.element_id {
                span.set_semantic(SemanticTarget::Id, id.clone());
            }

            stream.push(KfxToken::StartSpan(span));
            stream.push(KfxToken::Text(segment.text));
            stream.push(KfxToken::EndSpan);
        } else {
            // Plain text, no style event needed
            stream.push(KfxToken::Text(segment.text));
        }
    }
}

/// Emit a definition list with dt+dd pairs grouped together.
///
/// HTML `<dl>` has `<dt>` and `<dd>` as flat siblings, but KFX needs each
/// dt+dd pair wrapped in a container for float:left to work properly.
/// This matches KPR's output structure:
///   Paragraph (wrapper)
///     Container (dt with float:left)
///       Paragraph (dt content)
///         Link
///     Paragraph (dd content)
///       Link
fn emit_definition_list(
    chapter: &IRChapter,
    node_id: NodeId,
    sch: &crate::kfx::schema::KfxSchema,
    ctx: &mut ExportContext,
    stream: &mut TokenStream,
) {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return,
    };

    // Emit the outer dl container (becomes Paragraph like KPR)
    let mut dl_elem = ElementStart::new(Role::Paragraph);
    let dl_style = ctx.register_style_id(node.style, &chapter.styles);
    dl_elem.style_symbol = Some(dl_style);

    stream.push(KfxToken::StartElement(dl_elem));

    // Collect children and group dt+dd pairs
    let children: Vec<NodeId> = chapter.children(node_id).collect();
    let mut i = 0;

    while i < children.len() {
        let child_id = children[i];
        let child = match chapter.node(child_id) {
            Some(n) => n,
            None => {
                i += 1;
                continue;
            }
        };

        if child.role == Role::DefinitionTerm {
            // Find the paired dd (if any) to get its style for the wrapper
            let dd_info = if i + 1 < children.len() {
                let next_id = children[i + 1];
                chapter.node(next_id).and_then(|next| {
                    if next.role == Role::DefinitionDescription {
                        Some((next_id, next.style))
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // Start a wrapper Paragraph for this dt+dd pair
            // Use a neutral style (from dd or default)
            let mut wrapper_elem = ElementStart::new(Role::Paragraph);
            let wrapper_style = if let Some((_, dd_style_id)) = dd_info {
                ctx.register_style_id(dd_style_id, &chapter.styles)
            } else {
                ctx.default_style_symbol
            };
            wrapper_elem.style_symbol = Some(wrapper_style);
            stream.push(KfxToken::StartElement(wrapper_elem));

            // Emit the dt as a Container (with float:left style)
            // Use DefinitionTerm role since it maps to KfxSymbol::Container
            let dt_style = ctx.register_style_id(child.style, &chapter.styles);
            let mut dt_elem = ElementStart::new(Role::DefinitionTerm);
            dt_elem.style_symbol = Some(dt_style);
            stream.push(KfxToken::StartElement(dt_elem));

            // Emit dt's children wrapped in a Paragraph (like KPR)
            let mut dt_inner = ElementStart::new(Role::Paragraph);
            dt_inner.style_symbol = Some(dt_style);
            stream.push(KfxToken::StartElement(dt_inner));

            for dt_child in chapter.children(child_id) {
                walk_node_for_export(chapter, dt_child, sch, ctx, stream);
            }

            stream.push(KfxToken::EndElement); // end dt inner Paragraph
            stream.push(KfxToken::EndElement); // end dt Container

            // Emit the paired dd content
            if let Some((dd_id, _)) = dd_info {
                // Emit dd's children directly (they're already Paragraphs)
                for dd_child in chapter.children(dd_id) {
                    walk_node_for_export(chapter, dd_child, sch, ctx, stream);
                }

                i += 1; // Skip the dd, we've processed it
            }

            // End the wrapper
            stream.push(KfxToken::EndElement);
        } else {
            // Non-dt child (orphan dd or other), emit normally
            walk_node_for_export(chapter, child_id, sch, ctx, stream);
        }

        i += 1;
    }

    stream.push(KfxToken::EndElement);
}

/// Emit inline content (Link, Inline, Text) using the flattening algorithm.
///
/// This replaces the old StartSpan/EndSpan nesting approach with proper
/// "Push Down, Emit at Bottom" that produces non-overlapping style_events.
fn emit_inline_content_flat(
    chapter: &IRChapter,
    node_id: NodeId,
    sch: &crate::kfx::schema::KfxSchema,
    ctx: &mut ExportContext,
    stream: &mut TokenStream,
) {
    // Flatten the inline subtree into segments with computed state
    let mut segments = Vec::new();
    flatten_inline_content(chapter, node_id, InlineState::default(), &mut segments);

    // Convert segments to tokens
    emit_flattened_segments(segments, chapter, sch, ctx, stream);
}

/// Convert a TokenStream to KFX Ion structure (Storyline content_list).
///
/// This is the second stage of export: building the Ion tree from tokens.
///
/// **Critical**: This function SPLITS data between Structure and Text:
/// - **Structure** (containers) → returned as Ion (for Storyline Entity)
/// - **Text strings** → pushed to `ctx.text_accumulator` (for Content Entity)
///
/// Text containers get a `content: {name, index}` reference instead of inline text.
/// **All text within an element is concatenated into ONE content entry.**
///
/// **Inline Spans**: StartSpan/EndSpan tokens are converted to style_events arrays.
/// The span stack tracks (start_offset, span_info) and calculates length on EndSpan.
pub fn tokens_to_ion(tokens: &TokenStream, ctx: &mut ExportContext) -> IonValue {
    let mut stack: Vec<IonBuilder> = vec![IonBuilder::new()];

    // Span stack: (start_byte_offset, SpanStart info)
    // Used to calculate offset/length for style_events
    let mut span_stack: Vec<(usize, SpanStart)> = Vec::new();

    for token in tokens {
        match token {
            KfxToken::StartElement(elem) => {
                // Check if this element needs container wrapping for borders to render
                // KFX requires type: container with nested type: text for borders
                if elem.needs_container_wrapper {
                    // === CONTAINER WRAPPER PATH ===
                    // Create outer container with type: container, layout: vertical
                    // and all the semantic/style fields, then create inner text element

                    let mut outer_fields = Vec::new();

                    // Unique container ID for outer wrapper
                    let outer_id = ctx.fragment_ids.next_id();
                    outer_fields.push((sym!(Id), IonValue::Int(outer_id as i64)));

                    // Record this content ID for position_map
                    ctx.record_content_id(outer_id);

                    // Create chapter-start anchor with first content fragment ID (if pending)
                    ctx.resolve_pending_chapter_start_anchor(outer_id);

                    // Create fragment-based anchor if this element has an ID
                    if let Some(anchor_id) = elem.get_semantic(SemanticTarget::Id) {
                        ctx.create_anchor_if_needed(anchor_id, outer_id, 0);
                    }

                    // Style reference - outer container gets full style with borders
                    let style_sym = elem.style_symbol.unwrap_or(ctx.default_style_symbol);
                    outer_fields.push((sym!(Style), IonValue::Symbol(style_sym)));

                    // Type: container (not text) - this is key for borders to render
                    outer_fields.push((sym!(Type), IonValue::Symbol(KfxSymbol::Container as u64)));

                    // Layout: vertical (required for container)
                    outer_fields.push((sym!(Layout), IonValue::Symbol(KfxSymbol::Vertical as u64)));

                    // Add semantic type annotation if the strategy specifies one
                    if let Some(strategy) = schema().export_strategy(elem.role)
                        && let Some(semantic_type) = strategy.semantic_type()
                    {
                        let field_id = ctx.symbols.get_or_intern("yj.semantics.type");
                        let value_id = ctx.symbols.get_or_intern(semantic_type);
                        outer_fields.push((field_id, IonValue::Symbol(value_id)));
                    }

                    // Add heading level if this is a heading
                    if let Role::Heading(level) = elem.role {
                        outer_fields
                            .push((sym!(YjSemanticsHeadingLevel), IonValue::Int(level as i64)));
                        ctx.record_heading_with_id(level, outer_id);
                    }

                    // Add list_style for ordered lists
                    if elem.role == Role::OrderedList {
                        outer_fields.push((sym!(ListStyle), IonValue::Symbol(sym!(Numeric))));
                    }

                    // Add layout_hints
                    let layout_hint = match elem.role {
                        Role::Heading(_) => Some(KfxSymbol::TreatAsTitle),
                        Role::Figure => Some(KfxSymbol::Figure),
                        Role::Caption => Some(KfxSymbol::Caption),
                        _ => {
                            if let Some(epub_type) = elem.get_semantic(SemanticTarget::EpubType) {
                                let has_title_type = epub_type.split_whitespace().any(|t| {
                                    matches!(
                                        t,
                                        "title"
                                            | "fulltitle"
                                            | "subtitle"
                                            | "covertitle"
                                            | "halftitle"
                                    )
                                });
                                let has_caption_type = epub_type
                                    .split_whitespace()
                                    .any(|t| matches!(t, "caption" | "figcaption"));

                                if has_title_type {
                                    Some(KfxSymbol::TreatAsTitle)
                                } else if has_caption_type {
                                    Some(KfxSymbol::Caption)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                    };

                    if let Some(hint) = layout_hint {
                        outer_fields.push((
                            sym!(LayoutHints),
                            IonValue::List(vec![IonValue::Symbol(hint as u64)]),
                        ));
                    }

                    // Add yj.classification for footnote/endnote popup support
                    if let Some(epub_type) = elem.get_semantic(SemanticTarget::EpubType) {
                        let types: Vec<&str> = epub_type.split_whitespace().collect();
                        let is_footnote = types.contains(&"footnote");
                        let is_endnote = types.contains(&"endnote") || types.contains(&"rearnote");
                        let is_sidenote =
                            types.contains(&"sidebar") || types.contains(&"marginalia");

                        if is_endnote {
                            outer_fields.push((
                                sym!(YjClassification),
                                IonValue::Symbol(KfxSymbol::YjEndnote as u64),
                            ));
                        } else if is_sidenote {
                            outer_fields.push((
                                sym!(YjClassification),
                                IonValue::Symbol(KfxSymbol::YjSidenote as u64),
                            ));
                        } else if is_footnote {
                            outer_fields.push((
                                sym!(YjClassification),
                                IonValue::Symbol(KfxSymbol::Footnote as u64),
                            ));
                        }
                    }

                    // Add schema-driven attributes from kfx_attrs
                    for (field_id, value_str) in &elem.kfx_attrs {
                        let is_symbol_field = *field_id == sym!(ResourceName)
                            || *field_id == sym!(LinkTo)
                            || value_str.starts_with('#')
                            || value_str.contains('/');

                        if is_symbol_field {
                            let sym_id = ctx.symbols.get_or_intern(value_str);
                            outer_fields.push((*field_id, IonValue::Symbol(sym_id)));
                        } else {
                            outer_fields.push((*field_id, IonValue::String(value_str.clone())));
                        }
                    }

                    // Push outer container builder
                    stack.push(IonBuilder::with_fields(outer_fields, outer_id));

                    // Create inner text element
                    let mut inner_fields = Vec::new();

                    // Unique ID for inner text element
                    let inner_id = ctx.fragment_ids.next_id();
                    inner_fields.push((sym!(Id), IonValue::Int(inner_id as i64)));

                    // Record inner content ID too
                    ctx.record_content_id(inner_id);

                    // Inner element uses default style (minimal, no borders)
                    // This matches KPR behavior where inner text has separate style
                    inner_fields.push((sym!(Style), IonValue::Symbol(ctx.default_style_symbol)));

                    // Type: text - inner element holds the actual content
                    inner_fields.push((sym!(Type), IonValue::Symbol(KfxSymbol::Text as u64)));

                    // Push inner text builder and mark it as inner wrapper
                    let mut inner_builder = IonBuilder::with_fields(inner_fields, inner_id);
                    inner_builder.is_inner_wrapper_text = true;
                    stack.push(inner_builder);
                } else {
                    // === NORMAL ELEMENT PATH (unchanged) ===
                    let mut fields = Vec::new();

                    // Unique container ID - use the global generator to avoid collisions
                    let container_id = ctx.fragment_ids.next_id();
                    fields.push((sym!(Id), IonValue::Int(container_id as i64)));

                    // Record this content ID for position_map (so navigation targets are resolvable)
                    ctx.record_content_id(container_id);

                    // Create chapter-start anchor with first content fragment ID (if pending)
                    ctx.resolve_pending_chapter_start_anchor(container_id);

                    // Create fragment-based anchor if this element has an ID that's a TOC/link target
                    // Note: Kindle expects offset: 0 for all navigation entries (per reference KFX)
                    if let Some(anchor_id) = elem.get_semantic(SemanticTarget::Id) {
                        ctx.create_anchor_if_needed(anchor_id, container_id, 0);
                    }

                    // Style reference - use per-element style if available, else default
                    // Required for text rendering on Kindle
                    let style_sym = elem.style_symbol.unwrap_or(ctx.default_style_symbol);
                    fields.push((sym!(Style), IonValue::Symbol(style_sym)));

                    // Type field (as symbol ID)
                    if let Some(kfx_type) = schema().kfx_type_for_role(elem.role) {
                        fields.push((sym!(Type), IonValue::Symbol(kfx_type as u64)));
                    }

                    // Add semantic type annotation if the strategy specifies one
                    // (e.g., BlockQuote → yj.semantics.type: block_quote)
                    if let Some(strategy) = schema().export_strategy(elem.role)
                        && let Some(semantic_type) = strategy.semantic_type()
                    {
                        // Both field name and value are local symbols
                        let field_id = ctx.symbols.get_or_intern("yj.semantics.type");
                        let value_id = ctx.symbols.get_or_intern(semantic_type);
                        fields.push((field_id, IonValue::Symbol(value_id)));
                    }

                    // Add heading level if this is a heading
                    if let Role::Heading(level) = elem.role {
                        fields.push((sym!(YjSemanticsHeadingLevel), IonValue::Int(level as i64)));

                        // Record heading position with ACTUAL content fragment ID (Fix for navigation)
                        ctx.record_heading_with_id(level, container_id);
                    }

                    // Add list_style for ordered lists
                    if elem.role == Role::OrderedList {
                        fields.push((sym!(ListStyle), IonValue::Symbol(sym!(Numeric))));
                    }

                    // Add layout_hints based on element role and semantics
                    // This affects Kindle's rendering behavior for headings, figures, and captions
                    let layout_hint = match elem.role {
                        // Headings (h1-h6) → treat_as_title
                        Role::Heading(_) => Some(KfxSymbol::TreatAsTitle),
                        // <figure> → figure
                        Role::Figure => Some(KfxSymbol::Figure),
                        // <figcaption>/<caption> → caption
                        Role::Caption => Some(KfxSymbol::Caption),
                        _ => {
                            // Check epub:type for additional semantic hints
                            if let Some(epub_type) = elem.get_semantic(SemanticTarget::EpubType) {
                                // Check each epub:type value (space-separated)
                                let has_title_type = epub_type.split_whitespace().any(|t| {
                                    matches!(
                                        t,
                                        "title"
                                            | "fulltitle"
                                            | "subtitle"
                                            | "covertitle"
                                            | "halftitle"
                                    )
                                });
                                let has_caption_type = epub_type
                                    .split_whitespace()
                                    .any(|t| matches!(t, "caption" | "figcaption"));

                                if has_title_type {
                                    Some(KfxSymbol::TreatAsTitle)
                                } else if has_caption_type {
                                    Some(KfxSymbol::Caption)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                    };

                    if let Some(hint) = layout_hint {
                        fields.push((
                            sym!(LayoutHints),
                            IonValue::List(vec![IonValue::Symbol(hint as u64)]),
                        ));
                    }

                    // Add yj.classification for footnote/endnote popup support
                    // This marks the element so Kindle can show its content in a popup
                    // when a noteref link is tapped
                    //
                    // Mapping:
                    // - epub:type="footnote" → yj.chapternote ($618)
                    // - epub:type="endnote" or "rearnote" → yj.endnote ($619)
                    // - epub:type="sidebar" or "marginalia" → yj.sidenote ($620)
                    if let Some(epub_type) = elem.get_semantic(SemanticTarget::EpubType) {
                        let types: Vec<&str> = epub_type.split_whitespace().collect();
                        let is_footnote = types.contains(&"footnote");
                        let is_endnote = types.contains(&"endnote") || types.contains(&"rearnote");
                        let is_sidenote =
                            types.contains(&"sidebar") || types.contains(&"marginalia");

                        // Prefer endnote classification if both are present (common in EPUBs)
                        if is_endnote {
                            fields.push((
                                sym!(YjClassification),
                                IonValue::Symbol(KfxSymbol::YjEndnote as u64),
                            ));
                        } else if is_sidenote {
                            fields.push((
                                sym!(YjClassification),
                                IonValue::Symbol(KfxSymbol::YjSidenote as u64),
                            ));
                        } else if is_footnote {
                            fields.push((
                                sym!(YjClassification),
                                IonValue::Symbol(KfxSymbol::Footnote as u64),
                            ));
                        }
                    }

                    // Add schema-driven attributes from kfx_attrs
                    // The schema handles Image src→resource_name, Link href→link_to, etc.
                    for (field_id, value_str) in &elem.kfx_attrs {
                        // Determine if this field should be a symbol or string
                        // - ResourceName: always symbol (e.g., e0, e1)
                        // - LinkTo: always symbol (anchor references)
                        // - References with # or /: symbol
                        // - Alt text, other strings: string
                        let is_symbol_field = *field_id == sym!(ResourceName)
                            || *field_id == sym!(LinkTo)
                            || value_str.starts_with('#')
                            || value_str.contains('/');

                        if is_symbol_field {
                            let sym_id = ctx.symbols.get_or_intern(value_str);
                            fields.push((*field_id, IonValue::Symbol(sym_id)));
                        } else {
                            fields.push((*field_id, IonValue::String(value_str.clone())));
                        }
                    }

                    stack.push(IonBuilder::with_fields(fields, container_id));
                }
            }
            KfxToken::EndElement => {
                if let Some(completed) = stack.pop() {
                    let is_inner = completed.is_inner_wrapper_text;
                    if let Some(parent) = stack.last_mut() {
                        parent.add_child(completed.build(ctx));
                    }

                    // If this was an inner wrapper text element, we need to also
                    // close the outer container (which consumes the same EndElement token)
                    if is_inner
                        && let Some(outer_completed) = stack.pop()
                        && let Some(outer_parent) = stack.last_mut()
                    {
                        outer_parent.add_child(outer_completed.build(ctx));
                    }
                }
            }
            KfxToken::Text(text) => {
                // Append text to the current element's accumulated content
                // This ensures all text within an element is concatenated
                if let Some(current) = stack.last_mut() {
                    current.append_text(text);
                }
            }
            KfxToken::StartSpan(span) => {
                // Push the span onto the stack with current text offset
                // The offset is relative to the current element's accumulated text
                let current_offset = stack.last().map(|b| b.text_len()).unwrap_or(0);

                // Create anchor for inline elements with IDs (e.g., noteref backlinks)
                // Offset is relative to the current element's accumulated text
                if let Some(anchor_id) = span.get_semantic(SemanticTarget::Id)
                    && let Some(parent) = stack.last()
                    && let Some(container_id) = parent.container_id
                {
                    ctx.create_anchor_if_needed(anchor_id, container_id, current_offset);
                }

                span_stack.push((current_offset, span.clone()));
            }
            KfxToken::EndSpan => {
                // Pop the span and calculate its length
                if let Some((start_offset, mut span_info)) = span_stack.pop() {
                    // Calculate length based on accumulated text in the current element
                    let current_offset = stack.last().map(|b| b.text_len()).unwrap_or(0);
                    let length = current_offset.saturating_sub(start_offset);

                    // Update the span with calculated offset and length
                    span_info.offset = start_offset;
                    span_info.length = length;

                    // Add the span as a style_event (if non-empty)
                    // Note: The flattening algorithm ensures spans are non-overlapping
                    // and already have all accumulated attributes merged.
                    if length > 0
                        && let Some(current) = stack.last_mut()
                    {
                        current.add_style_event(span_info, ctx);
                    }
                }
            }
        }
    }

    // Return the root children as a list (the storyline content_list)
    if let Some(root) = stack.pop() {
        root.build(ctx)
    } else {
        IonValue::List(vec![])
    }
}

/// Builder for constructing Ion structures from tokens.
struct IonBuilder {
    fields: Vec<(u64, IonValue)>,
    children: Vec<IonValue>,
    /// Accumulated text content for this element (concatenated during build)
    accumulated_text: String,
    /// Character count of accumulated text (for style event offsets)
    /// KFX uses character offsets, not byte offsets
    accumulated_char_count: usize,
    /// Collected style events for this element (inline spans)
    style_events: Vec<IonValue>,
    /// Container ID for this element (set during StartElement, used for length tracking)
    container_id: Option<u64>,
    /// True if this is an inner text element inside a container wrapper.
    /// When EndElement is reached for this builder, we need an extra EndElement
    /// to close the outer container.
    is_inner_wrapper_text: bool,
}

impl IonBuilder {
    fn new() -> Self {
        Self {
            fields: Vec::new(),
            children: Vec::new(),
            accumulated_text: String::new(),
            accumulated_char_count: 0,
            style_events: Vec::new(),
            container_id: None,
            is_inner_wrapper_text: false,
        }
    }

    fn with_fields(fields: Vec<(u64, IonValue)>, container_id: u64) -> Self {
        Self {
            fields,
            children: Vec::new(),
            accumulated_text: String::new(),
            accumulated_char_count: 0,
            style_events: Vec::new(),
            container_id: Some(container_id),
            is_inner_wrapper_text: false,
        }
    }

    fn add_child(&mut self, child: IonValue) {
        self.children.push(child);
    }

    /// Append text to this element's accumulated content.
    /// Returns the character offset where this text starts (for span tracking).
    /// KFX style events use character offsets, not byte offsets.
    fn append_text(&mut self, text: &str) -> usize {
        let offset = self.accumulated_char_count;
        self.accumulated_text.push_str(text);
        self.accumulated_char_count += text.chars().count();
        offset
    }

    /// Get the current accumulated text length in characters.
    /// KFX style events use character offsets, not byte offsets.
    fn text_len(&self) -> usize {
        self.accumulated_char_count
    }

    /// Add a style event (inline span) to this element.
    ///
    /// Converts SpanStart into KFX style_event Ion struct:
    /// { offset: N, length: N, style: $symbol [, link_to: $anchor] }
    fn add_style_event(&mut self, span: SpanStart, ctx: &mut ExportContext) {
        let mut event_fields = Vec::new();

        // Offset and length (required)
        event_fields.push((sym!(Offset), IonValue::Int(span.offset as i64)));
        event_fields.push((sym!(Length), IonValue::Int(span.length as i64)));

        // Style reference (required for rendering)
        if let Some(style_sym) = span.style_symbol {
            event_fields.push((sym!(Style), IonValue::Symbol(style_sym)));
        } else {
            // Use default style if no specific style
            event_fields.push((sym!(Style), IonValue::Symbol(ctx.default_style_symbol)));
        }

        // Add span-specific attributes (e.g., link_to for links, yj.display for noterefs)
        for (field_id, value_str) in &span.kfx_attrs {
            if *field_id == sym!(LinkTo) {
                // LinkTo is always a symbol reference (anchor symbol)
                let sym_id = ctx.symbols.get_or_intern(value_str);
                event_fields.push((*field_id, IonValue::Symbol(sym_id)));
            } else if *field_id == sym!(YjDisplay) {
                // YjDisplay value is a symbol ID (e.g., YjNote = 617)
                if let Ok(sym_id) = value_str.parse::<u64>() {
                    event_fields.push((*field_id, IonValue::Symbol(sym_id)));
                }
            } else {
                event_fields.push((*field_id, IonValue::String(value_str.clone())));
            }
        }

        self.style_events.push(IonValue::Struct(event_fields));
    }

    /// Finalize and build the Ion struct, creating content reference if text was accumulated.
    fn build(mut self, ctx: &mut ExportContext) -> IonValue {
        // KFX storylines are flat lists of elements, not nested structs
        // Each element is a struct with type, content reference, and possibly nested content_list
        if !self.fields.is_empty() {
            // Record text length for this content ID (used by location_map)
            if let Some(container_id) = self.container_id {
                ctx.record_content_length(container_id, self.accumulated_text.len());
            }

            // If this element has accumulated text, create ONE content reference
            if !self.accumulated_text.is_empty() {
                let (content_idx, _offset) = ctx.append_text(&self.accumulated_text);
                let content_ref = IonValue::Struct(vec![
                    (sym!(Name), IonValue::Symbol(ctx.current_content_name)),
                    (sym!(Index), IonValue::Int(content_idx as i64)),
                ]);
                self.fields.push((sym!(Content), content_ref));
            }

            // Add style_events if any inline spans were collected
            if !self.style_events.is_empty() {
                self.fields
                    .push((sym!(StyleEvents), IonValue::List(self.style_events)));
            }

            // Add nested children as content_list if present
            if !self.children.is_empty() {
                self.fields
                    .push((sym!(ContentList), IonValue::List(self.children)));
            }

            IonValue::Struct(self.fields)
        } else if !self.children.is_empty() {
            // Root level: return list of children
            IonValue::List(self.children)
        } else {
            IonValue::Null
        }
    }
}

/// Build a storyline Ion structure from an IR chapter.
///
/// **Note**: This is now internal - use `build_chapter_entities` for the full
/// three-entity architecture (Content, Storyline, Section).
pub fn build_storyline_ion(chapter: &IRChapter, ctx: &mut ExportContext) -> IonValue {
    let tokens = ir_to_tokens(chapter, ctx);
    tokens_to_ion(&tokens, ctx)
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

        let chapter = build_ir_from_tokens(&stream, &[], None, |_, _| None);
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
            kfx_attrs: Vec::new(),
            style_symbol: None,
            style_name: None,
            needs_container_wrapper: false,
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, &[], None, |_, _| None);

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
            kfx_attrs: Vec::new(),
            style_symbol: None,
            style_name: None,
            needs_container_wrapper: false,
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, &[], None, |name, idx| {
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
            kfx_attrs: Vec::new(),
            style_symbol: None,
            style_name: None,
            needs_container_wrapper: false,
        }));
        stream.end_element();

        let chapter =
            build_ir_from_tokens(&stream, &[], None, |_, _| Some("Chapter 1".to_string()));

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
                style_symbol: None,
                kfx_attrs: Vec::new(),
            }],
            kfx_attrs: Vec::new(),
            style_symbol: None,
            style_name: None,
            needs_container_wrapper: false,
        }));
        stream.end_element();

        // Text is "Hello, world!" - span at offset 7, length 5 = "world"
        let chapter =
            build_ir_from_tokens(&stream, &[], None, |_, _| Some("Hello, world!".to_string()));

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
    fn test_char_to_byte_offset() {
        let text = "Hello ὑπόληψις world";

        // ASCII chars: byte offset = char offset
        assert_eq!(char_to_byte_offset(text, 0), 0); // 'H'
        assert_eq!(char_to_byte_offset(text, 5), 5); // ' '

        // Greek chars start at char 6, byte 6
        // Each Greek char is 3 bytes (extended Greek), so:
        // char 6 = byte 6 (ὑ)
        // char 7 = byte 9 (π)
        // char 13 = byte 21 (ς)
        // char 14 = byte 23 (' ')
        assert_eq!(char_to_byte_offset(text, 6), 6); // 'ὑ'
        assert_eq!(char_to_byte_offset(text, 7), 9); // 'π'
        assert_eq!(char_to_byte_offset(text, 14), 23); // ' ' after Greek

        // Past end returns text.len()
        assert_eq!(char_to_byte_offset(text, 100), text.len());
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

    // ========================================================================
    // Export tests
    // ========================================================================

    #[test]
    fn test_ir_to_tokens_basic() {
        let mut chapter = IRChapter::new();

        // Create a text node with content
        let text_range = chapter.append_text("Hello");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(chapter.root(), text_id);

        let mut ctx = ExportContext::new();
        let tokens = ir_to_tokens(&chapter, &mut ctx);

        // Should have tokens for the text node
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_build_storyline_ion() {
        let mut chapter = IRChapter::new();

        // Create a paragraph with a text child
        let para = Node::new(Role::Paragraph);
        let para_id = chapter.alloc_node(para);
        chapter.append_child(chapter.root(), para_id);

        let text_range = chapter.append_text("Test content");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(para_id, text_id);

        let mut ctx = ExportContext::new();
        let ion = build_storyline_ion(&chapter, &mut ctx);

        // Should produce some Ion structure
        assert!(!matches!(ion, IonValue::Null));
    }

    #[test]
    fn test_tokens_to_ion_empty() {
        let tokens = TokenStream::new();
        let mut ctx = ExportContext::new();
        let ion = tokens_to_ion(&tokens, &mut ctx);

        // Empty tokens should produce an empty list or null
        assert!(
            matches!(ion, IonValue::List(_)) || matches!(ion, IonValue::Null),
            "expected List or Null, got {:?}",
            ion
        );
    }

    #[test]
    fn test_heading_level_export() {
        use crate::kfx::symbols::KfxSymbol;

        let mut chapter = IRChapter::new();

        // Create an H2 heading
        let h2 = Node::new(Role::Heading(2));
        let h2_id = chapter.alloc_node(h2);
        chapter.append_child(chapter.root(), h2_id);

        // Add text content
        let text_range = chapter.append_text("Chapter Title");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(h2_id, text_id);

        let mut ctx = ExportContext::new();
        let ion = build_storyline_ion(&chapter, &mut ctx);

        // Find the heading container in the output and verify yj.semantics.heading_level = 2
        fn find_heading_level(ion: &IonValue) -> Option<i64> {
            match ion {
                IonValue::Struct(fields) => {
                    for (field_id, value) in fields {
                        if *field_id == KfxSymbol::YjSemanticsHeadingLevel as u64
                            && let IonValue::Int(level) = value
                        {
                            return Some(*level);
                        }
                    }
                    // Check content_list (children in KFX)
                    for (field_id, value) in fields {
                        if *field_id == KfxSymbol::ContentList as u64
                            && let Some(level) = find_heading_level(value)
                        {
                            return Some(level);
                        }
                    }
                }
                IonValue::List(items) => {
                    for item in items {
                        if let Some(level) = find_heading_level(item) {
                            return Some(level);
                        }
                    }
                }
                _ => {}
            }
            None
        }

        let heading_level = find_heading_level(&ion);
        assert_eq!(
            heading_level,
            Some(2),
            "Expected yj.semantics.heading_level = 2, got {:?}",
            heading_level
        );
    }

    #[test]
    fn test_style_event_offsets_use_char_count() {
        // KFX style events use character offsets, not byte offsets.
        // Greek characters are multi-byte in UTF-8, so this verifies
        // we count characters, not bytes.
        let mut builder = IonBuilder::new();

        // "Hello " = 6 chars, 6 bytes
        builder.append_text("Hello ");
        assert_eq!(builder.text_len(), 6);

        // "ὑπόληψις" = 8 chars, 17 bytes in UTF-8
        let greek_offset = builder.append_text("ὑπόληψις");
        assert_eq!(greek_offset, 6, "Greek text should start at char offset 6");
        assert_eq!(builder.text_len(), 14, "Total should be 14 chars (6 + 8)");

        // Verify byte length differs from char count
        assert_eq!(builder.accumulated_text.len(), 23); // 6 + 17 bytes
        assert_eq!(builder.accumulated_char_count, 14); // 6 + 8 chars
    }

    #[test]
    fn test_layout_hints_for_heading() {
        // Headings should emit layout_hints: [treat_as_title]
        let mut chapter = IRChapter::new();
        let text_range = chapter.append_text("Chapter 1");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);

        let heading = Node::new(Role::Heading(1));
        let heading_id = chapter.alloc_node(heading);
        chapter.append_child(heading_id, text_id);
        chapter.append_child(chapter.root(), heading_id);

        let mut ctx = crate::kfx::context::ExportContext::new();
        ctx.register_section("test_section");
        let ion = build_storyline_ion(&chapter, &mut ctx);

        // Find layout_hints in the generated Ion
        fn find_layout_hints(ion: &IonValue) -> Option<Vec<u64>> {
            match ion {
                IonValue::Struct(fields) => {
                    for (key, value) in fields {
                        if *key == sym!(LayoutHints)
                            && let IonValue::List(items) = value
                        {
                            return Some(
                                items
                                    .iter()
                                    .filter_map(|v| {
                                        if let IonValue::Symbol(s) = v {
                                            Some(*s)
                                        } else {
                                            None
                                        }
                                    })
                                    .collect(),
                            );
                        }
                        if let Some(hints) = find_layout_hints(value) {
                            return Some(hints);
                        }
                    }
                    None
                }
                IonValue::List(items) => {
                    for item in items {
                        if let Some(hints) = find_layout_hints(item) {
                            return Some(hints);
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        let hints = find_layout_hints(&ion);
        assert!(hints.is_some(), "Heading should have layout_hints");
        let hints = hints.unwrap();
        assert!(
            hints.contains(&(KfxSymbol::TreatAsTitle as u64)),
            "Heading layout_hints should contain treat_as_title"
        );
    }

    #[test]
    fn test_layout_hints_for_figure() {
        // Figure elements should emit layout_hints: [figure]
        let mut chapter = IRChapter::new();

        let figure = Node::new(Role::Figure);
        let figure_id = chapter.alloc_node(figure);
        chapter.append_child(chapter.root(), figure_id);

        let mut ctx = crate::kfx::context::ExportContext::new();
        ctx.register_section("test_section");
        let ion = build_storyline_ion(&chapter, &mut ctx);

        // Find layout_hints in the generated Ion
        fn find_layout_hints(ion: &IonValue) -> Option<Vec<u64>> {
            match ion {
                IonValue::Struct(fields) => {
                    for (key, value) in fields {
                        if *key == sym!(LayoutHints)
                            && let IonValue::List(items) = value
                        {
                            return Some(
                                items
                                    .iter()
                                    .filter_map(|v| {
                                        if let IonValue::Symbol(s) = v {
                                            Some(*s)
                                        } else {
                                            None
                                        }
                                    })
                                    .collect(),
                            );
                        }
                        if let Some(hints) = find_layout_hints(value) {
                            return Some(hints);
                        }
                    }
                    None
                }
                IonValue::List(items) => {
                    for item in items {
                        if let Some(hints) = find_layout_hints(item) {
                            return Some(hints);
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        let hints = find_layout_hints(&ion);
        assert!(hints.is_some(), "Figure should have layout_hints");
        let hints = hints.unwrap();
        assert!(
            hints.contains(&(KfxSymbol::Figure as u64)),
            "Figure layout_hints should contain figure"
        );
    }

    #[test]
    fn test_yj_classification_for_footnote_popup() {
        // Elements with epub:type="endnote" or "footnote" should emit
        // yj.classification: yj.endnote ($615: $619) for popup support
        let mut chapter = IRChapter::new();

        // Create a list item that represents an endnote
        let text_range = chapter.append_text("This is footnote content");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);

        let endnote = Node::new(Role::ListItem);
        let endnote_id = chapter.alloc_node(endnote);
        chapter.append_child(endnote_id, text_id);
        chapter.append_child(chapter.root(), endnote_id);

        // Set epub:type to indicate this is an endnote
        chapter
            .semantics
            .set_epub_type(endnote_id, "endnote footnote".to_string());
        chapter.semantics.set_id(endnote_id, "note-1".to_string());

        let mut ctx = crate::kfx::context::ExportContext::new();
        ctx.register_section("test_section");
        let ion = build_storyline_ion(&chapter, &mut ctx);

        // Find yj.classification in the generated Ion and check its value
        fn find_classification(ion: &IonValue) -> Option<u64> {
            match ion {
                IonValue::Struct(fields) => {
                    for (key, value) in fields {
                        if *key == sym!(YjClassification)
                            && let IonValue::Symbol(sym) = value
                        {
                            return Some(*sym);
                        }
                        if let Some(found) = find_classification(value) {
                            return Some(found);
                        }
                    }
                    None
                }
                IonValue::List(items) => {
                    for item in items {
                        if let Some(found) = find_classification(item) {
                            return Some(found);
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        let classification = find_classification(&ion);
        assert!(
            classification.is_some(),
            "Endnote element should have yj.classification attribute"
        );
        assert_eq!(
            classification.unwrap(),
            KfxSymbol::YjEndnote as u64,
            "yj.classification should be yj.endnote ($619) for endnote elements"
        );
    }

    #[test]
    fn test_heading_with_border_exports_as_container() {
        // Test that elements with borders are wrapped in type: container
        // with nested type: text for KFX border rendering
        use crate::ir::{BorderStyle, ComputedStyle, Length};

        let mut chapter = IRChapter::new();

        // Create a heading with border style
        let mut style = ComputedStyle::default();
        style.border_style_top = BorderStyle::Solid;
        style.border_width_top = Length::Px(2.0);
        let style_id = chapter.styles.intern(style);

        let mut h1 = Node::new(Role::Heading(1));
        h1.style = style_id;
        let h1_id = chapter.alloc_node(h1);
        chapter.append_child(chapter.root(), h1_id);

        // Add text content
        let text_range = chapter.append_text("Title with Border");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(h1_id, text_id);

        let mut ctx = crate::kfx::context::ExportContext::new();
        let ion = build_storyline_ion(&chapter, &mut ctx);

        // Helper to find element type and nested content_list structure
        fn find_container_structure(ion: &IonValue) -> Option<(u64, Option<u64>)> {
            match ion {
                IonValue::Struct(fields) => {
                    let mut elem_type = None;
                    let mut inner_type = None;

                    for (key, value) in fields {
                        if *key == KfxSymbol::Type as u64 {
                            if let IonValue::Symbol(sym) = value {
                                elem_type = Some(*sym);
                            }
                        }
                        if *key == KfxSymbol::ContentList as u64 {
                            if let IonValue::List(items) = value {
                                for item in items {
                                    if let Some((inner_elem_type, _)) =
                                        find_container_structure(item)
                                    {
                                        inner_type = Some(inner_elem_type);
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if elem_type.is_some() {
                        return Some((elem_type.unwrap(), inner_type));
                    }
                    None
                }
                IonValue::List(items) => {
                    for item in items {
                        if let Some(result) = find_container_structure(item) {
                            return Some(result);
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        let structure = find_container_structure(&ion);
        assert!(structure.is_some(), "Should find element structure");
        let (outer_type, inner_type) = structure.unwrap();

        // Outer element should be type: container
        assert_eq!(
            outer_type,
            KfxSymbol::Container as u64,
            "Heading with border should have type: container (not text)"
        );

        // Should have nested type: text child
        assert!(
            inner_type.is_some(),
            "Container should have nested content_list with inner element"
        );
        assert_eq!(
            inner_type.unwrap(),
            KfxSymbol::Text as u64,
            "Inner element should have type: text"
        );
    }

    #[test]
    fn test_heading_without_border_exports_as_text() {
        // Test that elements without borders use normal type: text
        let mut chapter = IRChapter::new();

        // Create a heading without border style
        let h1 = Node::new(Role::Heading(1));
        let h1_id = chapter.alloc_node(h1);
        chapter.append_child(chapter.root(), h1_id);

        // Add text content
        let text_range = chapter.append_text("Title without Border");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(h1_id, text_id);

        let mut ctx = crate::kfx::context::ExportContext::new();
        let ion = build_storyline_ion(&chapter, &mut ctx);

        // Helper to find first element type
        fn find_first_element_type(ion: &IonValue) -> Option<u64> {
            match ion {
                IonValue::Struct(fields) => {
                    for (key, value) in fields {
                        if *key == KfxSymbol::Type as u64 {
                            if let IonValue::Symbol(sym) = value {
                                return Some(*sym);
                            }
                        }
                    }
                    None
                }
                IonValue::List(items) => {
                    for item in items {
                        if let Some(result) = find_first_element_type(item) {
                            return Some(result);
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        let elem_type = find_first_element_type(&ion);
        assert!(elem_type.is_some(), "Should find element type");

        // Element without border should be type: text (normal heading)
        assert_eq!(
            elem_type.unwrap(),
            KfxSymbol::Text as u64,
            "Heading without border should have type: text"
        );
    }

    #[test]
    fn test_needs_container_wrapper_no_border() {
        let style = ComputedStyle::default();
        assert!(!needs_container_wrapper(&style));
    }

    #[test]
    fn test_needs_container_wrapper_with_top_border() {
        let mut style = ComputedStyle::default();
        style.border_style_top = BorderStyle::Solid;
        style.border_width_top = Length::Px(1.0);
        assert!(needs_container_wrapper(&style));
    }

    #[test]
    fn test_needs_container_wrapper_with_bottom_border() {
        let mut style = ComputedStyle::default();
        style.border_style_bottom = BorderStyle::Solid;
        style.border_width_bottom = Length::Px(2.0);
        assert!(needs_container_wrapper(&style));
    }

    #[test]
    fn test_needs_container_wrapper_border_style_none() {
        let mut style = ComputedStyle::default();
        // Has width but no style - should NOT need wrapper
        style.border_style_top = BorderStyle::None;
        style.border_width_top = Length::Px(1.0);
        assert!(!needs_container_wrapper(&style));
    }

    #[test]
    fn test_needs_container_wrapper_border_width_zero() {
        let mut style = ComputedStyle::default();
        // Has style but zero width - should NOT need wrapper
        style.border_style_top = BorderStyle::Solid;
        style.border_width_top = Length::Px(0.0);
        assert!(!needs_container_wrapper(&style));
    }

    #[test]
    fn test_nested_spans_link_containing_inline() {
        // Test that nested spans (Link containing Inline) are properly reconstructed.
        // This is the TOC case: "1. Easy Concurrency..." where "1." is in an Inline inside a Link.
        let mut stream = TokenStream::new();
        let mut link_semantics = HashMap::new();
        link_semantics.insert(SemanticTarget::Href, "#chapter1".to_string());

        // Text: "1. Easy Concurrency"
        // Link: offset 0, length 19 (entire text)
        // Inline: offset 0, length 2 ("1.")
        stream.push(KfxToken::StartElement(ElementStart {
            role: Role::Paragraph,
            id: None,
            semantics: HashMap::new(),
            content_ref: Some(ContentRef {
                name: "content_1".to_string(),
                index: 0,
            }),
            style_events: vec![
                SpanStart {
                    role: Role::Link,
                    semantics: link_semantics,
                    offset: 0,
                    length: 19,
                    style_symbol: None,
                    kfx_attrs: Vec::new(),
                },
                SpanStart {
                    role: Role::Inline,
                    semantics: HashMap::new(),
                    offset: 0,
                    length: 2,
                    style_symbol: None,
                    kfx_attrs: Vec::new(),
                },
            ],
            kfx_attrs: Vec::new(),
            style_symbol: None,
            style_name: None,
            needs_container_wrapper: false,
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, &[], None, |_, _| {
            Some("1. Easy Concurrency".to_string())
        });

        // Expected structure:
        // Paragraph
        //   Link [href="#chapter1"]
        //     Inline
        //       Text: "1."
        //     Text: " Easy Concurrency"
        let para_id = chapter.children(chapter.root()).next().unwrap();
        let para_children: Vec<_> = chapter.children(para_id).collect();

        // Should have exactly one child: the Link
        assert_eq!(
            para_children.len(),
            1,
            "Paragraph should have one Link child"
        );

        let link_id = para_children[0];
        let link_node = chapter.node(link_id).unwrap();
        assert_eq!(link_node.role, Role::Link, "First child should be Link");
        assert_eq!(
            chapter.semantics.href(link_id),
            Some("#chapter1"),
            "Link should have href"
        );

        // Link should have two children: Inline and Text
        let link_children: Vec<_> = chapter.children(link_id).collect();
        assert_eq!(
            link_children.len(),
            2,
            "Link should have Inline + Text children"
        );

        // First child: Inline containing "1."
        let inline_id = link_children[0];
        let inline_node = chapter.node(inline_id).unwrap();
        assert_eq!(
            inline_node.role,
            Role::Inline,
            "First Link child should be Inline"
        );

        let inline_children: Vec<_> = chapter.children(inline_id).collect();
        assert_eq!(
            inline_children.len(),
            1,
            "Inline should have one Text child"
        );
        let inline_text = chapter.node(inline_children[0]).unwrap();
        assert_eq!(chapter.text(inline_text.text), "1.");

        // Second child: Text " Easy Concurrency"
        let text_id = link_children[1];
        let text_node = chapter.node(text_id).unwrap();
        assert_eq!(text_node.role, Role::Text);
        assert_eq!(chapter.text(text_node.text), " Easy Concurrency");
    }

    #[test]
    fn test_flatten_inline_content_produces_non_overlapping_segments() {
        // Test the "Push Down, Emit at Bottom" flattening algorithm.
        // Given: Link > Inline > Text("1.") + Text("Easy...")
        // Expect: Two non-overlapping segments, each with correct accumulated state.

        let mut chapter = IRChapter::new();

        // Create distinct styles (use different margin values to distinguish)
        let link_style = chapter.styles.intern(ComputedStyle::default());
        let mut inline_computed = ComputedStyle::default();
        inline_computed.margin_top = Length::Px(10.0);
        let inline_style = chapter.styles.intern(inline_computed);

        // Build tree: Link > Inline > Text("1.") + Text(" Easy")
        // Create text nodes
        let text1_range = chapter.append_text("1.");
        let mut text1 = Node::new(Role::Text);
        text1.text = text1_range;
        let text1_id = chapter.alloc_node(text1);

        let text2_range = chapter.append_text(" Easy Concurrency");
        let mut text2 = Node::new(Role::Text);
        text2.text = text2_range;
        let text2_id = chapter.alloc_node(text2);

        // Create Inline containing text1
        let mut inline_node = Node::new(Role::Inline);
        inline_node.style = inline_style;
        let inline_id = chapter.alloc_node(inline_node);
        chapter.append_child(inline_id, text1_id);

        // Create Link containing Inline and text2
        let mut link_node = Node::new(Role::Link);
        link_node.style = link_style;
        let link_id = chapter.alloc_node(link_node);
        chapter.append_child(link_id, inline_id);
        chapter.append_child(link_id, text2_id);
        chapter.semantics.set_href(link_id, "#chapter1".to_string());

        // Flatten the Link subtree
        let mut segments = Vec::new();
        flatten_inline_content(&chapter, link_id, InlineState::default(), &mut segments);

        // Should produce exactly 2 segments
        assert_eq!(segments.len(), 2, "Should have 2 non-overlapping segments");

        // First segment: "1." with Inline's style and Link's href
        assert_eq!(segments[0].text, "1.");
        assert_eq!(
            segments[0].state.link_to,
            Some("#chapter1".to_string()),
            "First segment should have link_to from Link"
        );
        assert_eq!(
            segments[0].state.style,
            Some(inline_style),
            "First segment should have Inline's style (innermost wins)"
        );

        // Second segment: " Easy Concurrency" with Link's style and Link's href
        assert_eq!(segments[1].text, " Easy Concurrency");
        assert_eq!(
            segments[1].state.link_to,
            Some("#chapter1".to_string()),
            "Second segment should have link_to from Link"
        );
        assert_eq!(
            segments[1].state.style,
            Some(link_style),
            "Second segment should have Link's style"
        );
    }
}
