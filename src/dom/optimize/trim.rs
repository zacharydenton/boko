//! Pass: trim whitespace at block edges.
//!
//! CSS `white-space: normal` removes whitespace at the very start and end of
//! a block box. Markup like `<p> <span>Title</span> </p>` (calibre and many
//! EPUB toolchains indent inline content) therefore renders as `Title`, not
//! ` Title `. boko preserved the edge spaces, which added stray leading
//! space to headings and — because an inline run's `style_event` offset is
//! measured from the block's text — shifted every styled run by one
//! character.
//!
//! This trims leading whitespace off the block's first text run(s) and
//! trailing whitespace off its last, descending through inline wrappers but
//! stopping at nested block children (each block trims its own edges).

use crate::model::{Chapter, NodeId, Role, TextRange};

use super::pass::walk_bottom_up;
use super::predicates::is_inline_role;

/// Trim block-edge whitespace across the chapter.
pub fn trim_block_edges(chapter: &mut Chapter) {
    walk_bottom_up(chapter, |chapter, id| {
        trim_one(chapter, id);
    });
}

/// Block-level containers whose inline content has trimmed edges.
fn is_edge_trimmed_block(role: Role) -> bool {
    matches!(
        role,
        Role::Paragraph
            | Role::Heading(_)
            | Role::Caption
            | Role::ListItem
            | Role::DefinitionTerm
            | Role::DefinitionDescription
            | Role::BlockQuote
            | Role::TableCell
            | Role::Container
            | Role::Sidebar
            | Role::Footnote
    )
}

fn trim_one(chapter: &mut Chapter, block_id: NodeId) {
    let Some(node) = chapter.node(block_id) else {
        return;
    };
    if !is_edge_trimmed_block(node.role) {
        return;
    }
    // Preformatted content keeps its whitespace; only reached via role, but
    // guard the pre/code path explicitly.
    if node.role == Role::CodeBlock {
        return;
    }

    let leaves = leading_text_leaves(chapter, block_id);
    // Trim leading whitespace from the front leaves until real text is hit.
    for &id in &leaves {
        if trim_leading(chapter, id) {
            break; // this leaf has non-whitespace content: stop
        }
    }
    // Trim trailing whitespace from the back leaves.
    for &id in leaves.iter().rev() {
        if trim_trailing(chapter, id) {
            break;
        }
    }
}

/// Ordered text-leaf descendants reachable through inline flow (stopping at
/// nested block children). The order is document order, so the first is the
/// block's leading text and the last is its trailing text.
fn leading_text_leaves(chapter: &Chapter, block_id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect(chapter, block_id, true, &mut out);
    out
}

fn collect(chapter: &Chapter, id: NodeId, is_root: bool, out: &mut Vec<NodeId>) {
    let Some(node) = chapter.node(id) else {
        return;
    };
    // Don't descend into nested block children — they trim their own edges.
    if !is_root && !is_inline_role(node.role) {
        return;
    }
    if node.role == Role::Text {
        out.push(id);
        return;
    }
    // A break resets the "edge": text after a <br> is a new line start. For
    // simplicity we still collect across it; edge trimming of the block's
    // outermost text is the goal and breaks are rare at block edges.
    for child in chapter.children(id) {
        collect(chapter, child, false, out);
    }
}

/// Trim leading ASCII whitespace from a text node. Returns `true` when the
/// node still holds non-whitespace text afterwards (the block edge is
/// resolved), `false` when it was entirely whitespace.
fn trim_leading(chapter: &mut Chapter, id: NodeId) -> bool {
    let range = match chapter.node(id) {
        Some(n) if n.role == Role::Text => n.text,
        _ => return false,
    };
    let text = chapter.text(range);
    let trimmed = text.trim_start();
    let removed = text.len() - trimmed.len();
    let has_content = !trimmed.is_empty();
    if removed > 0 {
        let new = TextRange::new(range.start + removed as u32, range.len - removed as u32);
        if let Some(n) = chapter.node_mut(id) {
            n.text = new;
        }
    }
    has_content
}

/// Trim trailing ASCII whitespace. Returns `true` when non-whitespace text
/// remains.
fn trim_trailing(chapter: &mut Chapter, id: NodeId) -> bool {
    let range = match chapter.node(id) {
        Some(n) if n.role == Role::Text => n.text,
        _ => return false,
    };
    let text = chapter.text(range);
    let trimmed = text.trim_end();
    let removed = text.len() - trimmed.len();
    let has_content = !trimmed.is_empty();
    if removed > 0 {
        let new = TextRange::new(range.start, range.len - removed as u32);
        if let Some(n) = chapter.node_mut(id) {
            n.text = new;
        }
    }
    has_content
}
