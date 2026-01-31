//! Markdown Exporter - converts IR to Markdown.
//!
//! Walks the IR tree and emits formatted Markdown, preserving structure
//! through headers, lists, and markdown conventions.
//!
//! Design follows Pandoc's Markdown writer patterns:
//! - Text escaping for Markdown special characters
//! - Tight/loose list detection
//! - Footnote accumulation and end-of-document rendering
//! - Dynamic code fence length

use std::io::{self, Seek, Write};

use crate::model::{Book, NodeId, Role};
use crate::style::Display;

use super::Exporter;

/// Configuration for Markdown export.
#[derive(Debug, Clone, Default)]
pub struct MarkdownConfig {
    /// Line width for wrapping (0 = no wrapping).
    pub line_width: usize,
}

/// Exporter for Markdown output.
#[derive(Debug, Clone, Default)]
pub struct MarkdownExporter {
    config: MarkdownConfig,
}

impl MarkdownExporter {
    /// Create a new MarkdownExporter with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a MarkdownExporter with the specified configuration.
    pub fn with_config(config: MarkdownConfig) -> Self {
        Self { config }
    }
}

impl Exporter for MarkdownExporter {
    fn export<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> io::Result<()> {
        let _ = self.config; // Reserved for future use (line wrapping)
        let spine: Vec<_> = book.spine().to_vec();
        let mut first_chapter = true;

        for entry in spine {
            let chapter_path = book.source_id(entry.id).map(|s| s.to_string());
            let chapter = book.load_chapter(entry.id)?;

            if !first_chapter {
                // Chapter separator
                writeln!(writer)?;
                writeln!(writer, "---")?;
                writeln!(writer)?;
            }
            first_chapter = false;

            let mut ctx = ExportContext {
                writer,
                ir: &chapter,
                line_prefix: String::new(),
                list_stack: Vec::new(),
                at_line_start: true,
                has_line_content: false,
                pending_newline: false,
                last_block_role: None,
                footnotes: Vec::new(),
                chapter_path,
            };

            // Walk children of root
            for child_id in chapter.children(NodeId::ROOT) {
                ctx.walk_node(child_id)?;
            }

            // Ensure final newline
            if !ctx.at_line_start {
                writeln!(ctx.writer)?;
            }

            // Render accumulated footnotes at end of chapter
            if !ctx.footnotes.is_empty() {
                writeln!(ctx.writer)?;
                for note in &ctx.footnotes {
                    writeln!(ctx.writer, "[^{}]: {}", note.number, note.content)?;
                }
            }
        }

        Ok(())
    }
}

/// Tracks list context for numbering.
#[derive(Debug, Clone)]
struct ListContext {
    /// Whether this is an ordered list.
    is_ordered: bool,
    counter: usize,
    /// Indent string for continuation lines in this list item
    continuation_indent: String,
    /// Whether this is a tight list (no blank lines between items).
    is_tight: bool,
}

/// Accumulated footnote for end-of-document rendering.
#[derive(Debug, Clone)]
struct AccumulatedNote {
    /// Footnote number (1-based)
    number: usize,
    /// The collected text content
    content: String,
}

/// Context for the export walk.
struct ExportContext<'a, W: Write> {
    writer: &'a mut W,
    ir: &'a crate::model::Chapter,
    /// Prefix to write at the start of each new line (blockquote markers, indentation)
    line_prefix: String,
    list_stack: Vec<ListContext>,
    /// True if we're at the start of a line (need to write prefix before content)
    at_line_start: bool,
    /// True if actual content has been written on this line (not just prefix)
    has_line_content: bool,
    /// True if we need a blank line before the next block
    pending_newline: bool,
    /// The role of the last block element (for adjacent list detection)
    last_block_role: Option<Role>,
    /// Accumulated footnotes for end-of-document rendering
    footnotes: Vec<AccumulatedNote>,
    /// Current chapter's source path (for flattening cross-file links)
    #[allow(dead_code)]
    chapter_path: Option<String>,
}

impl<W: Write> ExportContext<'_, W> {
    /// Check if a list is "tight" (items are single paragraphs, no blank lines between).
    ///
    /// Following Pandoc's pattern: a list is tight if all items contain only
    /// simple inline content, not multiple block-level children.
    fn is_tight_list(&self, list_id: NodeId) -> bool {
        for item_id in self.ir.children(list_id) {
            let Some(item) = self.ir.node(item_id) else {
                continue;
            };
            if item.role != Role::ListItem {
                continue;
            }

            // Count block-level children
            let mut block_count = 0;
            for child_id in self.ir.children(item_id) {
                let Some(child) = self.ir.node(child_id) else {
                    continue;
                };
                // Check if this is a block-level element
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

    /// Ensure we're ready to write content (write prefix if at line start)
    fn ensure_line_started(&mut self) -> io::Result<()> {
        if self.at_line_start {
            write!(self.writer, "{}", self.line_prefix)?;
            self.at_line_start = false;
        }
        Ok(())
    }

    /// Write a newline, respecting the current prefix
    fn write_newline(&mut self) -> io::Result<()> {
        writeln!(self.writer)?;
        self.at_line_start = true;
        self.has_line_content = false;
        Ok(())
    }

    /// Write a hard line break (backslash in markdown)
    fn write_hard_break(&mut self) -> io::Result<()> {
        write!(self.writer, "\\")?;
        self.write_newline()
    }

    /// Start a new block element
    fn start_block(&mut self) -> io::Result<()> {
        if self.pending_newline {
            if !self.at_line_start {
                self.write_newline()?;
            }
            self.write_newline()?;
            self.pending_newline = false;
        }
        self.ensure_line_started()
    }

    /// End a block element
    fn end_block(&mut self, role: Role) {
        self.pending_newline = true;
        self.last_block_role = Some(role);
    }

    /// Check if we need a separator between adjacent lists.
    ///
    /// Following Pandoc's pattern: insert `<!-- -->` between adjacent lists
    /// of the same type to prevent Markdown parsers from merging them.
    fn needs_list_separator(&self, current_role: Role) -> bool {
        matches!(
            (self.last_block_role, current_role),
            (Some(Role::OrderedList), Role::OrderedList)
                | (Some(Role::UnorderedList), Role::UnorderedList)
                | (Some(Role::DefinitionList), Role::DefinitionList)
        )
    }

    /// Write a list separator comment (for adjacent lists).
    fn write_list_separator(&mut self) -> io::Result<()> {
        if !self.at_line_start {
            self.write_newline()?;
        }
        self.write_newline()?;
        self.ensure_line_started()?;
        writeln!(self.writer, "<!-- -->")?;
        self.at_line_start = true;
        Ok(())
    }

    fn walk_node(&mut self, id: NodeId) -> io::Result<()> {
        let Some(node) = self.ir.node(id) else {
            return Ok(());
        };

        let role = node.role;

        match role {
            Role::Text => {
                // Leaf text node - output the text content directly
                if !node.text.is_empty() {
                    let text = self.ir.text(node.text);
                    self.write_text(text)?;
                }
            }

            Role::Paragraph => {
                self.start_block()?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::Heading(level) => {
                self.start_block()?;
                for _ in 0..level {
                    write!(self.writer, "#")?;
                }
                write!(self.writer, " ")?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::OrderedList => {
                if self.needs_list_separator(role) {
                    self.write_list_separator()?;
                }
                self.start_block()?;
                let start = self.ir.semantics.list_start(id).unwrap_or(1) as usize;
                let is_tight = self.is_tight_list(id);
                self.list_stack.push(ListContext {
                    is_ordered: true,
                    counter: start.saturating_sub(1),
                    continuation_indent: String::new(),
                    is_tight,
                });
                self.walk_children(id)?;
                self.list_stack.pop();
                self.end_block(role);
            }

            Role::UnorderedList => {
                if self.needs_list_separator(role) {
                    self.write_list_separator()?;
                }
                self.start_block()?;
                let is_tight = self.is_tight_list(id);
                self.list_stack.push(ListContext {
                    is_ordered: false,
                    counter: 0,
                    continuation_indent: String::new(),
                    is_tight,
                });
                self.walk_children(id)?;
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
                        self.write_newline()?;
                    }
                    self.write_newline()?;
                } else if !self.at_line_start {
                    self.write_newline()?;
                }

                self.ensure_line_started()?;

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

                write!(self.writer, "{}", bullet)?;

                // Set continuation indent for subsequent lines
                let continuation = " ".repeat(bullet.len());
                let old_prefix = self.line_prefix.clone();
                self.line_prefix.push_str(&continuation);

                if let Some(list_ctx) = self.list_stack.last_mut() {
                    list_ctx.continuation_indent = continuation;
                }

                self.walk_children(id)?;

                self.line_prefix = old_prefix;
                self.pending_newline = false;
            }

            Role::BlockQuote => {
                if self.pending_newline {
                    if !self.at_line_start {
                        self.write_newline()?;
                    }
                    self.write_newline()?;
                    self.pending_newline = false;
                }

                let prefix = "> ";

                if !self.at_line_start {
                    write!(self.writer, "{}", prefix)?;
                }

                let old_prefix = self.line_prefix.clone();
                self.line_prefix.push_str(prefix);

                self.walk_children(id)?;

                self.line_prefix = old_prefix;
                self.end_block(role);
            }

            Role::Link => {
                self.ensure_line_started()?;

                let href = self.ir.semantics.href(id);
                let link = href.map(crate::model::Link::parse);

                // Determine if this is an external link that needs URL display
                let url_to_show = match &link {
                    Some(crate::model::Link::External(url)) => Some(url.as_str()),
                    Some(crate::model::Link::Unknown(raw))
                        if raw.contains("://") || raw.starts_with("mailto:") =>
                    {
                        Some(raw.as_str())
                    }
                    _ => None,
                };

                if let Some(url) = url_to_show {
                    // Markdown link: [styled content](url)
                    write!(self.writer, "[")?;
                    self.walk_children(id)?;
                    write!(self.writer, "]({})", url)?;
                } else {
                    // Internal link: just output styled content
                    self.walk_children(id)?;
                }
            }

            Role::Image => {
                self.start_block()?;
                let alt = self.ir.semantics.alt(id).unwrap_or("image");
                let src = self.ir.semantics.src(id).unwrap_or("");
                write!(self.writer, "![{}]({})", alt, src)?;
                self.end_block(role);
            }

            Role::Break => {
                self.write_hard_break()?;
            }

            Role::Rule => {
                self.start_block()?;
                write!(self.writer, "---")?;
                self.end_block(role);
            }

            Role::Table => {
                self.start_block()?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::TableRow => {
                self.ensure_line_started()?;
                let mut first = true;
                for child_id in self.ir.children(id) {
                    if !first {
                        write!(self.writer, " | ")?;
                    }
                    first = false;
                    let text = self.collect_text(child_id);
                    write!(self.writer, "{}", text)?;
                }
                self.write_newline()?;
            }

            Role::TableCell => {
                self.walk_children(id)?;
            }

            Role::Figure => {
                self.start_block()?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::Caption => {
                self.start_block()?;
                write!(self.writer, "*")?;
                self.walk_children(id)?;
                write!(self.writer, "*")?;
                self.end_block(role);
            }

            Role::Footnote => {
                self.ensure_line_started()?;
                let text = self.collect_text(id);
                let note_num = self.footnotes.len() + 1;
                self.footnotes.push(AccumulatedNote {
                    number: note_num,
                    content: text,
                });
                write!(self.writer, "[^{}]", note_num)?;
            }

            Role::Sidebar => {
                self.start_block()?;
                write!(self.writer, "> **Sidebar**")?;
                self.write_newline()?;
                self.ensure_line_started()?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::Inline => {
                let style = self.ir.styles.get(node.style);
                let is_bold = style.map(|s| s.is_bold()).unwrap_or(false);
                let is_italic = style.map(|s| s.is_italic()).unwrap_or(false);
                let is_code = style.map(|s| s.is_monospace()).unwrap_or(false);
                let is_block = node.style.0 != 0
                    && style.map(|s| s.display == Display::Block).unwrap_or(false);

                // Handle block-display inlines (e.g., verse lines)
                if is_block && self.has_line_content {
                    self.write_hard_break()?;
                }

                if is_code {
                    self.ensure_line_started()?;
                    let content = self.collect_text(id);
                    let tick_count = calculate_inline_code_ticks(&content);
                    let ticks: String = std::iter::repeat_n('`', tick_count).collect();

                    let spacer = if content.starts_with('`') || content.ends_with('`') {
                        " "
                    } else {
                        ""
                    };

                    write!(
                        self.writer,
                        "{}{}{}{}{}",
                        ticks, spacer, content, spacer, ticks
                    )?;
                } else {
                    self.ensure_line_started()?;
                    if is_bold {
                        write!(self.writer, "**")?;
                    }
                    if is_italic {
                        write!(self.writer, "*")?;
                    }

                    self.walk_children(id)?;

                    if is_italic {
                        write!(self.writer, "*")?;
                    }
                    if is_bold {
                        write!(self.writer, "**")?;
                    }
                }
            }

            Role::DefinitionList => {
                if self.needs_list_separator(role) {
                    self.write_list_separator()?;
                }
                self.start_block()?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::DefinitionTerm => {
                self.start_block()?;
                write!(self.writer, "**")?;
                self.walk_children(id)?;
                write!(self.writer, "**")?;
                self.pending_newline = false;
            }

            Role::DefinitionDescription => {
                if !self.at_line_start {
                    self.write_newline()?;
                }
                self.ensure_line_started()?;
                write!(self.writer, ": ")?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::CodeBlock => {
                self.start_block()?;
                let text = self.collect_text_verbatim(id);

                if !self.at_line_start {
                    self.write_newline()?;
                }
                let lang = self.ir.semantics.language(id).unwrap_or("");
                let fence_len = calculate_fence_length(&text, '`');
                let fence: String = std::iter::repeat_n('`', fence_len).collect();
                self.ensure_line_started()?;
                writeln!(self.writer, "{}{}", fence, lang)?;
                self.at_line_start = true;

                for line in text.lines() {
                    self.ensure_line_started()?;
                    writeln!(self.writer, "{}", line)?;
                    self.at_line_start = true;
                }

                self.ensure_line_started()?;
                write!(self.writer, "{}", fence)?;
                self.end_block(role);
            }

            Role::Container | Role::Root | Role::TableHead | Role::TableBody => {
                self.walk_children(id)?;
            }
        }

        Ok(())
    }

    fn walk_children(&mut self, id: NodeId) -> io::Result<()> {
        for child_id in self.ir.children(id) {
            self.walk_node(child_id)?;
        }
        Ok(())
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.ensure_line_started()?;

        // Strip soft hyphens (U+00AD) used for hyphenation hints in ebooks
        let text = text.replace('\u{00AD}', "");

        // Normalize internal whitespace while preserving leading/trailing
        let has_leading = text.starts_with(char::is_whitespace);
        let has_trailing = text.ends_with(char::is_whitespace);

        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            if !text.is_empty() {
                write!(self.writer, " ")?;
            }
            return Ok(());
        }

        if has_leading {
            write!(self.writer, " ")?;
        }

        let joined = words.join(" ");
        let output = escape_markdown(&joined);
        write!(self.writer, "{}", output)?;
        self.has_line_content = true;

        if has_trailing {
            write!(self.writer, " ")?;
        }

        Ok(())
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
        result.replace('\u{00AD}', "")
    }

    fn collect_text_recursive(&self, id: NodeId, result: &mut String, verbatim: bool) {
        let Some(node) = self.ir.node(id) else {
            return;
        };

        if node.role == Role::Text && !node.text.is_empty() {
            let text = self.ir.text(node.text);

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

        for child_id in self.ir.children(id) {
            self.collect_text_recursive(child_id, result, verbatim);
        }
    }
}

/// Calculate the minimum fence length needed for a code block.
fn calculate_fence_length(content: &str, fence_char: char) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;

    for c in content.chars() {
        if c == fence_char {
            current_run += 1;
            max_run = max_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    max_run.max(2) + 1
}

/// Calculate the minimum backtick count needed for inline code.
fn calculate_inline_code_ticks(content: &str) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;

    for c in content.chars() {
        if c == '`' {
            current_run += 1;
            max_run = max_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    max_run + 1
}

/// Escape special Markdown characters in text.
fn escape_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.len() / 10);
    let mut chars = text.chars().peekable();
    let mut at_line_start = true;

    while let Some(c) = chars.next() {
        match c {
            '\\' => result.push_str("\\\\"),
            '*' | '_' => {
                result.push('\\');
                result.push(c);
            }
            '[' | ']' => {
                result.push('\\');
                result.push(c);
            }
            '`' => {
                result.push('\\');
                result.push(c);
            }
            '#' if at_line_start => {
                result.push('\\');
                result.push(c);
            }
            '|' => {
                result.push('\\');
                result.push(c);
            }
            '<' | '>' => {
                result.push('\\');
                result.push(c);
            }
            '!' if chars.peek() == Some(&'[') => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
        at_line_start = c == '\n';
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Chapter, Node};
    use std::io::Cursor;

    fn export_to_string(chapter: &Chapter) -> String {
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);

        let mut ctx = ExportContext {
            writer: &mut cursor,
            ir: chapter,
            line_prefix: String::new(),
            list_stack: Vec::new(),
            at_line_start: true,
            has_line_content: false,
            pending_newline: false,
            last_block_role: None,
            footnotes: Vec::new(),
            chapter_path: None,
        };

        for child_id in chapter.children(NodeId::ROOT) {
            ctx.walk_node(child_id).unwrap();
        }

        if !ctx.footnotes.is_empty() {
            writeln!(ctx.writer).unwrap();
            for note in &ctx.footnotes {
                writeln!(ctx.writer, "[^{}]: {}", note.number, note.content).unwrap();
            }
        }

        String::from_utf8(output).unwrap()
    }

    #[test]
    fn test_simple_paragraph() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, para);

        let text_range = chapter.append_text("Hello, World!");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(para, text_node);

        let result = export_to_string(&chapter);
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

        let result = export_to_string(&chapter);
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

        let result = export_to_string(&chapter);
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

        let result = export_to_string(&chapter);
        assert!(result.contains("1. Item 1"));
        assert!(result.contains("2. Item 2"));
        assert!(result.contains("3. Item 3"));
    }

    #[test]
    fn test_link() {
        let mut chapter = Chapter::new();

        let link = chapter.alloc_node(Node::new(Role::Link));
        chapter.append_child(NodeId::ROOT, link);
        chapter.semantics.set_href(link, "https://example.com");

        let text_range = chapter.append_text("Click here");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(link, text_node);

        let result = export_to_string(&chapter);
        assert!(result.contains("[Click here](https://example.com)"));
    }

    #[test]
    fn test_image() {
        let mut chapter = Chapter::new();

        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter.semantics.set_src(img, "photo.jpg");
        chapter.semantics.set_alt(img, "A photo");

        let result = export_to_string(&chapter);
        assert!(result.contains("![A photo](photo.jpg)"));
    }

    #[test]
    fn test_blockquote_multiline() {
        let mut chapter = Chapter::new();

        let bq = chapter.alloc_node(Node::new(Role::BlockQuote));
        chapter.append_child(NodeId::ROOT, bq);

        let p1 = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(bq, p1);
        let t1 = chapter.append_text("Line one");
        let tn1 = chapter.alloc_node(Node::text(t1));
        chapter.append_child(p1, tn1);

        let p2 = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(bq, p2);
        let t2 = chapter.append_text("Line two");
        let tn2 = chapter.alloc_node(Node::text(t2));
        chapter.append_child(p2, tn2);

        let result = export_to_string(&chapter);

        let lines: Vec<&str> = result.lines().filter(|l| !l.is_empty()).collect();
        assert!(
            lines.iter().all(|l| l.starts_with('>')),
            "All blockquote lines should start with '>': {:?}",
            lines
        );
    }

    #[test]
    fn test_markdown_escaping() {
        let mut chapter = Chapter::new();

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);

        let text_range = chapter.append_text("*bold* and _italic_ and [link] and `code`");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(p, text_node);

        let result = export_to_string(&chapter);
        assert!(result.contains("\\*bold\\*"));
        assert!(result.contains("\\_italic\\_"));
        assert!(result.contains("\\[link\\]"));
        assert!(result.contains("\\`code\\`"));
    }

    #[test]
    fn test_tight_list() {
        let mut chapter = Chapter::new();

        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);

        for text in ["First", "Second", "Third"] {
            let li = chapter.alloc_node(Node::new(Role::ListItem));
            chapter.append_child(ul, li);
            let t = chapter.append_text(text);
            let tn = chapter.alloc_node(Node::text(t));
            chapter.append_child(li, tn);
        }

        let result = export_to_string(&chapter);

        let lines: Vec<&str> = result.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 3, "Tight list should have 3 lines: {:?}", lines);
    }

    #[test]
    fn test_adjacent_lists_separator() {
        let mut chapter = Chapter::new();

        for _ in 0..2 {
            let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
            chapter.append_child(NodeId::ROOT, ul);

            let li = chapter.alloc_node(Node::new(Role::ListItem));
            chapter.append_child(ul, li);
            let t = chapter.append_text("Item");
            let tn = chapter.alloc_node(Node::text(t));
            chapter.append_child(li, tn);
        }

        let result = export_to_string(&chapter);

        assert!(
            result.contains("<!-- -->"),
            "Adjacent lists should have separator: {:?}",
            result
        );
    }

    #[test]
    fn test_code_block_with_backticks() {
        let mut chapter = Chapter::new();

        let code = chapter.alloc_node(Node::new(Role::CodeBlock));
        chapter.append_child(NodeId::ROOT, code);

        let t = chapter.append_text("```rust\nlet x = 1;\n```");
        let tn = chapter.alloc_node(Node::text(t));
        chapter.append_child(code, tn);

        let result = export_to_string(&chapter);

        assert!(
            result.contains("````"),
            "Should use 4 backticks when content has 3: {:?}",
            result
        );
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

        let result = export_to_string(&chapter);

        assert!(result.contains("[^1]"));
        assert!(result.contains("[^1]: This is a footnote"));
    }
}
