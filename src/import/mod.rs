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
use std::sync::Arc;

use crate::dom::{Origin, Stylesheet, compile_html_bytes, extract_stylesheets};
use crate::model::{AnchorTarget, Chapter, FontFace, GlobalNodeId, Landmark, Metadata, TocEntry};

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
    fn load_chapter(&mut self, id: ChapterId) -> std::io::Result<Chapter> {
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

    /// Collect all @font-face definitions from CSS files.
    ///
    /// Parses all CSS assets and extracts @font-face rules that map font family
    /// names to font files. The returned font-faces have their `src` paths
    /// resolved to canonical paths within the book archive.
    ///
    /// This is used by KFX export to create font entities linking font-family
    /// names to resource locations.
    fn font_faces(&mut self) -> Vec<FontFace> {
        let mut font_faces = Vec::new();

        // Find all CSS files
        let css_paths: Vec<_> = self
            .list_assets()
            .into_iter()
            .filter(|p| {
                p.extension()
                    .map(|e| e.eq_ignore_ascii_case("css"))
                    .unwrap_or(false)
            })
            .collect();

        for css_path in css_paths {
            if let Ok(css_bytes) = self.load_asset(&css_path) {
                let css_str = String::from_utf8_lossy(&css_bytes);
                let stylesheet = Stylesheet::parse(&css_str);

                // Resolve relative font paths to canonical paths
                for mut font_face in stylesheet.font_faces {
                    // Resolve the src path relative to the CSS file location
                    let resolved =
                        resolve_relative_path(css_path.to_string_lossy().as_ref(), &font_face.src);
                    font_face.src = resolved.to_string_lossy().to_string();
                    font_faces.push(font_face);
                }
            }
        }

        font_faces
    }

    /// Whether this importer requires normalized export for HTML-based formats.
    ///
    /// Returns true for binary formats (KFX) where load_raw returns non-HTML data.
    /// Exporters should use IR-based output when this returns true.
    fn requires_normalized_export(&self) -> bool {
        false
    }

    // --- Link Resolution ---

    /// Index all anchor targets after chapters are loaded.
    ///
    /// This method is called once with all loaded chapters, allowing the importer
    /// to build format-specific anchor maps. The default implementation builds
    /// a path#id → GlobalNodeId map suitable for EPUB-style linking.
    ///
    /// Importers should override this to handle format-specific anchor systems
    /// (e.g., KFX anchor entities, AZW3 fragment IDs).
    fn index_anchors(&mut self, _chapters: &[(ChapterId, Arc<Chapter>)]) {
        // Default: no-op. Path-based resolution in resolve_href() handles EPUB.
        // Format-specific importers override to build their anchor maps.
    }

    /// Resolve TOC href fragments after chapters are loaded.
    ///
    /// This method is called after `index_anchors()` to fix up TOC entries
    /// that were built without fragment identifiers (e.g., AZW3/MOBI).
    /// The default implementation does nothing (EPUB/KFX have correct hrefs).
    fn resolve_toc(&mut self) {
        // Default: no-op. EPUB and KFX have correct TOC hrefs from source.
    }

    /// Get mutable access to TOC entries for resolution.
    fn toc_mut(&mut self) -> &mut [TocEntry];

    /// Resolve an href to its target.
    ///
    /// Handles format-specific href parsing and resolution.
    /// Returns `None` if the href cannot be resolved (broken link).
    ///
    /// The default implementation only handles external URLs.
    /// Importers should override to handle internal links.
    fn resolve_href(&self, _from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        let href = href.trim();

        // External URLs
        if href.starts_with("http://")
            || href.starts_with("https://")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
        {
            return Some(AnchorTarget::External(href.to_string()));
        }

        None
    }
}

/// Helper for path-based href resolution (used by EPUB, AZW3, MOBI).
///
/// Handles EPUB-style paths: `path#fragment`, `#fragment`, `path`
pub fn resolve_path_based_href(
    from_path: &str,
    href: &str,
    chapter_for_path: impl Fn(&str) -> Option<ChapterId>,
    anchor: impl Fn(&str) -> Option<GlobalNodeId>,
) -> Option<AnchorTarget> {
    let href = href.trim();

    // External URLs
    if href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
    {
        return Some(AnchorTarget::External(href.to_string()));
    }

    // Fragment-only link (#id) - same chapter
    if let Some(fragment) = href.strip_prefix('#') {
        let key = format!("{}#{}", from_path, fragment);
        if let Some(target) = anchor(&key) {
            return Some(AnchorTarget::Internal(target));
        }
        return None;
    }

    // Split path and fragment
    let (path, fragment) = if let Some(hash_pos) = href.find('#') {
        (&href[..hash_pos], Some(&href[hash_pos + 1..]))
    } else {
        (href, None)
    };

    // Look up target chapter
    let target_chapter = chapter_for_path(path)?;

    // If there's a fragment, resolve to specific node
    if let Some(frag) = fragment {
        let key = format!("{}#{}", path, frag);
        if let Some(target) = anchor(&key) {
            return Some(AnchorTarget::Internal(target));
        }
        return None;
    }

    // No fragment - link to chapter start
    Some(AnchorTarget::Chapter(target_chapter))
}

/// Resolve a relative path against a base path.
///
/// For example, if base is "OEBPS/text/ch01.xhtml" and relative is "../styles/main.css",
/// the result is "OEBPS/styles/main.css".
///
/// Fragment-only paths (e.g., "#anchor") are resolved to "base#anchor".
fn resolve_relative_path(base: &str, relative: &str) -> PathBuf {
    // Handle absolute paths and URLs
    if relative.starts_with('/') || relative.contains("://") {
        return PathBuf::from(relative);
    }

    // Handle fragment-only paths (#anchor) - resolve to base file + fragment
    if relative.starts_with('#') {
        return PathBuf::from(format!("{}{}", base, relative));
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
fn resolve_semantic_paths(chapter: &mut Chapter, base_path: &str) {
    chapter.semantics.resolve_paths(|path| {
        // Skip external URLs and data URIs
        if path.contains("://") || path.starts_with("data:") {
            return path.to_string();
        }

        // Resolve relative path to absolute archive path
        let resolved = resolve_relative_path(base_path, path);
        // Normalize to forward slashes (archive paths, not filesystem paths)
        resolved.to_string_lossy().replace('\\', "/")
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_fragment_only_path() {
        // Fragment-only paths should resolve to base + fragment
        let result = resolve_relative_path("f_0004.xhtml", "#FOOTNOTE-1");
        assert_eq!(result.to_string_lossy(), "f_0004.xhtml#FOOTNOTE-1");

        let result = resolve_relative_path("OEBPS/text/chapter.xhtml", "#anchor");
        assert_eq!(result.to_string_lossy(), "OEBPS/text/chapter.xhtml#anchor");
    }

    #[test]
    fn test_resolve_relative_path_with_fragment() {
        // Relative paths with fragments should resolve normally
        let result = resolve_relative_path("text/ch1.xhtml", "ch2.xhtml#section");
        // Normalize path separators for cross-platform comparison
        let normalized: String = result.to_string_lossy().replace('\\', "/");
        assert_eq!(normalized, "text/ch2.xhtml#section");
    }

    #[test]
    fn test_resolve_parent_directory() {
        let result = resolve_relative_path("OEBPS/text/ch01.xhtml", "../styles/main.css");
        // Normalize path separators for cross-platform comparison
        let normalized: String = result.to_string_lossy().replace('\\', "/");
        assert_eq!(normalized, "OEBPS/styles/main.css");
    }

    #[test]
    fn test_resolve_absolute_path_unchanged() {
        let result = resolve_relative_path("text/chapter.xhtml", "/absolute/path.css");
        assert_eq!(result.to_string_lossy(), "/absolute/path.css");
    }

    #[test]
    fn test_resolve_url_unchanged() {
        let result = resolve_relative_path("text/chapter.xhtml", "https://example.com/");
        assert_eq!(result.to_string_lossy(), "https://example.com/");
    }
}
