//! Text Exporter - converts IR to plain text or Markdown.
//!
//! Walks the IR tree and emits formatted text, preserving structure
//! through indentation, bullets, and markdown conventions.

use std::io::{self, Seek, Write};

use crate::book::Book;
use crate::ir::{Display, ListKind, NodeId, Role};

use super::Exporter;

/// Output format for text export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextFormat {
    /// Markdown format with headers, links, etc.
    #[default]
    Markdown,
    /// Plain text with minimal formatting.
    Plain,
}

/// Configuration for text export.
#[derive(Debug, Clone, Default)]
pub struct TextConfig {
    /// Output format (markdown or plain text).
    pub format: TextFormat,
    /// Line width for wrapping (0 = no wrapping).
    pub line_width: usize,
}

/// Exporter for plain text and Markdown output.
#[derive(Debug, Clone, Default)]
pub struct TextExporter {
    config: TextConfig,
}

impl TextExporter {
    /// Create a new TextExporter with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a TextExporter with the specified configuration.
    pub fn with_config(config: TextConfig) -> Self {
        Self { config }
    }

    /// Set the output format.
    pub fn format(mut self, format: TextFormat) -> Self {
        self.config.format = format;
        self
    }
}

impl Exporter for TextExporter {
    fn export<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> io::Result<()> {
        let spine: Vec<_> = book.spine().to_vec();
        let mut first_chapter = true;

        for entry in spine {
            let chapter = book.load_chapter(entry.id)?;

            if !first_chapter {
                // Chapter separator
                writeln!(writer)?;
                if self.config.format == TextFormat::Markdown {
                    writeln!(writer, "---")?;
                }
                writeln!(writer)?;
            }
            first_chapter = false;

            let mut ctx = ExportContext {
                writer,
                ir: &chapter,
                format: self.config.format,
                line_prefix: String::new(),
                list_stack: Vec::new(),
                at_line_start: true,
                pending_newline: false,
            };

            // Walk children of root
            for child_id in chapter.children(NodeId::ROOT) {
                ctx.walk_node(child_id)?;
            }

            // Ensure final newline
            if !ctx.at_line_start {
                writeln!(ctx.writer)?;
            }
        }

        Ok(())
    }
}

/// Tracks list context for numbering.
#[derive(Debug, Clone)]
struct ListContext {
    kind: ListKind,
    counter: usize,
    /// Indent string for continuation lines in this list item
    continuation_indent: String,
}

/// Context for the export walk.
struct ExportContext<'a, W: Write> {
    writer: &'a mut W,
    ir: &'a crate::ir::IRChapter,
    format: TextFormat,
    /// Prefix to write at the start of each new line (blockquote markers, indentation)
    line_prefix: String,
    list_stack: Vec<ListContext>,
    /// True if we're at the start of a line (need to write prefix before content)
    at_line_start: bool,
    /// True if we need a blank line before the next block
    pending_newline: bool,
}

impl<W: Write> ExportContext<'_, W> {
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
        Ok(())
    }

    /// Write a hard line break (backslash in markdown, newline in plain)
    fn write_hard_break(&mut self) -> io::Result<()> {
        if self.format == TextFormat::Markdown {
            write!(self.writer, "\\")?;
        }
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
    fn end_block(&mut self) {
        self.pending_newline = true;
    }

    fn walk_node(&mut self, id: NodeId) -> io::Result<()> {
        let Some(node) = self.ir.node(id) else {
            return Ok(());
        };

        let role = node.role;

        match role {
            Role::Text => {
                // Leaf text node
                if !node.text.is_empty() {
                    let text = self.ir.text(node.text);
                    self.write_text(text)?;
                } else {
                    // Container text (paragraph)
                    self.start_block()?;
                    self.walk_children(id)?;
                    self.end_block();
                }
            }

            Role::Heading(level) => {
                self.start_block()?;
                if self.format == TextFormat::Markdown {
                    for _ in 0..level {
                        write!(self.writer, "#")?;
                    }
                    write!(self.writer, " ")?;
                }
                self.walk_children(id)?;
                self.end_block();
            }

            Role::List(kind) => {
                self.start_block()?;
                self.list_stack.push(ListContext {
                    kind,
                    counter: 0,
                    continuation_indent: String::new(),
                });
                self.walk_children(id)?;
                self.list_stack.pop();
                self.end_block();
            }

            Role::ListItem => {
                // Ensure we're on a new line
                if !self.at_line_start {
                    self.write_newline()?;
                }
                self.ensure_line_started()?;

                // Get bullet/number from parent list
                let bullet = if let Some(list_ctx) = self.list_stack.last_mut() {
                    list_ctx.counter += 1;
                    match list_ctx.kind {
                        ListKind::Unordered => {
                            if self.format == TextFormat::Markdown {
                                "- ".to_string()
                            } else {
                                "• ".to_string()
                            }
                        }
                        ListKind::Ordered => {
                            format!("{}. ", list_ctx.counter)
                        }
                    }
                } else {
                    "".to_string()
                };

                write!(self.writer, "{}", bullet)?;

                // Set continuation indent for SUBSEQUENT lines (not the current line)
                let continuation = " ".repeat(bullet.len());
                let old_prefix = self.line_prefix.clone();
                self.line_prefix.push_str(&continuation);

                // Update list context with continuation indent
                if let Some(list_ctx) = self.list_stack.last_mut() {
                    list_ctx.continuation_indent = continuation;
                }

                // Keep at_line_start = false - we're mid-line after the bullet
                // Children will use the continuation prefix only on NEW lines

                self.walk_children(id)?;

                self.line_prefix = old_prefix;
                self.pending_newline = false; // Don't double-space list items
            }

            Role::BlockQuote => {
                // Handle block separation
                if self.pending_newline {
                    if !self.at_line_start {
                        self.write_newline()?;
                    }
                    self.write_newline()?;
                    self.pending_newline = false;
                }

                let prefix = if self.format == TextFormat::Markdown {
                    "> "
                } else {
                    "  "
                };

                // If we're mid-line (e.g., right after a list bullet), write prefix directly
                if !self.at_line_start {
                    write!(self.writer, "{}", prefix)?;
                }

                // Add to line_prefix for subsequent lines
                let old_prefix = self.line_prefix.clone();
                self.line_prefix.push_str(prefix);

                self.walk_children(id)?;

                self.line_prefix = old_prefix;
                self.end_block();
            }

            Role::Link => {
                self.ensure_line_started()?;
                let text = self.collect_text(id);
                let href = self.ir.semantics.href(id).unwrap_or("");

                if self.format == TextFormat::Markdown && !href.is_empty() {
                    write!(self.writer, "[{}]({})", text, href)?;
                } else if !href.is_empty() && href != text {
                    write!(self.writer, "{} ({})", text, href)?;
                } else {
                    write!(self.writer, "{}", text)?;
                }
            }

            Role::Image => {
                self.ensure_line_started()?;
                let alt = self.ir.semantics.alt(id).unwrap_or("image");
                let src = self.ir.semantics.src(id).unwrap_or("");

                if self.format == TextFormat::Markdown {
                    write!(self.writer, "![{}]({})", alt, src)?;
                } else {
                    write!(self.writer, "[Image: {}]", alt)?;
                }
            }

            Role::Break => {
                self.write_hard_break()?;
            }

            Role::Rule => {
                self.start_block()?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "---")?;
                } else {
                    write!(self.writer, "────────────────────")?;
                }
                self.end_block();
            }

            Role::Table => {
                self.start_block()?;
                self.walk_children(id)?;
                self.end_block();
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
                // Handled by TableRow
                self.walk_children(id)?;
            }

            Role::Figure => {
                self.start_block()?;
                self.walk_children(id)?;
                self.end_block();
            }

            Role::Footnote => {
                // Render inline as [note: ...]
                self.ensure_line_started()?;
                let text = self.collect_text(id);
                write!(self.writer, "[note: {}]", text)?;
            }

            Role::Sidebar => {
                self.start_block()?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "> **Sidebar**")?;
                    self.write_newline()?;
                    self.ensure_line_started()?;
                }
                self.walk_children(id)?;
                self.end_block();
            }

            Role::Inline => {
                // Check for style-based formatting
                let style = self.ir.styles.get(node.style);
                let is_bold = style.map(|s| s.is_bold()).unwrap_or(false);
                let is_italic = style.map(|s| s.is_italic()).unwrap_or(false);
                let is_code = style.map(|s| s.is_monospace()).unwrap_or(false);
                // Only treat as block if style explicitly sets display: block
                // (not just default style, since default ComputedStyle has display: Block)
                let is_block = node.style.0 != 0
                    && style.map(|s| s.display == Display::Block).unwrap_or(false);

                // Handle block-display inlines (e.g., verse lines)
                // Only break if we have content already on this line
                if is_block && !self.at_line_start {
                    self.write_hard_break()?;
                }

                if self.format == TextFormat::Markdown {
                    self.ensure_line_started()?;
                    if is_code {
                        write!(self.writer, "`")?;
                    }
                    if is_bold {
                        write!(self.writer, "**")?;
                    }
                    if is_italic {
                        write!(self.writer, "*")?;
                    }
                }

                self.walk_children(id)?;

                if self.format == TextFormat::Markdown {
                    if is_italic {
                        write!(self.writer, "*")?;
                    }
                    if is_bold {
                        write!(self.writer, "**")?;
                    }
                    if is_code {
                        write!(self.writer, "`")?;
                    }
                }
            }

            Role::Container | Role::Root => {
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

        // Normalize internal whitespace while preserving leading/trailing
        let has_leading = text.starts_with(char::is_whitespace);
        let has_trailing = text.ends_with(char::is_whitespace);

        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            // Pure whitespace - output a single space
            if !text.is_empty() {
                write!(self.writer, " ")?;
            }
            return Ok(());
        }

        if has_leading {
            write!(self.writer, " ")?;
        }
        write!(self.writer, "{}", words.join(" "))?;
        if has_trailing {
            write!(self.writer, " ")?;
        }

        Ok(())
    }

    /// Collect all text content from a node and its children.
    fn collect_text(&self, id: NodeId) -> String {
        let mut result = String::new();
        self.collect_text_recursive(id, &mut result);
        result
    }

    fn collect_text_recursive(&self, id: NodeId, result: &mut String) {
        let Some(node) = self.ir.node(id) else {
            return;
        };

        if node.role == Role::Text && !node.text.is_empty() {
            let text = self.ir.text(node.text);
            // Preserve whitespace structure
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
                // Pure whitespace
                result.push(' ');
            }
        }

        for child_id in self.ir.children(id) {
            self.collect_text_recursive(child_id, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IRChapter, Node};
    use std::io::Cursor;

    fn export_to_string(chapter: &IRChapter, format: TextFormat) -> String {
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);

        let mut ctx = ExportContext {
            writer: &mut cursor,
            ir: chapter,
            format,
            line_prefix: String::new(),
            list_stack: Vec::new(),
            at_line_start: true,
            pending_newline: false,
        };

        for child_id in chapter.children(NodeId::ROOT) {
            ctx.walk_node(child_id).unwrap();
        }

        String::from_utf8(output).unwrap()
    }

    #[test]
    fn test_simple_paragraph() {
        let mut chapter = IRChapter::new();

        let para = chapter.alloc_node(Node::new(Role::Text));
        chapter.append_child(NodeId::ROOT, para);

        let text_range = chapter.append_text("Hello, World!");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(para, text_node);

        let result = export_to_string(&chapter, TextFormat::Plain);
        assert!(result.contains("Hello, World!"));
    }

    #[test]
    fn test_heading_markdown() {
        let mut chapter = IRChapter::new();

        let h1 = chapter.alloc_node(Node::new(Role::Heading(1)));
        chapter.append_child(NodeId::ROOT, h1);

        let text_range = chapter.append_text("Chapter One");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(h1, text_node);

        let result = export_to_string(&chapter, TextFormat::Markdown);
        assert!(result.contains("# Chapter One"));
    }

    #[test]
    fn test_unordered_list_markdown() {
        let mut chapter = IRChapter::new();

        let ul = chapter.alloc_node(Node::new(Role::List(ListKind::Unordered)));
        chapter.append_child(NodeId::ROOT, ul);

        let li = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul, li);

        let text_range = chapter.append_text("Item one");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(li, text_node);

        let result = export_to_string(&chapter, TextFormat::Markdown);
        assert!(result.contains("- Item one"));
    }

    #[test]
    fn test_ordered_list() {
        let mut chapter = IRChapter::new();

        let ol = chapter.alloc_node(Node::new(Role::List(ListKind::Ordered)));
        chapter.append_child(NodeId::ROOT, ol);

        for i in 1..=3 {
            let li = chapter.alloc_node(Node::new(Role::ListItem));
            chapter.append_child(ol, li);

            let text_range = chapter.append_text(&format!("Item {}", i));
            let text_node = chapter.alloc_node(Node::text(text_range));
            chapter.append_child(li, text_node);
        }

        let result = export_to_string(&chapter, TextFormat::Plain);
        assert!(result.contains("1. Item 1"));
        assert!(result.contains("2. Item 2"));
        assert!(result.contains("3. Item 3"));
    }

    #[test]
    fn test_link_markdown() {
        let mut chapter = IRChapter::new();

        let link = chapter.alloc_node(Node::new(Role::Link));
        chapter.append_child(NodeId::ROOT, link);
        chapter
            .semantics
            .set_href(link, "https://example.com".to_string());

        let text_range = chapter.append_text("Click here");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(link, text_node);

        let result = export_to_string(&chapter, TextFormat::Markdown);
        assert!(result.contains("[Click here](https://example.com)"));
    }

    #[test]
    fn test_image_markdown() {
        let mut chapter = IRChapter::new();

        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter
            .semantics
            .set_src(img, "photo.jpg".to_string());
        chapter.semantics.set_alt(img, "A photo".to_string());

        let result = export_to_string(&chapter, TextFormat::Markdown);
        assert!(result.contains("![A photo](photo.jpg)"));
    }

    #[test]
    fn test_image_plain() {
        let mut chapter = IRChapter::new();

        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter.semantics.set_alt(img, "A sunset".to_string());

        let result = export_to_string(&chapter, TextFormat::Plain);
        assert!(result.contains("[Image: A sunset]"));
    }

    #[test]
    fn test_blockquote_multiline() {
        let mut chapter = IRChapter::new();

        let bq = chapter.alloc_node(Node::new(Role::BlockQuote));
        chapter.append_child(NodeId::ROOT, bq);

        // First paragraph in blockquote
        let p1 = chapter.alloc_node(Node::new(Role::Text));
        chapter.append_child(bq, p1);
        let t1 = chapter.append_text("Line one");
        let tn1 = chapter.alloc_node(Node::text(t1));
        chapter.append_child(p1, tn1);

        // Second paragraph in blockquote
        let p2 = chapter.alloc_node(Node::new(Role::Text));
        chapter.append_child(bq, p2);
        let t2 = chapter.append_text("Line two");
        let tn2 = chapter.alloc_node(Node::text(t2));
        chapter.append_child(p2, tn2);

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Both lines should have > prefix
        let lines: Vec<&str> = result.lines().filter(|l| !l.is_empty()).collect();
        assert!(
            lines.iter().all(|l| l.starts_with('>')),
            "All blockquote lines should start with '>': {:?}",
            lines
        );
    }

    #[test]
    fn test_list_with_blockquote() {
        let mut chapter = IRChapter::new();

        let ol = chapter.alloc_node(Node::new(Role::List(ListKind::Ordered)));
        chapter.append_child(NodeId::ROOT, ol);

        let li = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ol, li);

        let bq = chapter.alloc_node(Node::new(Role::BlockQuote));
        chapter.append_child(li, bq);

        let p = chapter.alloc_node(Node::new(Role::Text));
        chapter.append_child(bq, p);

        let t = chapter.append_text("Quoted text");
        let tn = chapter.alloc_node(Node::text(t));
        chapter.append_child(p, tn);

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Should have list number followed by blockquote
        assert!(
            result.contains("1.") && result.contains('>'),
            "Should have list number and blockquote marker: {}",
            result
        );
    }

    #[test]
    fn test_whitespace_preservation() {
        let mut chapter = IRChapter::new();

        let p = chapter.alloc_node(Node::new(Role::Text));
        chapter.append_child(NodeId::ROOT, p);

        // Text with leading space
        let t1 = chapter.append_text("word1 ");
        let tn1 = chapter.alloc_node(Node::text(t1));
        chapter.append_child(p, tn1);

        // Inline element
        let span = chapter.alloc_node(Node::new(Role::Inline));
        chapter.append_child(p, span);
        let t2 = chapter.append_text("word2");
        let tn2 = chapter.alloc_node(Node::text(t2));
        chapter.append_child(span, tn2);

        // Text with trailing space
        let t3 = chapter.append_text(" word3");
        let tn3 = chapter.alloc_node(Node::text(t3));
        chapter.append_child(p, tn3);

        let result = export_to_string(&chapter, TextFormat::Plain);
        assert!(
            result.contains("word1 word2 word3"),
            "Spaces should be preserved: {}",
            result
        );
    }
}
