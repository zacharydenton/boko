//! Markdown Exporter - I/O orchestration for markdown output.
//!
//! This module provides the thin I/O layer for exporting books to Markdown.
//! The actual rendering logic is in [`crate::markdown`].

use std::io::{self, Seek, Write};

use crate::markdown::{build_heading_slugs, render_chapter};
use crate::model::Book;

use super::Exporter;

/// Configuration for Markdown export.
#[derive(Debug, Clone, Default)]
pub struct MarkdownConfig {
    /// Line width for wrapping (0 = no wrapping).
    /// Reserved for future use.
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

        // 1. Resolve all links (I/O: loads chapters internally)
        let resolved = book.resolve_links()?;

        let spine: Vec<_> = book.spine().to_vec();

        // 2. Load all chapters and build heading slugs
        let chapters: Vec<_> = spine
            .iter()
            .map(|e| Ok((e.id, book.load_chapter_cached(e.id)?)))
            .collect::<io::Result<Vec<_>>>()?;

        let heading_slugs = build_heading_slugs(&chapters, &resolved);

        // 3. Render each chapter (pure) and write (I/O)
        let mut first = true;
        for (chapter_id, chapter) in &chapters {
            if !first {
                // Chapter separator
                writeln!(writer)?;
                writeln!(writer, "---")?;
                writeln!(writer)?;
            }
            first = false;

            // Pure rendering
            let result = render_chapter(chapter, *chapter_id, &resolved, &heading_slugs);

            // I/O: write content
            write!(writer, "{}", result.content)?;

            // I/O: write footnotes
            if !result.footnotes.is_empty() {
                writeln!(writer)?;
                for note in &result.footnotes {
                    writeln!(writer, "[^{}]: {}", note.number, note.content)?;
                }
            }
        }

        Ok(())
    }
}

// Unit tests for rendering are in `markdown/render.rs`.
// Integration tests using real EPUB files are in `tests/`.
