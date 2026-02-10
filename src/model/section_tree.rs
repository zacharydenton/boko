//! Section tree extraction from book IR.
//!
//! Transforms boko's flat IR tree (where headings are siblings of paragraphs)
//! into a hierarchical section tree (where content is nested under headings).

use std::io;

use super::chapter::Chapter;
use super::node::{NodeId, Role};
use super::Book;
use crate::util::strip_ebook_chars;

// ============================================================================
// Public Types
// ============================================================================

/// A book's content as a hierarchical section tree.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "cli", derive(serde::Serialize))]
pub struct SectionTree {
    pub title: String,
    pub authors: Vec<String>,
    pub language: String,
    #[cfg_attr(feature = "cli", serde(skip_serializing_if = "Vec::is_empty"))]
    pub preamble: Vec<ContentBlock>,
    pub sections: Vec<SectionNode>,
}

/// A section defined by a heading and its content.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "cli", derive(serde::Serialize))]
pub struct SectionNode {
    /// Heading level (1-6).
    pub level: u8,
    /// Heading text.
    pub title: String,
    /// Content blocks before any child section.
    #[cfg_attr(feature = "cli", serde(skip_serializing_if = "Vec::is_empty"))]
    pub content: Vec<ContentBlock>,
    /// Subsections (headings at a deeper level).
    #[cfg_attr(feature = "cli", serde(skip_serializing_if = "Vec::is_empty"))]
    pub children: Vec<SectionNode>,
}

/// An atomic content block.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "cli", derive(serde::Serialize))]
#[cfg_attr(feature = "cli", serde(tag = "type", rename_all = "snake_case"))]
pub enum ContentBlock {
    Paragraph {
        text: String,
    },
    CodeBlock {
        #[cfg_attr(feature = "cli", serde(skip_serializing_if = "Option::is_none"))]
        language: Option<String>,
        code: String,
    },
    BlockQuote {
        text: String,
    },
    List {
        ordered: bool,
        items: Vec<String>,
    },
    Table {
        #[cfg_attr(feature = "cli", serde(skip_serializing_if = "Vec::is_empty"))]
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Image {
        src: String,
        alt: String,
    },
    Rule,
}

// ============================================================================
// Extraction
// ============================================================================

/// Extract a section tree from a book.
///
/// Walks each chapter's IR tree, splits on heading nodes, and nests content
/// under headings to form a hierarchical section tree.
pub fn extract_section_tree(book: &mut Book) -> io::Result<SectionTree> {
    let meta = book.metadata().clone();
    let spine: Vec<_> = book.spine().to_vec();

    let mut events = Vec::new();
    for entry in &spine {
        let chapter = book.load_chapter(entry.id)?;
        collect_events(&chapter, NodeId::ROOT, &mut events);
    }

    let (preamble, sections) = nest_events(&events);

    Ok(SectionTree {
        title: meta.title,
        authors: meta.authors,
        language: meta.language,
        preamble,
        sections,
    })
}

// ============================================================================
// Event collection (pass 1: flatten IR into heading/content events)
// ============================================================================

enum Event {
    Heading { level: u8, title: String },
    Content(ContentBlock),
}

/// Walk the IR tree, emitting heading and content events.
/// Containers and Root are transparent — their children are processed directly.
fn collect_events(chapter: &Chapter, node_id: NodeId, events: &mut Vec<Event>) {
    let Some(node) = chapter.node(node_id) else {
        return;
    };

    match node.role {
        Role::Root | Role::Container => {
            for child_id in chapter.children(node_id) {
                collect_events(chapter, child_id, events);
            }
        }

        Role::Heading(level) => {
            let title = collect_text(chapter, node_id).trim().to_string();
            if !title.is_empty() {
                events.push(Event::Heading { level, title });
            }
        }

        Role::Paragraph | Role::Caption => {
            let text = collect_text(chapter, node_id).trim().to_string();
            if !text.is_empty() {
                events.push(Event::Content(ContentBlock::Paragraph { text }));
            }
        }

        Role::CodeBlock => {
            let code = collect_text_verbatim(chapter, node_id);
            let language = chapter.semantics.language(node_id).map(String::from);
            if !code.trim().is_empty() {
                events.push(Event::Content(ContentBlock::CodeBlock { language, code }));
            }
        }

        Role::BlockQuote | Role::Sidebar => {
            let text = collect_text(chapter, node_id).trim().to_string();
            if !text.is_empty() {
                events.push(Event::Content(ContentBlock::BlockQuote { text }));
            }
        }

        Role::OrderedList | Role::UnorderedList => {
            let ordered = node.role == Role::OrderedList;
            let items = collect_list_items(chapter, node_id);
            if !items.is_empty() {
                events.push(Event::Content(ContentBlock::List { ordered, items }));
            }
        }

        Role::DefinitionList => {
            let items = collect_definition_items(chapter, node_id);
            if !items.is_empty() {
                events.push(Event::Content(ContentBlock::List {
                    ordered: false,
                    items,
                }));
            }
        }

        Role::Table => {
            let (headers, rows) = collect_table(chapter, node_id);
            if !rows.is_empty() || !headers.is_empty() {
                events.push(Event::Content(ContentBlock::Table { headers, rows }));
            }
        }

        Role::Image => {
            let src = chapter.semantics.src(node_id).unwrap_or("").to_string();
            let alt = chapter.semantics.alt(node_id).unwrap_or("").to_string();
            if !src.is_empty() {
                events.push(Event::Content(ContentBlock::Image { src, alt }));
            }
        }

        Role::Figure => {
            for child_id in chapter.children(node_id) {
                collect_events(chapter, child_id, events);
            }
        }

        Role::Rule => {
            events.push(Event::Content(ContentBlock::Rule));
        }

        // Inline-level or structural nodes handled by their parents.
        _ => {}
    }
}

// ============================================================================
// Nesting (pass 2: group flat events into a section tree)
// ============================================================================

fn nest_events(events: &[Event]) -> (Vec<ContentBlock>, Vec<SectionNode>) {
    let mut preamble = Vec::new();
    let mut i = 0;

    // Collect preamble (content before first heading)
    while i < events.len() {
        match &events[i] {
            Event::Content(block) => {
                preamble.push(block.clone());
                i += 1;
            }
            Event::Heading { .. } => break,
        }
    }

    let (sections, _) = parse_siblings(events, i, 0);
    (preamble, sections)
}

/// Parse sibling sections. Stops when a heading with level < min_level is hit.
fn parse_siblings(events: &[Event], mut i: usize, min_level: u8) -> (Vec<SectionNode>, usize) {
    let mut sections = Vec::new();

    while i < events.len() {
        match &events[i] {
            Event::Heading { level, .. } if *level < min_level => break,
            Event::Heading { level, title } => {
                let level = *level;
                let title = title.clone();
                let mut content = Vec::new();
                i += 1;

                // Collect content until next heading
                while i < events.len() {
                    match &events[i] {
                        Event::Heading { .. } => break,
                        Event::Content(block) => {
                            content.push(block.clone());
                            i += 1;
                        }
                    }
                }

                // Recurse for child sections (deeper headings)
                let (children, next_i) = parse_siblings(events, i, level + 1);
                i = next_i;

                sections.push(SectionNode {
                    level,
                    title,
                    content,
                    children,
                });
            }
            Event::Content(_) => {
                i += 1;
            }
        }
    }

    (sections, i)
}

// ============================================================================
// Text collection helpers
// ============================================================================

fn collect_text(chapter: &Chapter, node_id: NodeId) -> String {
    let mut result = String::new();
    collect_text_recursive(chapter, node_id, &mut result);
    strip_ebook_chars(&result)
}

fn collect_text_recursive(chapter: &Chapter, node_id: NodeId, result: &mut String) {
    let Some(node) = chapter.node(node_id) else {
        return;
    };

    if node.role == Role::Footnote {
        return;
    }

    if node.role == Role::Break {
        if !result.is_empty() && !result.ends_with(' ') {
            result.push(' ');
        }
        return;
    }

    if node.role == Role::Text && !node.text.is_empty() {
        let text = chapter.text(node.text);
        let has_leading = text.starts_with(char::is_whitespace);
        let has_trailing = text.ends_with(char::is_whitespace);
        let words: Vec<&str> = text.split_whitespace().collect();

        if !words.is_empty() {
            if has_leading && !result.is_empty() && !result.ends_with(' ') {
                result.push(' ');
            }
            result.push_str(&words.join(" "));
            if has_trailing {
                result.push(' ');
            }
        } else if !text.is_empty() && !result.is_empty() && !result.ends_with(' ') {
            result.push(' ');
        }
    }

    for child_id in chapter.children(node_id) {
        collect_text_recursive(chapter, child_id, result);
    }
}

fn collect_text_verbatim(chapter: &Chapter, node_id: NodeId) -> String {
    let mut result = String::new();
    collect_text_verbatim_recursive(chapter, node_id, &mut result);
    strip_ebook_chars(&result)
}

fn collect_text_verbatim_recursive(chapter: &Chapter, node_id: NodeId, result: &mut String) {
    let Some(node) = chapter.node(node_id) else {
        return;
    };

    if node.role == Role::Text && !node.text.is_empty() {
        result.push_str(chapter.text(node.text));
    }

    for child_id in chapter.children(node_id) {
        collect_text_verbatim_recursive(chapter, child_id, result);
    }
}

// ============================================================================
// Structured content extraction
// ============================================================================

fn collect_list_items(chapter: &Chapter, list_id: NodeId) -> Vec<String> {
    chapter
        .children(list_id)
        .filter_map(|child_id| {
            let child = chapter.node(child_id)?;
            if child.role == Role::ListItem {
                let text = collect_text(chapter, child_id).trim().to_string();
                if text.is_empty() { None } else { Some(text) }
            } else {
                None
            }
        })
        .collect()
}

fn collect_definition_items(chapter: &Chapter, dl_id: NodeId) -> Vec<String> {
    let mut items = Vec::new();
    let mut current_term: Option<String> = None;

    for child_id in chapter.children(dl_id) {
        let Some(child) = chapter.node(child_id) else {
            continue;
        };
        match child.role {
            Role::DefinitionTerm => {
                current_term = Some(collect_text(chapter, child_id).trim().to_string());
            }
            Role::DefinitionDescription => {
                let desc = collect_text(chapter, child_id).trim().to_string();
                if let Some(term) = current_term.take() {
                    items.push(format!("{}: {}", term, desc));
                } else if !desc.is_empty() {
                    items.push(desc);
                }
            }
            _ => {}
        }
    }

    items
}

fn collect_table(chapter: &Chapter, table_id: NodeId) -> (Vec<String>, Vec<Vec<String>>) {
    let mut headers = Vec::new();
    let mut rows = Vec::new();

    for section_id in chapter.children(table_id) {
        let Some(section) = chapter.node(section_id) else {
            continue;
        };
        match section.role {
            Role::TableRow => {
                let cells = collect_row_cells(chapter, section_id);
                if is_header_row(chapter, section_id) && headers.is_empty() {
                    headers = cells;
                } else {
                    rows.push(cells);
                }
            }
            Role::TableHead | Role::TableBody => {
                for row_id in chapter.children(section_id) {
                    let Some(row) = chapter.node(row_id) else {
                        continue;
                    };
                    if row.role == Role::TableRow {
                        let cells = collect_row_cells(chapter, row_id);
                        if section.role == Role::TableHead && headers.is_empty() {
                            headers = cells;
                        } else {
                            rows.push(cells);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    (headers, rows)
}

fn collect_row_cells(chapter: &Chapter, row_id: NodeId) -> Vec<String> {
    chapter
        .children(row_id)
        .filter_map(|id| {
            let node = chapter.node(id)?;
            if node.role == Role::TableCell {
                Some(collect_text(chapter, id).trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

fn is_header_row(chapter: &Chapter, row_id: NodeId) -> bool {
    chapter
        .children(row_id)
        .any(|id| chapter.semantics.is_header_cell(id))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    fn make_section_tree(
        build: impl FnOnce(&mut Chapter),
    ) -> (Vec<ContentBlock>, Vec<SectionNode>) {
        let mut chapter = Chapter::new();
        build(&mut chapter);
        let mut events = Vec::new();
        collect_events(&chapter, NodeId::ROOT, &mut events);
        nest_events(&events)
    }

    fn add_heading(chapter: &mut Chapter, level: u8, text: &str) {
        let h = chapter.alloc_node(Node::new(Role::Heading(level)));
        chapter.append_child(NodeId::ROOT, h);
        let range = chapter.append_text(text);
        let t = chapter.alloc_node(Node::text(range));
        chapter.append_child(h, t);
    }

    fn add_paragraph(chapter: &mut Chapter, text: &str) {
        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);
        let range = chapter.append_text(text);
        let t = chapter.alloc_node(Node::text(range));
        chapter.append_child(p, t);
    }

    #[test]
    fn preamble_only() {
        let (preamble, sections) = make_section_tree(|ch| {
            add_paragraph(ch, "No headings here.");
        });
        assert_eq!(preamble.len(), 1);
        assert!(sections.is_empty());
    }

    #[test]
    fn single_section() {
        let (preamble, sections) = make_section_tree(|ch| {
            add_heading(ch, 1, "Title");
            add_paragraph(ch, "Content");
        });
        assert!(preamble.is_empty());
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "Title");
        assert_eq!(sections[0].level, 1);
        assert_eq!(sections[0].content.len(), 1);
    }

    #[test]
    fn nested_sections() {
        let (_, sections) = make_section_tree(|ch| {
            add_heading(ch, 1, "Chapter");
            add_paragraph(ch, "Intro");
            add_heading(ch, 2, "Section A");
            add_paragraph(ch, "A content");
            add_heading(ch, 2, "Section B");
            add_paragraph(ch, "B content");
        });
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].content.len(), 1); // "Intro"
        assert_eq!(sections[0].children.len(), 2);
        assert_eq!(sections[0].children[0].title, "Section A");
        assert_eq!(sections[0].children[1].title, "Section B");
    }

    #[test]
    fn sibling_top_level() {
        let (_, sections) = make_section_tree(|ch| {
            add_heading(ch, 1, "One");
            add_paragraph(ch, "First");
            add_heading(ch, 1, "Two");
            add_paragraph(ch, "Second");
        });
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "One");
        assert_eq!(sections[1].title, "Two");
    }

    #[test]
    fn skipped_levels() {
        let (_, sections) = make_section_tree(|ch| {
            add_heading(ch, 1, "Top");
            add_heading(ch, 3, "Deep");
            add_paragraph(ch, "Content");
        });
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].children.len(), 1);
        assert_eq!(sections[0].children[0].level, 3);
        assert_eq!(sections[0].children[0].content.len(), 1);
    }

    #[test]
    fn preamble_then_sections() {
        let (preamble, sections) = make_section_tree(|ch| {
            add_paragraph(ch, "Preamble");
            add_heading(ch, 1, "Chapter 1");
            add_paragraph(ch, "Content");
        });
        assert_eq!(preamble.len(), 1);
        assert_eq!(sections.len(), 1);
    }

    #[test]
    fn deep_nesting() {
        let (_, sections) = make_section_tree(|ch| {
            add_heading(ch, 1, "H1");
            add_heading(ch, 2, "H2");
            add_heading(ch, 3, "H3");
            add_paragraph(ch, "Deep content");
        });
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].children.len(), 1);
        assert_eq!(sections[0].children[0].children.len(), 1);
        assert_eq!(sections[0].children[0].children[0].title, "H3");
        assert_eq!(sections[0].children[0].children[0].content.len(), 1);
    }

    #[test]
    fn content_between_children() {
        let (_, sections) = make_section_tree(|ch| {
            add_heading(ch, 1, "Parent");
            add_paragraph(ch, "Parent intro");
            add_heading(ch, 2, "Child A");
            add_paragraph(ch, "A content");
            add_paragraph(ch, "Still A");
            add_heading(ch, 2, "Child B");
            add_paragraph(ch, "B content");
        });
        assert_eq!(sections[0].content.len(), 1); // "Parent intro"
        assert_eq!(sections[0].children[0].content.len(), 2); // "A content" + "Still A"
        assert_eq!(sections[0].children[1].content.len(), 1); // "B content"
    }

    #[test]
    fn container_transparency() {
        let (_, sections) = make_section_tree(|ch| {
            // Wrap heading and paragraph in a Container (like <section> or <div>)
            let container = ch.alloc_node(Node::new(Role::Container));
            ch.append_child(NodeId::ROOT, container);

            let h = ch.alloc_node(Node::new(Role::Heading(1)));
            ch.append_child(container, h);
            let range = ch.append_text("Inside Container");
            let t = ch.alloc_node(Node::text(range));
            ch.append_child(h, t);

            let p = ch.alloc_node(Node::new(Role::Paragraph));
            ch.append_child(container, p);
            let range = ch.append_text("Container content");
            let t = ch.alloc_node(Node::text(range));
            ch.append_child(p, t);
        });
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "Inside Container");
        assert_eq!(sections[0].content.len(), 1);
    }

    #[test]
    fn empty_headings_skipped() {
        let (_, sections) = make_section_tree(|ch| {
            // Heading with no text content
            let h = ch.alloc_node(Node::new(Role::Heading(1)));
            ch.append_child(NodeId::ROOT, h);

            add_heading(ch, 1, "Real Title");
            add_paragraph(ch, "Content");
        });
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "Real Title");
    }
}
