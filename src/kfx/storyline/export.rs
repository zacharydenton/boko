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

/// The layout hint a node's style should carry (`layout_hints:
/// [treat_as_title]` etc). Reference KFX puts these in style structs, not on
/// content nodes; they affect Kindle's rendering of headings, figures and
/// captions.
fn layout_hint_for(chapter: &Chapter, node_id: NodeId, role: Role) -> Option<KfxSymbol> {
    match role {
        Role::Heading(_) => Some(KfxSymbol::TreatAsTitle),
        Role::Figure => Some(KfxSymbol::Figure),
        Role::Caption => Some(KfxSymbol::Caption),
        _ => {
            let epub_type = chapter.semantics.epub_type(node_id)?;
            let has_title_type = epub_type.split_whitespace().any(|t| {
                matches!(
                    t,
                    "title" | "fulltitle" | "subtitle" | "covertitle" | "halftitle"
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
        }
    }
}

/// The first authored link color inside the node's inline flow, packed as
/// ARGB. Reference output carries it on the containing block as
/// link_unvisited_style/link_visited_style; recursion stops at nested
/// blocks (they carry their own).
fn link_color_for(chapter: &Chapter, node_id: NodeId) -> Option<u32> {
    fn scan(chapter: &Chapter, id: NodeId, depth: u32) -> Option<u32> {
        let node = chapter.node(id)?;
        let inline = matches!(
            node.role,
            Role::Link | Role::Inline | Role::Text | Role::Break
        );
        if depth > 0 && !inline {
            return None;
        }
        if node.role == Role::Link
            && let Some(style) = chapter.styles.get(node.style)
            && let Some(c) = style.color
            // Black links are default ink; reference output only carries
            // actually-colored links (black would break night mode).
            && (c.r, c.g, c.b) != (0, 0, 0)
        {
            return Some(
                ((c.a as u32) << 24) | ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32,
            );
        }
        chapter
            .children(id)
            .find_map(|child| scan(chapter, child, depth + 1))
    }
    scan(chapter, node_id, 0)
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

    // Pre-compute static margin collapsing: the Kindle renderer does not
    // collapse adjoining margins, so the collapsed values are baked into
    // per-position style overrides (see collapse.rs).
    let adjust = super::collapse::compute_margin_collapse(chapter);

    walk_node_for_export(
        chapter,
        chapter.root(),
        crate::style::StyleId::DEFAULT,
        &adjust,
        sch,
        ctx,
        &mut stream,
    );
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
    parent_style: crate::style::StyleId,
    adjust: &super::collapse::MarginAdjustMap,
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
            walk_node_for_export(chapter, child, parent_style, adjust, sch, ctx, stream);
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
        emit_definition_list(chapter, node_id, parent_style, adjust, sch, ctx, stream);
        return;
    }

    // Inline elements (Link, Inline): use the flattening algorithm.
    // This produces non-overlapping style_events where each text segment
    // carries the accumulated state from all ancestors.
    if node.role == Role::Link || node.role == Role::Inline {
        emit_inline_content_flat(chapter, node_id, parent_style, sch, ctx, stream);
        return;
    }

    // Math: emit a classified `container` carrying the source MathML and a
    // spoken alt_text as annotations, plus a readable linearization as its
    // content. On firmware with Enhanced-Typesetting math (≈5.18.2+) the
    // MathML renders live; older readers show the text fallback. KVG glyph
    // rendering (the on-device vector form) is a deferred spoke.
    if node.role == Role::Math {
        if let Some(math) = chapter.math.get(&node_id) {
            let text = math.to_text();
            let mathml = crate::math::mathml::to_mathml(math);
            if !mathml.is_empty() || !text.is_empty() {
                let alttext = math.alttext.clone().unwrap_or_else(|| text.clone());
                let style_symbol =
                    ctx.register_style_id(node.style, parent_style, &chapter.styles);
                stream.push(KfxToken::Math(Box::new(crate::kfx::tokens::MathToken {
                    mathml,
                    alttext,
                    text,
                    display: math.display,
                    style_symbol: Some(style_symbol),
                    node_id: Some(node_id),
                })));
            }
        }
        return;
    }

    // Build element start with semantics
    let mut elem = ElementStart::new(node.role);
    elem.node_id = Some(node_id);

    if node.role == Role::Table {
        // Drives the yj_table / yj_table_viewer content features ($585).
        ctx.has_tables = true;
    }

    // Register the node's style and get a KFX style symbol
    // This converts IR ComputedStyle → KFX style and deduplicates
    let hint = layout_hint_for(chapter, node_id, node.role);
    let adj = adjust.get(&node_id).copied().unwrap_or_default();
    let link_color = link_color_for(chapter, node_id);
    // Dropcap paragraphs suppress their leading span's float/large font.
    let is_dropcap = chapter
        .styles
        .get(node.style)
        .is_some_and(|s| s.dropcap_chars > 0);
    let style_symbol = if hint.is_some() || !adj.is_identity() || link_color.is_some() {
        ctx.register_style_id_adjusted(
            node.style,
            parent_style,
            &chapter.styles,
            adj,
            hint,
            link_color,
        )
    } else {
        ctx.register_style_id(node.style, parent_style, &chapter.styles)
    };
    elem.style_symbol = Some(style_symbol);

    // Check if this element needs container wrapping for borders to render
    // KFX requires type: container with nested type: text for borders.
    //
    // Table-structural elements are exempt: the wrapper replaces the element
    // type with `container`, which destroys the table/table_row/cell
    // structure (a bordered table degraded to nested containers). Kindle
    // Previewer keeps table element types and renders their borders from
    // styles directly.
    //
    // Images are exempt too: the wrapper assumes text content — its inner
    // element is `type: text` — so a bordered image was swallowed entirely,
    // leaving resource_name/alt_text stranded on a childless container.
    // Border styling stays on the image element itself.
    let is_table_structural = matches!(
        node.role,
        Role::Table
            | Role::TableRow
            | Role::TableCell
            | Role::TableHead
            | Role::TableBody
            | Role::Image
    );
    elem.needs_container_wrapper = !is_table_structural
        && chapter
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

    // Table cell spans and ordered-list start value aren't in the schema's
    // string-valued AttrRule table (they must be emitted as Ion integers),
    // so carry them through kfx_attrs by hand; `tokens_to_ion` emits these
    // three symbols as Int. Without this, tables lose merged/spanned cells
    // and `<ol start=N>` loses its numbering on KFX export.
    if node.role == Role::TableCell {
        if let Some(cols) = chapter.semantics.col_span(node_id)
            && cols > 1
        {
            kfx_attrs.push((sym!(TableColumnSpan), cols.to_string()));
        }
        if let Some(rows) = chapter.semantics.row_span(node_id)
            && rows > 1
        {
            kfx_attrs.push((sym!(TableRowSpan), rows.to_string()));
        }
        elem.is_header_cell = chapter.semantics.is_header_cell(node_id);
    }
    if node.role == Role::OrderedList
        && let Some(start) = chapter.semantics.list_start(node_id)
        && start != 1
    {
        kfx_attrs.push((sym!(ListStartOffset), start.to_string()));
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

    let run_style_symbol = elem.style_symbol;
    stream.push(KfxToken::StartElement(elem));

    // Arm dropcap suppression for this block's first inline run.
    if is_dropcap {
        ctx.dropcap_suppress = true;
    }

    // Reference content model: an element never carries BOTH its own text
    // and element children. When inline flow (text, breaks, links, spans)
    // interleaves with element children (images, nested blocks), each run of
    // inline flow is wrapped in its own text element with its own content
    // ref — Kindle Previewer's encoding. The hybrid shape (one ref plus
    // children) mis-accounts reading positions and never occurs in
    // Amazon-produced books.
    // Math is NOT inline flow: it now emits its own classified `container`
    // (with MathML/alt_text annotations), so it must sit as a sibling element
    // in the parent's content_list — closing any open inline run before it,
    // exactly like an inline image. KP nests math directly in a `type: text`
    // content_list; boko's split-run model places it between text-run wrappers.
    let is_inline_flow = |role: Role| {
        matches!(role, Role::Text | Role::Break | Role::Link | Role::Inline)
    };
    let children: Vec<NodeId> = chapter.children(node_id).collect();
    let has_own_text = !node.text.is_empty() && !chapter.text(node.text).is_empty();
    let has_flow = has_own_text
        || children
            .iter()
            .any(|&c| chapter.node(c).is_some_and(|n| is_inline_flow(n.role)));
    let has_elements = children
        .iter()
        .any(|&c| chapter.node(c).is_some_and(|n| !is_inline_flow(n.role)));

    if !(has_flow && has_elements) {
        // Uniform content: emit text and children directly (single-ref or
        // pure-children element).
        if !node.text.is_empty() {
            let text = chapter.text(node.text);
            if !text.is_empty() {
                stream.push(KfxToken::Text(text.to_string()));
            }
        }
        for child in children {
            walk_node_for_export(chapter, child, node.style, adjust, sch, ctx, stream);
        }
    } else {
        // Mixed content: wrap each contiguous inline-flow run in a text
        // element. Inline-flow nodes emit tokens without creating elements,
        // so recursion inside the run wrapper does the right thing.
        let mut run_open = false;
        let open_run = |stream: &mut TokenStream, run_open: &mut bool| {
            if !*run_open {
                let mut run = ElementStart::new(Role::Text);
                run.style_symbol = run_style_symbol;
                stream.push(KfxToken::StartElement(run));
                *run_open = true;
            }
        };
        let close_run = |stream: &mut TokenStream, run_open: &mut bool| {
            if *run_open {
                stream.push(KfxToken::EndElement);
                *run_open = false;
            }
        };

        if has_own_text {
            open_run(stream, &mut run_open);
            stream.push(KfxToken::Text(chapter.text(node.text).to_string()));
        }
        for child in children {
            let child_is_flow = chapter.node(child).is_some_and(|n| is_inline_flow(n.role));
            if child_is_flow {
                open_run(stream, &mut run_open);
            } else {
                close_run(stream, &mut run_open);
            }
            walk_node_for_export(chapter, child, node.style, adjust, sch, ctx, stream);
        }
        close_run(stream, &mut run_open);
    }

    // Clear any unconsumed dropcap arming (e.g. a dropcap block whose first
    // run was plain text) so it can't leak into a later block.
    ctx.dropcap_suppress = false;

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
        // MATH inside an inline element (e.g. `<span>… <math/> …</span>`):
        // a math container cannot be nested mid-style-event, so emit the
        // equation's readable linearization as an inline text segment rather
        // than dropping it. (Math at block level gets the full container with
        // annotations via walk_node_for_export.)
        Role::Math => {
            if let Some(m) = chapter.math.get(&node_id) {
                let text = m.to_text();
                if !text.is_empty() {
                    segments.push(FlatSegment {
                        text,
                        state: effective_state,
                    });
                }
            }
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
    parent_style: crate::style::StyleId,
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

            // Set style (innermost). Inline runs use the inline projection:
            // block-only properties (box_align) never ride style_events. The
            // first run of a dropcap paragraph additionally drops its float
            // and large font (the native dropcap replaces them).
            if let Some(style_id) = segment.state.style {
                let suppress = ctx.dropcap_suppress;
                ctx.dropcap_suppress = false;
                let style_symbol = ctx.register_inline_style_id_inner(
                    style_id,
                    parent_style,
                    &chapter.styles,
                    suppress,
                );
                span.style_symbol = Some(style_symbol);
            }

            // Build KFX attributes
            let mut kfx_attrs = crate::kfx::tokens::KfxAttrs::new();

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
    parent_style: crate::style::StyleId,
    adjust: &super::collapse::MarginAdjustMap,
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
    let dl_style = ctx.register_style_id(node.style, parent_style, &chapter.styles);
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
                ctx.register_style_id(dd_style_id, node.style, &chapter.styles)
            } else {
                ctx.default_style_symbol
            };
            if wrapper_style == ctx.default_style_symbol {
                ctx.default_style_used = true;
            }
            wrapper_elem.style_symbol = Some(wrapper_style);
            stream.push(KfxToken::StartElement(wrapper_elem));

            // Emit the dt as a Container (with float:left style)
            // Use DefinitionTerm role since it maps to KfxSymbol::Container
            let dt_style = ctx.register_style_id(child.style, node.style, &chapter.styles);
            let mut dt_elem = ElementStart::new(Role::DefinitionTerm);
            dt_elem.style_symbol = Some(dt_style);
            stream.push(KfxToken::StartElement(dt_elem));

            // Emit dt's children wrapped in a Paragraph (like KPR). Inline
            // flow among the dt's children must be run-wrapped or the inner
            // Paragraph becomes a hybrid (ref + element children) — the shape
            // the reference model forbids. A dt like `<dt>Move to a point
            // <math>…</math></dt>` mixes text with a math container, so it
            // needs the same discipline as the dd path below.
            let mut dt_inner = ElementStart::new(Role::Paragraph);
            dt_inner.style_symbol = Some(dt_style);
            stream.push(KfxToken::StartElement(dt_inner));

            {
                let is_inline_flow = |role: Role| {
                    matches!(role, Role::Text | Role::Break | Role::Link | Role::Inline)
                };
                let mut run_open = false;
                for dt_child in chapter.children(child_id) {
                    let child_is_flow = chapter
                        .node(dt_child)
                        .is_some_and(|n| is_inline_flow(n.role));
                    if child_is_flow && !run_open {
                        let mut run = ElementStart::new(Role::Text);
                        run.style_symbol = Some(dt_style);
                        stream.push(KfxToken::StartElement(run));
                        run_open = true;
                    } else if !child_is_flow && run_open {
                        stream.push(KfxToken::EndElement);
                        run_open = false;
                    }
                    walk_node_for_export(chapter, dt_child, child.style, adjust, sch, ctx, stream);
                }
                if run_open {
                    stream.push(KfxToken::EndElement);
                }
            }

            stream.push(KfxToken::EndElement); // end dt inner Paragraph
            stream.push(KfxToken::EndElement); // end dt Container

            // Emit the paired dd content. The wrapper already holds the dt
            // element child, so any inline flow among the dd's children must
            // be run-wrapped or the wrapper becomes a hybrid element
            // (ref + children) — the shape the reference model forbids.
            if let Some((dd_id, dd_style_id)) = dd_info {
                let is_inline_flow = |role: Role| {
                    matches!(role, Role::Text | Role::Break | Role::Link | Role::Inline)
                };
                let mut run_open = false;
                for dd_child in chapter.children(dd_id) {
                    let child_is_flow = chapter
                        .node(dd_child)
                        .is_some_and(|n| is_inline_flow(n.role));
                    if child_is_flow && !run_open {
                        let mut run = ElementStart::new(Role::Text);
                        run.style_symbol = Some(wrapper_style);
                        stream.push(KfxToken::StartElement(run));
                        run_open = true;
                    } else if !child_is_flow && run_open {
                        stream.push(KfxToken::EndElement);
                        run_open = false;
                    }
                    walk_node_for_export(chapter, dd_child, dd_style_id, adjust, sch, ctx, stream);
                }
                if run_open {
                    stream.push(KfxToken::EndElement);
                }

                i += 1; // Skip the dd, we've processed it
            }

            // End the wrapper
            stream.push(KfxToken::EndElement);
        } else {
            // Non-dt child (orphan dd or other), emit normally
            walk_node_for_export(chapter, child_id, node.style, adjust, sch, ctx, stream);
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
    parent_style: crate::style::StyleId,
    sch: &crate::kfx::schema::KfxSchema,
    ctx: &mut ExportContext,
    stream: &mut TokenStream,
) {
    // Flatten the inline subtree into segments with computed state
    let mut segments = Vec::new();
    flatten_inline_content(chapter, node_id, InlineState::default(), &mut segments);

    // Convert segments to tokens
    emit_flattened_segments(segments, chapter, parent_style, sch, ctx, stream);
}
