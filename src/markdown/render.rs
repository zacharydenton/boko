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

use super::escape::escape_markdown;
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
}

impl<'a> RenderContext<'a> {
    /// Create a new render context for a chapter.
    pub fn new(
        chapter: &'a Chapter,
        chapter_id: ChapterId,
        resolved: &'a ResolvedLinks,
        heading_slugs: &'a HashMap<GlobalNodeId, String>,
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
                self.walk_children(id);
                self.end_block(role);
            }

            Role::TableRow => {
                self.ensure_line_started();
                let mut first = true;
                for child_id in self.chapter.children(id) {
                    if !first {
                        self.output.push_str(" | ");
                    }
                    first = false;
                    let text = self.collect_text(child_id);
                    self.output.push_str(&text);
                }
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
                let note_num = self.footnotes.len() + 1;
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

    fn walk_children(&mut self, id: NodeId) {
        for child_id in self.chapter.children(id) {
            self.walk_node(child_id);
        }
    }

    fn write_text(&mut self, text: &str) {
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
        let escaped = escape_markdown(&joined);
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
) -> RenderResult {
    let ctx = RenderContext::new(chapter, chapter_id, resolved, heading_slugs);
    ctx.render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    fn render_to_string(chapter: &Chapter) -> String {
        let resolved = ResolvedLinks::default();
        let heading_slugs = HashMap::new();
        let result = render_chapter(chapter, ChapterId(0), &resolved, &heading_slugs);
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
        let result = render_chapter(&chapter, ChapterId(0), &resolved, &heading_slugs);

        assert!(result.content.contains("[^1]"));
        assert_eq!(result.footnotes.len(), 1);
        assert_eq!(result.footnotes[0].number, 1);
        assert_eq!(result.footnotes[0].content, "This is a footnote");
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
