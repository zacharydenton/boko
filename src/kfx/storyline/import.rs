use super::*;

/// Context for tokenization including anchor resolution.
pub(super) struct TokenizeContext<'a> {
    doc_symbols: &'a [String],
    anchors: Option<&'a HashMap<String, String>>,
    /// Raw style structs by style name, for style-level layout_hints
    /// (reference KFX carries treat_as_title/figure/caption in styles).
    styles: Option<&'a HashMap<String, Vec<(u64, IonValue)>>>,
}

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
    styles: Option<&HashMap<String, Vec<(u64, IonValue)>>>,
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
        styles,
    };
    tokenize_content_list(content_list, &ctx, &mut stream);
    stream
}

/// Tokenize a content_list array.
pub(super) fn tokenize_content_list(
    list: &IonValue,
    ctx: &TokenizeContext,
    stream: &mut TokenStream,
) {
    let items = match list.as_list() {
        Some(l) => l,
        None => return,
    };

    for item in items {
        // Mixed content model (reference books): a content_list may hold
        // literal strings interleaved with element structs (inline math,
        // images). Strings are text runs, not elements.
        if let Some(text) = item.unwrap_annotated().as_string() {
            if !text.is_empty() {
                stream.push(KfxToken::Text(text.to_string()));
            }
            continue;
        }
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
pub(super) fn tokenize_content_item(
    item: &IonValue,
    ctx: &TokenizeContext,
    stream: &mut TokenStream,
) {
    // Unwrap annotation if present
    let inner = item.unwrap_annotated();
    let fields = match inner.as_struct() {
        Some(f) => f,
        None => return,
    };

    // Math container: `yj.classification: math` carries the source MathML
    // and spoken alt text as annotations. Import those into a Role::Math IR
    // node instead of walking the KVG/text rendering below it (the render is
    // derived data; the MathML is the content).
    if get_field(fields, sym!(YjClassification)).and_then(|v| v.as_symbol())
        == Some(KfxSymbol::Math as u64)
    {
        let annotation_ref = |wanted: u64| -> Option<ContentRef> {
            let anns = get_field(fields, sym!(Annotations))?.as_list()?;
            for ann in anns {
                let Some(f) = ann.unwrap_annotated().as_struct() else {
                    continue;
                };
                if get_field(f, sym!(AnnotationType)).and_then(|v| v.as_symbol()) != Some(wanted) {
                    continue;
                }
                let cr = get_field(f, sym!(Content))?.as_struct()?;
                let name = get_field(cr, sym!(Name))
                    .and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols))?;
                let index = get_field(cr, sym!(Index))
                    .and_then(|v| v.as_int())
                    .and_then(|n| usize::try_from(n).ok())?;
                return Some(ContentRef { name, index });
            }
            None
        };
        stream.push(KfxToken::MathImport(Box::new(MathImportToken {
            mathml_ref: annotation_ref(sym!(Mathml)),
            alttext_ref: annotation_ref(sym!(AltText)),
            id: get_field(fields, sym!(Id)).and_then(|v| v.as_int()),
            style_name: get_field(fields, sym!(Style))
                .and_then(|v| resolve_symbol_or_string(v, ctx.doc_symbols)),
        })));
        return;
    }

    // Get element type symbol ID (u64 from IonValue, converted to u32 for the
    // schema). The symbol is untrusted: `as u32` would alias an id above
    // u32::MAX to a valid type, so out-of-range ids fall back to Container.
    let kfx_type_id = get_field(fields, sym!(Type))
        .and_then(|v| v.as_symbol())
        .and_then(|s| u32::try_from(s).ok())
        .unwrap_or(KfxSymbol::Container as u32);

    // Use schema to resolve role with attribute lookup closure.
    // Return int values directly, or symbol IDs converted to i64 for
    // symbol-based attributes (ids above i64::MAX would wrap negative under
    // `as i64`, so they're treated as absent).
    let mut role = schema().resolve_element_role(kfx_type_id, |symbol| {
        get_field(fields, symbol as u64).and_then(|v| {
            v.as_int()
                .or_else(|| v.as_symbol().and_then(|s| i64::try_from(s).ok()))
        })
    });

    // Check for semantic type annotation (yj.semantics.type) which uses local symbols.
    // The schema's StructureWithSemanticType strategies define what values map to what roles.
    // "table_header_cell" isn't a strategy — it's a TableCell plus a header flag.
    let mut is_header_cell = false;
    if let Some(semantic_type) = get_semantic_type_annotation(fields, ctx.doc_symbols) {
        if semantic_type == "table_header_cell" {
            role = Role::TableCell;
            is_header_cell = true;
        } else if let Some(mapped_role) = schema().role_for_semantic_type(&semantic_type) {
            role = mapped_role;
        }
    }

    // Check for layout_hints to detect Figure and Caption elements
    // (schema-driven). Reference KFX carries the hints in the element's
    // *style* struct; older boko output put them on the content node —
    // consult both, node first.
    let style_hints = ctx.styles.and_then(|styles| {
        let style_sym = get_field(fields, sym!(Style))?.as_symbol()?;
        let name = resolve_symbol(style_sym, ctx.doc_symbols)?.to_string();
        let style = styles.get(&name)?;
        get_field(style, sym!(LayoutHints)).cloned()
    });
    let node_hints = get_field(fields, sym!(LayoutHints)).cloned();
    for hints in [node_hints, style_hints].into_iter().flatten() {
        let Some(hints_list) = hints.as_list() else {
            continue;
        };
        let mut mapped = None;
        for hint in hints_list {
            // `as u32` would alias an untrusted id above u32::MAX to a valid
            // hint; skip out-of-range ids instead.
            if let Some(hint_id) = hint.as_symbol().and_then(|s| u32::try_from(s).ok())
                && let Some(mapped_role) = schema().role_for_layout_hint(hint_id)
            {
                mapped = Some(mapped_role);
                break;
            }
        }
        if let Some(m) = mapped {
            role = m;
            break;
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
            // Reject negative indexes: `as usize` would wrap them to huge values.
            let index = get_field(content_fields, sym!(Index))
                .and_then(|v| v.as_int())
                .and_then(|n| usize::try_from(n).ok())?;
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

    // Carry the integer table-span / list-start fields through kfx_attrs so
    // the token→IR builder can restore them (they have no SemanticTarget and
    // are the inverse of the export-side hand-emitted attrs).
    let mut kfx_attrs = crate::kfx::tokens::KfxAttrs::new();
    for sym in [
        KfxSymbol::TableColumnSpan,
        KfxSymbol::TableRowSpan,
        KfxSymbol::ListStartOffset,
    ] {
        if let Some(n) = get_field(fields, sym as u64).and_then(|v| v.as_int()) {
            kfx_attrs.push((sym as u64, n.to_string()));
        }
    }

    // Emit StartElement token
    stream.push(KfxToken::StartElement(ElementStart {
        role,
        node_id: None, // Only used during export
        id,
        semantics,
        content_ref,
        style_events,
        kfx_attrs,
        style_symbol: None,             // Symbol ID (for export)
        style_name,                     // Style name (for import lookup)
        needs_container_wrapper: false, // Only used during export
        is_header_cell,
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
pub(super) fn get_semantic_type_annotation(
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
pub(super) fn extract_all_element_attrs(
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
pub(super) fn parse_style_events(events: &[IonValue], ctx: &TokenizeContext) -> Vec<SpanStart> {
    events
        .iter()
        .filter_map(|event| {
            let fields = event.as_struct()?;
            // Reject negative offsets/lengths: `as usize` would wrap a negative
            // i64 to a near-`usize::MAX` value and corrupt the span tree.
            let offset = get_field(fields, sym!(Offset))
                .and_then(|v| v.as_int())
                .and_then(|n| usize::try_from(n).ok())?;
            let length = get_field(fields, sym!(Length))
                .and_then(|v| v.as_int())
                .and_then(|n| usize::try_from(n).ok())?;

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
                node_id: None, // Only used during export
                semantics,
                offset,
                length,
                style_symbol,
                kfx_attrs: crate::kfx::tokens::KfxAttrs::new(),
            })
        })
        .collect()
}

/// Extract ALL semantic attributes for a span using schema rules.
///
/// This is **fully generic** - no hardcoded SemanticTarget checks.
pub(super) fn extract_all_span_attrs<F>(
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

// Token Stream → IR (Stack-based builder)

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
) -> Chapter
where
    F: FnMut(&str, usize) -> Option<String>,
{
    let mut chapter = Chapter::new();
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

                // Restore the header-cell flag.
                if elem.is_header_cell {
                    chapter.semantics.set_header_cell(node_id, true);
                }

                // Restore integer table-span / list-start attributes.
                for (field_id, value) in &elem.kfx_attrs {
                    let Ok(n) = value.parse::<u32>() else {
                        continue;
                    };
                    if *field_id == sym!(TableColumnSpan) {
                        chapter.semantics.set_col_span(node_id, n);
                    } else if *field_id == sym!(TableRowSpan) {
                        chapter.semantics.set_row_span(node_id, n);
                    } else if *field_id == sym!(ListStartOffset) {
                        chapter.semantics.set_list_start(node_id, n);
                    }
                }

                // Apply element ID if present (KFX stores as integer, we store as string)
                if let Some(id) = elem.id {
                    chapter.semantics.set_id(node_id, &id.to_string());
                }

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

            KfxToken::MathKvg(_) => {
                // Export-only token; never produced by the tokenizer.
            }

            KfxToken::MathImport(mi) => {
                let parent = *stack.last().unwrap_or(&chapter.root());
                let mathml = mi
                    .mathml_ref
                    .as_ref()
                    .and_then(|r| content_lookup(&r.name, r.index));
                let alttext = mi
                    .alttext_ref
                    .as_ref()
                    .and_then(|r| content_lookup(&r.name, r.index))
                    .filter(|s| !s.trim().is_empty() && s != "no accessible name found.");
                if let Some(mut math) = mathml
                    .as_deref()
                    .and_then(crate::math::mathml::parse_math_str)
                {
                    if math.alttext.is_none() {
                        math.alttext = alttext;
                    }
                    let node_id = chapter.alloc_node(Node::new(Role::Math));
                    chapter.append_child(parent, node_id);
                    if let Some(style_name) = &mi.style_name
                        && let Some(styles_map) = styles
                        && let Some(kfx_props) = styles_map.get(style_name)
                    {
                        let style_id = chapter.styles.intern(kfx_style_to_ir(kfx_props));
                        if let Some(node) = chapter.node_mut(node_id) {
                            node.style = style_id;
                        }
                    }
                    if let Some(id) = mi.id {
                        chapter.semantics.set_id(node_id, &id.to_string());
                    }
                    chapter.math.insert(node_id, math);
                } else if let Some(text) = alttext {
                    // Unparseable/absent MathML: keep the readable text.
                    let range = chapter.append_text(&text);
                    let text_node = chapter.alloc_node(Node::text(range));
                    chapter.append_child(parent, text_node);
                }
            }
        }
    }

    chapter
}

/// Apply semantic attributes to a node from a generic map.
///
/// This is the **only place** that knows about SemanticTarget → IR mapping.
/// It's a simple dispatcher, not format-specific logic.
pub(super) fn apply_semantics_to_node(
    chapter: &mut Chapter,
    node_id: NodeId,
    semantics: &HashMap<SemanticTarget, String>,
) {
    for (target, value) in semantics {
        match target {
            SemanticTarget::Src => chapter.semantics.set_src(node_id, value),
            SemanticTarget::Href => {
                chapter.semantics.set_href(node_id, value);
            }
            SemanticTarget::Alt => chapter.semantics.set_alt(node_id, value),
            SemanticTarget::Id => chapter.semantics.set_id(node_id, value),
            SemanticTarget::EpubType => chapter.semantics.set_epub_type(node_id, value),
        }
    }
}

/// Convert KFX style properties to an IR ComputedStyle using the schema.
///
/// This is schema-driven: iterates schema rules with KFX symbol mappings,
/// applies inverse transforms to convert KFX values back to IR values.
pub(super) fn kfx_style_to_ir(props: &[(u64, IonValue)]) -> crate::style::ComputedStyle {
    use crate::kfx::style_schema::{StyleSchema, import_kfx_style};

    let schema = StyleSchema::standard();
    import_kfx_style(schema, props)
}

/// Build text nodes with inline spans applied.
///
/// The `doc_symbols` and `styles` parameters are used to resolve span styles:
/// - `doc_symbols`: resolves style symbol IDs to style names
/// - `styles`: maps style names to KFX style properties
pub(super) fn build_text_with_spans(
    chapter: &mut Chapter,
    parent: NodeId,
    text: &str,
    spans: &[SpanStart],
    doc_symbols: &[String],
    styles: Option<&HashMap<String, Vec<(u64, IonValue)>>>,
) {
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
    let create_span_node = |chapter: &mut Chapter, span: &SpanStart| -> NodeId {
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
        // Saturate: offset/length are bounded-but-untrusted, and out-of-range
        // char offsets are clamped to the text length by `char_to_byte_offset`.
        let span_end = span.offset.saturating_add(span.length);

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
pub(super) fn char_to_byte_offset(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(text.len())
}

// Helper functions

/// Resolve a symbol ID to its string representation.
pub(super) fn resolve_symbol(id: u64, doc_symbols: &[String]) -> Option<&str> {
    crate::kfx::container::resolve_symbol(id, doc_symbols)
}

/// Resolve a value that could be either a symbol or string.
pub(super) fn resolve_symbol_or_string(value: &IonValue, doc_symbols: &[String]) -> Option<String> {
    match value {
        IonValue::String(s) => Some(s.clone()),
        IonValue::Symbol(id) => resolve_symbol(*id, doc_symbols).map(|s| s.to_string()),
        _ => None,
    }
}

// High-level API (used by KfxImporter)

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
) -> Chapter
where
    F: FnMut(&str, usize) -> Option<String>,
{
    let tokens = tokenize_storyline(storyline, doc_symbols, anchors, styles);
    build_ir_from_tokens(&tokens, doc_symbols, styles, content_lookup)
}
