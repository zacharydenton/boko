//! Format importers for reading ebook files.
//!
//! The `Importer` trait defines a two-track interface:
//! - **Track 1 (Normalization)**: Parse content into IR for rendering
//! - **Track 2 (Raw Access)**: Provide raw bytes for high-fidelity conversion

mod azw3;
mod epub;
mod kfx;
mod mobi;

pub use azw3::Azw3Importer;
pub use epub::EpubImporter;
pub use kfx::KfxImporter;
pub use mobi::MobiImporter;

use std::path::{Path, PathBuf};

use crate::book::{Landmark, Metadata, TocEntry};
use crate::compiler::{compile_html_bytes, extract_stylesheets, Origin, Stylesheet};
use crate::ir::IRChapter;

/// Unique identifier for a chapter/spine item within a book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChapterId(pub u32);

/// Entry in the reading order (spine).
#[derive(Debug, Clone)]
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

    /// Landmarks (structural navigation points like cover, start reading location).
    fn landmarks(&self) -> &[Landmark];

    /// Reading order (spine).
    fn spine(&self) -> &[SpineEntry];

    // --- Track 1: Normalization (The Reader) ---

    /// Load a chapter as normalized IR.
    ///
    /// The default implementation:
    /// 1. Loads raw HTML via `load_raw()`
    /// 2. Extracts linked stylesheets and inline styles
    /// 3. Loads and parses linked CSS via `load_asset()`
    /// 4. Compiles HTML + CSS to IR via `compile_html()`
    ///
    /// Implementations may override for format-specific optimizations.
    fn load_chapter(&mut self, id: ChapterId) -> std::io::Result<IRChapter> {
        // Load raw HTML
        let html_bytes = self.load_raw(id)?;
        let html_str = String::from_utf8_lossy(&html_bytes);

        // Extract stylesheet references
        let (linked, inline) = extract_stylesheets(&html_str);

        // Build stylesheets list
        let mut stylesheets = Vec::new();

        // Load linked stylesheets
        for href in linked {
            // Resolve relative path based on chapter's source path
            let css_path = if let Some(chapter_path) = self.source_id(id) {
                resolve_relative_path(chapter_path, &href)
            } else {
                PathBuf::from(&href)
            };

            if let Ok(css_bytes) = self.load_asset(&css_path) {
                let css_str = String::from_utf8_lossy(&css_bytes);
                stylesheets.push((Stylesheet::parse(&css_str), Origin::Author));
            }
        }

        // Parse inline styles
        for css in inline {
            stylesheets.push((Stylesheet::parse(&css), Origin::Author));
        }

        // Compile to IR
        let mut chapter = compile_html_bytes(&html_bytes, &stylesheets);

        // Post-process: Resolve relative paths in semantic attributes (src, href)
        // This canonicalizes paths like "../images/photo.jpg" to "OEBPS/images/photo.jpg"
        if let Some(base_path) = self.source_id(id) {
            resolve_semantic_paths(&mut chapter, base_path);
        }

        Ok(chapter)
    }

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

    /// Whether this importer requires normalized export for HTML-based formats.
    ///
    /// Returns true for binary formats (KFX) where load_raw returns non-HTML data.
    /// Exporters should use IR-based output when this returns true.
    fn requires_normalized_export(&self) -> bool {
        false
    }
}

/// Resolve a relative path against a base path.
///
/// For example, if base is "OEBPS/text/ch01.xhtml" and relative is "../styles/main.css",
/// the result is "OEBPS/styles/main.css".
fn resolve_relative_path(base: &str, relative: &str) -> PathBuf {
    // Handle absolute paths and URLs
    if relative.starts_with('/') || relative.contains("://") {
        return PathBuf::from(relative);
    }

    // Get the directory of the base path
    let base_path = Path::new(base);
    let base_dir = base_path.parent().unwrap_or(Path::new(""));

    // Join and normalize
    let joined = base_dir.join(relative);

    // Normalize by iterating through components
    let mut result = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::Normal(name) => {
                result.push(name);
            }
            std::path::Component::CurDir => {}
            std::path::Component::RootDir => {
                result.push("/");
            }
            std::path::Component::Prefix(prefix) => {
                result.push(prefix.as_os_str());
            }
        }
    }

    result
}

/// Resolve relative paths in a chapter's semantic attributes.
///
/// This canonicalizes paths like `../images/photo.jpg` relative to the
/// chapter's source path (e.g., `OEBPS/text/ch1.html`) to absolute archive
/// paths (e.g., `OEBPS/images/photo.jpg`).
fn resolve_semantic_paths(chapter: &mut IRChapter, base_path: &str) {
    chapter.semantics.resolve_paths(|path| {
        // Skip external URLs and data URIs
        if path.contains("://") || path.starts_with("data:") {
            return path.to_string();
        }

        // Resolve relative path to absolute archive path
        let resolved = resolve_relative_path(base_path, path);
        resolved.to_string_lossy().to_string()
    });
}
