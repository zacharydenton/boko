//! Pass 5: Normalize Table Structure

use crate::model::{Chapter, Node, NodeId, Role};

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
/// Only `TableRow` children are moved into the wrappers. A `Caption` belongs
/// to the table itself (HTML places `<caption>` first inside `<table>`), so
/// captions are kept as direct children of the table, ahead of the wrappers.
/// Any other non-row children are also left as direct children of the table,
/// after the wrappers.
///
/// Tables that already have TableHead/TableBody are left unchanged.
pub fn normalize_table_structure(chapter: &mut Chapter) {
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

    // Partition the table's children:
    // - Captions stay on the table itself (first, per HTML).
    // - TableRows move into TableHead/TableBody wrappers. A row is a
    //   "header row" if ALL its cells are header cells (th).
    // - Anything else stays on the table, after the wrappers.
    let mut captions: Vec<NodeId> = Vec::new();
    let mut header_rows: Vec<NodeId> = Vec::new();
    let mut body_rows: Vec<NodeId> = Vec::new();
    let mut others: Vec<NodeId> = Vec::new();

    for child_id in chapter.children(table_id).collect::<Vec<_>>() {
        match chapter.node(child_id).map(|n| n.role) {
            Some(Role::Caption) => captions.push(child_id),
            Some(Role::TableRow) => {
                if is_table_header_row(chapter, child_id) {
                    header_rows.push(child_id);
                } else {
                    body_rows.push(child_id);
                }
            }
            _ => others.push(child_id),
        }
    }

    // Detach all children from the table, clearing their stale sibling links
    // so `append_child` can rebuild each chain cleanly. `append_child`
    // maintains parent / first_child / last_child / next_sibling, so every
    // moved node ends up with a correct parent pointer and no dangling
    // sibling pointer into its old chain.
    for &child_id in captions
        .iter()
        .chain(&header_rows)
        .chain(&body_rows)
        .chain(&others)
    {
        if let Some(child) = chapter.node_mut(child_id) {
            child.next_sibling = None;
        }
    }
    if let Some(table) = chapter.node_mut(table_id) {
        table.first_child = None;
        table.last_child = None;
    }

    // Captions come first (HTML: <caption> precedes the row sections).
    for &caption_id in &captions {
        chapter.append_child(table_id, caption_id);
    }

    // Create TableHead wrapper if we have header rows
    if !header_rows.is_empty() {
        let head_id = chapter.alloc_node(Node::new(Role::TableHead));
        chapter.append_child(table_id, head_id);
        for &row_id in &header_rows {
            chapter.append_child(head_id, row_id);
        }
    }

    // Create TableBody wrapper if we have body rows
    if !body_rows.is_empty() {
        let body_id = chapter.alloc_node(Node::new(Role::TableBody));
        chapter.append_child(table_id, body_id);
        for &row_id in &body_rows {
            chapter.append_child(body_id, row_id);
        }
    }

    // Edge case: table with no rows at all - create empty TableBody
    // (This shouldn't happen in practice, but handle it for robustness)
    if header_rows.is_empty() && body_rows.is_empty() {
        let body_id = chapter.alloc_node(Node::new(Role::TableBody));
        chapter.append_child(table_id, body_id);
    }

    // Non-row, non-caption children keep their relative order after the
    // wrappers.
    for &other_id in &others {
        chapter.append_child(table_id, other_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a TableRow with `cells` cells under `parent`. When `header` is
    /// true, every cell is marked as a header cell (th).
    fn add_row(chapter: &mut Chapter, parent: NodeId, header: bool, cells: usize) -> NodeId {
        let row = chapter.alloc_node(Node::new(Role::TableRow));
        chapter.append_child(parent, row);
        for _ in 0..cells {
            let cell = chapter.alloc_node(Node::new(Role::TableCell));
            chapter.append_child(row, cell);
            if header {
                chapter.semantics.set_header_cell(cell, true);
            }
        }
        row
    }

    fn role_of(chapter: &Chapter, id: NodeId) -> Role {
        chapter.node(id).unwrap().role
    }

    /// Walk the whole tree asserting that every child's parent pointer names
    /// the node it actually hangs off, and that no node is reachable twice
    /// (i.e. no sibling chain leaks into another).
    fn assert_tree_consistent(chapter: &Chapter) {
        let mut seen = vec![false; chapter.node_count()];
        for parent in chapter.iter_dfs() {
            assert!(
                !seen[parent.0 as usize],
                "node {parent:?} reachable more than once (broken sibling chain)"
            );
            seen[parent.0 as usize] = true;
            for child in chapter.children(parent) {
                assert_eq!(
                    chapter.node(child).unwrap().parent,
                    Some(parent),
                    "stale parent pointer on {child:?} (role {:?})",
                    role_of(chapter, child)
                );
            }
        }
    }

    /// Roles of a node's direct children, in order.
    fn child_roles(chapter: &Chapter, id: NodeId) -> Vec<Role> {
        chapter.children(id).map(|c| role_of(chapter, c)).collect()
    }

    #[test]
    fn parent_pointers_correct_after_normalization() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);
        let caption = chapter.alloc_node(Node::new(Role::Caption));
        chapter.append_child(table, caption);
        add_row(&mut chapter, table, true, 2);
        add_row(&mut chapter, table, false, 2);
        add_row(&mut chapter, table, false, 2);

        normalize_table_structure(&mut chapter);

        assert_tree_consistent(&chapter);

        // Every reachable node is accounted for: all rows moved into wrappers
        // must still be reachable from the root exactly once.
        let reachable = chapter.iter_dfs().count();
        assert_eq!(reachable, chapter.node_count());

        // Moved rows point at their new wrappers, not at the table.
        for wrapper in chapter.children(table) {
            match role_of(&chapter, wrapper) {
                Role::TableHead | Role::TableBody => {
                    for row in chapter.children(wrapper) {
                        assert_eq!(chapter.node(row).unwrap().parent, Some(wrapper));
                    }
                }
                Role::Caption => {
                    assert_eq!(chapter.node(wrapper).unwrap().parent, Some(table));
                }
                other => panic!("unexpected table child role {other:?}"),
            }
        }
    }

    #[test]
    fn caption_stays_direct_child_of_table_and_first() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);
        // Caption deliberately placed after the rows: the pass must still
        // keep it on the table itself and hoist it to first position.
        add_row(&mut chapter, table, true, 1);
        add_row(&mut chapter, table, false, 1);
        let caption = chapter.alloc_node(Node::new(Role::Caption));
        chapter.append_child(table, caption);

        normalize_table_structure(&mut chapter);

        assert_eq!(
            child_roles(&chapter, table),
            vec![Role::Caption, Role::TableHead, Role::TableBody]
        );
        let first = chapter.children(table).next().unwrap();
        assert_eq!(first, caption);
        assert_eq!(chapter.node(caption).unwrap().parent, Some(table));
        // The caption must not have been swept into a wrapper.
        for wrapper in chapter.children(table).skip(1) {
            assert!(
                chapter
                    .children(wrapper)
                    .all(|c| role_of(&chapter, c) == Role::TableRow),
                "wrapper contains a non-row child"
            );
        }
        assert_tree_consistent(&chapter);
    }

    #[test]
    fn th_rows_to_head_td_rows_to_body() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);
        let th_row = add_row(&mut chapter, table, true, 3);
        let td_row1 = add_row(&mut chapter, table, false, 3);
        let td_row2 = add_row(&mut chapter, table, false, 3);

        normalize_table_structure(&mut chapter);

        let sections: Vec<NodeId> = chapter.children(table).collect();
        assert_eq!(sections.len(), 2);
        assert_eq!(role_of(&chapter, sections[0]), Role::TableHead);
        assert_eq!(role_of(&chapter, sections[1]), Role::TableBody);
        assert_eq!(
            chapter.children(sections[0]).collect::<Vec<_>>(),
            vec![th_row]
        );
        assert_eq!(
            chapter.children(sections[1]).collect::<Vec<_>>(),
            vec![td_row1, td_row2]
        );
        assert_tree_consistent(&chapter);
    }

    #[test]
    fn already_normalized_table_unchanged() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);
        let caption = chapter.alloc_node(Node::new(Role::Caption));
        chapter.append_child(table, caption);
        let head = chapter.alloc_node(Node::new(Role::TableHead));
        chapter.append_child(table, head);
        add_row(&mut chapter, head, true, 2);
        let body = chapter.alloc_node(Node::new(Role::TableBody));
        chapter.append_child(table, body);
        add_row(&mut chapter, body, false, 2);

        let before: Vec<NodeId> = chapter.iter_dfs().collect();
        let count_before = chapter.node_count();

        normalize_table_structure(&mut chapter);

        assert_eq!(chapter.node_count(), count_before, "no nodes allocated");
        assert_eq!(
            chapter.iter_dfs().collect::<Vec<_>>(),
            before,
            "tree structure unchanged"
        );
        assert_tree_consistent(&chapter);
    }

    #[test]
    fn normalization_is_idempotent() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);
        let caption = chapter.alloc_node(Node::new(Role::Caption));
        chapter.append_child(table, caption);
        add_row(&mut chapter, table, true, 2);
        add_row(&mut chapter, table, false, 2);

        normalize_table_structure(&mut chapter);
        let after_first: Vec<NodeId> = chapter.iter_dfs().collect();
        let count_first = chapter.node_count();

        normalize_table_structure(&mut chapter);

        assert_eq!(chapter.node_count(), count_first);
        assert_eq!(chapter.iter_dfs().collect::<Vec<_>>(), after_first);
        assert_tree_consistent(&chapter);
    }

    #[test]
    fn empty_table_gets_empty_body() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);

        normalize_table_structure(&mut chapter);

        assert_eq!(child_roles(&chapter, table), vec![Role::TableBody]);
        let body = chapter.children(table).next().unwrap();
        assert_eq!(chapter.node(body).unwrap().parent, Some(table));
        assert_eq!(chapter.children(body).count(), 0);
        assert_tree_consistent(&chapter);
    }
}
