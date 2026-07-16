use super::*;

/// Check if a style has borders that require container wrapping in KFX.
///
/// KFX requires block elements with borders to be wrapped in a `type: container`
/// with nested `type: text` for the content. Without this wrapper, borders don't
/// render on Kindle devices.
pub(super) fn needs_container_wrapper(style: &ComputedStyle) -> bool {
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
pub fn ir_to_tokens(chapter: &Chapter, ctx: &mut ExportContext) -> TokenStream {
    let sch = schema();
    let mut stream = TokenStream::new();

    // The style memo is keyed by chapter-local StyleId; this chapter's pool is
    // about to be the one in scope, so drop any prior chapter's entries. The
    // shipped export path also clears via begin_chapter, but clearing here
    // keeps the public build_storyline_ion/ir_to_tokens entry points safe when
    // one ExportContext is reused across chapters.
    ctx.reset_style_memo();

    walk_node_for_export(chapter, chapter.root(), sch, ctx, &mut stream);
    stream
}

/// Walk a node and emit tokens for export.
///
/// Uses schema-driven attribute export (FIX for Issue #2: Attribute Hardcoding).
/// Inline roles (Link, Inline) are emitted as StartSpan/EndSpan instead of
/// StartElement/EndElement, enabling proper style_events generation.
pub(super) fn walk_node_for_export(
    chapter: &Chapter,
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

    // Build element start with semantics
    let mut elem = ElementStart::new(node.role);
    elem.node_id = Some(node_id);

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
            let anchor_symbol = ctx.anchor_registry.get_or_create_href_symbol(value);
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

// Inline Content Flattening ("Push Down, Emit at Bottom")
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
pub(super) struct InlineState {
    /// Active link target (from Link ancestor), as anchor symbol string
    pub(super) link_to: Option<String>,
    /// Active style (innermost wins)
    pub(super) style: Option<crate::style::StyleId>,
    /// Active epub:type for noteref detection
    pub(super) epub_type: Option<String>,
    /// Active element ID (for anchor creation)
    pub(super) element_id: Option<String>,
    /// Active node ID (for anchor creation with GlobalNodeId)
    pub(super) node_id: Option<NodeId>,
}

/// A flattened text segment with its computed state.
pub(super) struct FlatSegment {
    pub(super) text: String,
    pub(super) state: InlineState,
}

/// Flatten inline content into segments with computed state.
///
/// This is the "Push Down, Emit at Bottom" algorithm:
/// - Traverse the tree depth-first
/// - Accumulate state (link_to, style) as we go down
/// - Only emit segments when we hit Text leaves
pub(super) fn flatten_inline_content(
    chapter: &Chapter,
    node_id: NodeId,
    state: InlineState,
    segments: &mut Vec<FlatSegment>,
) {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return,
    };

    // MERGE STATE: Calculate effective state for this node
    // Track both element_id (string) and node_id (for GlobalNodeId lookup)
    let has_id = chapter.semantics.id(node_id).is_some();
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
        // Element ID: for anchor creation (string ID)
        element_id: chapter
            .semantics
            .id(node_id)
            .map(|s| s.to_string())
            .or(state.element_id),
        // Node ID: track which node has the ID (for GlobalNodeId lookup)
        node_id: if has_id { Some(node_id) } else { state.node_id },
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
            let children: Vec<_> = chapter.children(node_id).collect();
            if children.is_empty() && effective_state.element_id.is_some() {
                // Empty element with ID (anchor marker) - emit zero-width space to carry the ID
                segments.push(FlatSegment {
                    text: "\u{200B}".to_string(), // Zero-width space
                    state: effective_state,
                });
            } else {
                for child_id in children {
                    flatten_inline_content(chapter, child_id, effective_state.clone(), segments);
                }
            }
        }
    }
}

/// Convert flattened segments into KfxTokens (Text + style info for style_events).
///
/// This emits the text and creates SpanStart markers that will become style_events.
pub(super) fn emit_flattened_segments(
    segments: Vec<FlatSegment>,
    chapter: &Chapter,
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
                let anchor_symbol = ctx.anchor_registry.get_or_create_href_symbol(href);
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

            // Store element ID and node_id for anchor creation
            if let Some(ref id) = segment.state.element_id {
                span.set_semantic(SemanticTarget::Id, id.clone());
            }
            span.node_id = segment.state.node_id;

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
pub(super) fn emit_definition_list(
    chapter: &Chapter,
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
pub(super) fn emit_inline_content_flat(
    chapter: &Chapter,
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
