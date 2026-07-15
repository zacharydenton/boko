//! Core IR → Markdown rendering.
//!
//! This module provides pure rendering logic that transforms the book IR
//! into Markdown strings. No I/O is performed here - the export layer
//! handles writing to files/writers.

use std::collections::HashMap;

use crate::import::ChapterId;
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, NodeId, ResolvedLinks, Role};
use crate::style::Display;
use crate::util::strip_ebook_chars;

use super::escape::escape_markdown_at;
use super::escape::{calculate_fence_length, calculate_inline_code_ticks};

/// Result of rendering a chapter to markdown.
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// The rendered markdown content.
    pub content: String,
    /// Accumulated footnotes for end-of-document rendering.
    pub footnotes: Vec<Footnote>,
}

/// Footnote collected during rendering.
#[derive(Debug, Clone)]
pub struct Footnote {
    /// Footnote number (1-based).
    pub number: usize,
    /// The collected text content.
    pub content: String,
}

/// Tracks list context for numbering.
#[derive(Debug, Clone)]
struct ListContext {
    /// Whether this is an ordered list.
    is_ordered: bool,
    /// Current item counter.
    counter: usize,
    /// Whether this is a tight list (no blank lines between items).
    is_tight: bool,
}

/// Context for rendering (pure string accumulation, no I/O).
pub struct RenderContext<'a> {
    chapter: &'a Chapter,
    chapter_id: ChapterId,
    resolved: &'a ResolvedLinks,
    heading_slugs: &'a HashMap<GlobalNodeId, String>,
    // Accumulated output
    output: String,
    footnotes: Vec<Footnote>,
    // Formatting state
    line_prefix: String,
    list_stack: Vec<ListContext>,
    at_line_start: bool,
    has_line_content: bool,
    pending_newline: bool,
    last_block_role: Option<Role>,
    // Current recursion depth, to bound stack usage on hostile trees.
    depth: usize,
    // Number of footnotes emitted by earlier chapters, so labels stay unique
    // once chapters are concatenated into one markdown document.
    footnote_start: usize,
}

impl<'a> RenderContext<'a> {
    /// Create a new render context for a chapter.
    pub fn new(
        chapter: &'a Chapter,
        chapter_id: ChapterId,
        resolved: &'a ResolvedLinks,
        heading_slugs: &'a HashMap<GlobalNodeId, String>,
        footnote_start: usize,
    ) -> Self {
        Self {
            chapter,
            chapter_id,
            resolved,
            heading_slugs,
            output: String::new(),
            footnotes: Vec::new(),
            line_prefix: String::new(),
            list_stack: Vec::new(),
            at_line_start: true,
            has_line_content: false,
            pending_newline: false,
            last_block_role: None,
            depth: 0,
            footnote_start,
        }
    }

    /// Render the chapter, consuming the context and returning the result.
    pub fn render(mut self) -> RenderResult {
        // Walk children of root
        for child_id in self.chapter.children(NodeId::ROOT) {
            self.walk_node(child_id);
        }

        // Ensure final newline
        if !self.at_line_start {
            self.output.push('\n');
        }

        RenderResult {
            content: self.output,
            footnotes: self.footnotes,
        }
    }

    /// Check if a list is "tight" (items are single paragraphs, no blank lines between).
    fn is_tight_list(&self, list_id: NodeId) -> bool {
        for item_id in self.chapter.children(list_id) {
            let Some(item) = self.chapter.node(item_id) else {
                continue;
            };
            if item.role != Role::ListItem {
                continue;
            }

            // Count block-level children
            let mut block_count = 0;
            for child_id in self.chapter.children(item_id) {
                let Some(child) = self.chapter.node(child_id) else {
                    continue;
                };
                match child.role {
                    Role::Paragraph => {
                        block_count += 1;
                    }
                    Role::BlockQuote
                    | Role::OrderedList
                    | Role::UnorderedList
                    | Role::DefinitionList
                    | Role::CodeBlock
                    | Role::Table
                    | Role::Figure => {
                        // Nested block elements make it loose
                        return false;
                    }
                    _ => {}
                }
            }

            // More than one block-level child means loose
            if block_count > 1 {
                return false;
            }
        }
        true
    }

    /// Write an HTML anchor if this node is targeted by internal links.
    fn write_anchor_if_targeted(&mut self, node_id: NodeId) {
        let global_id = GlobalNodeId::new(self.chapter_id, node_id);
        if self.resolved.is_internal_target(global_id) {
            // Skip headings - they get automatic slug-based IDs
            if let Some(node) = self.chapter.node(node_id)
                && matches!(node.role, Role::Heading(_))
            {
                return;
            }
            self.ensure_line_started();
            self.output.push_str(&format!(
                "<a id=\"c{}n{}\"></a>",
                self.chapter_id.0, node_id.0
            ));
        }
    }

    /// Ensure we're ready to write content (write prefix if at line start).
    fn ensure_line_started(&mut self) {
        if self.at_line_start {
            self.output.push_str(&self.line_prefix);
            self.at_line_start = false;
        }
    }

    /// Write a newline.
    fn write_newline(&mut self) {
        self.output.push('\n');
        self.at_line_start = true;
        self.has_line_content = false;
    }

    /// Write a hard line break (backslash in markdown).
    fn write_hard_break(&mut self) {
        self.output.push('\\');
        self.write_newline();
    }

    /// Start a new block element.
    fn start_block(&mut self) {
        if self.pending_newline {
            if !self.at_line_start {
                self.write_newline();
            }
            self.write_newline();
            self.pending_newline = false;
        }
        self.ensure_line_started();
    }

    /// End a block element.
    fn end_block(&mut self, role: Role) {
        self.pending_newline = true;
        self.last_block_role = Some(role);
    }

    /// Check if we need a separator between adjacent lists.
    fn needs_list_separator(&self, current_role: Role) -> bool {
        matches!(
            (self.last_block_role, current_role),
            (Some(Role::OrderedList), Role::OrderedList)
                | (Some(Role::UnorderedList), Role::UnorderedList)
                | (Some(Role::DefinitionList), Role::DefinitionList)
        )
    }

    /// Write a list separator comment (for adjacent lists).
    fn write_list_separator(&mut self) {
        if !self.at_line_start {
            self.write_newline();
        }
        self.write_newline();
        self.ensure_line_started();
        self.output.push_str("<!-- -->\n");
        self.at_line_start = true;
    }

    fn walk_node(&mut self, id: NodeId) {
        let Some(node) = self.chapter.node(id) else {
            return;
        };

        // Output anchor if this node is a link target
        self.write_anchor_if_targeted(id);

        let role = node.role;

        match role {
            Role::Text => {
                if !node.text.is_empty() {
                    let text = self.chapter.text(node.text);
                    self.write_text(text);
                }
            }

            Role::Paragraph => {
                self.start_block();
                self.walk_children(id);
                self.end_block(role);
            }

            Role::Heading(level) => {
                self.start_block();
                for _ in 0..level {
                    self.output.push('#');
                }
                self.output.push(' ');
                self.walk_children(id);
                self.end_block(role);
            }

            Role::OrderedList => {
                if self.needs_list_separator(role) {
                    self.write_list_separator();
                }
                self.start_block();
                let start = self.chapter.semantics.list_start(id).unwrap_or(1) as usize;
                let is_tight = self.is_tight_list(id);
                self.list_stack.push(ListContext {
                    is_ordered: true,
                    counter: start.saturating_sub(1),
                    is_tight,
                });
                self.walk_children(id);
                self.list_stack.pop();
                self.end_block(role);
            }

            Role::UnorderedList => {
                if self.needs_list_separator(role) {
                    self.write_list_separator();
                }
                self.start_block();
                let is_tight = self.is_tight_list(id);
                self.list_stack.push(ListContext {
                    is_ordered: false,
                    counter: 0,
                    is_tight,
                });
                self.walk_children(id);
                self.list_stack.pop();
                self.end_block(role);
            }

            Role::ListItem => {
                let (is_tight, counter) = self
                    .list_stack
                    .last()
                    .map(|ctx| (ctx.is_tight, ctx.counter))
                    .unwrap_or((true, 0));

                // For loose lists, add blank line before items (except the first)
                if !is_tight && counter > 0 {
                    if !self.at_line_start {
                        self.write_newline();
                    }
                    self.write_newline();
                } else if !self.at_line_start {
                    self.write_newline();
                }

                self.ensure_line_started();

                // Get bullet/number from parent list
                let bullet = if let Some(list_ctx) = self.list_stack.last_mut() {
                    list_ctx.counter += 1;
                    if list_ctx.is_ordered {
                        format!("{}. ", list_ctx.counter)
                    } else {
                        "- ".to_string()
                    }
                } else {
                    String::new()
                };

                self.output.push_str(&bullet);

                // Set continuation indent for subsequent lines
                let old_prefix = self.line_prefix.clone();
                self.line_prefix.push_str(&" ".repeat(bullet.len()));

                self.walk_children(id);

                self.line_prefix = old_prefix;
                self.pending_newline = false;
            }

            Role::BlockQuote => {
                if self.pending_newline {
                    if !self.at_line_start {
                        self.write_newline();
                    }
                    self.write_newline();
                    self.pending_newline = false;
                }

                let prefix = "> ";

                if !self.at_line_start {
                    self.output.push_str(prefix);
                }

                let old_prefix = self.line_prefix.clone();
                self.line_prefix.push_str(prefix);

                self.walk_children(id);

                self.line_prefix = old_prefix;
                self.end_block(role);
            }

            Role::Link => {
                self.ensure_line_started();

                // Look up resolved target
                let global_id = GlobalNodeId::new(self.chapter_id, id);
                let anchor = match self.resolved.get(global_id) {
                    Some(AnchorTarget::External(url)) => url.clone(),
                    Some(AnchorTarget::Internal(target)) => {
                        if let Some(slug) = self.heading_slugs.get(target) {
                            format!("#{}", slug)
                        } else {
                            format!("#c{}n{}", target.chapter.0, target.node.0)
                        }
                    }
                    Some(AnchorTarget::Chapter(chapter_id)) => {
                        format!("#c{}", chapter_id.0)
                    }
                    None => self.chapter.semantics.href(id).unwrap_or("").to_string(),
                };

                // Skip link formatting if anchor is empty
                if anchor.is_empty() {
                    self.walk_children(id);
                } else {
                    self.output.push('[');
                    self.walk_children(id);
                    self.output.push_str(&format!("]({})", anchor));
                }
            }

            Role::Image => {
                self.start_block();
                let alt = self.chapter.semantics.alt(id).unwrap_or("image");
                let src = self.chapter.semantics.src(id).unwrap_or("");
                self.output.push_str(&format!("![{}]({})", alt, src));
                self.end_block(role);
            }

            Role::Break => {
                self.write_hard_break();
            }

            Role::Rule => {
                self.start_block();
                self.output.push_str("---");
                self.end_block(role);
            }

            Role::Table => {
                self.start_block();
                // The optimizer (normalize_table_structure) wraps rows in
                // TableHead/TableBody, so descend one level into section
                // wrappers when collecting rows. Header rows are emitted
                // first regardless of section order in the tree.
                let mut header_rows: Vec<NodeId> = Vec::new();
                let mut body_rows: Vec<NodeId> = Vec::new();
                for child_id in self.chapter.children(id) {
                    let Some(child) = self.chapter.node(child_id) else {
                        continue;
                    };
                    match child.role {
                        Role::TableHead => {
                            header_rows.extend(self.chapter.children(child_id));
                        }
                        Role::TableBody => {
                            body_rows.extend(self.chapter.children(child_id));
                        }
                        Role::TableRow => body_rows.push(child_id),
                        _ => {}
                    }
                }
                // GFM requires a delimiter row after the header so parsers
                // recognize the block as a table rather than plain text.
                // Rows from TableHead are the header; without a head, the
                // first row serves as the header.
                let header_count = if header_rows.is_empty() {
                    1
                } else {
                    header_rows.len()
                };
                let rows: Vec<NodeId> = header_rows.into_iter().chain(body_rows).collect();
                for (i, row_id) in rows.iter().enumerate() {
                    let cells = self.table_cells(*row_id);
                    self.ensure_line_started();
                    self.output.push_str("| ");
                    self.output.push_str(&cells.join(" | "));
                    self.output.push_str(" |");
                    self.write_newline();
                    if i + 1 == header_count {
                        self.ensure_line_started();
                        self.output.push('|');
                        for _ in 0..cells.len().max(1) {
                            self.output.push_str(" --- |");
                        }
                        self.write_newline();
                    }
                }
                self.end_block(role);
            }

            Role::TableRow => {
                // Reached only for a stray row outside a Table; render the cells
                // as a GFM row (escaped) without a delimiter.
                let cells = self.table_cells(id);
                self.ensure_line_started();
                self.output.push_str("| ");
                self.output.push_str(&cells.join(" | "));
                self.output.push_str(" |");
                self.write_newline();
            }

            Role::TableCell => {
                self.walk_children(id);
            }

            Role::Figure => {
                self.start_block();
                self.walk_children(id);
                self.end_block(role);
            }

            Role::Caption => {
                self.start_block();
                self.output.push('*');
                self.walk_children(id);
                self.output.push('*');
                self.end_block(role);
            }

            Role::Footnote => {
                self.ensure_line_started();
                let text = self.collect_text(id);
                let note_num = self.footnote_start + self.footnotes.len() + 1;
                self.footnotes.push(Footnote {
                    number: note_num,
                    content: text,
                });
                self.output.push_str(&format!("[^{}]", note_num));
            }

            Role::Sidebar => {
                self.start_block();
                self.output.push_str("> **Sidebar**");
                self.write_newline();
                self.ensure_line_started();
                self.walk_children(id);
                self.end_block(role);
            }

            Role::Inline => {
                let style = self.chapter.styles.get(node.style);
                let is_bold = style.map(|s| s.is_bold()).unwrap_or(false);
                let is_italic = style.map(|s| s.is_italic()).unwrap_or(false);
                let is_code = style.map(|s| s.is_monospace()).unwrap_or(false);
                let is_block = node.style.0 != 0
                    && style.map(|s| s.display == Display::Block).unwrap_or(false);

                // Handle block-display inlines (e.g., verse lines)
                if is_block && self.has_line_content {
                    self.write_hard_break();
                }

                if is_code {
                    self.ensure_line_started();
                    let content = self.collect_text(id);
                    let tick_count = calculate_inline_code_ticks(&content);
                    let ticks: String = std::iter::repeat_n('`', tick_count).collect();

                    let spacer = if content.starts_with('`') || content.ends_with('`') {
                        " "
                    } else {
                        ""
                    };

                    self.output.push_str(&format!(
                        "{}{}{}{}{}",
                        ticks, spacer, content, spacer, ticks
                    ));
                } else {
                    self.ensure_line_started();
                    if is_bold {
                        self.output.push_str("**");
                    }
                    if is_italic {
                        self.output.push('*');
                    }

                    self.walk_children(id);

                    if is_italic {
                        self.output.push('*');
                    }
                    if is_bold {
                        self.output.push_str("**");
                    }
                }
            }

            Role::DefinitionList => {
                if self.needs_list_separator(role) {
                    self.write_list_separator();
                }
                self.start_block();
                self.walk_children(id);
                self.end_block(role);
            }

            Role::DefinitionTerm => {
                self.start_block();
                self.output.push_str("**");
                self.walk_children(id);
                self.output.push_str("**");
                self.pending_newline = false;
            }

            Role::DefinitionDescription => {
                if !self.at_line_start {
                    self.write_newline();
                }
                self.ensure_line_started();
                self.output.push_str(": ");
                self.walk_children(id);
                self.end_block(role);
            }

            Role::CodeBlock => {
                self.start_block();
                let text = self.collect_text_verbatim(id);

                if !self.at_line_start {
                    self.write_newline();
                }
                let lang = self.chapter.semantics.language(id).unwrap_or("");
                let fence_len = calculate_fence_length(&text, '`');
                let fence: String = std::iter::repeat_n('`', fence_len).collect();
                self.ensure_line_started();
                self.output.push_str(&format!("{}{}\n", fence, lang));
                self.at_line_start = true;

                for line in text.lines() {
                    self.ensure_line_started();
                    self.output.push_str(&format!("{}\n", line));
                    self.at_line_start = true;
                }

                self.ensure_line_started();
                self.output.push_str(&fence);
                self.end_block(role);
            }

            Role::Container | Role::Root | Role::TableHead | Role::TableBody => {
                self.walk_children(id);
            }
        }
    }

    /// Collect the cell texts of a table row, escaped for a GFM table cell
    /// (backslashes, pipes, and inline markers escaped, newlines flattened
    /// to spaces).
    fn table_cells(&mut self, row_id: NodeId) -> Vec<String> {
        let cell_ids: Vec<NodeId> = self.chapter.children(row_id).collect();
        cell_ids
            .into_iter()
            .map(|cell| {
                self.collect_text(cell)
                    .replace('\\', "\\\\")
                    .replace('|', "\\|")
                    .replace('*', "\\*")
                    .replace('[', "\\[")
                    .replace('\n', " ")
            })
            .collect()
    }

    fn walk_children(&mut self, id: NodeId) {
        // Bound recursion depth: a hostile chapter can nest arbitrarily deep.
        // All descent flows through here, so guarding this one site suffices.
        if self.depth > crate::util::MAX_TREE_DEPTH {
            return;
        }
        self.depth += 1;
        for child_id in self.chapter.children(id) {
            self.walk_node(child_id);
        }
        self.depth -= 1;
    }

    fn write_text(&mut self, text: &str) {
        // Whether this chunk lands at a block-start position: no inline
        // content has been written on the current line yet (only the line
        // prefix and/or block markers like `- ` or `> `, after which
        // markdown still recognizes list/heading markers). Line-start-only
        // characters are escaped only in that position.
        let at_line_start = !self.has_line_content;
        self.ensure_line_started();

        let text = strip_ebook_chars(text);

        // Normalize internal whitespace while preserving leading/trailing
        let has_leading = text.starts_with(char::is_whitespace);
        let has_trailing = text.ends_with(char::is_whitespace);

        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            if !text.is_empty() {
                self.output.push(' ');
            }
            return;
        }

        if has_leading {
            self.output.push(' ');
        }

        let joined = words.join(" ");
        let escaped = escape_markdown_at(&joined, at_line_start);
        self.output.push_str(&escaped);
        self.has_line_content = true;

        if has_trailing {
            self.output.push(' ');
        }
    }

    /// Collect all text content from a node and its children.
    fn collect_text(&self, id: NodeId) -> String {
        self.collect_text_inner(id, false)
    }

    /// Collect text content preserving literal whitespace (for code blocks).
    fn collect_text_verbatim(&self, id: NodeId) -> String {
        self.collect_text_inner(id, true)
    }

    fn collect_text_inner(&self, id: NodeId, verbatim: bool) -> String {
        let mut result = String::new();
        self.collect_text_recursive(id, &mut result, verbatim);
        strip_ebook_chars(&result)
    }

    fn collect_text_recursive(&self, id: NodeId, result: &mut String, verbatim: bool) {
        let Some(node) = self.chapter.node(id) else {
            return;
        };

        if node.role == Role::Text && !node.text.is_empty() {
            let text = self.chapter.text(node.text);

            if verbatim {
                result.push_str(text);
            } else {
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
        }

        for child_id in self.chapter.children(id) {
            self.collect_text_recursive(child_id, result, verbatim);
        }
    }
}

/// Render a single chapter to markdown.
///
/// This is the main entry point for chapter rendering. It creates a
/// `RenderContext`, processes all nodes, and returns the result.
///
/// # Arguments
///
/// * `chapter` - The chapter to render
/// * `chapter_id` - The chapter's ID for building GlobalNodeIds
/// * `resolved` - Resolved links for internal link output
/// * `heading_slugs` - Map of heading targets to slugs
///
/// # Returns
///
/// A `RenderResult` containing the rendered markdown and any footnotes.
pub fn render_chapter(
    chapter: &Chapter,
    chapter_id: ChapterId,
    resolved: &ResolvedLinks,
    heading_slugs: &HashMap<GlobalNodeId, String>,
    footnote_start: usize,
) -> RenderResult {
    let ctx = RenderContext::new(chapter, chapter_id, resolved, heading_slugs, footnote_start);
    ctx.render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    fn render_to_string(chapter: &Chapter) -> String {
        let resolved = ResolvedLinks::default();
        let heading_slugs = HashMap::new();
        let result = render_chapter(chapter, ChapterId(0), &resolved, &heading_slugs, 0);
        result.content
    }

    #[test]
    fn test_simple_paragraph() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let text_range = chapter.append_text("Hello, World!");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(para, text_node);

        let result = render_to_string(&chapter);
        assert!(result.contains("Hello, World!"));
    }

    #[test]
    fn test_heading() {
        let mut chapter = Chapter::new();

        let h1 = chapter.alloc_node(Node::new(Role::Heading(1)));
        chapter.append_child(NodeId::ROOT, h1);

        let text_range = chapter.append_text("Chapter One");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(h1, text_node);

        let result = render_to_string(&chapter);
        assert!(result.contains("# Chapter One"));
    }

    #[test]
    fn test_unordered_list() {
        let mut chapter = Chapter::new();

        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);

        let li = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul, li);

        let text_range = chapter.append_text("Item one");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(li, text_node);

        let result = render_to_string(&chapter);
        assert!(result.contains("- Item one"));
    }

    #[test]
    fn test_ordered_list() {
        let mut chapter = Chapter::new();

        let ol = chapter.alloc_node(Node::new(Role::OrderedList));
        chapter.append_child(NodeId::ROOT, ol);

        for i in 1..=3 {
            let li = chapter.alloc_node(Node::new(Role::ListItem));
            chapter.append_child(ol, li);

            let text_range = chapter.append_text(&format!("Item {}", i));
            let text_node = chapter.alloc_node(Node::text(text_range));
            chapter.append_child(li, text_node);
        }

        let result = render_to_string(&chapter);
        assert!(result.contains("1. Item 1"));
        assert!(result.contains("2. Item 2"));
        assert!(result.contains("3. Item 3"));
    }

    #[test]
    fn test_footnote_accumulation() {
        let mut chapter = Chapter::new();

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);

        let t1 = chapter.append_text("Main text");
        let tn1 = chapter.alloc_node(Node::text(t1));
        chapter.append_child(p, tn1);

        let note = chapter.alloc_node(Node::new(Role::Footnote));
        chapter.append_child(p, note);
        let t2 = chapter.append_text("This is a footnote");
        let tn2 = chapter.alloc_node(Node::text(t2));
        chapter.append_child(note, tn2);

        let resolved = ResolvedLinks::default();
        let heading_slugs = HashMap::new();
        let result = render_chapter(&chapter, ChapterId(0), &resolved, &heading_slugs, 0);

        assert!(result.content.contains("[^1]"));
        assert_eq!(result.footnotes.len(), 1);
        assert_eq!(result.footnotes[0].number, 1);
        assert_eq!(result.footnotes[0].content, "This is a footnote");
    }

    #[test]
    fn footnote_numbering_honors_start_offset() {
        let mut chapter = Chapter::new();
        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);
        let note = chapter.alloc_node(Node::new(Role::Footnote));
        chapter.append_child(p, note);
        let t = chapter.append_text("note");
        let tn = chapter.alloc_node(Node::text(t));
        chapter.append_child(note, tn);

        let resolved = ResolvedLinks::default();
        let heading_slugs = HashMap::new();
        // Simulate two earlier chapters' footnotes: this one starts at 3.
        let result = render_chapter(&chapter, ChapterId(0), &resolved, &heading_slugs, 2);
        assert!(result.content.contains("[^3]"));
        assert_eq!(result.footnotes[0].number, 3);
    }

    #[test]
    fn table_emits_gfm_delimiter_and_escapes_pipes() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);
        for cells in [["A", "B|C"], ["1", "2"]] {
            let row = chapter.alloc_node(Node::new(Role::TableRow));
            chapter.append_child(table, row);
            for cell in cells {
                let c = chapter.alloc_node(Node::new(Role::TableCell));
                chapter.append_child(row, c);
                let t = chapter.append_text(cell);
                let tn = chapter.alloc_node(Node::text(t));
                chapter.append_child(c, tn);
            }
        }

        let resolved = ResolvedLinks::default();
        let heading_slugs = HashMap::new();
        let out = render_chapter(&chapter, ChapterId(0), &resolved, &heading_slugs, 0).content;
        assert!(
            out.contains("| A | B\\|C |"),
            "header + escaped pipe: {out}"
        );
        assert!(out.contains("| --- | --- |"), "delimiter row: {out}");
        assert!(out.contains("| 1 | 2 |"), "body row: {out}");
    }

    #[test]
    fn table_with_head_body_wrappers_renders_rows() {
        // The optimizer (normalize_table_structure) wraps rows in
        // TableHead/TableBody; the renderer must descend into the wrappers
        // rather than treating them as rows.
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);

        let head = chapter.alloc_node(Node::new(Role::TableHead));
        chapter.append_child(table, head);
        let body = chapter.alloc_node(Node::new(Role::TableBody));
        chapter.append_child(table, body);

        for (section, cells) in [(head, ["Sample", "Voltage"]), (body, ["A", "1.5"])] {
            let row = chapter.alloc_node(Node::new(Role::TableRow));
            chapter.append_child(section, row);
            for cell in cells {
                let c = chapter.alloc_node(Node::new(Role::TableCell));
                chapter.append_child(row, c);
                let t = chapter.append_text(cell);
                let tn = chapter.alloc_node(Node::text(t));
                chapter.append_child(c, tn);
            }
        }

        let out = render_to_string(&chapter);
        let header = out.find("| Sample | Voltage |").expect("header row");
        let delim = out.find("| --- | --- |").expect("delimiter row");
        let body_row = out.find("| A | 1.5 |").expect("body row");
        assert!(header < delim && delim < body_row, "row order: {out}");
    }

    #[test]
    fn table_cells_escape_inline_markers() {
        let mut chapter = Chapter::new();
        let table = chapter.alloc_node(Node::new(Role::Table));
        chapter.append_child(NodeId::ROOT, table);
        let row = chapter.alloc_node(Node::new(Role::TableRow));
        chapter.append_child(table, row);
        for cell in ["*star*", "[link]"] {
            let c = chapter.alloc_node(Node::new(Role::TableCell));
            chapter.append_child(row, c);
            let t = chapter.append_text(cell);
            let tn = chapter.alloc_node(Node::text(t));
            chapter.append_child(c, tn);
        }

        let out = render_to_string(&chapter);
        assert!(out.contains("\\*star\\*"), "star escaped: {out}");
        assert!(out.contains("\\[link]"), "bracket escaped: {out}");
    }

    #[test]
    fn paragraph_text_resembling_list_markers_is_escaped() {
        for (text, escaped) in [
            ("- not a list", "\\- not a list"),
            ("1. not a list", "1\\. not a list"),
        ] {
            let mut chapter = Chapter::new();
            let p = chapter.alloc_node(Node::new(Role::Paragraph));
            chapter.append_child(NodeId::ROOT, p);
            let t = chapter.append_text(text);
            let tn = chapter.alloc_node(Node::text(t));
            chapter.append_child(p, tn);

            let out = render_to_string(&chapter);
            assert!(out.contains(escaped), "expected {escaped:?} in {out:?}");
        }
    }

    #[test]
    fn mid_line_dash_not_escaped_across_chunks() {
        // Two text nodes on one line: the second begins mid-line, so its
        // leading dash must not be treated as a list marker.
        let mut chapter = Chapter::new();
        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);
        for text in ["seven ", "- eight"] {
            let t = chapter.append_text(text);
            let tn = chapter.alloc_node(Node::text(t));
            chapter.append_child(p, tn);
        }

        let out = render_to_string(&chapter);
        assert!(out.contains("seven - eight"), "no escape mid-line: {out}");
    }

    #[test]
    fn test_markdown_escaping() {
        let mut chapter = Chapter::new();

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);

        let text_range = chapter.append_text("*bold* and _italic_");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(p, text_node);

        let result = render_to_string(&chapter);
        assert!(result.contains("\\*bold\\*"));
        assert!(result.contains("\\_italic\\_"));
    }
}
