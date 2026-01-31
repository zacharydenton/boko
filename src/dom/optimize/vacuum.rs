//! Pass 1: Vacuum (Structural Whitespace Culling)

use crate::model::{Chapter, NodeId, Role};

use super::pass::walk_bottom_up;
use super::predicates::is_structural_container;

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
pub fn vacuum(chapter: &mut Chapter) {
    walk_bottom_up(chapter, |chapter, parent_id| {
        vacuum_siblings(chapter, parent_id);
    });
}

fn vacuum_siblings(chapter: &mut Chapter, parent_id: NodeId) {
    // Check if parent is a structural container where whitespace is irrelevant
    let parent_role = chapter.node(parent_id).map(|n| n.role);
    if !is_structural_container(parent_role) {
        return;
    }

    // Walk siblings and unlink whitespace-only Text nodes
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
fn should_vacuum(chapter: &Chapter, node_id: NodeId) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    #[test]
    fn test_removes_whitespace_in_structural_container() {
        let mut chapter = Chapter::new();

        // Create: Root > UnorderedList > [whitespace, ListItem, whitespace]
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

        assert_eq!(chapter.children(list).count(), 3);

        vacuum(&mut chapter);

        let children: Vec<_> = chapter.children(list).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(chapter.node(children[0]).unwrap().role, Role::ListItem);
    }

    #[test]
    fn test_preserves_whitespace_in_inline() {
        let mut chapter = Chapter::new();

        let inline = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(NodeId::ROOT, inline);

        let ws = chapter.append_text(" ");
        let ws_node = chapter.alloc_node(Node::text(ws));
        chapter.append_child(inline, ws_node);

        let text = chapter.append_text("word");
        let text_node = chapter.alloc_node(Node::text(text));
        chapter.append_child(inline, text_node);

        vacuum(&mut chapter);

        assert_eq!(chapter.children(inline).count(), 2);
    }

    #[test]
    fn test_preserves_whitespace_in_paragraph() {
        let mut chapter = Chapter::new();

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

        assert_eq!(chapter.children(para).count(), 3);

        vacuum(&mut chapter);

        assert_eq!(chapter.children(para).count(), 3);
    }

    #[test]
    fn test_preserves_node_with_id() {
        let mut chapter = Chapter::new();

        let list = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, list);

        let ws = chapter.append_text(" ");
        let ws_node = chapter.alloc_node(Node::text(ws));
        chapter.append_child(list, ws_node);
        chapter.semantics.set_id(ws_node, "anchor");

        let item = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(list, item);

        vacuum(&mut chapter);

        assert_eq!(chapter.children(list).count(), 2);
    }
}
