//! Tree traversal utilities for optimization passes.

use crate::model::{Chapter, NodeId};

/// Walk the tree bottom-up and call visitor at each parent.
///
/// This allows optimization passes to process children before parents,
/// enabling cascading effects (e.g., prune empty containers after
/// children are removed).
pub fn walk_bottom_up<F>(chapter: &mut Chapter, mut visitor: F)
where
    F: FnMut(&mut Chapter, NodeId),
{
    if chapter.node_count() > 0 {
        walk_children(chapter, NodeId::ROOT, &mut visitor);
    }
}

fn walk_children<F>(chapter: &mut Chapter, parent_id: NodeId, visitor: &mut F)
where
    F: FnMut(&mut Chapter, NodeId),
{
    // 1. Recurse into children first (bottom-up)
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        walk_children(chapter, child_id, visitor);
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }
    // 2. Visit this parent after children
    visitor(chapter, parent_id);
}
