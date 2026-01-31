//! Pass 4: Wrap Mixed Content (Inline/Block Normalization)

use crate::model::{Chapter, Node, NodeId, Role};

use super::pass::walk_bottom_up;
use super::predicates::{is_block_container, is_inline_role};

/// Wrap consecutive inline children in a Container when they're siblings to block elements.
///
/// HTML allows mixed inline and block content in some containers:
/// ```html
/// <blockquote>
///   <p>Some verse...</p>
///   <cite>— Author</cite>
/// </blockquote>
/// ```
///
/// In this example, `<cite>` (mapped to `Inline`) is a sibling to `<p>` (a block element).
/// During KFX export, inline children become spans on the parent container, which inverts
/// the content order (inlines appear before blocks).
///
/// This pass detects block containers with mixed inline/block children and wraps
/// consecutive inline runs in a Container node, normalizing the structure:
///
/// Before: BlockQuote > [Paragraph, Inline "cite"]
/// After:  BlockQuote > [Paragraph, Container > [Inline "cite"]]
pub fn wrap_mixed_content(chapter: &mut Chapter) {
    walk_bottom_up(chapter, |chapter, parent_id| {
        wrap_mixed_children(chapter, parent_id);
    });
}

fn wrap_mixed_children(chapter: &mut Chapter, parent_id: NodeId) {
    // Check if parent is a block container that might have mixed content
    let parent_role = chapter.node(parent_id).map(|n| n.role);
    if !is_block_container(parent_role) {
        return;
    }

    // Analyze children: do we have both inline and block children?
    let (has_inline, has_block) = analyze_children(chapter, parent_id);
    if !has_inline || !has_block {
        return; // No mixed content
    }

    // Find runs of consecutive inline children and wrap them
    wrap_inline_runs(chapter, parent_id);
}

/// Analyze children to detect if we have mixed inline/block content.
fn analyze_children(chapter: &Chapter, parent_id: NodeId) -> (bool, bool) {
    let mut has_inline = false;
    let mut has_block = false;

    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        if let Some(child) = chapter.node(child_id) {
            if is_inline_role(child.role) {
                has_inline = true;
            } else {
                has_block = true;
            }
        }
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    (has_inline, has_block)
}

/// Find and wrap consecutive runs of inline children.
fn wrap_inline_runs(chapter: &mut Chapter, parent_id: NodeId) {
    // Collect child info to avoid borrow issues
    let mut children_info: Vec<(NodeId, bool)> = Vec::new();
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        let is_inline = chapter
            .node(child_id)
            .map(|n| is_inline_role(n.role))
            .unwrap_or(false);
        children_info.push((child_id, is_inline));
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    // Find inline runs (consecutive inline children)
    let mut runs: Vec<(usize, usize)> = Vec::new(); // (start_idx, end_idx) inclusive
    let mut run_start: Option<usize> = None;

    for (idx, &(_, is_inline)) in children_info.iter().enumerate() {
        if is_inline {
            if run_start.is_none() {
                run_start = Some(idx);
            }
        } else if let Some(start) = run_start {
            runs.push((start, idx - 1));
            run_start = None;
        }
    }
    // Handle trailing run
    if let Some(start) = run_start {
        runs.push((start, children_info.len() - 1));
    }

    // Wrap runs in reverse order to preserve indices
    for (start_idx, end_idx) in runs.into_iter().rev() {
        wrap_run(chapter, parent_id, &children_info, start_idx, end_idx);
    }
}

/// Wrap a single run of inline children in a Container.
fn wrap_run(
    chapter: &mut Chapter,
    parent_id: NodeId,
    children_info: &[(NodeId, bool)],
    start_idx: usize,
    end_idx: usize,
) {
    // Create wrapper Container
    let wrapper_id = chapter.alloc_node(Node::new(Role::Container));

    // Set wrapper's parent
    if let Some(wrapper) = chapter.node_mut(wrapper_id) {
        wrapper.parent = Some(parent_id);
    }

    // Get the last node in this run
    let last_inline_id = children_info[end_idx].0;

    // Get next sibling after the run (if any)
    let after_run = chapter.node(last_inline_id).and_then(|n| n.next_sibling);

    // Reparent inline nodes to wrapper
    let mut prev_in_wrapper: Option<NodeId> = None;
    for (child_id, _) in &children_info[start_idx..=end_idx] {
        let child_id = *child_id;

        // Set new parent
        if let Some(child) = chapter.node_mut(child_id) {
            child.parent = Some(wrapper_id);
        }

        // Link within wrapper
        if let Some(prev_id) = prev_in_wrapper {
            if let Some(prev) = chapter.node_mut(prev_id) {
                prev.next_sibling = Some(child_id);
            }
        } else {
            // First child of wrapper
            if let Some(wrapper) = chapter.node_mut(wrapper_id) {
                wrapper.first_child = Some(child_id);
            }
        }
        prev_in_wrapper = Some(child_id);
    }

    // Clear next_sibling of last inline node
    if let Some(last) = chapter.node_mut(last_inline_id) {
        last.next_sibling = None;
    }

    // Set wrapper's next_sibling
    if let Some(wrapper) = chapter.node_mut(wrapper_id) {
        wrapper.next_sibling = after_run;
    }

    // Link wrapper into parent's child chain
    if start_idx == 0 {
        // Wrapper becomes first child
        if let Some(parent) = chapter.node_mut(parent_id) {
            parent.first_child = Some(wrapper_id);
        }
    } else {
        // Link previous sibling to wrapper
        let prev_sibling_id = children_info[start_idx - 1].0;
        if let Some(prev) = chapter.node_mut(prev_sibling_id) {
            prev.next_sibling = Some(wrapper_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wraps_mixed_content_in_blockquote() {
        let mut chapter = Chapter::new();

        let bq = chapter.alloc_node(Node::new(Role::BlockQuote));
        chapter.append_child(NodeId::ROOT, bq);

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(bq, para);
        let verse_range = chapter.append_text("Some verse...");
        let verse = chapter.alloc_node(Node::text(verse_range));
        chapter.append_child(para, verse);

        let cite = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(bq, cite);
        let author_range = chapter.append_text("— Author");
        let author = chapter.alloc_node(Node::text(author_range));
        chapter.append_child(cite, author);

        assert_eq!(chapter.children(bq).count(), 2);

        wrap_mixed_content(&mut chapter);

        let children: Vec<_> = chapter.children(bq).collect();
        assert_eq!(children.len(), 2);

        assert_eq!(chapter.node(children[0]).unwrap().role, Role::Paragraph);
        assert_eq!(chapter.node(children[1]).unwrap().role, Role::Container);

        let wrapper_children: Vec<_> = chapter.children(children[1]).collect();
        assert_eq!(wrapper_children.len(), 1);
        assert_eq!(
            chapter.node(wrapper_children[0]).unwrap().role,
            Role::Inline
        );
    }

    #[test]
    fn test_no_wrap_when_only_block_children() {
        let mut chapter = Chapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let p1 = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(container, p1);

        let p2 = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(container, p2);

        assert_eq!(chapter.children(container).count(), 2);

        wrap_mixed_content(&mut chapter);

        let children: Vec<_> = chapter.children(container).collect();
        assert_eq!(children.len(), 2);
        assert_eq!(chapter.node(children[0]).unwrap().role, Role::Paragraph);
        assert_eq!(chapter.node(children[1]).unwrap().role, Role::Paragraph);
    }

    #[test]
    fn test_no_wrap_when_only_inline_children() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let t1_range = chapter.append_text("Hello ");
        let t1 = chapter.alloc_node(Node::text(t1_range));
        chapter.append_child(para, t1);

        let span = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(para, span);

        let t2_range = chapter.append_text(" World");
        let t2 = chapter.alloc_node(Node::text(t2_range));
        chapter.append_child(para, t2);

        assert_eq!(chapter.children(para).count(), 3);

        wrap_mixed_content(&mut chapter);

        let children: Vec<_> = chapter.children(para).collect();
        assert_eq!(children.len(), 3);
    }
}
