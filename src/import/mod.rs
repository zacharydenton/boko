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

use std::path::Path;
use std::sync::Arc;

use crate::dom::{Origin, Stylesheet};
use crate::model::{AnchorTarget, Chapter, FontFace, GlobalNodeId, Landmark, Metadata, TocEntry};

// `ChapterId` is a pure identifier defined in the data model; re-exported
// here for backwards compatibility (`crate::import::ChapterId`).
pub use crate::model::ChapterId;

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
    fn open(path: &Path) -> crate::Result<Self>
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
    /// 2. Parses the DOM once and extracts linked/inline stylesheets from it
    /// 3. Loads and parses linked CSS via `load_asset()`
    /// 4. Compiles the parsed DOM + CSS to IR via `compile_dom()`
    ///
    /// Implementations may override for format-specific optimizations.
    fn load_chapter(&self, id: ChapterId) -> crate::Result<Chapter> {
        let html_bytes = self.load_raw(id)?;
        let base_path = self.source_id(id).map(str::to_string);
        Ok(compile_chapter_html(
            &html_bytes,
            base_path.as_deref(),
            &mut |path| self.load_stylesheet(path),
        ))
    }

    /// Load several chapters as normalized IR.
    ///
    /// Importers are `Sync` and chapter loads take `&self`, so the default
    /// implementation compiles chapters in parallel (with the `parallel`
    /// feature, native targets) — HTML parsing, the CSS cascade, and IR
    /// transformation dominate cold conversion and are independent per
    /// chapter.
    fn load_chapters(&self, ids: &[ChapterId]) -> Vec<crate::Result<Chapter>> {
        #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
        {
            use rayon::prelude::*;
            ids.par_iter().map(|&id| self.load_chapter(id)).collect()
        }
        #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
        {
            ids.iter().map(|&id| self.load_chapter(id)).collect()
        }
    }

    // --- Track 2: Raw Access (The Converter) ---

    /// Returns the internal source path for a chapter (e.g., "OEBPS/text/ch01.xhtml").
    fn source_id(&self, id: ChapterId) -> Option<&str>;

    /// Returns the raw bytes of a chapter.
    fn load_raw(&self, id: ChapterId) -> crate::Result<Vec<u8>>;

    // --- Assets ---

    /// List all assets (images, fonts, CSS, etc.).
    ///
    /// Asset paths are archive entry names (e.g. `"OEBPS/images/cover.jpg"`),
    /// always separated by forward slashes; they are not filesystem paths.
    fn list_assets(&self) -> &[String];

    /// Load an asset by archive entry name.
    fn load_asset(&self, path: &str) -> crate::Result<Vec<u8>>;

    /// Load and parse a stylesheet, optionally using a cache.
    ///
    /// The default implementation loads the asset bytes and parses CSS.
    /// Returns an `Arc` so cached sheets are shared across chapters instead
    /// of deep-cloning the parsed rules per chapter.
    fn load_stylesheet(&self, path: &str) -> Option<Arc<Stylesheet>> {
        if let Ok(css_bytes) = self.load_asset(path) {
            let css_str = String::from_utf8_lossy(&css_bytes);
            return Some(Arc::new(Stylesheet::parse(&css_str)));
        }
        None
    }

    /// Collect all @font-face definitions from CSS files.
    ///
    /// Parses all CSS assets and extracts @font-face rules that map font family
    /// names to font files. The returned font-faces have their `src` paths
    /// resolved to canonical paths within the book archive.
    ///
    /// This is used by KFX export to create font entities linking font-family
    /// names to resource locations.
    fn font_faces(&self) -> Vec<FontFace> {
        let mut font_faces = Vec::new();

        // Find all CSS files
        let css_paths: Vec<_> = self
            .list_assets()
            .iter()
            .filter(|p| {
                Path::new(p.as_str())
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("css"))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        for css_path in css_paths {
            if let Some(stylesheet) = self.load_stylesheet(&css_path) {
                // Resolve relative font paths to canonical paths
                for font_face in &stylesheet.font_faces {
                    let mut font_face = font_face.clone();
                    // Resolve the src path relative to the CSS file location
                    font_face.src = resolve_relative_path(&css_path, &font_face.src);
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
    fn index_anchors(&self, _chapters: &[(ChapterId, Arc<Chapter>)]) {
        // Default: no-op. Path-based resolution in resolve_href() handles EPUB.
        // Format-specific importers override to build their anchor maps.
    }

    /// Resolve TOC href fragments after chapters are loaded.
    ///
    /// Called after `index_anchors()`. Importers whose TOC entries are built
    /// without fragment identifiers (AZW3/MOBI) return a fixed-up copy of the
    /// TOC; the default returns `None` (EPUB/KFX have correct hrefs from
    /// source). The resolved TOC is cached by [`Book`](crate::Book) — the
    /// importer's own entries are never mutated.
    fn resolve_toc(&self) -> Option<Vec<TocEntry>> {
        None
    }

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

/// Compile a chapter's raw HTML bytes to normalized IR.
///
/// This is the body shared by [`Importer::load_chapter`]'s default
/// implementation and parallel [`Importer::load_chapters`] overrides:
/// decode, parse the DOM exactly once (the same parse serves stylesheet
/// discovery and IR compilation), resolve linked CSS through `load_sheet`,
/// and compile to IR.
pub(crate) fn compile_chapter_html(
    html_bytes: &[u8],
    base_path: Option<&str>,
    load_sheet: &mut dyn FnMut(&str) -> Option<Arc<Stylesheet>>,
) -> Chapter {
    let hint_encoding = crate::util::extract_xml_encoding(html_bytes);
    let html_str = crate::util::decode_text(html_bytes, hint_encoding);
    let dom = crate::dom::parse_dom(&html_str);

    // Extract stylesheet references
    let (linked, inline) = crate::dom::extract_stylesheets_from_dom(&dom);

    // Build stylesheets list (Arc-shared: cached sheets are not cloned)
    let mut stylesheets: Vec<(Arc<Stylesheet>, Origin)> = Vec::new();

    // Load linked stylesheets, resolving relative to the chapter's source path
    for href in linked {
        let css_path = match base_path {
            Some(chapter_path) => resolve_relative_path(chapter_path, &href),
            // Archive lookup keys use forward slashes; hrefs come from parsed
            // content and may use backslashes.
            None => normalize_separators(href),
        };
        if let Some(sheet) = load_sheet(&css_path) {
            stylesheets.push((sheet, Origin::Author));
        }
    }

    // Parse inline styles
    for css in inline {
        stylesheets.push((Arc::new(Stylesheet::parse(&css)), Origin::Author));
    }

    // Compile to IR from the DOM parsed above
    let sheet_refs: Vec<(&Stylesheet, Origin)> =
        stylesheets.iter().map(|(s, o)| (s.as_ref(), *o)).collect();
    let mut chapter = crate::dom::compile_dom(&dom, &sheet_refs);

    // Post-process: Resolve relative paths in semantic attributes (src, href)
    // This canonicalizes paths like "../images/photo.jpg" to "OEBPS/images/photo.jpg"
    if let Some(base) = base_path {
        resolve_semantic_paths(&mut chapter, base);
    }

    chapter
}

/// Normalize backslashes to forward slashes.
///
/// Archive entry names always use forward slashes; backslashes can only
/// arrive from parsed content (hrefs, CSS urls) written by sloppy tooling.
fn normalize_separators(path: String) -> String {
    if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path
    }
}

/// Resolve a relative path against a base path.
///
/// For example, if base is "OEBPS/text/ch01.xhtml" and relative is "../styles/main.css",
/// the result is "OEBPS/styles/main.css".
///
/// Fragment-only paths (e.g., "#anchor") are resolved to "base#anchor".
///
/// Both inputs are archive entry names separated by forward slashes; the
/// result is normalized to forward slashes as well.
fn resolve_relative_path(base: &str, relative: &str) -> String {
    // Handle absolute paths and URLs
    if relative.starts_with('/') || relative.contains("://") {
        return normalize_separators(relative.to_string());
    }

    // Handle fragment-only paths (#anchor) - resolve to base file + fragment
    if relative.starts_with('#') {
        return normalize_separators(format!("{}{}", base, relative));
    }

    // Normalize backslashes BEFORE splitting on '/': `..\styles\main.css`
    // from Windows-authored content must split into components, or the `..`
    // survives as a literal segment and the archive lookup silently misses.
    let base = normalize_separators(base.to_string());
    let relative = normalize_separators(relative.to_string());

    // Get the directory of the base path
    let base_dir = base.rsplit_once('/').map_or("", |(dir, _)| dir);

    // Join and normalize `.` / `..` / empty components
    let mut result: Vec<&str> = Vec::new();
    for component in base_dir.split('/').chain(relative.split('/')) {
        match component {
            "" | "." => {}
            ".." => {
                result.pop();
            }
            name => result.push(name),
        }
    }

    let leading = if base.starts_with('/') { "/" } else { "" };
    normalize_separators(format!("{}{}", leading, result.join("/")))
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
        resolve_relative_path(base_path, path)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Landmark, Metadata, TocEntry};
    use proptest::prelude::*;
    use std::collections::HashMap;
    use std::io;

    #[test]
    fn test_resolve_fragment_only_path() {
        // Fragment-only paths should resolve to base + fragment
        let result = resolve_relative_path("f_0004.xhtml", "#FOOTNOTE-1");
        assert_eq!(result, "f_0004.xhtml#FOOTNOTE-1");

        let result = resolve_relative_path("OEBPS/text/chapter.xhtml", "#anchor");
        assert_eq!(result, "OEBPS/text/chapter.xhtml#anchor");
    }

    #[test]
    fn test_resolve_relative_path_with_fragment() {
        // Relative paths with fragments should resolve normally
        let result = resolve_relative_path("text/ch1.xhtml", "ch2.xhtml#section");
        assert_eq!(result, "text/ch2.xhtml#section");
    }

    #[test]
    fn test_resolve_parent_directory() {
        let result = resolve_relative_path("OEBPS/text/ch01.xhtml", "../styles/main.css");
        assert_eq!(result, "OEBPS/styles/main.css");
    }

    #[test]
    fn test_resolve_absolute_path_unchanged() {
        let result = resolve_relative_path("text/chapter.xhtml", "/absolute/path.css");
        assert_eq!(result, "/absolute/path.css");
    }

    #[test]
    fn test_resolve_url_unchanged() {
        let result = resolve_relative_path("text/chapter.xhtml", "https://example.com/");
        assert_eq!(result, "https://example.com/");
    }

    #[test]
    fn test_load_chapter_stylesheet_cache() {
        struct TestImporter {
            chapters: HashMap<u32, String>,
            assets: HashMap<String, Vec<u8>>,
            asset_list: Vec<String>,
            css_cache: std::sync::Mutex<HashMap<String, Arc<Stylesheet>>>,
            css_loads: std::sync::atomic::AtomicUsize,
            metadata: Metadata,
            toc: Vec<TocEntry>,
            landmarks: Vec<Landmark>,
            spine: Vec<SpineEntry>,
            source_ids: Vec<String>,
        }

        impl Importer for TestImporter {
            fn open(_path: &Path) -> crate::Result<Self> {
                unreachable!()
            }

            fn metadata(&self) -> &Metadata {
                &self.metadata
            }

            fn toc(&self) -> &[TocEntry] {
                &self.toc
            }

            fn landmarks(&self) -> &[Landmark] {
                &self.landmarks
            }

            fn spine(&self) -> &[SpineEntry] {
                &self.spine
            }

            fn source_id(&self, id: ChapterId) -> Option<&str> {
                self.source_ids.get(id.0 as usize).map(|s| s.as_str())
            }

            fn load_raw(&self, id: ChapterId) -> crate::Result<Vec<u8>> {
                self.chapters
                    .get(&id.0)
                    .map(|s| s.as_bytes().to_vec())
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::NotFound, "chapter not found").into()
                    })
            }

            fn list_assets(&self) -> &[String] {
                &self.asset_list
            }

            fn load_asset(&self, path: &str) -> crate::Result<Vec<u8>> {
                self.assets.get(path).cloned().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "asset not found").into()
                })
            }

            fn load_stylesheet(&self, path: &str) -> Option<Arc<Stylesheet>> {
                let mut cache = self.css_cache.lock().unwrap();
                if let Some(sheet) = cache.get(path) {
                    return Some(Arc::clone(sheet));
                }
                let css_bytes = self.load_asset(path).ok()?;
                let css_str = String::from_utf8_lossy(&css_bytes);
                let sheet = Arc::new(Stylesheet::parse(&css_str));
                cache.insert(path.to_string(), sheet.clone());
                self.css_loads
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some(sheet)
            }
        }

        let importer = TestImporter {
            chapters: HashMap::from([
                (
                    0,
                    r#"<html><head><link rel="stylesheet" href="style.css"></head><body>One</body></html>"#
                        .to_string(),
                ),
                (
                    1,
                    r#"<html><head><link rel="stylesheet" href="style.css"></head><body>Two</body></html>"#
                        .to_string(),
                ),
            ]),
            assets: HashMap::from([(
                "text/style.css".to_string(),
                b"p { color: red; }".to_vec(),
            )]),
            asset_list: vec!["text/style.css".to_string()],
            css_cache: std::sync::Mutex::new(HashMap::new()),
            css_loads: std::sync::atomic::AtomicUsize::new(0),
            metadata: Metadata::default(),
            toc: Vec::new(),
            landmarks: Vec::new(),
            spine: vec![
                SpineEntry {
                    id: ChapterId(0),
                    size_estimate: 0,
                },
                SpineEntry {
                    id: ChapterId(1),
                    size_estimate: 0,
                },
            ],
            source_ids: vec!["text/ch1.xhtml".to_string(), "text/ch2.xhtml".to_string()],
        };

        let _ = importer.load_chapter(ChapterId(0)).unwrap();
        let _ = importer.load_chapter(ChapterId(1)).unwrap();

        assert_eq!(importer.css_loads.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn test_font_faces_uses_load_stylesheet() {
        struct TestImporter {
            asset_list: Vec<String>,
            metadata: Metadata,
            toc: Vec<TocEntry>,
            landmarks: Vec<Landmark>,
            spine: Vec<SpineEntry>,
        }

        impl Importer for TestImporter {
            fn open(_path: &Path) -> crate::Result<Self> {
                unreachable!()
            }

            fn metadata(&self) -> &Metadata {
                &self.metadata
            }

            fn toc(&self) -> &[TocEntry] {
                &self.toc
            }

            fn landmarks(&self) -> &[Landmark] {
                &self.landmarks
            }

            fn spine(&self) -> &[SpineEntry] {
                &self.spine
            }

            fn source_id(&self, _id: ChapterId) -> Option<&str> {
                None
            }

            fn load_raw(&self, _id: ChapterId) -> crate::Result<Vec<u8>> {
                Err(io::Error::other("unused").into())
            }

            fn list_assets(&self) -> &[String] {
                &self.asset_list
            }

            fn load_asset(&self, _path: &str) -> crate::Result<Vec<u8>> {
                Err(io::Error::other("load_asset should not be called").into())
            }

            fn load_stylesheet(&self, _path: &str) -> Option<Arc<Stylesheet>> {
                let css = "@font-face { font-family: Test; src: url(../fonts/test.woff); }";
                Some(Arc::new(Stylesheet::parse(css)))
            }
        }

        let importer = TestImporter {
            asset_list: vec!["styles/main.css".to_string()],
            metadata: Metadata::default(),
            toc: Vec::new(),
            landmarks: Vec::new(),
            spine: Vec::new(),
        };

        let fonts = importer.font_faces();
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].font_family, "Test");
        assert_eq!(fonts[0].src, "fonts/test.woff");
    }

    proptest! {
        #[test]
        fn prop_resolve_relative_path_preserves_fragment_and_no_backslashes(
            base_parts in prop::collection::vec("[a-z]{1,8}", 1..5),
            target_parts in prop::collection::vec("[a-z]{1,8}", 1..5),
            fragment in "[A-Za-z0-9_-]{1,12}",
            up_levels in 0usize..3
        ) {
            // Build a base like "dir/sub/chapter.xhtml"
            let mut base = base_parts.join("/");
            base.push_str("/chapter.xhtml");

            // Build a relative target like "../a/b.xhtml#frag"
            let mut target = String::new();
            for _ in 0..up_levels {
                target.push_str("../");
            }
            target.push_str(&target_parts.join("/"));
            target.push_str(".xhtml#");
            target.push_str(&fragment);

            let normalized = resolve_relative_path(&base, &target);

            // Fragment preserved.
            let expected_fragment = format!("#{}", fragment);
            prop_assert!(normalized.ends_with(&expected_fragment));
            // Archive paths should be normalized to forward slashes.
            prop_assert!(!normalized.contains('\\'));
        }

        #[test]
        fn prop_resolve_relative_path_preserves_absolute_and_urls(
            base_parts in prop::collection::vec("[a-z]{1,8}", 1..5),
            absolute in "[A-Za-z0-9/_\\-]{1,24}",
            path in "[A-Za-z0-9/_\\-]{1,24}",
        ) {
            let mut base = base_parts.join("/");
            base.push_str("/chapter.xhtml");

            let absolute_path = format!("/{}", absolute);
            let url = format!("https://example.com/{}", path);

            let resolved_abs = resolve_relative_path(&base, &absolute_path);
            prop_assert_eq!(resolved_abs, absolute_path);

            let resolved_url = resolve_relative_path(&base, &url);
            prop_assert_eq!(resolved_url, url);
        }

        #[test]
        fn prop_resolve_relative_path_eliminates_dotdot(
            base_parts in prop::collection::vec("[a-z]{1,8}", 2..5),
            target_parts in prop::collection::vec("[a-z]{1,8}", 1..4),
            up_levels in 0usize..2
        ) {
            let mut base = base_parts.join("/");
            base.push_str("/chapter.xhtml");

            let mut target = String::new();
            for _ in 0..up_levels {
                target.push_str("../");
            }
            target.push_str(&target_parts.join("/"));
            target.push_str(".xhtml");

            let normalized = resolve_relative_path(&base, &target);

            prop_assert!(!normalized.contains("/../"));
        }

        #[test]
        fn prop_resolve_fragment_only_appends_to_base(
            base_parts in prop::collection::vec("[a-z]{1,8}", 1..5),
            fragment in "[A-Za-z0-9_-]{1,12}"
        ) {
            let mut base = base_parts.join("/");
            base.push_str("/chapter.xhtml");

            let target = format!("#{}", fragment);
            let normalized = resolve_relative_path(&base, &target);

            let expected = format!("{}#{}", base, fragment);
            prop_assert_eq!(normalized, expected);
        }
    }
}
