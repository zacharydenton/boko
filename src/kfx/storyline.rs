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

    // Get element type symbol ID (u64 from IonValue, cast to u32 for schema)
    let kfx_type_id = get_field(fields, sym!(Type))
        .and_then(|v| v.as_symbol())
        .unwrap_or(sym!(Container)) as u32;

    // Use schema to resolve role with attribute lookup closure
    // Return int values directly, or symbol IDs cast to i64 for symbol-based attributes
    let role = schema().resolve_element_role(kfx_type_id, |symbol| {
        get_field(fields, symbol as u64).and_then(|v| {
            v.as_int().or_else(|| v.as_symbol().map(|s| s as i64))
        })
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

    // Get style reference (symbol ID or name) for later lookup
    let style_name = get_field(fields, sym!(Style))
        .and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols));

    // Emit StartElement token
    stream.push(KfxToken::StartElement(ElementStart {
        role,
        id,
        semantics,
        content_ref,
        style_events,
        kfx_attrs: Vec::new(),
        style_symbol: None, // Symbol ID (for export)
        style_name,         // Style name (for import lookup)
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

    for rule in schema().element_attr_rules(kfx_type_id) {
        if let Some(raw_value) =
            get_field(fields, rule.kfx_field as u64).and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))
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
            let has_field = |symbol: KfxSymbol| get_field(fields, symbol as u64).is_some();

            // Use schema to determine role
            let role = schema().resolve_span_role(&has_field);

            // Extract ALL semantic attributes using schema rules (GENERIC!)
            let semantics = extract_all_span_attrs(fields, &has_field, ctx);

            Some(SpanStart {
                role,
                semantics,
                offset,
                length,
                style_symbol: None, // Populated by import/style lookup
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
        if let Some(raw_value) =
            get_field(fields, rule.kfx_field as u64).and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))
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
pub fn build_ir_from_tokens<F>(
    tokens: &TokenStream,
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
                if let Some(style_name) = &elem.style_name {
                    if let Some(styles_map) = styles {
                        if let Some(kfx_props) = styles_map.get(style_name) {
                            let ir_style = kfx_style_to_ir(kfx_props);
                            let style_id = chapter.styles.intern(ir_style);
                            if let Some(node) = chapter.node_mut(node_id) {
                                node.style = style_id;
                            }
                        }
                    }
                }

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

/// Convert KFX style properties to an IR ComputedStyle using the schema.
///
/// This is schema-driven: iterates schema rules with KFX symbol mappings,
/// applies inverse transforms to convert KFX values back to IR values.
fn kfx_style_to_ir(props: &[(u64, IonValue)]) -> crate::ir::ComputedStyle {
    use crate::kfx::style_schema::{import_kfx_style, StyleSchema};

    let schema = StyleSchema::standard();
    import_kfx_style(&schema, props)
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
        // KFX style_events use character offsets, not byte offsets
        // Convert char offset to byte offset for string slicing
        let span_start = char_to_byte_offset(text, span.offset);
        let span_end = char_to_byte_offset(text, span.offset + span.length);

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
    build_ir_from_tokens(&tokens, styles, content_lookup)
}

// ============================================================================
// EXPORT: IR → TokenStream → Ion
// ============================================================================

use crate::ir::Role;
use crate::kfx::context::ExportContext;

/// Convert an IR chapter to a TokenStream.
///
/// This is the first stage of export: walking the IR tree and emitting tokens.
pub fn ir_to_tokens(chapter: &IRChapter, ctx: &mut ExportContext) -> TokenStream {
    let sch = schema();
    let mut stream = TokenStream::new();

    walk_node_for_export(chapter, chapter.root(), &sch, ctx, &mut stream);
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

    // Check if this is an inline role that should become a span (style_event)
    if sch.is_inline_role(node.role) {
        emit_span_for_export(chapter, node_id, node, sch, ctx, stream);
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

    // SCHEMA-DRIVEN attribute export (FIX: no more hardcoded checks!)
    // Create a closure to get semantic values by target
    let export_ctx = crate::kfx::transforms::ExportContext {
        spine_map: None,
        resource_registry: Some(&ctx.resource_registry),
    };
    let kfx_attrs = sch.export_attributes(
        node.role,
        |target| match target {
            SemanticTarget::Href => chapter.semantics.href(node_id).map(|s| s.to_string()),
            SemanticTarget::Src => chapter.semantics.src(node_id).map(|s| s.to_string()),
            SemanticTarget::Alt => chapter.semantics.alt(node_id).map(|s| s.to_string()),
            SemanticTarget::Id => chapter.semantics.id(node_id).map(|s| s.to_string()),
        },
        &export_ctx,
    );

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

/// Emit a span (inline) node as StartSpan/EndSpan tokens.
///
/// Inline roles like Link and Inline (bold/italic) should be emitted as spans,
/// not nested containers, so they can be converted to KFX style_events.
fn emit_span_for_export(
    chapter: &IRChapter,
    node_id: NodeId,
    node: &crate::ir::Node,
    sch: &crate::kfx::schema::KfxSchema,
    ctx: &mut ExportContext,
    stream: &mut TokenStream,
) {
    // Build span with semantics
    let mut span = SpanStart::new(node.role, 0, 0); // offset/length calculated in tokens_to_ion

    // Register the node's style and get a KFX style symbol
    let style_symbol = ctx.register_style_id(node.style, &chapter.styles);
    span.style_symbol = Some(style_symbol);

    // SCHEMA-DRIVEN attribute export for spans
    let export_ctx = crate::kfx::transforms::ExportContext {
        spine_map: None,
        resource_registry: Some(&ctx.resource_registry),
    };
    let mut kfx_attrs = sch.export_span_attributes(
        node.role,
        |target| match target {
            SemanticTarget::Href => chapter.semantics.href(node_id).map(|s| s.to_string()),
            SemanticTarget::Src => chapter.semantics.src(node_id).map(|s| s.to_string()),
            SemanticTarget::Alt => chapter.semantics.alt(node_id).map(|s| s.to_string()),
            SemanticTarget::Id => chapter.semantics.id(node_id).map(|s| s.to_string()),
        },
        &export_ctx,
    );

    // Convert href values to anchor symbols via the AnchorRegistry.
    // KFX uses indirect anchor references: link_to points to an anchor symbol,
    // and anchor entities define where those symbols resolve to.
    for (field_id, value) in &mut kfx_attrs {
        if *field_id == sym!(LinkTo) {
            // Register the link target and get an anchor symbol
            let anchor_symbol = ctx.anchor_registry.register_link_target(value);
            *value = anchor_symbol;
        }
    }

    span.kfx_attrs = kfx_attrs;

    // Populate semantics map
    if let Some(href) = chapter.semantics.href(node_id) {
        span.set_semantic(SemanticTarget::Href, href.to_string());
    }
    if let Some(id) = chapter.semantics.id(node_id) {
        span.set_semantic(SemanticTarget::Id, id.to_string());
    }

    stream.push(KfxToken::StartSpan(span));

    // Emit text content and children
    if !node.text.is_empty() {
        let text = chapter.text(node.text);
        if !text.is_empty() {
            stream.push(KfxToken::Text(text.to_string()));
        }
    }

    // Walk children (may contain more text or nested spans)
    for child in chapter.children(node_id) {
        walk_node_for_export(chapter, child, sch, ctx, stream);
    }

    stream.push(KfxToken::EndSpan);
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
                let mut fields = Vec::new();

                // Unique container ID - use the global generator to avoid collisions
                let container_id = ctx.fragment_ids.next();
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

                // Add heading level if this is a heading
                if let Role::Heading(level) = elem.role {
                    fields.push((sym!(YjSemanticsHeadingLevel), IonValue::Int(level as i64)));

                    // Record heading position with ACTUAL content fragment ID (Fix for navigation)
                    ctx.record_heading_with_id(level, container_id);
                }

                // Add list_style for ordered lists
                if elem.role == Role::OrderedList {
                    fields.push((sym!(ListStyle), IonValue::Symbol(sym!(Numeric) as u64)));
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
            KfxToken::EndElement => {
                if let Some(completed) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.add_child(completed.build(ctx));
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

                    // Add to the current element's style_events
                    if let Some(current) = stack.last_mut() {
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

        // Add span-specific attributes (e.g., link_to for links)
        for (field_id, value_str) in &span.kfx_attrs {
            // LinkTo is always a symbol reference
            if *field_id == sym!(LinkTo) {
                let sym_id = ctx.symbols.get_or_intern(value_str);
                event_fields.push((*field_id, IonValue::Symbol(sym_id)));
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

        let chapter = build_ir_from_tokens(&stream, None, |_, _| None);
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
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, None, |_, _| None);

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
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, None, |name, idx| {
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
        }));
        stream.end_element();

        let chapter = build_ir_from_tokens(&stream, None, |_, _| Some("Chapter 1".to_string()));

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
        }));
        stream.end_element();

        // Text is "Hello, world!" - span at offset 7, length 5 = "world"
        let chapter = build_ir_from_tokens(&stream, None, |_, _| Some("Hello, world!".to_string()));

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
                        if *field_id == KfxSymbol::YjSemanticsHeadingLevel as u64 {
                            if let IonValue::Int(level) = value {
                                return Some(*level);
                            }
                        }
                    }
                    // Check content_list (children in KFX)
                    for (field_id, value) in fields {
                        if *field_id == KfxSymbol::ContentList as u64 {
                            if let Some(level) = find_heading_level(value) {
                                return Some(level);
                            }
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
}
