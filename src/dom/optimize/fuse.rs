//! Pass 3: List Fuser (Fragmented List Repair)

use crate::model::{Chapter, NodeId, Role};

use super::pass::walk_bottom_up;

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
pub fn fuse_lists(chapter: &mut Chapter) {
    walk_bottom_up(chapter, |chapter, parent_id| {
        fuse_list_siblings(chapter, parent_id);
    });
}

fn fuse_list_siblings(chapter: &mut Chapter, parent_id: NodeId) {
    let mut cursor_opt = chapter.node(parent_id).and_then(|n| n.first_child);

    while let Some(current_id) = cursor_opt {
        let next_opt = chapter.node(current_id).and_then(|n| n.next_sibling);

        if let Some(next_id) = next_opt
            && can_fuse_lists(chapter, current_id, next_id)
        {
            fuse_list_pair(chapter, current_id, next_id);
            // Don't advance - check if new next is also fuseable
            continue;
        }

        cursor_opt = next_opt;
    }
}

/// Check if two adjacent nodes are lists that can be fused.
fn can_fuse_lists(chapter: &Chapter, left_id: NodeId, right_id: NodeId) -> bool {
    let (left, right) = match (chapter.node(left_id), chapter.node(right_id)) {
        (Some(l), Some(r)) => (l, r),
        _ => return false,
    };

    // Must be same list type
    matches!(
        (left.role, right.role),
        (Role::OrderedList, Role::OrderedList) | (Role::UnorderedList, Role::UnorderedList)
    )
}

/// Fuse two adjacent lists by moving children from right to left.
fn fuse_list_pair(chapter: &mut Chapter, left_id: NodeId, right_id: NodeId) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    #[test]
    fn test_fuse_adjacent_unordered_lists() {
        let mut chapter = Chapter::new();

        let ul1 = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul1);

        let li1 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul1, li1);

        let ul2 = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul2);

        let li2 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul2, li2);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 2);

        fuse_lists(&mut chapter);

        let root_children: Vec<_> = chapter.children(NodeId::ROOT).collect();
        assert_eq!(root_children.len(), 1);

        let list_children: Vec<_> = chapter.children(root_children[0]).collect();
        assert_eq!(list_children.len(), 2);
    }

    #[test]
    fn test_no_fuse_different_list_types() {
        let mut chapter = Chapter::new();

        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);

        let ol = chapter.alloc_node(Node::new(Role::OrderedList));
        chapter.append_child(NodeId::ROOT, ol);

        fuse_lists(&mut chapter);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 2);
    }
}
