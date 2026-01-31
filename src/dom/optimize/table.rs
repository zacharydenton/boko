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
