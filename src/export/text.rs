//! Text Exporter - converts IR to plain text or Markdown.
//!
//! Walks the IR tree and emits formatted text, preserving structure
//! through indentation, bullets, and markdown conventions.
//!
//! Design follows Pandoc's Markdown writer patterns:
//! - Variant-driven output (Markdown vs PlainText)
//! - Text escaping for Markdown special characters
//! - Tight/loose list detection
//! - Footnote accumulation and end-of-document rendering
//! - Dynamic code fence length

use std::io::{self, Seek, Write};

use crate::book::Book;
use crate::ir::{Display, NodeId, Role};

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
            let chapter_path = book.source_id(entry.id).map(|s| s.to_string());
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
            if !ctx.footnotes.is_empty() && ctx.format == TextFormat::Markdown {
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
    ir: &'a crate::ir::IRChapter,
    format: TextFormat,
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
    /// TODO: Use this when internal link resolution is implemented.
    #[allow(dead_code)]
    chapter_path: Option<String>,
}

impl<W: Write> ExportContext<'_, W> {
    /// Output an anchor for an element's ID if present.
    /// TODO: Implement anchor output when internal link resolution is added.
    fn write_anchor_if_present(&mut self, _node_id: NodeId) -> io::Result<()> {
        // Currently a no-op since internal links aren't resolved yet.
        // When implemented, this should output `<a id="..."></a>` for elements
        // with IDs so that internal links can target them.
        Ok(())
    }

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
                        // Paragraph is a block element
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
    fn end_block(&mut self, role: Role) {
        self.pending_newline = true;
        self.last_block_role = Some(role);
    }

    /// Check if we need a separator between adjacent lists.
    ///
    /// Following Pandoc's pattern: insert `<!-- -->` between adjacent lists
    /// of the same type to prevent Markdown parsers from merging them.
    fn needs_list_separator(&self, current_role: Role) -> bool {
        if self.format != TextFormat::Markdown {
            return false;
        }
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
                    self.write_text(text, true)?;
                }
                // Text nodes with no text content are skipped
            }

            Role::Paragraph => {
                // Block-level text container
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::Heading(level) => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                if self.format == TextFormat::Markdown {
                    for _ in 0..level {
                        write!(self.writer, "#")?;
                    }
                    write!(self.writer, " ")?;
                }
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::OrderedList => {
                // Insert separator between adjacent lists of same type
                if self.needs_list_separator(role) {
                    self.write_list_separator()?;
                }
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                // Get start number from semantics (defaults to 1)
                let start = self.ir.semantics.list_start(id).unwrap_or(1) as usize;
                let is_tight = self.is_tight_list(id);
                self.list_stack.push(ListContext {
                    is_ordered: true,
                    counter: start.saturating_sub(1), // Will be incremented before use
                    continuation_indent: String::new(),
                    is_tight,
                });
                self.walk_children(id)?;
                self.list_stack.pop();
                self.end_block(role);
            }

            Role::UnorderedList => {
                // Insert separator between adjacent lists of same type
                if self.needs_list_separator(role) {
                    self.write_list_separator()?;
                }
                self.start_block()?;
                self.write_anchor_if_present(id)?;
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
                // Check if we need blank line before this item (loose list, not first item)
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
                self.write_anchor_if_present(id)?;

                // Get bullet/number from parent list
                let bullet = if let Some(list_ctx) = self.list_stack.last_mut() {
                    list_ctx.counter += 1;
                    if list_ctx.is_ordered {
                        format!("{}. ", list_ctx.counter)
                    } else if self.format == TextFormat::Markdown {
                        "- ".to_string()
                    } else {
                        "• ".to_string()
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

                self.write_anchor_if_present(id)?;

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
                self.end_block(role);
            }

            Role::Link => {
                self.ensure_line_started()?;

                // Parse link from semantics.href (single source of truth)
                let href = self.ir.semantics.href(id);
                let link = href.map(crate::ir::Link::parse);

                // Determine if this is an external link that needs URL display
                let url_to_show = match &link {
                    Some(crate::ir::Link::External(url)) => Some(url.as_str()),
                    Some(crate::ir::Link::Unknown(raw))
                        if raw.contains("://") || raw.starts_with("mailto:") =>
                    {
                        Some(raw.as_str())
                    }
                    _ => None,
                };

                if self.format == TextFormat::Markdown {
                    if let Some(url) = url_to_show {
                        // Markdown link: [styled content](url)
                        write!(self.writer, "[")?;
                        self.walk_children(id)?;
                        write!(self.writer, "]({})", url)?;
                    } else {
                        // Internal link: just output styled content
                        self.walk_children(id)?;
                    }
                } else {
                    // Plain text: collect text and maybe show URL
                    let text = self.collect_text(id);
                    if let Some(url) = url_to_show {
                        if url != text {
                            write!(self.writer, "{} ({})", text, url)?;
                        } else {
                            write!(self.writer, "{}", text)?;
                        }
                    } else {
                        write!(self.writer, "{}", text)?;
                    }
                }
            }

            Role::Image => {
                self.start_block()?;
                let alt = self.ir.semantics.alt(id).unwrap_or("image");
                let src = self.ir.semantics.src(id).unwrap_or("");

                if self.format == TextFormat::Markdown {
                    write!(self.writer, "![{}]({})", alt, src)?;
                } else {
                    write!(self.writer, "[Image: {}]", alt)?;
                }
                self.end_block(role);
            }

            Role::Break => {
                self.write_hard_break()?;
            }

            Role::Rule => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "---")?;
                } else {
                    write!(self.writer, "────────────────────")?;
                }
                self.end_block(role);
            }

            Role::Table => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
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
                // Handled by TableRow
                self.walk_children(id)?;
            }

            Role::Figure => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::Caption => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "*")?;
                }
                self.walk_children(id)?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "*")?;
                }
                self.end_block(role);
            }

            Role::Footnote => {
                self.ensure_line_started()?;
                let text = self.collect_text(id);

                if self.format == TextFormat::Markdown {
                    // Accumulate footnote and render inline reference
                    let note_num = self.footnotes.len() + 1;
                    self.footnotes.push(AccumulatedNote {
                        number: note_num,
                        content: text,
                    });
                    write!(self.writer, "[^{}]", note_num)?;
                } else {
                    // Plain text: render inline
                    write!(self.writer, "[note: {}]", text)?;
                }
            }

            Role::Sidebar => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "> **Sidebar**")?;
                    self.write_newline()?;
                    self.ensure_line_started()?;
                }
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::Inline => {
                // Check for style-based formatting
                let style = self.ir.styles.get(node.style);
                let is_bold = style.map(|s| s.is_bold()).unwrap_or(false);
                let is_italic = style.map(|s| s.is_italic()).unwrap_or(false);
                let is_code = style.map(|s| s.is_monospace()).unwrap_or(false);
                let is_small_caps = style.map(|s| s.is_small_caps()).unwrap_or(false);
                // Only treat as block if style explicitly sets display: block
                // (not just default style, since default ComputedStyle has display: Block)
                let is_block = node.style.0 != 0
                    && style.map(|s| s.display == Display::Block).unwrap_or(false);

                // Handle block-display inlines (e.g., verse lines)
                // Only break if we have actual content on this line (not just prefix)
                if is_block && self.has_line_content {
                    self.write_hard_break()?;
                }

                if self.format == TextFormat::Markdown && is_code {
                    // For inline code, collect content first to determine backtick count
                    self.ensure_line_started()?;
                    let content = self.collect_text(id);
                    let tick_count = calculate_inline_code_ticks(&content);
                    let ticks: String = std::iter::repeat_n('`', tick_count).collect();

                    // Add space if content starts/ends with backtick
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
                    if self.format == TextFormat::Markdown {
                        self.ensure_line_started()?;
                        if is_bold {
                            write!(self.writer, "**")?;
                        }
                        if is_italic {
                            write!(self.writer, "*")?;
                        }
                    }

                    // SmallCaps: uppercase in plain text
                    if is_small_caps && self.format == TextFormat::Plain {
                        let content = self.collect_text(id);
                        self.ensure_line_started()?;
                        write!(self.writer, "{}", content.to_uppercase())?;
                    } else {
                        self.walk_children(id)?;
                    }

                    if self.format == TextFormat::Markdown {
                        if is_italic {
                            write!(self.writer, "*")?;
                        }
                        if is_bold {
                            write!(self.writer, "**")?;
                        }
                    }
                }
            }

            Role::DefinitionList => {
                // Insert separator between adjacent definition lists
                if self.needs_list_separator(role) {
                    self.write_list_separator()?;
                }
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::DefinitionTerm => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "**")?;
                }
                self.walk_children(id)?;
                if self.format == TextFormat::Markdown {
                    write!(self.writer, "**")?;
                }
                self.pending_newline = false; // Don't add blank line before dd
            }

            Role::DefinitionDescription => {
                // Ensure we're on a new line but don't add blank line
                if !self.at_line_start {
                    self.write_newline()?;
                }
                self.ensure_line_started()?;
                self.write_anchor_if_present(id)?;
                write!(self.writer, ": ")?;
                self.walk_children(id)?;
                self.end_block(role);
            }

            Role::CodeBlock => {
                self.start_block()?;
                self.write_anchor_if_present(id)?;
                // Collect text verbatim to preserve newlines in code
                let text = self.collect_text_verbatim(id);

                if self.format == TextFormat::Markdown {
                    // Code fence must start on its own line
                    if !self.at_line_start {
                        self.write_newline()?;
                    }
                    let lang = self.ir.semantics.language(id).unwrap_or("");
                    // Calculate fence length based on content
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
                } else {
                    // Plain text: just output the code
                    for line in text.lines() {
                        self.ensure_line_started()?;
                        writeln!(self.writer, "{}", line)?;
                        self.at_line_start = true;
                    }
                }
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

    fn write_text(&mut self, text: &str, escape: bool) -> io::Result<()> {
        self.ensure_line_started()?;

        // Strip soft hyphens (U+00AD) used for hyphenation hints in ebooks
        let text = text.replace('\u{00AD}', "");

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

        let joined = words.join(" ");
        let output = if escape && self.format == TextFormat::Markdown {
            escape_markdown(&joined)
        } else {
            joined
        };
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
        // Strip soft hyphens (U+00AD) used for hyphenation hints in ebooks
        result.replace('\u{00AD}', "")
    }

    fn collect_text_recursive(&self, id: NodeId, result: &mut String, verbatim: bool) {
        let Some(node) = self.ir.node(id) else {
            return;
        };

        if node.role == Role::Text && !node.text.is_empty() {
            let text = self.ir.text(node.text);

            if verbatim {
                // Preserve whitespace exactly as-is (for code blocks)
                result.push_str(text);
            } else {
                // Normalize whitespace structure
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
        }

        for child_id in self.ir.children(id) {
            self.collect_text_recursive(child_id, result, verbatim);
        }
    }
}

/// Calculate the minimum fence length needed for a code block.
///
/// Following Pandoc's pattern: find the longest run of backticks or tildes
/// in the content, then use one more than that.
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

    // Minimum fence is 3, or one more than the longest run
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

    // Use one more than the longest run, minimum 1
    max_run + 1
}

/// Flatten a path-like link to a single anchor.
///
/// Transforms paths like `chapter2.xhtml#section-3` into `chapter2-xhtml-section-3`.
/// This creates valid markdown anchor targets from multi-file references.
///
/// TODO: Use this when internal link resolution is implemented.
#[allow(dead_code)]
fn flatten_to_anchor(path: &str) -> String {
    // Remove leading # if present
    let path = path.strip_prefix('#').unwrap_or(path);

    // Replace problematic characters with hyphens
    let result: String = path
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            '#' | '/' | '.' => '-',
            _ => '-',
        })
        .collect();

    // Clean up: remove leading/trailing hyphens and collapse multiple hyphens
    let result = result.to_lowercase();
    let mut cleaned = String::new();
    let mut last_was_hyphen = true; // Start true to skip leading hyphens
    for c in result.chars() {
        if c == '-' {
            if !last_was_hyphen {
                cleaned.push('-');
            }
            last_was_hyphen = true;
        } else {
            cleaned.push(c);
            last_was_hyphen = false;
        }
    }
    // Remove trailing hyphen
    if cleaned.ends_with('-') {
        cleaned.pop();
    }
    cleaned
}

/// Create a flattened ID by combining chapter path and element ID.
///
/// For example, if chapter_path is "OEBPS/text/chapter1.xhtml" and id is "section-3",
/// the result is "oebps-text-chapter1-xhtml-section-3".
///
/// TODO: Use this when internal link resolution is implemented.
#[allow(dead_code)]
fn flatten_id(chapter_path: Option<&str>, id: &str) -> String {
    match chapter_path {
        Some(path) => {
            let path_part = flatten_to_anchor(path);
            let id_part = flatten_to_anchor(id);
            if path_part.is_empty() {
                id_part
            } else if id_part.is_empty() {
                path_part
            } else {
                format!("{}-{}", path_part, id_part)
            }
        }
        None => flatten_to_anchor(id),
    }
}

/// Escape special Markdown characters in text.
///
/// Following Pandoc's escapeText pattern, this escapes:
/// - Backslash (must be first)
/// - Asterisks and underscores (emphasis)
/// - Brackets (links/images)
/// - Backticks (code)
/// - Hash at line start (headers)
/// - Pipes (tables)
fn escape_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.len() / 10);
    let mut chars = text.chars().peekable();
    let mut at_line_start = true;

    while let Some(c) = chars.next() {
        match c {
            // Backslash must be escaped first
            '\\' => result.push_str("\\\\"),
            // Emphasis markers
            '*' | '_' => {
                result.push('\\');
                result.push(c);
            }
            // Link/image brackets
            '[' | ']' => {
                result.push('\\');
                result.push(c);
            }
            // Code backticks
            '`' => {
                result.push('\\');
                result.push(c);
            }
            // Headers at line start
            '#' if at_line_start => {
                result.push('\\');
                result.push(c);
            }
            // Table pipes
            '|' => {
                result.push('\\');
                result.push(c);
            }
            // Angle brackets (autolinks)
            '<' | '>' => {
                result.push('\\');
                result.push(c);
            }
            // Image marker
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
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::ir::{IRChapter, Node};
    use std::io::Cursor;

    fn export_to_string(chapter: &IRChapter, format: TextFormat) -> String {
        export_to_string_with_path(chapter, format, None)
    }

    fn export_to_string_with_path(
        chapter: &IRChapter,
        format: TextFormat,
        chapter_path: Option<&str>,
    ) -> String {
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);

        let mut ctx = ExportContext {
            writer: &mut cursor,
            ir: chapter,
            format,
            line_prefix: String::new(),
            list_stack: Vec::new(),
            at_line_start: true,
            has_line_content: false,
            pending_newline: false,
            last_block_role: None,
            footnotes: Vec::new(),
            chapter_path: chapter_path.map(|s| s.to_string()),
        };

        for child_id in chapter.children(NodeId::ROOT) {
            ctx.walk_node(child_id).unwrap();
        }

        // Render accumulated footnotes
        if !ctx.footnotes.is_empty() && format == TextFormat::Markdown {
            writeln!(ctx.writer).unwrap();
            for note in &ctx.footnotes {
                writeln!(ctx.writer, "[^{}]: {}", note.number, note.content).unwrap();
            }
        }

        String::from_utf8(output).unwrap()
    }

    #[test]
    fn test_simple_paragraph() {
        let mut chapter = IRChapter::new();

        let para = chapter.alloc_node(Node::new(Role::Paragraph));
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

        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
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

        let ol = chapter.alloc_node(Node::new(Role::OrderedList));
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
        chapter.semantics.set_src(img, "photo.jpg".to_string());
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
    fn test_image_has_blank_lines() {
        let mut chapter = IRChapter::new();

        // Heading before image
        let h1 = chapter.alloc_node(Node::new(Role::Heading(1)));
        chapter.append_child(NodeId::ROOT, h1);
        let t1 = chapter.append_text("Title");
        let tn1 = chapter.alloc_node(Node::text(t1));
        chapter.append_child(h1, tn1);

        // Image
        let img = chapter.alloc_node(Node::new(Role::Image));
        chapter.append_child(NodeId::ROOT, img);
        chapter.semantics.set_src(img, "photo.jpg".to_string());
        chapter.semantics.set_alt(img, "A photo".to_string());

        // Paragraph after image
        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);
        let t2 = chapter.append_text("Some text");
        let tn2 = chapter.alloc_node(Node::text(t2));
        chapter.append_child(p, tn2);

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Image should have blank lines around it (be a block element)
        assert!(
            result.contains("# Title\n\n![A photo](photo.jpg)\n\nSome text"),
            "Image should have blank lines around it: {:?}",
            result
        );
    }

    #[test]
    fn test_blockquote_multiline() {
        let mut chapter = IRChapter::new();

        let bq = chapter.alloc_node(Node::new(Role::BlockQuote));
        chapter.append_child(NodeId::ROOT, bq);

        // First paragraph in blockquote
        let p1 = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(bq, p1);
        let t1 = chapter.append_text("Line one");
        let tn1 = chapter.alloc_node(Node::text(t1));
        chapter.append_child(p1, tn1);

        // Second paragraph in blockquote
        let p2 = chapter.alloc_node(Node::new(Role::Paragraph));
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

        let ol = chapter.alloc_node(Node::new(Role::OrderedList));
        chapter.append_child(NodeId::ROOT, ol);

        let li = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ol, li);

        let bq = chapter.alloc_node(Node::new(Role::BlockQuote));
        chapter.append_child(li, bq);

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
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

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
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

    #[test]
    fn test_markdown_escaping() {
        let mut chapter = IRChapter::new();

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);

        let text_range = chapter.append_text("*bold* and _italic_ and [link] and `code`");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(p, text_node);

        let result = export_to_string(&chapter, TextFormat::Markdown);
        // Special chars should be escaped
        assert!(
            result.contains("\\*bold\\*"),
            "Asterisks should be escaped: {}",
            result
        );
        assert!(
            result.contains("\\_italic\\_"),
            "Underscores should be escaped: {}",
            result
        );
        assert!(
            result.contains("\\[link\\]"),
            "Brackets should be escaped: {}",
            result
        );
        assert!(
            result.contains("\\`code\\`"),
            "Backticks should be escaped: {}",
            result
        );
    }

    #[test]
    fn test_plain_no_escaping() {
        let mut chapter = IRChapter::new();

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);

        let text_range = chapter.append_text("*bold* and _italic_");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(p, text_node);

        let result = export_to_string(&chapter, TextFormat::Plain);
        // Plain text should NOT escape
        assert!(
            result.contains("*bold*"),
            "Plain should not escape: {}",
            result
        );
        assert!(
            result.contains("_italic_"),
            "Plain should not escape: {}",
            result
        );
    }

    #[test]
    fn test_tight_list() {
        let mut chapter = IRChapter::new();

        // Create a tight list (items with simple text)
        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);

        for text in ["First", "Second", "Third"] {
            let li = chapter.alloc_node(Node::new(Role::ListItem));
            chapter.append_child(ul, li);
            let t = chapter.append_text(text);
            let tn = chapter.alloc_node(Node::text(t));
            chapter.append_child(li, tn);
        }

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Tight list should NOT have blank lines between items
        let lines: Vec<&str> = result.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines.len(),
            3,
            "Tight list should have 3 lines: {:?}",
            lines
        );
    }

    #[test]
    fn test_loose_list() {
        let mut chapter = IRChapter::new();

        // Create a loose list (items with multiple paragraphs)
        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);

        for _ in 0..2 {
            let li = chapter.alloc_node(Node::new(Role::ListItem));
            chapter.append_child(ul, li);

            // First paragraph
            let p1 = chapter.alloc_node(Node::new(Role::Paragraph));
            chapter.append_child(li, p1);
            let t1 = chapter.append_text("First para");
            let tn1 = chapter.alloc_node(Node::text(t1));
            chapter.append_child(p1, tn1);

            // Second paragraph (makes it loose)
            let p2 = chapter.alloc_node(Node::new(Role::Paragraph));
            chapter.append_child(li, p2);
            let t2 = chapter.append_text("Second para");
            let tn2 = chapter.alloc_node(Node::text(t2));
            chapter.append_child(p2, tn2);
        }

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Loose list SHOULD have blank lines between items
        // There should be a blank line somewhere in the output
        let has_blank_line = result.contains("\n\n");
        assert!(
            has_blank_line,
            "Loose list should have blank lines: {:?}",
            result
        );
    }

    #[test]
    fn test_adjacent_lists_separator() {
        let mut chapter = IRChapter::new();

        // Create two adjacent unordered lists
        for _ in 0..2 {
            let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
            chapter.append_child(NodeId::ROOT, ul);

            let li = chapter.alloc_node(Node::new(Role::ListItem));
            chapter.append_child(ul, li);
            let t = chapter.append_text("Item");
            let tn = chapter.alloc_node(Node::text(t));
            chapter.append_child(li, tn);
        }

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Should have HTML comment separator between the lists
        assert!(
            result.contains("<!-- -->"),
            "Adjacent lists should have separator: {:?}",
            result
        );
    }

    #[test]
    fn test_different_list_types_no_separator() {
        let mut chapter = IRChapter::new();

        // Create an unordered list followed by an ordered list
        let ul = chapter.alloc_node(Node::new(Role::UnorderedList));
        chapter.append_child(NodeId::ROOT, ul);
        let li1 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ul, li1);
        let t1 = chapter.append_text("Bullet item");
        let tn1 = chapter.alloc_node(Node::text(t1));
        chapter.append_child(li1, tn1);

        let ol = chapter.alloc_node(Node::new(Role::OrderedList));
        chapter.append_child(NodeId::ROOT, ol);
        let li2 = chapter.alloc_node(Node::new(Role::ListItem));
        chapter.append_child(ol, li2);
        let t2 = chapter.append_text("Numbered item");
        let tn2 = chapter.alloc_node(Node::text(t2));
        chapter.append_child(li2, tn2);

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Different list types should NOT have separator
        assert!(
            !result.contains("<!-- -->"),
            "Different list types should not need separator: {:?}",
            result
        );
    }

    #[test]
    fn test_code_block_with_backticks() {
        let mut chapter = IRChapter::new();

        let code = chapter.alloc_node(Node::new(Role::CodeBlock));
        chapter.append_child(NodeId::ROOT, code);

        // Content with backticks
        let t = chapter.append_text("```rust\nlet x = 1;\n```");
        let tn = chapter.alloc_node(Node::text(t));
        chapter.append_child(code, tn);

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Should use 4 backticks since content has 3
        assert!(
            result.contains("````"),
            "Should use 4 backticks when content has 3: {:?}",
            result
        );
    }

    #[test]
    fn test_inline_code_with_backtick() {
        use crate::ir::ComputedStyle;

        let mut chapter = IRChapter::new();

        let p = chapter.alloc_node(Node::new(Role::Paragraph));
        chapter.append_child(NodeId::ROOT, p);

        // Create inline code span with monospace font
        let mut code_style = ComputedStyle::default();
        code_style.font_family = Some("monospace".to_string());
        let style_id = chapter.styles.intern(code_style);

        let mut code_node = Node::new(Role::Inline);
        code_node.style = style_id;
        let code = chapter.alloc_node(code_node);
        chapter.append_child(p, code);

        // Content with a backtick
        let t = chapter.append_text("`var`");
        let tn = chapter.alloc_node(Node::text(t));
        chapter.append_child(code, tn);

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Should use 2 backticks since content has 1
        assert!(
            result.contains("`` `var` ``"),
            "Should use double backticks with spacing: {:?}",
            result
        );
    }

    #[test]
    fn test_footnote_accumulation() {
        let mut chapter = IRChapter::new();

        // Paragraph with footnote
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

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Should have inline reference
        assert!(
            result.contains("[^1]"),
            "Should have inline footnote reference: {:?}",
            result
        );

        // Should have footnote definition at end
        assert!(
            result.contains("[^1]: This is a footnote"),
            "Should have footnote definition: {:?}",
            result
        );
    }

    #[test]
    fn test_footnote_plain_text() {
        let mut chapter = IRChapter::new();

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

        let result = export_to_string(&chapter, TextFormat::Plain);

        // Plain text should render inline
        assert!(
            result.contains("[note: This is a footnote]"),
            "Plain text should render footnote inline: {:?}",
            result
        );
    }

    #[test]
    fn test_anchor_id_not_output_yet() {
        // TODO: Update this test when internal link resolution is implemented
        let mut chapter = IRChapter::new();

        // Heading with ID
        let h1 = chapter.alloc_node(Node::new(Role::Heading(1)));
        chapter.append_child(NodeId::ROOT, h1);
        chapter.semantics.set_id(h1, "chapter-one".to_string());

        let text_range = chapter.append_text("Chapter One");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(h1, text_node);

        let result =
            export_to_string_with_path(&chapter, TextFormat::Markdown, Some("text/ch1.xhtml"));

        // Anchors are not output yet (internal links not implemented)
        assert!(
            !result.contains("<a id="),
            "Should not have anchors yet: {:?}",
            result
        );
    }

    #[test]
    fn test_internal_link_outputs_text_only() {
        let mut chapter = IRChapter::new();

        let link = chapter.alloc_node(Node::new(Role::Link));
        chapter.append_child(NodeId::ROOT, link);
        // Set an internal link to another file
        chapter
            .semantics
            .set_href(link, "chapter2.xhtml#section-3".to_string());

        let text_range = chapter.append_text("See section 3");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(link, text_node);

        let result = export_to_string(&chapter, TextFormat::Markdown);

        // Internal links currently just output text (TODO: resolve to anchors)
        assert!(
            result.contains("See section 3"),
            "Should output link text: {:?}",
            result
        );
        assert!(
            !result.contains("[See section 3]"),
            "Should not have markdown link syntax: {:?}",
            result
        );
    }
}
