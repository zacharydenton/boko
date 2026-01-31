//! Pass 6: Pruner (Empty Container Removal)

use crate::model::{Chapter, NodeId};

use super::pass::walk_bottom_up;
use super::predicates::is_prunable_role;

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
pub fn prune_empty(chapter: &mut Chapter) {
    walk_bottom_up(chapter, |chapter, parent_id| {
        prune_siblings(chapter, parent_id);
    });
}

fn prune_siblings(chapter: &mut Chapter, parent_id: NodeId) {
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
            } else if let Some(parent_node) = chapter.node_mut(parent_id) {
                parent_node.first_child = next_opt;
            }
            // Don't update prev_opt
        } else {
            prev_opt = Some(current_id);
        }

        cursor_opt = next_opt;
    }
}

/// Check if a node should be pruned (empty container).
fn should_prune(chapter: &Chapter, node_id: NodeId) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, Role};

    #[test]
    fn test_prunes_empty_container() {
        let mut chapter = Chapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        prune_empty(&mut chapter);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 0);
    }

    #[test]
    fn test_prune_cascades() {
        let mut chapter = Chapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let inline = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(container, inline);

        assert_eq!(chapter.children(container).count(), 1);

        prune_empty(&mut chapter);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 0);
    }

    #[test]
    fn test_preserves_node_with_id() {
        let mut chapter = Chapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);
        chapter.semantics.set_id(container, "anchor");

        prune_empty(&mut chapter);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 1);
    }

    #[test]
    fn test_preserves_node_with_content() {
        let mut chapter = Chapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let text_range = chapter.append_text("Content");
        let text = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(container, text);

        prune_empty(&mut chapter);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 1);
    }
}
