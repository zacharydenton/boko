//! IR optimization passes.
//!
//! This module contains optimization passes that run after the initial
//! HTML-to-IR transform but before export. All passes follow the "Zipper"
//! principle: O(n) traversal, in-place mutation, stable NodeIds.
//!
//! ## Pipeline Order
//!
//! The passes run in this order for optimal results:
//!
//! 1. **Vacuum** - Remove structural whitespace noise
//! 2. **Span Merge** - Coalesce adjacent text with same style
//! 3. **List Fuser** - Merge fragmented lists
//! 4. **Pruner** - Remove empty containers (cascading)

use crate::ir::{IRChapter, NodeId, Role};

/// Run all optimization passes on a chapter.
///
/// Passes are ordered for maximum effectiveness:
/// 1. Vacuum removes whitespace noise
/// 2. Hoist dissolves redundant wrapper containers (enables span merge)
/// 3. Span merge coalesces adjacent text with same style
/// 4. List fuser repairs fragmented lists
/// 5. Pruner removes any containers emptied by previous passes
pub fn optimize(chapter: &mut IRChapter) {
    vacuum(chapter);
    hoist_nested_inlines(chapter);
    merge_adjacent_spans(chapter);
    fuse_lists(chapter);
    prune_empty(chapter);
}

// ============================================================================
// Pass 1: Vacuum (Structural Whitespace Culling)
// ============================================================================

/// Remove whitespace-only Text nodes that are structurally irrelevant.
///
/// HTML parsers treat indentation between tags as Text nodes:
/// ```html
/// <div>
///     <p>Text</p>
///     \n    <-- This becomes a Node!
/// </div>
/// ```
///
/// These nodes waste memory and can cause ghost elements in TUI rendering.
/// We delete them when:
/// - Role is Text
/// - Content is whitespace-only
/// - Parent is a block container (not inline, not preformatted)
fn vacuum(chapter: &mut IRChapter) {
    if chapter.node_count() > 0 {
        vacuum_children(chapter, NodeId::ROOT);
    }
}

fn vacuum_children(chapter: &mut IRChapter, parent_id: NodeId) {
    // 1. Recurse into children first (bottom-up)
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        vacuum_children(chapter, child_id);
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    // 2. Check if parent is a structural container where whitespace is irrelevant
    let parent_role = chapter.node(parent_id).map(|n| n.role);
    if !is_structural_container(parent_role) {
        return;
    }

    // 3. Walk siblings and unlink whitespace-only Text nodes
    let mut prev_opt: Option<NodeId> = None;
    let mut cursor_opt = chapter.node(parent_id).and_then(|n| n.first_child);

    while let Some(current_id) = cursor_opt {
        let next_opt = chapter.node(current_id).and_then(|n| n.next_sibling);

        if should_vacuum(chapter, current_id) {
            // Unlink this node
            if let Some(prev_id) = prev_opt {
                // Middle or end of list: prev.next_sibling = current.next_sibling
                if let Some(prev_node) = chapter.node_mut(prev_id) {
                    prev_node.next_sibling = next_opt;
                }
            } else {
                // First child: parent.first_child = current.next_sibling
                if let Some(parent_node) = chapter.node_mut(parent_id) {
                    parent_node.first_child = next_opt;
                }
            }
            // Don't update prev_opt - we removed current, so prev stays the same
        } else {
            prev_opt = Some(current_id);
        }

        cursor_opt = next_opt;
    }
}

/// Check if a node should be vacuumed (removed as structural whitespace).
fn should_vacuum(chapter: &IRChapter, node_id: NodeId) -> bool {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return false,
    };

    // Only vacuum Text nodes
    if node.role != Role::Text {
        return false;
    }

    // Safety: Never vacuum nodes with children (defensive - Text shouldn't have children)
    if node.first_child.is_some() {
        return false;
    }

    // Safety: Never vacuum nodes with IDs (they might be link targets)
    if chapter.semantics.id(node_id).is_some() {
        return false;
    }

    // Must have text content to check
    if node.text.is_empty() {
        return true; // Empty text nodes can always be vacuumed
    }

    // Check if content is whitespace-only
    let text = chapter.text(node.text);
    text.trim().is_empty()
}

/// Check if a role is a structural container where inter-element whitespace is irrelevant.
///
/// These are containers that should only contain other block elements, not raw text.
/// Whitespace between their children is formatting noise (indentation, newlines).
///
/// Safe: Root, Container (div/section), Table, TableRow, Lists, Figure
/// Unsafe: Paragraph (contains inline content), Heading, Inline, etc.
fn is_structural_container(role: Option<Role>) -> bool {
    matches!(
        role,
        Some(
            Role::Root
                | Role::Container  // Now safe - <p> is Role::Paragraph
                | Role::Figure
                | Role::Sidebar
                | Role::Footnote
                | Role::Table
                | Role::TableRow
                | Role::OrderedList
                | Role::UnorderedList
                | Role::DefinitionList
        )
    )
}

// ============================================================================
// Pass 2: Hoist Nested Inlines (Wrapper Hell Fix)
// ============================================================================

/// Dissolve redundant wrapper containers to expose siblings for merging.
///
/// Legacy formats (MOBI, AZW) emulate "Small Caps" by wrapping each character
/// in a separate `<font>` or `<div>` container:
/// ```html
/// <div><span>T</span></div><div><span>HE</span></div>
/// ```
///
/// This creates "Wrapper Hell" where text nodes aren't siblings, preventing
/// span merge. This pass identifies containers that:
/// 1. Are generic (Container or Inline)
/// 2. Have exactly one child
/// 3. Have no semantic attributes (id, href, etc.)
///
/// These wrappers are dissolved by promoting the child to the wrapper's position.
fn hoist_nested_inlines(chapter: &mut IRChapter) {
    if chapter.node_count() == 0 {
        return;
    }

    // Run multiple passes to handle deeply nested wrappers (Div > Div > Span)
    // In practice, 2-3 passes clear even the worst MOBI soup.
    let mut changed = true;
    let mut attempts = 0;

    while changed && attempts < 5 {
        changed = hoist_pass(chapter, NodeId::ROOT);
        attempts += 1;
    }
}

/// Run one hoisting pass over the tree. Returns true if any changes were made.
fn hoist_pass(chapter: &mut IRChapter, parent_id: NodeId) -> bool {
    let mut changed = false;

    // Recurse into children first (bottom-up for efficiency)
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        if hoist_pass(chapter, child_id) {
            changed = true;
        }
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    // Now check each child for redundant wrappers
    let mut prev_opt: Option<NodeId> = None;
    let mut cursor_opt = chapter.node(parent_id).and_then(|n| n.first_child);

    while let Some(current_id) = cursor_opt {
        let next_opt = chapter.node(current_id).and_then(|n| n.next_sibling);

        if is_redundant_wrapper(chapter, current_id) {
            // Get the single child that we'll promote
            let child_id = chapter.node(current_id).and_then(|n| n.first_child).unwrap();

            // 1. Reparent child to grandparent
            if let Some(child_node) = chapter.node_mut(child_id) {
                child_node.parent = Some(parent_id);
                child_node.next_sibling = next_opt;
            }

            // 2. Patch sibling chain: splice child into wrapper's position
            if let Some(prev_id) = prev_opt {
                if let Some(prev_node) = chapter.node_mut(prev_id) {
                    prev_node.next_sibling = Some(child_id);
                }
            } else {
                if let Some(parent_node) = chapter.node_mut(parent_id) {
                    parent_node.first_child = Some(child_id);
                }
            }

            // 3. Detach the wrapper (leave it as a dead node)
            if let Some(wrapper_node) = chapter.node_mut(current_id) {
                wrapper_node.first_child = None;
                wrapper_node.next_sibling = None;
            }

            // Continue from the promoted child (it's now in the wrapper's position)
            prev_opt = Some(child_id);
            cursor_opt = next_opt;
            changed = true;
        } else {
            prev_opt = Some(current_id);
            cursor_opt = next_opt;
        }
    }

    changed
}

/// Check if a node is a redundant wrapper that can be dissolved.
fn is_redundant_wrapper(chapter: &IRChapter, node_id: NodeId) -> bool {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return false,
    };

    // 1. Must be a generic container (Container or Inline)
    if !matches!(node.role, Role::Container | Role::Inline) {
        return false;
    }

    // 2. Must have exactly one child
    let first_child = match node.first_child {
        Some(id) => id,
        None => return false,
    };

    // Check that first child has no sibling (exactly one child)
    if chapter.node(first_child).and_then(|n| n.next_sibling).is_some() {
        return false;
    }

    // 3. Must have no semantic attributes
    if has_semantic_attrs(chapter, node_id) {
        return false;
    }

    // 4. Must not have text content (containers shouldn't, but be defensive)
    if !node.text.is_empty() {
        return false;
    }

    true
}

// ============================================================================
// Pass 3: Span Merge (Adjacent Text Coalescing)
// ============================================================================

/// Merge adjacent inline/text nodes with identical styles.
///
/// This pass walks the tree using the first_child/next_sibling links.
/// When two adjacent siblings have the same role (Text), same style,
/// no semantic attributes, and contiguous text ranges, they are merged
/// by extending the first node's text range and unlinking the second.
///
/// # Why this matters
///
/// MOBI/AZW files often store small-caps as fragmented elements:
/// ```html
/// <font size="5"><b>T</b></font><font size="2"><b>HE </b></font>
/// ```
///
/// Without this pass, the markdown export would produce:
/// ```text
/// **T****HE **
/// ```
///
/// After merging, we get the expected:
/// ```text
/// **THE **
/// ```
fn merge_adjacent_spans(chapter: &mut IRChapter) {
    if chapter.node_count() > 0 {
        merge_children(chapter, NodeId::ROOT);
    }
}

fn merge_children(chapter: &mut IRChapter, parent_id: NodeId) {
    // 1. Recurse into children first (bottom-up)
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        merge_children(chapter, child_id);
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    // 2. Walk the sibling chain and merge where possible
    let mut cursor_opt = chapter.node(parent_id).and_then(|n| n.first_child);

    while let Some(current_id) = cursor_opt {
        let next_opt = chapter.node(current_id).and_then(|n| n.next_sibling);

        if let Some(next_id) = next_opt {
            if can_merge_spans(chapter, current_id, next_id) {
                // Merge: extend current's text range to include next's text
                let next_len = chapter.node(next_id).map(|n| n.text.len).unwrap_or(0);
                if let Some(current_node) = chapter.node_mut(current_id) {
                    current_node.text.len += next_len;
                }

                // Unlink next: current.next_sibling = next.next_sibling
                let new_next = chapter.node(next_id).and_then(|n| n.next_sibling);
                if let Some(current_node) = chapter.node_mut(current_id) {
                    current_node.next_sibling = new_next;
                }

                // Don't advance cursor - the new next might also be mergeable
                continue;
            }
        }

        cursor_opt = next_opt;
    }
}

/// Check if two adjacent siblings can be merged.
fn can_merge_spans(chapter: &IRChapter, left_id: NodeId, right_id: NodeId) -> bool {
    let (left, right) = match (chapter.node(left_id), chapter.node(right_id)) {
        (Some(l), Some(r)) => (l, r),
        _ => return false,
    };

    // 1. Both must be Text nodes
    if left.role != Role::Text || right.role != Role::Text {
        return false;
    }

    // 2. Both must have actual text content
    if left.text.is_empty() || right.text.is_empty() {
        return false;
    }

    // 3. Same style
    if left.style != right.style {
        return false;
    }

    // 4. Neither has semantic attributes
    if has_semantic_attrs(chapter, left_id) || has_semantic_attrs(chapter, right_id) {
        return false;
    }

    // 5. Text ranges must be contiguous
    if left.text.end() != right.text.start {
        return false;
    }

    true
}

// ============================================================================
// Pass 4: List Fuser (Fragmented List Repair)
// ============================================================================

/// Fuse adjacent lists of the same type.
///
/// Converters often emit a separate `<ul>` for every `<li>`:
/// ```html
/// <ul><li>Item 1</li></ul>
/// <ul><li>Item 2</li></ul>
/// ```
///
/// This looks fine in browsers but breaks:
/// - Ordered list numbering (resets each time)
/// - Margins (double spacing between items)
/// - Semantic structure
///
/// We fuse adjacent lists by moving children from the second list to the first.
fn fuse_lists(chapter: &mut IRChapter) {
    if chapter.node_count() > 0 {
        fuse_list_children(chapter, NodeId::ROOT);
    }
}

fn fuse_list_children(chapter: &mut IRChapter, parent_id: NodeId) {
    // 1. Recurse into children first (bottom-up)
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        fuse_list_children(chapter, child_id);
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    // 2. Walk siblings and fuse adjacent lists
    let mut cursor_opt = chapter.node(parent_id).and_then(|n| n.first_child);

    while let Some(current_id) = cursor_opt {
        let next_opt = chapter.node(current_id).and_then(|n| n.next_sibling);

        if let Some(next_id) = next_opt {
            if can_fuse_lists(chapter, current_id, next_id) {
                fuse_list_pair(chapter, current_id, next_id);
                // Don't advance - check if new next is also fuseable
                continue;
            }
        }

        cursor_opt = next_opt;
    }
}

/// Check if two adjacent nodes are lists that can be fused.
fn can_fuse_lists(chapter: &IRChapter, left_id: NodeId, right_id: NodeId) -> bool {
    let (left, right) = match (chapter.node(left_id), chapter.node(right_id)) {
        (Some(l), Some(r)) => (l, r),
        _ => return false,
    };

    // Must be same list type
    match (left.role, right.role) {
        (Role::OrderedList, Role::OrderedList) => true,
        (Role::UnorderedList, Role::UnorderedList) => true,
        _ => false,
    }
}

/// Fuse two adjacent lists by moving children from right to left.
fn fuse_list_pair(chapter: &mut IRChapter, left_id: NodeId, right_id: NodeId) {
    // Get right's children
    let right_first = chapter.node(right_id).and_then(|n| n.first_child);

    if right_first.is_none() {
        // Right list is empty, just unlink it
        let right_next = chapter.node(right_id).and_then(|n| n.next_sibling);
        if let Some(left_node) = chapter.node_mut(left_id) {
            left_node.next_sibling = right_next;
        }
        return;
    }

    // 1. Reparent all children of right to left
    let mut child_opt = right_first;
    while let Some(child_id) = child_opt {
        let next_child = chapter.node(child_id).and_then(|n| n.next_sibling);
        if let Some(child_node) = chapter.node_mut(child_id) {
            child_node.parent = Some(left_id);
        }
        child_opt = next_child;
    }

    // 2. Find left's last child
    let mut left_last = chapter.node(left_id).and_then(|n| n.first_child);
    if let Some(mut current) = left_last {
        while let Some(next) = chapter.node(current).and_then(|n| n.next_sibling) {
            current = next;
        }
        left_last = Some(current);
    }

    // 3. Stitch: left_last.next_sibling = right_first
    if let Some(last_id) = left_last {
        if let Some(last_node) = chapter.node_mut(last_id) {
            last_node.next_sibling = right_first;
        }
    } else {
        // Left was empty, right's children become left's children
        if let Some(left_node) = chapter.node_mut(left_id) {
            left_node.first_child = right_first;
        }
    }

    // 4. Unlink right from sibling chain
    let right_next = chapter.node(right_id).and_then(|n| n.next_sibling);
    if let Some(left_node) = chapter.node_mut(left_id) {
        left_node.next_sibling = right_next;
    }
}

// ============================================================================
// Pass 5: Pruner (Empty Container Removal)
// ============================================================================

/// Remove empty containers in post-order (cascading).
///
/// "Div soup" from old converters leaves empty containers:
/// ```html
/// <div class="clear"></div>
/// <span id="ad-placeholder"></span>
/// ```
///
/// Post-order traversal enables cascading:
/// - `<div><span></span></div>`
/// - Step 1: Visit span, it's empty, delete
/// - Step 2: Visit div, now empty, delete
/// - Result: Entire dead subtree vanishes
fn prune_empty(chapter: &mut IRChapter) {
    if chapter.node_count() > 0 {
        prune_children(chapter, NodeId::ROOT);
    }
}

fn prune_children(chapter: &mut IRChapter, parent_id: NodeId) {
    // 1. Recurse into children first (post-order - children before parent)
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        prune_children(chapter, child_id);
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    // 2. Walk siblings and prune empty containers
    let mut prev_opt: Option<NodeId> = None;
    let mut cursor_opt = chapter.node(parent_id).and_then(|n| n.first_child);

    while let Some(current_id) = cursor_opt {
        let next_opt = chapter.node(current_id).and_then(|n| n.next_sibling);

        if should_prune(chapter, current_id) {
            // Unlink this node
            if let Some(prev_id) = prev_opt {
                if let Some(prev_node) = chapter.node_mut(prev_id) {
                    prev_node.next_sibling = next_opt;
                }
            } else {
                if let Some(parent_node) = chapter.node_mut(parent_id) {
                    parent_node.first_child = next_opt;
                }
            }
            // Don't update prev_opt
        } else {
            prev_opt = Some(current_id);
        }

        cursor_opt = next_opt;
    }
}

/// Check if a node should be pruned (empty container).
fn should_prune(chapter: &IRChapter, node_id: NodeId) -> bool {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return false,
    };

    // Only prune containers (never Text, Image, Break, Rule)
    if !is_prunable_role(node.role) {
        return false;
    }

    // Must have no children
    if node.first_child.is_some() {
        return false;
    }

    // Must have no text content
    if !node.text.is_empty() {
        return false;
    }

    // Safety: Don't prune if it has an ID (might be a link target)
    if chapter.semantics.id(node_id).is_some() {
        return false;
    }

    // Safety: Don't prune if it has src (might be loading content)
    if chapter.semantics.src(node_id).is_some() {
        return false;
    }

    true
}

/// Check if a role can be pruned when empty.
fn is_prunable_role(role: Role) -> bool {
    matches!(
        role,
        Role::Container
            | Role::Inline
            | Role::Figure
            | Role::Sidebar
            | Role::Footnote
            | Role::BlockQuote
            | Role::OrderedList
            | Role::UnorderedList
            | Role::DefinitionList
            | Role::Table
            | Role::TableRow
    )
}

// ============================================================================
// Shared Helpers
// ============================================================================

/// Check if a node has any semantic attributes that prevent optimization.
fn has_semantic_attrs(chapter: &IRChapter, node_id: NodeId) -> bool {
    let s = &chapter.semantics;
    s.href(node_id).is_some()
        || s.src(node_id).is_some()
        || s.alt(node_id).is_some()
        || s.id(node_id).is_some()
        || s.title(node_id).is_some()
        || s.lang(node_id).is_some()
        || s.epub_type(node_id).is_some()
        || s.aria_role(node_id).is_some()
        || s.datetime(node_id).is_some()
        || s.language(node_id).is_some()
        || s.list_start(node_id).is_some()
        || s.row_span(node_id).is_some()
        || s.col_span(node_id).is_some()
        || s.is_header_cell(node_id)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Node;

    /// Create a simple chapter for testing.
    fn make_test_chapter() -> IRChapter {
        let mut chapter = IRChapter::new();
        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);
        chapter
    }

    // ------------------------------------------------------------------------
    // Vacuum Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_vacuum_removes_whitespace_in_structural_container() {
        let mut chapter = IRChapter::new();

        // Create: Root > UnorderedList > [whitespace, ListItem, whitespace]
        // Lists are structural containers - whitespace between items is noise
        let list = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, list);

        let ws1 = chapter.append_text("\n    ");
        let ws1_node = chapter.alloc_node(Node::text(ws1));
        chapter.append_child(list, ws1_node);

        let item = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(list, item);

        let ws2 = chapter.append_text("\n  ");
        let ws2_node = chapter.alloc_node(Node::text(ws2));
        chapter.append_child(list, ws2_node);

        // Before: 3 children
        assert_eq!(chapter.children(list).count(), 3);

        vacuum(&mut chapter);

        // After: 1 child (just the ListItem)
        let children: Vec<_> = chapter.children(list).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(chapter.node(children[0]).unwrap().role, Role::ListItem);
    }

    #[test]
    fn test_vacuum_preserves_whitespace_in_inline() {
        let mut chapter = IRChapter::new();

        // Create: Root > Inline > [whitespace, Text]
        // Whitespace inside Inline should be preserved
        let inline = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(NodeId::ROOT, inline);

        let ws = chapter.append_text(" ");
        let ws_node = chapter.alloc_node(Node::text(ws));
        chapter.append_child(inline, ws_node);

        let text = chapter.append_text("word");
        let text_node = chapter.alloc_node(Node::text(text));
        chapter.append_child(inline, text_node);

        vacuum(&mut chapter);

        // Both should still be there
        assert_eq!(chapter.children(inline).count(), 2);
    }

    #[test]
    fn test_vacuum_preserves_whitespace_in_paragraph() {
        let mut chapter = IRChapter::new();

        // Create: Root > Paragraph > [Inline "Hello", whitespace " ", Inline "World"]
        // This simulates: <p><span>Hello</span> <span>World</span></p>
        // The space is significant!
        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let span1 = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(para, span1);
        let text1 = chapter.append_text("Hello");
        let text1_node = chapter.alloc_node(Node::text(text1));
        chapter.append_child(span1, text1_node);

        let ws = chapter.append_text(" ");
        let ws_node = chapter.alloc_node(Node::text(ws));
        chapter.append_child(para, ws_node);

        let span2 = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(para, span2);
        let text2 = chapter.append_text("World");
        let text2_node = chapter.alloc_node(Node::text(text2));
        chapter.append_child(span2, text2_node);

        // Before: 3 children (span, space, span)
        assert_eq!(chapter.children(para).count(), 3);

        vacuum(&mut chapter);

        // After: Still 3 children - space is preserved!
        assert_eq!(chapter.children(para).count(), 3);
    }

    #[test]
    fn test_vacuum_preserves_node_with_id() {
        let mut chapter = IRChapter::new();

        // Create: Root > UnorderedList > [whitespace with ID, ListItem]
        // Even in a structural container, nodes with IDs should be preserved
        let list = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, list);

        let ws = chapter.append_text(" ");
        let ws_node = chapter.alloc_node(Node::text(ws));
        chapter.append_child(list, ws_node);
        chapter.semantics.set_id(ws_node, "anchor".to_string());

        let item = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(list, item);

        vacuum(&mut chapter);

        // Whitespace should be preserved because it has an ID (link target)
        assert_eq!(chapter.children(list).count(), 2);
    }

    // ------------------------------------------------------------------------
    // Hoist Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_hoist_dissolves_single_child_wrapper() {
        let mut chapter = IRChapter::new();

        // Create: Root > Container > Inline > Text "Hello"
        // Both Container and Inline are redundant wrappers (single child each)
        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let inline = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(container, inline);

        let text_range = chapter.append_text("Hello");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(inline, text_node);

        // Before: Root > Container > Inline > Text
        assert_eq!(chapter.children(NodeId::ROOT).count(), 1);
        let root_child = chapter.children(NodeId::ROOT).next().unwrap();
        assert_eq!(chapter.node(root_child).unwrap().role, Role::Container);

        hoist_nested_inlines(&mut chapter);

        // After: Root > Text (both Container and Inline dissolved)
        // Multiple passes dissolve the nested wrappers
        let root_children: Vec<_> = chapter.children(NodeId::ROOT).collect();
        assert_eq!(root_children.len(), 1);
        assert_eq!(chapter.node(root_children[0]).unwrap().role, Role::Text);
    }

    #[test]
    fn test_hoist_enables_span_merge() {
        let mut chapter = IRChapter::new();

        // Create "Wrapper Hell" structure (like MOBI small caps):
        // Root > [Container > Inline > Text "T", Container > Inline > Text "HE"]
        let c1 = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, c1);
        let i1 = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(c1, i1);
        let t1 = chapter.append_text("T");
        let t1_node = chapter.alloc_node(Node::text(t1));
        chapter.append_child(i1, t1_node);

        let c2 = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, c2);
        let i2 = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(c2, i2);
        let t2 = chapter.append_text("HE");
        let t2_node = chapter.alloc_node(Node::text(t2));
        chapter.append_child(i2, t2_node);

        // Before: 2 containers at root
        assert_eq!(chapter.children(NodeId::ROOT).count(), 2);

        // Full optimization pipeline
        optimize(&mut chapter);

        // After: Should have a single text node with "THE"
        // (Containers dissolved, Inlines dissolved, Text merged)
        let mut found_the = false;
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Text && !node.text.is_empty() {
                let text = chapter.text(node.text);
                if text == "THE" {
                    found_the = true;
                }
            }
        }
        assert!(found_the, "Expected merged text 'THE' not found");
    }

    #[test]
    fn test_hoist_preserves_multi_child_container() {
        let mut chapter = IRChapter::new();

        // Create: Root > Container > [Inline "A", Inline "B"]
        // Container has 2 children, so it's NOT redundant
        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let i1 = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(container, i1);
        let t1 = chapter.append_text("A");
        let t1_node = chapter.alloc_node(Node::text(t1));
        chapter.append_child(i1, t1_node);

        let i2 = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(container, i2);
        let t2 = chapter.append_text("B");
        let t2_node = chapter.alloc_node(Node::text(t2));
        chapter.append_child(i2, t2_node);

        hoist_nested_inlines(&mut chapter);

        // Container should still be there (has 2 children)
        let root_children: Vec<_> = chapter.children(NodeId::ROOT).collect();
        assert_eq!(root_children.len(), 1);
        assert_eq!(chapter.node(root_children[0]).unwrap().role, Role::Container);
    }

    #[test]
    fn test_hoist_preserves_container_with_id() {
        let mut chapter = IRChapter::new();

        // Create: Root > Container[id="anchor"] > Inline > Text
        // Container has semantic attribute, so it's NOT redundant
        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);
        chapter.semantics.set_id(container, "anchor".to_string());

        let inline = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(container, inline);

        let text_range = chapter.append_text("Hello");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(inline, text_node);

        hoist_nested_inlines(&mut chapter);

        // Container should still be there (has ID)
        let root_children: Vec<_> = chapter.children(NodeId::ROOT).collect();
        assert_eq!(root_children.len(), 1);
        assert_eq!(chapter.node(root_children[0]).unwrap().role, Role::Container);
    }

    // ------------------------------------------------------------------------
    // Span Merge Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_merge_adjacent_text_nodes() {
        let mut chapter = IRChapter::new();

        // Use Paragraph - it contains inline content where spaces matter
        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        // Add "THE " as three separate text nodes
        let range1 = chapter.append_text("T");
        let node1 = chapter.alloc_node(Node::text(range1));
        chapter.append_child(para, node1);

        let range2 = chapter.append_text("HE");
        let node2 = chapter.alloc_node(Node::text(range2));
        chapter.append_child(para, node2);

        let range3 = chapter.append_text(" ");
        let node3 = chapter.alloc_node(Node::text(range3));
        chapter.append_child(para, node3);

        assert_eq!(chapter.children(para).count(), 3);

        optimize(&mut chapter);

        let children: Vec<_> = chapter.children(para).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(chapter.text(chapter.node(children[0]).unwrap().text), "THE ");
    }

    #[test]
    fn test_no_merge_different_styles() {
        let mut chapter = IRChapter::new();

        // Use Paragraph - it contains inline content
        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let range1 = chapter.append_text("Hello");
        let mut node1 = Node::text(range1);
        let bold = chapter.styles.intern(crate::ir::ComputedStyle {
            font_weight: crate::ir::FontWeight::BOLD,
            ..Default::default()
        });
        node1.style = bold;
        let id1 = chapter.alloc_node(node1);
        chapter.append_child(para, id1);

        let range2 = chapter.append_text(" World");
        let node2 = Node::text(range2);
        let id2 = chapter.alloc_node(node2);
        chapter.append_child(para, id2);

        optimize(&mut chapter);

        assert_eq!(chapter.children(para).count(), 2);
    }

    // ------------------------------------------------------------------------
    // List Fuser Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_fuse_adjacent_unordered_lists() {
        let mut chapter = IRChapter::new();

        // Create two adjacent ul elements
        let ul1 = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul1);

        let li1 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul1, li1);

        let ul2 = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul2);

        let li2 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul2, li2);

        // Before: 2 lists at root
        assert_eq!(chapter.children(NodeId::ROOT).count(), 2);

        fuse_lists(&mut chapter);

        // After: 1 list with 2 items
        let root_children: Vec<_> = chapter.children(NodeId::ROOT).collect();
        assert_eq!(root_children.len(), 1);

        let list_children: Vec<_> = chapter.children(root_children[0]).collect();
        assert_eq!(list_children.len(), 2);
    }

    #[test]
    fn test_no_fuse_different_list_types() {
        let mut chapter = IRChapter::new();

        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);

        let ol = chapter.alloc_node(Node::new(Role::OrderedList));
        chapter.append_child(NodeId::ROOT, ol);

        fuse_lists(&mut chapter);

        // Should still be 2 lists
        assert_eq!(chapter.children(NodeId::ROOT).count(), 2);
    }

    // ------------------------------------------------------------------------
    // Pruner Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_prune_empty_container() {
        let mut chapter = IRChapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        // Empty container should be pruned
        prune_empty(&mut chapter);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 0);
    }

    #[test]
    fn test_prune_cascades() {
        let mut chapter = IRChapter::new();

        // Create: Root > Container > Inline (empty)
        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let inline = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(container, inline);

        // Before: Container has 1 child
        assert_eq!(chapter.children(container).count(), 1);

        prune_empty(&mut chapter);

        // After: Both should be gone (cascading)
        assert_eq!(chapter.children(NodeId::ROOT).count(), 0);
    }

    #[test]
    fn test_prune_preserves_id() {
        let mut chapter = IRChapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);
        chapter.semantics.set_id(container, "anchor".to_string());

        prune_empty(&mut chapter);

        // Should NOT be pruned (has ID, might be link target)
        assert_eq!(chapter.children(NodeId::ROOT).count(), 1);
    }

    #[test]
    fn test_prune_preserves_content() {
        let mut chapter = IRChapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let text_range = chapter.append_text("Content");
        let text = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(container, text);

        prune_empty(&mut chapter);

        // Container has content, should NOT be pruned
        assert_eq!(chapter.children(NodeId::ROOT).count(), 1);
    }

    // ------------------------------------------------------------------------
    // Pipeline Integration Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_full_pipeline() {
        let mut chapter = IRChapter::new();

        // Create a structure with adjacent lists:
        // Root > Container > [ul1, ul2]
        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let ul1 = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(container, ul1);
        let li1 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul1, li1);

        let ul2 = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(container, ul2);
        let li2 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul2, li2);

        // Before: 2 lists under container
        assert_eq!(chapter.children(container).count(), 2);

        optimize(&mut chapter);

        // After optimization:
        // - Lists fused into 1
        // = 1 child (the fused list)
        let children: Vec<_> = chapter.children(container).collect();
        assert_eq!(children.len(), 1);

        // The fused list should have 2 items
        let list_items: Vec<_> = chapter.children(children[0]).collect();
        assert_eq!(list_items.len(), 2);
    }
}

