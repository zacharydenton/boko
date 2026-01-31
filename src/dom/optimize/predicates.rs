//! Shared predicates for optimization passes.

use crate::model::{Chapter, NodeId, Role};

/// Check if a role is a structural container where inter-element whitespace is irrelevant.
///
/// These are containers that should only contain other block elements, not raw text.
/// Whitespace between their children is formatting noise (indentation, newlines).
///
/// Safe: Root, Container (div/section), Table, TableRow, Lists, Figure
/// Unsafe: Paragraph (contains inline content), Heading, Inline, etc.
pub fn is_structural_container(role: Option<Role>) -> bool {
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

/// Check if a role is a block container that can have mixed content.
pub fn is_block_container(role: Option<Role>) -> bool {
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
pub fn is_inline_role(role: Role) -> bool {
    matches!(
        role,
        Role::Text | Role::Inline | Role::Link | Role::Image | Role::Break
    )
}

/// Check if a role can be pruned when empty.
pub fn is_prunable_role(role: Role) -> bool {
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

/// Check if a node has any semantic attributes that prevent optimization.
pub fn has_semantic_attrs(chapter: &Chapter, node_id: NodeId) -> bool {
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
