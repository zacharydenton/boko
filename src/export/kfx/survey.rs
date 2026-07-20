use super::*;

/// Survey a chapter during Pass 1.
///
/// This walks the IR tree to:
/// - Assign a fragment ID to this chapter
/// - Build position map entries for every node
/// - Intern all text and attribute strings
/// - Track text offsets for link resolution
///
/// NO ION GENERATION happens here.
pub(super) fn survey_chapter(
    chapter: &Chapter,
    chapter_id: ChapterId,
    source_path: &str,
    ctx: &mut ExportContext,
) {
    // Begin surveying this chapter (with source path for TOC resolution)
    let _fragment_id = ctx.begin_chapter_survey(chapter_id, source_path);

    // Walk the IR tree
    survey_node(chapter, chapter.root(), (1.0, 1.2), ctx);

    // End surveying
    ctx.end_chapter_survey();
}

/// Recursively survey a node and its children. `inherited` is the nearest
/// styled ancestor's (absolute font size, line-height in em of its font) —
/// text leaves often carry the default StyleId while their metrics live on
/// the paragraph node.
pub(super) fn survey_node(
    chapter: &Chapter,
    node_id: NodeId,
    inherited: (f32, f32),
    ctx: &mut ExportContext,
) {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return,
    };

    let metrics = if node.style == crate::style::StyleId::DEFAULT {
        inherited
    } else {
        chapter
            .styles
            .get(node.style)
            .map(|s| {
                let abs = s.font_size_abs.0;
                let line_em = match s.line_height {
                    crate::style::Length::Auto => 1.2,
                    crate::style::Length::Em(x) => x,
                    crate::style::Length::Percent(p) => p / 100.0,
                    crate::style::Length::Px(x) => x / 16.0 / abs.max(1e-6),
                    crate::style::Length::Rem(x) => x / abs.max(1e-6),
                };
                (abs, line_em)
            })
            .unwrap_or(inherited)
    };

    // Skip root node processing but walk children
    if node.role == Role::Root {
        for child in chapter.children(node_id) {
            survey_node(chapter, child, metrics, ctx);
        }
        return;
    }

    // Record position for this node (for link targets)
    ctx.record_position(node_id);

    // Note: Heading positions are recorded during Pass 2 in tokens_to_ion()
    // where actual content fragment IDs are available.
    // Anchor entities are created during Pass 2 using GlobalNodeId targets
    // from ResolvedLinks.

    // Register resources (src attributes) - creates short names like "e0"
    // Note: href and alt are used as string values, not symbols
    if let Some(src) = chapter.semantics.src(node_id) {
        ctx.resource_registry.register(src, &mut ctx.symbols);
    }

    // Track text content and advance offset
    if !node.text.is_empty() {
        let text = chapter.text(node.text);
        ctx.advance_text_offset(text.len());
        // Weight this text's metrics for body-size/leading normalization.
        ctx.record_text_metrics(metrics.0, metrics.1, text.len());
        // We don't need to intern plain text content
    }

    // Recurse into children
    for child in chapter.children(node_id) {
        survey_node(chapter, child, metrics, ctx);
    }
}

/// Register link targets from ResolvedLinks with the AnchorRegistry.
///
/// This walks all chapters and registers each link's target with the
/// anchor registry, mapping hrefs to their resolved targets (GlobalNodeId,
/// ChapterId, or external URL).
pub(super) fn register_link_targets(
    book: &Book,
    spine_info: &[(ChapterId, String)],
    resolved: &ResolvedLinks,
    ctx: &mut ExportContext,
) -> io::Result<()> {
    for (chapter_id, _) in spine_info {
        // Skip chapters that fail to load, like every other export stage
        // (survey, landmarks, spine building) does — one broken chapter must
        // not abort the whole export, and the position map has dedicated
        // handling for chapters that never loaded.
        let Ok(chapter) = book.load_chapter_cached(*chapter_id) else {
            continue;
        };
        register_chapter_link_targets(&chapter, *chapter_id, resolved, ctx);
    }
    Ok(())
}

/// Register link targets for a single chapter.
pub(super) fn register_chapter_link_targets(
    chapter: &Chapter,
    chapter_id: ChapterId,
    resolved: &ResolvedLinks,
    ctx: &mut ExportContext,
) {
    for node_id in chapter.iter_dfs() {
        let Some(node) = chapter.node(node_id) else {
            continue;
        };

        // Only process Link nodes
        if node.role != Role::Link {
            continue;
        }

        // Get the href attribute
        let Some(href) = chapter.semantics.href(node_id) else {
            continue;
        };

        let source = GlobalNodeId::new(chapter_id, node_id);

        // Look up the resolved target and register it
        if let Some(target) = resolved.get(source) {
            match target {
                AnchorTarget::Internal(target_node) => {
                    ctx.anchor_registry
                        .register_internal_target(*target_node, href);
                }
                AnchorTarget::Chapter(target_chapter) => {
                    ctx.anchor_registry
                        .register_chapter_target(*target_chapter, href);
                }
                AnchorTarget::External(url) => {
                    ctx.anchor_registry.register_external(url);
                }
            }
        }
    }
}
