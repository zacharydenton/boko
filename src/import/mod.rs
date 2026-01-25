//! Format importers for reading ebook files.
//!
//! The `Importer` trait defines a two-track interface:
//! - **Track 1 (Normalization)**: Parse content into IR (not yet implemented)
//! - **Track 2 (Raw Access)**: Provide raw bytes for conversion

mod epub;

pub use epub::EpubImporter;

use std::path::{Path, PathBuf};

use crate::book::{Metadata, TocEntry};

/// Unique identifier for a chapter/spine item within a book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChapterId(pub u32);

/// Entry in the reading order (spine).
pub struct SpineEntry {
    /// Unique identifier for this chapter.
    pub id: ChapterId,
    /// Estimated size in bytes (for progress indication).
    pub size_estimate: usize,
}

/// Polymorphic interface for format-specific backends.
///
/// Implementors provide access to book content via two tracks:
/// - Normalized access (IR) for rendering
/// - Raw access (bytes) for high-fidelity conversion
pub trait Importer: Send + Sync {
    // --- Lifecycle ---

    /// Open a file and parse structure (metadata, TOC, spine).
    fn open(path: &Path) -> std::io::Result<Self>
    where
        Self: Sized;

    /// Book metadata (title, authors, etc.).
    fn metadata(&self) -> &Metadata;

    /// Table of contents.
    fn toc(&self) -> &[TocEntry];

    /// Reading order (spine).
    fn spine(&self) -> &[SpineEntry];

    // --- Track 1: Normalization (The Reader) ---
    // TODO: fn load_chapter(&mut self, id: ChapterId) -> io::Result<IRChapter>;

    // --- Track 2: Raw Access (The Converter) ---

    /// Returns the internal source path for a chapter (e.g., "OEBPS/text/ch01.xhtml").
    fn source_id(&self, id: ChapterId) -> Option<&str>;

    /// Returns the raw bytes of a chapter.
    fn load_raw(&mut self, id: ChapterId) -> std::io::Result<Vec<u8>>;

    // --- Assets ---

    /// List all assets (images, fonts, CSS, etc.).
    fn list_assets(&self) -> Vec<PathBuf>;

    /// Load an asset by path.
    fn load_asset(&mut self, path: &Path) -> std::io::Result<Vec<u8>>;
}
