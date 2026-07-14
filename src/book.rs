//! The `Book` runtime handle for reading ebooks via importers.
//!
//! This module wires the pure data model (`crate::model`) to the
//! format-specific importer and exporter backends. It sits above both
//! `crate::import` and `crate::export` in the layering.

use std::collections::HashMap;
use std::io::{self, Seek, Write};
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::export::{Azw3Exporter, EpubExporter, Exporter, KfxExporter, MarkdownExporter};
use crate::import::{
    Azw3Importer, ChapterId, EpubImporter, Importer, KfxImporter, MobiImporter, SpineEntry,
};
use crate::io::MemorySource;
use crate::model::{AnchorTarget, Chapter, Format, Landmark, Metadata, ResolvedLinks, TocEntry};
use crate::resolved::resolve_book_links;

/// Runtime handle for an ebook.
///
/// `Book` wraps a format-specific `Importer` backend and provides
/// unified access to metadata, table of contents, and content.
///
/// # Example
///
/// ```no_run
/// use boko::Book;
///
/// let mut book = Book::open("input.epub")?;
/// println!("Title: {}", book.metadata().title);
///
/// // Load chapter content (collect spine first to avoid borrow issues)
/// let spine: Vec<_> = book.spine().to_vec();
/// for entry in spine {
///     let raw = book.load_raw(entry.id)?;
///     println!("Chapter {}: {} bytes", entry.id.0, raw.len());
/// }
/// # Ok::<(), boko::Error>(())
/// ```
pub struct Book {
    backend: Box<dyn Importer>,
    /// Cache of parsed IR chapters to avoid re-parsing during normalized export.
    /// Uses RwLock for thread-safe access and Arc for cheap cloning.
    ir_cache: Arc<RwLock<HashMap<ChapterId, Arc<Chapter>>>>,
}

impl Book {
    /// Open an ebook file, auto-detecting the format.
    pub fn open(path: impl AsRef<Path>) -> crate::Result<Self> {
        let path = path.as_ref();
        let format = Format::from_path(path).ok_or_else(|| crate::Error::UnsupportedFormat {
            detail: format!("unknown file format: {}", path.display()),
        })?;
        Self::open_format(path, format)
    }

    /// Open an ebook file with an explicit format.
    pub fn open_format(path: impl AsRef<Path>, format: Format) -> crate::Result<Self> {
        let backend: Box<dyn Importer> = match format {
            Format::Epub => Box::new(EpubImporter::open(path.as_ref())?),
            Format::Azw3 => Box::new(Azw3Importer::open(path.as_ref())?),
            Format::Mobi => Box::new(MobiImporter::open(path.as_ref())?),
            Format::Kfx => Box::new(KfxImporter::open(path.as_ref())?),
            Format::Markdown => {
                return Err(crate::Error::UnsupportedFormat {
                    detail: "Markdown format is export-only".into(),
                });
            }
        };
        Ok(Self {
            backend,
            ir_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a Book from in-memory bytes with an explicit format.
    ///
    /// This is useful for reading from stdin or other non-file sources.
    pub fn from_bytes(data: &[u8], format: Format) -> crate::Result<Self> {
        let source = Arc::new(MemorySource::new(data.to_vec()));
        let backend: Box<dyn Importer> = match format {
            Format::Epub => Box::new(EpubImporter::from_source(source)?),
            Format::Azw3 => Box::new(Azw3Importer::from_source(source)?),
            Format::Mobi => Box::new(MobiImporter::from_source(source)?),
            Format::Kfx => Box::new(KfxImporter::from_source(source)?),
            Format::Markdown => {
                return Err(crate::Error::UnsupportedFormat {
                    detail: "Markdown format is export-only".into(),
                });
            }
        };
        Ok(Self {
            backend,
            ir_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Book metadata.
    pub fn metadata(&self) -> &Metadata {
        self.backend.metadata()
    }

    /// Table of contents.
    pub fn toc(&self) -> &[TocEntry] {
        self.backend.toc()
    }

    /// Landmarks (structural navigation points).
    pub fn landmarks(&self) -> &[Landmark] {
        self.backend.landmarks()
    }

    /// Reading order (spine).
    pub fn spine(&self) -> &[SpineEntry] {
        self.backend.spine()
    }

    /// Get the internal source path for a chapter.
    pub fn source_id(&self, id: ChapterId) -> Option<&str> {
        self.backend.source_id(id)
    }

    /// Load raw chapter bytes.
    pub fn load_raw(&mut self, id: ChapterId) -> crate::Result<Vec<u8>> {
        self.backend.load_raw(id)
    }

    /// Load a chapter as normalized IR.
    ///
    /// This parses the chapter's HTML content and any linked or inline CSS,
    /// producing a normalized tree structure suitable for rendering.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::{Book, Role};
    ///
    /// let mut book = Book::open("input.epub")?;
    /// let spine: Vec<_> = book.spine().to_vec();
    ///
    /// for entry in spine {
    ///     let chapter = book.load_chapter(entry.id)?;
    ///     for id in chapter.iter_dfs() {
    ///         let node = chapter.node(id).unwrap();
    ///         if matches!(node.role, Role::Heading(_)) {
    ///             // Process heading...
    ///         }
    ///     }
    /// }
    /// # Ok::<(), boko::Error>(())
    /// ```
    pub fn load_chapter(&mut self, id: ChapterId) -> crate::Result<Chapter> {
        self.backend.load_chapter(id)
    }

    /// Load a chapter as IR with caching.
    ///
    /// This method caches parsed IR chapters to avoid re-parsing when the same
    /// chapter is loaded multiple times (e.g., during normalized export).
    /// Returns an `Arc<Chapter>` for cheap cloning and thread-safe sharing.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::Book;
    ///
    /// let mut book = Book::open("input.epub")?;
    /// let spine: Vec<_> = book.spine().to_vec();
    ///
    /// // First call parses the chapter
    /// let chapter1 = book.load_chapter_cached(spine[0].id)?;
    ///
    /// // Second call returns cached version (cheap Arc clone)
    /// let chapter2 = book.load_chapter_cached(spine[0].id)?;
    /// # Ok::<(), boko::Error>(())
    /// ```
    pub fn load_chapter_cached(&mut self, id: ChapterId) -> crate::Result<Arc<Chapter>> {
        // Fast path: check read lock first
        {
            let cache = self
                .ir_cache
                .read()
                .map_err(|_| io::Error::other("IR cache lock poisoned"))?;
            if let Some(chapter) = cache.get(&id) {
                return Ok(Arc::clone(chapter));
            }
        }

        // Slow path: load chapter (no lock held during IO)
        let chapter = self.backend.load_chapter(id)?;
        let chapter_arc = Arc::new(chapter);

        // Write to cache
        {
            let mut cache = self
                .ir_cache
                .write()
                .map_err(|_| io::Error::other("IR cache lock poisoned"))?;
            cache.insert(id, Arc::clone(&chapter_arc));
        }

        Ok(chapter_arc)
    }

    /// Load several chapters as IR with caching, in spine order.
    ///
    /// Like calling [`load_chapter_cached`](Self::load_chapter_cached) per
    /// id, but uncached chapters are handed to the backend as one batch so
    /// importers that support it (EPUB) compile them in parallel.
    pub fn load_chapters_cached(
        &mut self,
        ids: &[ChapterId],
    ) -> crate::Result<Vec<Arc<Chapter>>> {
        // Collect the ids that still need compiling.
        let missing: Vec<ChapterId> = {
            let cache = self
                .ir_cache
                .read()
                .map_err(|_| io::Error::other("IR cache lock poisoned"))?;
            ids.iter()
                .copied()
                .filter(|id| !cache.contains_key(id))
                .collect()
        };

        if !missing.is_empty() {
            let loaded = self.backend.load_chapters(&missing);
            let mut cache = self
                .ir_cache
                .write()
                .map_err(|_| io::Error::other("IR cache lock poisoned"))?;
            for (id, chapter) in missing.into_iter().zip(loaded) {
                cache.insert(id, Arc::new(chapter?));
            }
        }

        let cache = self
            .ir_cache
            .read()
            .map_err(|_| io::Error::other("IR cache lock poisoned"))?;
        ids.iter()
            .map(|id| {
                cache
                    .get(id)
                    .cloned()
                    .ok_or_else(|| crate::Error::NotFound {
                        what: format!("chapter {}", id.0),
                    })
            })
            .collect()
    }

    /// Clear the IR cache.
    ///
    /// Call this to free memory after normalized export is complete.
    pub fn clear_cache(&mut self) {
        if let Ok(mut cache) = self.ir_cache.write() {
            cache.clear();
        }
    }

    /// Resolve all internal links in the book.
    ///
    /// Uses `load_chapter_cached()` internally, so chapters are parsed once
    /// and reused for subsequent export operations. Call this before export
    /// to benefit from caching.
    ///
    /// Returns both forward mappings (source -> target) and reverse mappings
    /// (target -> sources) for efficient lookup during traversal.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::Book;
    ///
    /// let mut book = Book::open("input.epub")?;
    /// let resolved = book.resolve_links()?;
    ///
    /// // Check for broken links
    /// for (source, href) in resolved.broken_links() {
    ///     eprintln!("Broken link at {:?}: {}", source, href);
    /// }
    /// # Ok::<(), boko::Error>(())
    /// ```
    pub fn resolve_links(&mut self) -> crate::Result<ResolvedLinks> {
        resolve_book_links(self)
    }

    /// Index anchors for link resolution.
    ///
    /// Called internally by `resolve_links()`. Delegates to the format-specific
    /// importer to build anchor maps.
    pub(crate) fn index_anchors(&mut self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        self.backend.index_anchors(chapters);
    }

    /// Resolve TOC hrefs (fills in fragments for AZW3/MOBI).
    ///
    /// Called internally by `resolve_links()`. Delegates to the format-specific
    /// importer.
    pub(crate) fn resolve_toc(&mut self) {
        self.backend.resolve_toc();
    }

    /// Resolve TOC entry targets using `resolve_href()`.
    ///
    /// Called internally by `resolve_links()`. Walks TOC entries and sets their
    /// `target` field.
    pub(crate) fn resolve_toc_targets(&mut self) {
        // First, collect all hrefs with their targets
        fn collect_targets(
            entries: &[TocEntry],
            backend: &dyn Importer,
            default_chapter: ChapterId,
            results: &mut Vec<Option<AnchorTarget>>,
        ) {
            for entry in entries {
                results.push(backend.resolve_href(default_chapter, &entry.href));
                collect_targets(&entry.children, backend, default_chapter, results);
            }
        }

        let mut targets = Vec::new();
        collect_targets(
            self.backend.toc(),
            &*self.backend,
            ChapterId(0),
            &mut targets,
        );

        // Then apply the targets to the TOC entries
        fn apply_targets(
            entries: &mut [TocEntry],
            targets: &mut impl Iterator<Item = Option<AnchorTarget>>,
        ) {
            for entry in entries {
                entry.target = targets.next().flatten();
                apply_targets(&mut entry.children, targets);
            }
        }

        let toc = self.backend.toc_mut();
        apply_targets(toc, &mut targets.into_iter());
    }

    /// Resolve a single href using format-specific logic.
    ///
    /// Called internally by `resolve_links()`. Delegates to the format-specific
    /// importer.
    pub(crate) fn resolve_href(&self, from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        self.backend.resolve_href(from_chapter, href)
    }

    /// Load an asset by path.
    pub fn load_asset(&mut self, path: &Path) -> crate::Result<Vec<u8>> {
        self.backend.load_asset(path)
    }

    /// List all assets.
    pub fn list_assets(&self) -> &[std::path::PathBuf] {
        self.backend.list_assets()
    }

    /// Collect all @font-face definitions from CSS files.
    ///
    /// Returns font-face rules that map font family names to font files.
    /// Used by KFX export to create font entities linking font-family
    /// names to resource locations.
    pub fn font_faces(&mut self) -> Vec<crate::model::FontFace> {
        self.backend.font_faces()
    }

    /// Whether this book requires normalized export for HTML-based formats.
    ///
    /// Returns true for binary formats (KFX) where the raw content is not HTML.
    /// Exporters should use IR-based output when this returns true.
    pub fn requires_normalized_export(&self) -> bool {
        self.backend.requires_normalized_export()
    }

    /// Export the book to a different format.
    ///
    /// # Supported Export Formats
    ///
    /// | Format   | Support |
    /// |----------|---------|
    /// | EPUB     | ✓       |
    /// | AZW3     | ✓       |
    /// | MOBI     | ✗       |
    /// | Text     | ✓       |
    /// | Markdown | ✓       |
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::{Book, Format};
    /// use std::fs::File;
    ///
    /// let mut book = Book::open("input.azw3")?;
    /// let mut file = File::create("output.epub")?;
    /// book.export(Format::Epub, &mut file)?;
    /// # Ok::<(), boko::Error>(())
    /// ```
    pub fn export<W: Write + Seek>(&mut self, format: Format, writer: &mut W) -> crate::Result<()> {
        match format {
            Format::Epub => EpubExporter::new().export(self, writer),
            Format::Azw3 => Azw3Exporter::new().export(self, writer),
            Format::Markdown => MarkdownExporter::new().export(self, writer),
            Format::Kfx => KfxExporter::new().export(self, writer),
            Format::Mobi => Err(crate::Error::UnsupportedFormat {
                detail: format!("{:?} export is not supported", format),
            }),
        }
    }
}
