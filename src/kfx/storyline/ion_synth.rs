use super::*;

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
                    let (outer_fields, outer_id) = start_element_fields(elem, ctx, true);

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
                    ctx.default_style_used = true;
                    inner_fields.push((sym!(Style), IonValue::Symbol(ctx.default_style_symbol)));

                    // Type: text - inner element holds the actual content
                    inner_fields.push((sym!(Type), IonValue::Symbol(KfxSymbol::Text as u64)));

                    // The semantic marker rides on the inner $269 text element:
                    // readers only consume `yj.semantics.*` keys on $269, so a
                    // marker on the $270 wrapper would be flagged (and ignored)
                    // as unexpected data.
                    if let Some(semantic_type) = semantic_type_for(elem) {
                        let field_id = ctx.symbols.get_or_intern("yj.semantics.type");
                        let value_id = ctx.symbols.get_or_intern(semantic_type);
                        inner_fields.push((field_id, IonValue::Symbol(value_id)));
                    }

                    // Push inner text builder and mark it as inner wrapper
                    // Store outer_id so anchors inside use the top-level container for navigation
                    let mut inner_builder = IonBuilder::with_fields(inner_fields, inner_id);
                    inner_builder.is_inner_wrapper_text = true;
                    inner_builder.outer_container_id = Some(outer_id);
                    stack.push(inner_builder);
                } else {
                    // === NORMAL ELEMENT PATH ===
                    let (fields, container_id) = start_element_fields(elem, ctx, false);
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

                // Create anchor for inline elements with IDs or that are link/TOC targets
                // For elements inside container wrappers, use the outer container's ID
                if let Some(node_id) = span.node_id
                    && let Some(parent) = stack.last()
                {
                    let has_id = span.get_semantic(SemanticTarget::Id).is_some();
                    let is_target = ctx.is_registered_target(node_id);
                    if has_id || is_target {
                        // Prefer outer_container_id (for wrapped elements) over container_id
                        let target_id = parent.outer_container_id.or(parent.container_id);
                        if let Some(container_id) = target_id {
                            ctx.create_anchor_if_needed(node_id, container_id, current_offset);
                        }
                    }
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

/// The `yj.semantics.type` marker value for an element, if its export
/// strategy carries one. A header cell overrides the strategy's "table_cell"
/// with "table_header_cell" so the th/td distinction survives the round trip.
fn semantic_type_for(elem: &ElementStart) -> Option<&'static str> {
    if elem.is_header_cell {
        Some("table_header_cell")
    } else {
        schema()
            .export_strategy(elem.role)
            .and_then(|s| s.semantic_type())
    }
}

/// Build the Ion field list for a `StartElement` token.
///
/// Shared by both `tokens_to_ion` paths: the container-wrapper path (outer
/// element) and the normal element path emit the same field sequence and
/// context side effects; they differ only in the `type` field:
/// - `container_wrapper == true`: `type: container` + `layout: vertical`
///   (required for borders to render)
/// - `container_wrapper == false`: the schema's KFX type for the role
///
/// Returns the field list and the freshly assigned container ID.
fn start_element_fields(
    elem: &ElementStart,
    ctx: &mut ExportContext,
    container_wrapper: bool,
) -> (Vec<(u64, IonValue)>, u64) {
    let mut fields = Vec::new();

    // Unique container ID - use the global generator to avoid collisions
    let container_id = ctx.fragment_ids.next_id();
    fields.push((sym!(Id), IonValue::Int(container_id as i64)));

    // Record this content ID for position_map (so navigation targets are resolvable)
    ctx.record_content_id(container_id);

    // Create chapter-start anchor with first content fragment ID (if pending)
    ctx.resolve_pending_chapter_anchor(container_id);

    // Create fragment-based anchor if this element is a link/TOC target
    // Note: Kindle expects offset: 0 for all navigation entries (per reference KFX)
    // Check both: elements with IDs AND elements that are registered targets (for TOC)
    if let Some(node_id) = elem.node_id {
        let has_id = elem.get_semantic(SemanticTarget::Id).is_some();
        let is_target = ctx.is_registered_target(node_id);
        if has_id || is_target {
            ctx.create_anchor_if_needed(node_id, container_id, 0);
        }
    }

    // Style reference - use per-element style if available, else default
    // Required for text rendering on Kindle
    let style_sym = elem.style_symbol.unwrap_or(ctx.default_style_symbol);
    if style_sym == ctx.default_style_symbol {
        ctx.default_style_used = true;
    }
    fields.push((sym!(Style), IonValue::Symbol(style_sym)));

    if container_wrapper {
        // Type: container (not text) - this is key for borders to render
        fields.push((sym!(Type), IonValue::Symbol(KfxSymbol::Container as u64)));

        // Layout: vertical (required for container)
        fields.push((sym!(Layout), IonValue::Symbol(KfxSymbol::Vertical as u64)));
    } else if let Some(kfx_type) = schema().kfx_type_for_role(elem.role) {
        // Type field (as symbol ID)
        fields.push((sym!(Type), IonValue::Symbol(kfx_type as u64)));
    }

    // Add semantic type annotation if the strategy specifies one
    // (e.g., BlockQuote → yj.semantics.type: block_quote). A header cell
    // overrides the strategy's "table_cell" with "table_header_cell" so the
    // th/td distinction survives the round trip.
    //
    // When this element is border-wrapped, the marker is emitted on the inner
    // $269 text element instead (see tokens_to_ion): readers only consume
    // `yj.semantics.*` keys on $269, never on the $270 wrapper container.
    let semantic_type = if container_wrapper {
        None
    } else {
        semantic_type_for(elem)
    };
    if let Some(semantic_type) = semantic_type {
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

    // (layout_hints ride the element's *style*, not the content node —
    // reference KFX puts treat_as_title/figure/caption in style structs;
    // see layout_hint_for in export.rs.)

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
        // Note: epub:type sidebar/marginalia gets no yj.classification.
        // Kindle Previewer never emits yj.sidenote ($620) — the sidebar-ness
        // is carried by the element's `yj.semantics.type: sidebar` marker
        // instead (see the schema's Sidebar strategy).

        // Prefer endnote classification if both are present (common in EPUBs)
        if is_endnote {
            fields.push((
                sym!(YjClassification),
                IonValue::Symbol(KfxSymbol::YjEndnote as u64),
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
        // Symbol-vs-string is decided by the FIELD, not the value: reference
        // fields (resource_name, link_to) are interned symbols; the span/
        // start fields are integers; everything else (alt text, titles) is a
        // plain string. Sniffing the value used to intern prose like
        // alt="black/white photo" into the symbol table.
        let is_symbol_field = *field_id == sym!(ResourceName) || *field_id == sym!(LinkTo);
        let is_int_field = *field_id == sym!(TableColumnSpan)
            || *field_id == sym!(TableRowSpan)
            || *field_id == sym!(ListStartOffset);

        if is_symbol_field {
            let sym_id = ctx.symbols.get_or_intern(value_str);
            fields.push((*field_id, IonValue::Symbol(sym_id)));
        } else if is_int_field {
            if let Ok(n) = value_str.parse::<i64>() {
                fields.push((*field_id, IonValue::Int(n)));
            }
        } else {
            fields.push((*field_id, IonValue::String(value_str.clone())));
        }
    }

    (fields, container_id)
}

/// Builder for constructing Ion structures from tokens.
pub(super) struct IonBuilder {
    pub(super) fields: Vec<(u64, IonValue)>,
    pub(super) children: Vec<IonValue>,
    /// Accumulated text content for this element (concatenated during build)
    pub(super) accumulated_text: String,
    /// Character count of accumulated text (for style event offsets)
    /// KFX uses character offsets, not byte offsets
    pub(super) accumulated_char_count: usize,
    /// Collected style events for this element (inline spans)
    pub(super) style_events: Vec<IonValue>,
    /// Container ID for this element (set during StartElement, used for length tracking)
    pub(super) container_id: Option<u64>,
    /// True if this is an inner text element inside a container wrapper.
    /// When EndElement is reached for this builder, we need an extra EndElement
    /// to close the outer container.
    pub(super) is_inner_wrapper_text: bool,
    /// For inner wrapper text elements, stores the outer container's ID.
    /// Anchors inside wrapped elements should use this ID for correct TOC navigation.
    pub(super) outer_container_id: Option<u64>,
}

impl IonBuilder {
    pub(super) fn new() -> Self {
        Self {
            fields: Vec::new(),
            children: Vec::new(),
            accumulated_text: String::new(),
            accumulated_char_count: 0,
            style_events: Vec::new(),
            container_id: None,
            is_inner_wrapper_text: false,
            outer_container_id: None,
        }
    }

    pub(super) fn with_fields(fields: Vec<(u64, IonValue)>, container_id: u64) -> Self {
        Self {
            fields,
            children: Vec::new(),
            accumulated_text: String::new(),
            accumulated_char_count: 0,
            style_events: Vec::new(),
            container_id: Some(container_id),
            is_inner_wrapper_text: false,
            outer_container_id: None,
        }
    }

    pub(super) fn add_child(&mut self, child: IonValue) {
        self.children.push(child);
    }

    /// Append text to this element's accumulated content.
    /// Returns the character offset where this text starts (for span tracking).
    /// KFX style events use character offsets, not byte offsets.
    pub(super) fn append_text(&mut self, text: &str) -> usize {
        let offset = self.accumulated_char_count;
        self.accumulated_text.push_str(text);
        self.accumulated_char_count += text.chars().count();
        offset
    }

    /// Get the current accumulated text length in characters.
    /// KFX style events use character offsets, not byte offsets.
    pub(super) fn text_len(&self) -> usize {
        self.accumulated_char_count
    }

    /// Add a style event (inline span) to this element.
    ///
    /// Converts SpanStart into KFX style_event Ion struct:
    /// { offset: N, length: N, style: $symbol [, link_to: $anchor] }
    pub(super) fn add_style_event(&mut self, span: SpanStart, ctx: &mut ExportContext) {
        let mut event_fields = Vec::new();

        // Offset and length (required)
        event_fields.push((sym!(Offset), IonValue::Int(span.offset as i64)));
        event_fields.push((sym!(Length), IonValue::Int(span.length as i64)));

        // Style reference (required for rendering)
        let style_sym = span.style_symbol.unwrap_or(ctx.default_style_symbol);
        if style_sym == ctx.default_style_symbol {
            ctx.default_style_used = true;
        }
        event_fields.push((sym!(Style), IonValue::Symbol(style_sym)));

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
    pub(super) fn build(mut self, ctx: &mut ExportContext) -> IonValue {
        // KFX storylines are flat lists of elements, not nested structs
        // Each element is a struct with type, content reference, and possibly nested content_list
        if !self.fields.is_empty() {
            // Record text length for this content ID (used by location_map)
            // Must use char count, not byte count, since location_map divides by characters
            if let Some(container_id) = self.container_id {
                ctx.record_content_length(container_id, self.accumulated_char_count);
            }

            // If this element has accumulated text, create ONE content reference
            // Skip if the only content is zero-width spaces (anchor markers from empty ID elements)
            // These interfere with image display when mixed with image children
            let has_real_text = self.accumulated_text.chars().any(|c| c != '\u{200B}');

            // Dropping marker-only text orphans any anchor offset that was
            // counted against it (a second anchor in the same empty run sits
            // at offset 1 past nothing): clamp them back to the element
            // start, which is where an empty target resolves.
            if !has_real_text
                && self.children.is_empty()
                && let Some(container_id) = self.container_id
            {
                ctx.anchor_registry.clamp_offsets_at(container_id);
            }
            if has_real_text {
                let (content_name, content_idx) = ctx.append_text(&self.accumulated_text);
                let content_ref = IonValue::Struct(vec![
                    (sym!(Name), IonValue::Symbol(content_name)),
                    (sym!(Index), IonValue::Int(content_idx as i64)),
                ]);
                self.fields.push((sym!(Content), content_ref));
            }

            // Add style_events if any inline spans were collected AND there's real text
            // (style_events reference character offsets in the content, so skip if no content)
            if !self.style_events.is_empty() && has_real_text {
                self.fields
                    .push((sym!(StyleEvents), IonValue::List(self.style_events)));
            }

            // Add nested children as content_list if present
            if !self.children.is_empty() {
                // Block-children typing rule: a text-typed element may hold
                // inline-class children (text runs, images) but never block
                // ones — Previewer output has no $269 parents with list or
                // table children; such parents are containers.
                let has_block_child = self.children.iter().any(|child| {
                    let IonValue::Struct(fields) = child else {
                        return false;
                    };
                    fields.iter().any(|(k, v)| {
                        *k == sym!(Type)
                            && matches!(v, IonValue::Symbol(t)
                                if *t != KfxSymbol::Text as u64 && *t != KfxSymbol::Image as u64)
                    })
                });
                if has_block_child && !has_real_text {
                    let mut promoted = false;
                    for (k, v) in self.fields.iter_mut() {
                        if *k == sym!(Type)
                            && matches!(v, IonValue::Symbol(t) if *t == KfxSymbol::Text as u64)
                        {
                            *v = IonValue::Symbol(KfxSymbol::Container as u64);
                            promoted = true;
                        }
                    }
                    // Containers admit no semantic markers: readers only
                    // consume `yj.semantics.*` on $269 text elements and
                    // flag them as unexpected data on $270. Previewer
                    // likewise renders block-holding asides/quotes as plain
                    // styled containers, so drop the marker with the type.
                    if promoted {
                        let marker_id = ctx.symbols.get_or_intern("yj.semantics.type");
                        self.fields.retain(|(k, _)| *k != marker_id);
                    }
                }
                self.fields
                    .push((sym!(ContentList), IonValue::List(self.children)));
            }

            IonValue::Struct(self.fields)
        } else {
            // Root level: return the children as the storyline content_list.
            // An empty chapter must still yield an empty list — a null
            // content_list is rejected by KFX consumers ("unknown
            // content_list data type: NoneType").
            IonValue::List(self.children)
        }
    }
}
