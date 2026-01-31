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
//! 4. **Wrap Mixed Content** - Normalize inline/block siblings
//! 5. **Pruner** - Remove empty containers (cascading)

use crate::model::{Chapter, Node, NodeId, Role};

/// Run all optimization passes on a chapter.
///
/// Passes are ordered for maximum effectiveness:
/// 1. Vacuum removes whitespace noise
/// 2. Span merge coalesces adjacent text with same style
/// 3. List fuser repairs fragmented lists
/// 4. Wrap mixed content normalizes inline/block siblings
/// 5. Normalize table structure (add thead/tbody wrappers)
/// 6. Pruner removes any containers emptied by previous passes
pub fn optimize(chapter: &mut Chapter) {
    vacuum(chapter);
    merge_adjacent_spans(chapter);
    fuse_lists(chapter);
    wrap_mixed_content(chapter);
    normalize_table_structure(chapter);
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
fn vacuum(chapter: &mut Chapter) {
    if chapter.node_count() > 0 {
        vacuum_children(chapter, NodeId::ROOT);
    }
}

fn vacuum_children(chapter: &mut Chapter, parent_id: NodeId) {
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
                | Role::TableHead
                | Role::TableBody
                | Role::TableRow
                | Role::OrderedList
                | Role::UnorderedList
                | Role::DefinitionList
        )
    )
}

// ============================================================================
// Pass 2: Span Merge (Adjacent Text Coalescing)
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
fn merge_adjacent_spans(chapter: &mut Chapter) {
    if chapter.node_count() > 0 {
        merge_children(chapter, NodeId::ROOT);
    }
}

fn merge_children(chapter: &mut Chapter, parent_id: NodeId) {
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

// ============================================================================
// Pass 3: List Fuser (Fragmented List Repair)
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
fn fuse_lists(chapter: &mut Chapter) {
    if chapter.node_count() > 0 {
        fuse_list_children(chapter, NodeId::ROOT);
    }
}

fn fuse_list_children(chapter: &mut Chapter, parent_id: NodeId) {
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

// ============================================================================
// Pass 4: Wrap Mixed Content (Inline/Block Normalization)
// ============================================================================

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
fn wrap_mixed_content(chapter: &mut Chapter) {
    if chapter.node_count() > 0 {
        wrap_mixed_children(chapter, NodeId::ROOT);
    }
}

fn wrap_mixed_children(chapter: &mut Chapter, parent_id: NodeId) {
    // 1. Recurse into children first (bottom-up)
    let mut child_opt = chapter.node(parent_id).and_then(|n| n.first_child);
    while let Some(child_id) = child_opt {
        wrap_mixed_children(chapter, child_id);
        child_opt = chapter.node(child_id).and_then(|n| n.next_sibling);
    }

    // 2. Check if parent is a block container that might have mixed content
    let parent_role = chapter.node(parent_id).map(|n| n.role);
    if !is_block_container(parent_role) {
        return;
    }

    // 3. Analyze children: do we have both inline and block children?
    let (has_inline, has_block) = analyze_children(chapter, parent_id);
    if !has_inline || !has_block {
        return; // No mixed content
    }

    // 4. Find runs of consecutive inline children and wrap them
    wrap_inline_runs(chapter, parent_id);
}

/// Check if a role is a block container that can have mixed content.
fn is_block_container(role: Option<Role>) -> bool {
    matches!(
        role,
        Some(
            Role::Root
                | Role::Container
                | Role::BlockQuote
                | Role::Figure
                | Role::Sidebar
                | Role::Footnote
                | Role::ListItem
                | Role::TableCell
        )
    )
}

/// Check if a role represents inline content.
fn is_inline_role(role: Role) -> bool {
    matches!(
        role,
        Role::Text | Role::Inline | Role::Link | Role::Image | Role::Break
    )
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

// ============================================================================
// Pass 5: Normalize Table Structure
// ============================================================================

/// Ensure tables have proper thead/tbody structure for KFX export.
///
/// KFX requires tables to have explicit `type: header` and `type: body` wrappers
/// around table rows. This pass ensures all tables have this structure:
///
/// Before:
/// ```text
/// Table
///   TableRow (with th cells)
///   TableRow (with td cells)
/// ```
///
/// After:
/// ```text
/// Table
///   TableHead
///     TableRow (with th cells)
///   TableBody
///     TableRow (with td cells)
/// ```
///
/// Tables that already have TableHead/TableBody are left unchanged.
fn normalize_table_structure(chapter: &mut Chapter) {
    // Collect all table nodes first to avoid borrow issues
    let tables: Vec<NodeId> = (0..chapter.node_count())
        .filter_map(|i| {
            let id = NodeId(i as u32);
            chapter.node(id).and_then(|node| {
                if node.role == Role::Table {
                    Some(id)
                } else {
                    None
                }
            })
        })
        .collect();

    for table_id in tables {
        normalize_single_table(chapter, table_id);
    }
}

/// Normalize a single table's structure.
fn normalize_single_table(chapter: &mut Chapter, table_id: NodeId) {
    // Check if table already has TableHead or TableBody children
    let has_section_wrapper = chapter.children(table_id).any(|child_id| {
        chapter
            .node(child_id)
            .map(|n| matches!(n.role, Role::TableHead | Role::TableBody))
            .unwrap_or(false)
    });

    if has_section_wrapper {
        // Already has proper structure, nothing to do
        return;
    }

    // Collect all table rows, separating header rows from body rows
    // A row is a "header row" if ALL its cells are header cells (th)
    let mut header_rows: Vec<NodeId> = Vec::new();
    let mut body_rows: Vec<NodeId> = Vec::new();

    for row_id in chapter.children(table_id).collect::<Vec<_>>() {
        let is_header_row = is_table_header_row(chapter, row_id);
        if is_header_row {
            header_rows.push(row_id);
        } else {
            body_rows.push(row_id);
        }
    }

    // If we found header rows, wrap them in TableHead
    // If we have body rows (or any rows at all), wrap them in TableBody
    // We process body first, then header, so header ends up first in the child list

    // Clear table's children - we'll re-add them under wrappers
    if let Some(table) = chapter.node_mut(table_id) {
        table.first_child = None;
    }

    // Create TableBody wrapper if we have body rows
    if !body_rows.is_empty() {
        let body_id = chapter.alloc_node(Node::new(Role::TableBody));

        // Set body's first child
        if let Some(body) = chapter.node_mut(body_id) {
            body.first_child = Some(body_rows[0]);
        }

        // Link body rows as siblings
        for i in 0..body_rows.len() {
            if let Some(row) = chapter.node_mut(body_rows[i]) {
                row.next_sibling = body_rows.get(i + 1).copied();
            }
        }

        // Add body as child of table
        if let Some(table) = chapter.node_mut(table_id) {
            table.first_child = Some(body_id);
        }
    }

    // Create TableHead wrapper if we have header rows
    if !header_rows.is_empty() {
        let head_id = chapter.alloc_node(Node::new(Role::TableHead));

        // Set head's first child
        if let Some(head) = chapter.node_mut(head_id) {
            head.first_child = Some(header_rows[0]);
        }

        // Link header rows as siblings
        for i in 0..header_rows.len() {
            if let Some(row) = chapter.node_mut(header_rows[i]) {
                row.next_sibling = header_rows.get(i + 1).copied();
            }
        }

        // Insert head before body (or as only child if no body)
        let current_first = chapter.node(table_id).and_then(|n| n.first_child);
        if let Some(head) = chapter.node_mut(head_id) {
            head.next_sibling = current_first;
        }
        if let Some(table) = chapter.node_mut(table_id) {
            table.first_child = Some(head_id);
        }
    }

    // Edge case: table with no rows at all - create empty TableBody
    // (This shouldn't happen in practice, but handle it for robustness)
    if header_rows.is_empty() && body_rows.is_empty() {
        let body_id = chapter.alloc_node(Node::new(Role::TableBody));
        if let Some(table) = chapter.node_mut(table_id) {
            table.first_child = Some(body_id);
        }
    }
}

/// Check if a table row contains only header cells (th).
fn is_table_header_row(chapter: &Chapter, row_id: NodeId) -> bool {
    let mut has_cells = false;
    let mut all_header = true;

    for cell_id in chapter.children(row_id) {
        if let Some(cell) = chapter.node(cell_id)
            && cell.role == Role::TableCell
        {
            has_cells = true;
            if !chapter.semantics.is_header_cell(cell_id) {
                all_header = false;
                break;
            }
        }
    }

    // Row is a header row if it has cells and all are header cells
    has_cells && all_header
}

// ============================================================================
// Pass 6: Pruner (Empty Container Removal)
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
fn prune_empty(chapter: &mut Chapter) {
    if chapter.node_count() > 0 {
        prune_children(chapter, NodeId::ROOT);
    }
}

fn prune_children(chapter: &mut Chapter, parent_id: NodeId) {
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
            | Role::TableHead
            | Role::TableBody
            | Role::TableRow
    )
}

// ============================================================================
// Shared Helpers
// ============================================================================

/// Check if a node has any semantic attributes that prevent optimization.
fn has_semantic_attrs(chapter: &Chapter, node_id: NodeId) -> bool {
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
    use crate::style::{ComputedStyle, FontWeight};

    // ------------------------------------------------------------------------
    // Vacuum Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_vacuum_removes_whitespace_in_structural_container() {
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

        // Create: Root > UnorderedList > [whitespace with ID, ListItem]
        // Even in a structural container, nodes with IDs should be preserved
        let list = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, list);

        let ws = chapter.append_text(" ");
        let ws_node = chapter.alloc_node(Node::text(ws));
        chapter.append_child(list, ws_node);
        chapter.semantics.set_id(ws_node, "anchor");

        let item = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(list, item);

        vacuum(&mut chapter);

        // Whitespace should be preserved because it has an ID (link target)
        assert_eq!(chapter.children(list).count(), 2);
    }

    // ------------------------------------------------------------------------
    // Span Merge Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_merge_adjacent_text_nodes() {
        let mut chapter = Chapter::new();

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
        assert_eq!(
            chapter.text(chapter.node(children[0]).unwrap().text),
            "THE "
        );
    }

    #[test]
    fn test_no_merge_different_styles() {
        let mut chapter = Chapter::new();

        // Use Paragraph - it contains inline content
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

        optimize(&mut chapter);

        assert_eq!(chapter.children(para).count(), 2);
    }

    // ------------------------------------------------------------------------
    // List Fuser Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_fuse_adjacent_unordered_lists() {
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        // Empty container should be pruned
        prune_empty(&mut chapter);

        assert_eq!(chapter.children(NodeId::ROOT).count(), 0);
    }

    #[test]
    fn test_prune_cascades() {
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);
        chapter.semantics.set_id(container, "anchor");

        prune_empty(&mut chapter);

        // Should NOT be pruned (has ID, might be link target)
        assert_eq!(chapter.children(NodeId::ROOT).count(), 1);
    }

    #[test]
    fn test_prune_preserves_content() {
        let mut chapter = Chapter::new();

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
        let mut chapter = Chapter::new();

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

    // ------------------------------------------------------------------------
    // Mixed Content Wrapping Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_wrap_mixed_content_in_blockquote() {
        let mut chapter = Chapter::new();

        // Create: BlockQuote > [Paragraph "verse", Inline "cite"]
        // This simulates: <blockquote><p>verse</p><cite>author</cite></blockquote>
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

        // Before: BlockQuote has 2 children (Paragraph, Inline)
        assert_eq!(chapter.children(bq).count(), 2);

        wrap_mixed_content(&mut chapter);

        // After: BlockQuote > [Paragraph, Container > [Inline]]
        // The Inline is wrapped in a Container
        let children: Vec<_> = chapter.children(bq).collect();
        assert_eq!(children.len(), 2);

        // First child is still the Paragraph
        assert_eq!(chapter.node(children[0]).unwrap().role, Role::Paragraph);

        // Second child is now a Container (wrapper)
        assert_eq!(chapter.node(children[1]).unwrap().role, Role::Container);

        // The wrapper contains the Inline
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

        // Create: Container > [Paragraph, Paragraph]
        // All children are blocks, no wrapping needed
        let container = chapter.alloc_node(Node::new(Role::Container));
        chapter.append_child(NodeId::ROOT, container);

        let p1 = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(container, p1);

        let p2 = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(container, p2);

        // Before
        assert_eq!(chapter.children(container).count(), 2);

        wrap_mixed_content(&mut chapter);

        // After: No change
        let children: Vec<_> = chapter.children(container).collect();
        assert_eq!(children.len(), 2);
        assert_eq!(chapter.node(children[0]).unwrap().role, Role::Paragraph);
        assert_eq!(chapter.node(children[1]).unwrap().role, Role::Paragraph);
    }

    #[test]
    fn test_no_wrap_when_only_inline_children() {
        let mut chapter = Chapter::new();

        // Create: Paragraph > [Text, Inline, Text]
        // All children are inline, no wrapping needed
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

        // Before
        assert_eq!(chapter.children(para).count(), 3);

        wrap_mixed_content(&mut chapter);

        // After: No change (Paragraph is not a block container in is_block_container)
        let children: Vec<_> = chapter.children(para).collect();
        assert_eq!(children.len(), 3);
    }
}
