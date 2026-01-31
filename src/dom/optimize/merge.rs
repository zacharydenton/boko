//! Pass 2: Span Merge (Adjacent Text Coalescing)

use crate::model::{Chapter, NodeId, Role};

use super::pass::walk_bottom_up;
use super::predicates::has_semantic_attrs;

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
pub fn merge_adjacent_spans(chapter: &mut Chapter) {
    walk_bottom_up(chapter, |chapter, parent_id| {
        merge_siblings(chapter, parent_id);
    });
}

fn merge_siblings(chapter: &mut Chapter, parent_id: NodeId) {
    let mut cursor_opt = chapter.node(parent_id).and_then(|n| n.first_child);

    while let Some(current_id) = cursor_opt {
        let next_opt = chapter.node(current_id).and_then(|n| n.next_sibling);

        if let Some(next_id) = next_opt
            && can_merge_spans(chapter, current_id, next_id)
        {
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

        cursor_opt = next_opt;
    }
}

/// Check if two adjacent siblings can be merged.
fn can_merge_spans(chapter: &Chapter, left_id: NodeId, right_id: NodeId) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;
    use crate::style::{ComputedStyle, FontWeight};

    #[test]
    fn test_merges_adjacent_text_nodes() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

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

        merge_adjacent_spans(&mut chapter);

        let children: Vec<_> = chapter.children(para).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(
            chapter.text(chapter.node(children[0]).unwrap().text),
            "THE "
        );
    }

    #[test]
    fn test_no_merge_different_styles() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let range1 = chapter.append_text("Hello");
        let mut node1 = Node::text(range1);
        let bold = chapter.styles.intern(ComputedStyle {
            font_weight: FontWeight::BOLD,
            ..Default::default()
        });
        node1.style = bold;
        let id1 = chapter.alloc_node(node1);
        chapter.append_child(para, id1);

        let range2 = chapter.append_text(" World");
        let node2 = Node::text(range2);
        let id2 = chapter.alloc_node(node2);
        chapter.append_child(para, id2);

        merge_adjacent_spans(&mut chapter);

        assert_eq!(chapter.children(para).count(), 2);
    }
}
