//! Book-level output optimization.
//!
//! [`Book::optimize`](crate::Book::optimize) shrinks a book before export by
//! running a sequence of optimization passes, so the saving applies to every
//! output format. Each pass examines the book through its [`Importer`] view
//! and proposes `AssetEdit`s; a generic overlay importer applies them —
//! serving replaced bytes under (possibly renamed) asset paths and rewriting
//! references (chapter `src` attributes, the cover path) on the way out.
//! Exporters need no knowledge of any of it, and passes compose by wrapping
//! the previous pass's view.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::dom::Stylesheet;
use crate::import::{ChapterId, Importer, SpineEntry};
use crate::model::{AnchorTarget, Chapter, FontFace, Landmark, Metadata, TocEntry};

/// What one optimization pass changed.
#[derive(Debug, Clone)]
pub struct PassReport {
    /// Pass name (e.g. `"images"`).
    pub pass: &'static str,
    /// Assets the pass replaced.
    pub assets_changed: usize,
    /// Total bytes saved by the pass.
    pub bytes_saved: u64,
}

/// What [`crate::Book::optimize`] changed, per pass.
#[derive(Debug, Clone, Default)]
pub struct OptimizeReport {
    /// One entry per pass that ran.
    pub passes: Vec<PassReport>,
}

impl OptimizeReport {
    /// Assets replaced across all passes.
    pub fn assets_changed(&self) -> usize {
        self.passes.iter().map(|p| p.assets_changed).sum()
    }

    /// Bytes saved across all passes.
    pub fn bytes_saved(&self) -> u64 {
        self.passes.iter().map(|p| p.bytes_saved).sum()
    }
}

/// A planned change to one asset, proposed by a pass.
pub(crate) struct AssetEdit {
    /// Path of the asset in the pass's input view.
    pub path: String,
    /// New path, when the replacement changes the asset's format. `None`
    /// keeps the original path. Renames rewrite chapter `src` references and
    /// the cover path; the old path also stays loadable (serving the new
    /// bytes) so stale references degrade to a working asset, not a broken
    /// one.
    pub new_path: Option<String>,
    /// Replacement bytes.
    pub data: Vec<u8>,
}

/// One optimization pass: examines the book through an [`Importer`] view and
/// proposes asset edits. Passes must only propose edits that shrink the book;
/// an edit whose data is not smaller than the original is discarded.
pub(crate) trait OptimizePass {
    fn name(&self) -> &'static str;
    fn run(&self, backend: &dyn Importer) -> Vec<AssetEdit>;
}

/// The default pass list for [`crate::Book::optimize`].
// push-after-new keeps each pass independently cfg-gatable; vec![] can't
// hold per-element cfg attributes.
#[allow(clippy::vec_init_then_push)]
fn default_passes() -> Vec<Box<dyn OptimizePass>> {
    #[allow(unused_mut)]
    let mut passes: Vec<Box<dyn OptimizePass>> = Vec::new();
    #[cfg(feature = "optimize-images")]
    passes.push(Box::new(passes::Images {
        quality: 80,
        min_size: 10 * 1024,
        // 11th-gen Kindle Paperwhite panel: 1236x1648. Reflowable images
        // render at content width (1236) in portrait reading, so that's the
        // long-edge cap; larger images are downscaled before re-encoding.
        // Measured across a library sample, q80 at this cap roughly halves
        // image-heavy books while staying pixel-exact at reading size.
        max_dimension: 1236,
    }));
    passes
}

/// Basenames referenced from CSS assets. Passes that rename must skip these:
/// CSS is exported verbatim by some formats, and a rename would leave its
/// `url(...)` references dangling.
pub(crate) fn css_referenced_text(backend: &dyn Importer) -> String {
    let mut css_text = String::new();
    for path in backend.list_assets() {
        if path
            .rsplit('.')
            .next()
            .is_some_and(|e| e.eq_ignore_ascii_case("css"))
            && let Ok(data) = backend.load_asset(path)
        {
            css_text.push_str(&String::from_utf8_lossy(&data));
        }
    }
    css_text
}

/// Importer wrapper that overlays one pass's asset edits onto an inner
/// backend. Multiple passes stack by wrapping the previous wrapper.
pub(crate) struct OptimizedImporter {
    inner: Box<dyn Importer>,
    /// Asset list with renames applied, same order as the inner backend's.
    assets: Vec<String>,
    /// Serving path → replaced bytes.
    overrides: HashMap<String, Vec<u8>>,
    /// Old path → new path, for edits that renamed.
    renames: HashMap<String, String>,
    /// Inner metadata with the cover path rewritten.
    metadata: Metadata,
}

impl OptimizedImporter {
    /// Run one pass against the inner backend and build the overlay from the
    /// edits it proposes. Edits that don't shrink the asset, or that rename
    /// onto an existing path, are discarded.
    pub(crate) fn apply(inner: Box<dyn Importer>, pass: &dyn OptimizePass) -> (Self, PassReport) {
        let mut report = PassReport {
            pass: pass.name(),
            assets_changed: 0,
            bytes_saved: 0,
        };

        let asset_paths: Vec<String> = inner.list_assets().to_vec();
        let mut overrides = HashMap::new();
        let mut renames = HashMap::new();

        for edit in pass.run(inner.as_ref()) {
            if !asset_paths.contains(&edit.path) {
                continue;
            }
            let serving_path = edit.new_path.as_ref().unwrap_or(&edit.path);
            if *serving_path != edit.path
                && (asset_paths.iter().any(|p| p == serving_path)
                    || overrides.contains_key(serving_path))
            {
                continue;
            }
            let Ok(original) = inner.load_asset(&edit.path) else {
                continue;
            };
            if edit.data.len() >= original.len() {
                continue;
            }
            report.assets_changed += 1;
            report.bytes_saved += (original.len() - edit.data.len()) as u64;
            if *serving_path != edit.path {
                renames.insert(edit.path.clone(), serving_path.clone());
            }
            overrides.insert(serving_path.clone(), edit.data);
        }

        let assets = asset_paths
            .iter()
            .map(|p| renames.get(p).unwrap_or(p).clone())
            .collect();

        let mut metadata = inner.metadata().clone();
        if let Some(cover) = &metadata.cover_image
            && let Some(new_path) = renames.get(cover)
        {
            metadata.cover_image = Some(new_path.clone());
        }

        (
            Self {
                inner,
                assets,
                overrides,
                renames,
                metadata,
            },
            report,
        )
    }

    /// Rewrite image `src` references through the rename map.
    fn rewrite_chapter(&self, mut chapter: Chapter) -> Chapter {
        if self.renames.is_empty() {
            return chapter;
        }
        let updates: Vec<(crate::model::NodeId, String)> = chapter
            .iter_dfs()
            .filter_map(|node| {
                let src = chapter.semantics.src(node)?;
                Some((node, self.renames.get(src)?.clone()))
            })
            .collect();
        for (node, new_src) in updates {
            chapter.semantics.set_src(node, &new_src);
        }
        chapter
    }
}

impl Importer for OptimizedImporter {
    fn open(_path: &Path) -> crate::Result<Self>
    where
        Self: Sized,
    {
        Err(crate::Error::UnsupportedFormat {
            detail: "OptimizedImporter wraps an existing backend".to_string(),
        })
    }

    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn toc(&self) -> &[TocEntry] {
        self.inner.toc()
    }

    fn landmarks(&self) -> &[Landmark] {
        self.inner.landmarks()
    }

    fn spine(&self) -> &[SpineEntry] {
        self.inner.spine()
    }

    fn load_chapter(&self, id: ChapterId) -> crate::Result<Chapter> {
        self.inner
            .load_chapter(id)
            .map(|ch| self.rewrite_chapter(ch))
    }

    fn load_chapters(&self, ids: &[ChapterId]) -> Vec<crate::Result<Chapter>> {
        self.inner
            .load_chapters(ids)
            .into_iter()
            .map(|res| res.map(|ch| self.rewrite_chapter(ch)))
            .collect()
    }

    fn source_id(&self, id: ChapterId) -> Option<&str> {
        self.inner.source_id(id)
    }

    fn load_raw(&self, id: ChapterId) -> crate::Result<Vec<u8>> {
        self.inner.load_raw(id)
    }

    fn list_assets(&self) -> &[String] {
        &self.assets
    }

    fn load_asset(&self, path: &str) -> crate::Result<Vec<u8>> {
        if let Some(data) = self.overrides.get(path) {
            return Ok(data.clone());
        }
        if let Some(new_path) = self.renames.get(path) {
            return Ok(self.overrides[new_path].clone());
        }
        self.inner.load_asset(path)
    }

    fn load_stylesheet(&self, path: &str) -> Option<Arc<Stylesheet>> {
        self.inner.load_stylesheet(path)
    }

    fn font_faces(&self) -> Vec<FontFace> {
        self.inner.font_faces()
    }

    fn requires_normalized_export(&self) -> bool {
        // Renames rewrite `src` references at the IR level; raw-passthrough
        // export (EPUB→EPUB) would ship the original markup with dangling
        // references. Same-path replacements are fine either way.
        !self.renames.is_empty() || self.inner.requires_normalized_export()
    }

    fn index_anchors(&self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        self.inner.index_anchors(chapters)
    }

    fn resolve_toc(&self) -> Option<Vec<TocEntry>> {
        self.inner.resolve_toc()
    }

    fn resolve_href(&self, from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        self.inner.resolve_href(from_chapter, href)
    }
}

/// Placeholder backend used only while swapping in the optimized wrapper.
struct EmptyBackend(Metadata);

impl Importer for EmptyBackend {
    fn open(_path: &Path) -> crate::Result<Self>
    where
        Self: Sized,
    {
        unreachable!("EmptyBackend is never opened from a path")
    }

    fn metadata(&self) -> &Metadata {
        &self.0
    }

    fn toc(&self) -> &[TocEntry] {
        &[]
    }

    fn landmarks(&self) -> &[Landmark] {
        &[]
    }

    fn spine(&self) -> &[SpineEntry] {
        &[]
    }

    fn source_id(&self, _id: ChapterId) -> Option<&str> {
        None
    }

    fn load_raw(&self, _id: ChapterId) -> crate::Result<Vec<u8>> {
        Err(crate::Error::UnsupportedFormat {
            detail: "placeholder backend".to_string(),
        })
    }

    fn list_assets(&self) -> &[String] {
        &[]
    }

    fn load_asset(&self, _path: &str) -> crate::Result<Vec<u8>> {
        Err(crate::Error::UnsupportedFormat {
            detail: "placeholder backend".to_string(),
        })
    }
}

impl crate::Book {
    /// Shrink the book for output by running the default optimization
    /// passes. Every pass only replaces an asset when the result is smaller;
    /// renamed references (chapter `src` attributes, the cover path) are
    /// rewritten transparently, so the saving applies to every output
    /// format.
    ///
    /// Current passes: `images` (downscale raster images to the 11th-gen
    /// Kindle Paperwhite content width of 1236px and re-encode as JPEG
    /// quality 80, keeping the original whenever the result isn't
    /// meaningfully smaller; requires the `optimize-images` feature).
    pub fn optimize(&mut self) -> OptimizeReport {
        let mut report = OptimizeReport::default();
        let mut backend = self.replace_backend(Box::new(EmptyBackend(Metadata::default())));
        for pass in default_passes() {
            let (wrapped, pass_report) = OptimizedImporter::apply(backend, pass.as_ref());
            backend = Box::new(wrapped);
            report.passes.push(pass_report);
        }
        self.replace_backend(backend);
        report
    }
}

/// The optimization passes themselves.
mod passes {
    /// Shrink oversized raster images by re-encoding them as JPEG.
    ///
    /// Every PNG or JPEG at least `min_size` bytes is re-encoded at
    /// `quality`, downscaling first when the long edge exceeds
    /// `max_dimension` (device panels can't show the extra pixels anyway);
    /// the result is kept only when meaningfully smaller, so line art and
    /// flat-color PNGs (which JPEG regularly loses to) and already
    /// well-compressed JPEGs pass through untouched. Small images aren't
    /// worth the quality loss and are skipped outright.
    #[cfg(feature = "optimize-images")]
    pub(super) struct Images {
        pub quality: u8,
        pub min_size: usize,
        /// Long-edge cap in pixels; larger images are downscaled to fit.
        pub max_dimension: u32,
    }

    /// A re-encode must save at least this fraction of the original size to
    /// justify the generational quality loss of recompression.
    #[cfg(feature = "optimize-images")]
    const MIN_SAVING: f64 = 0.10;

    #[cfg(feature = "optimize-images")]
    impl super::OptimizePass for Images {
        fn name(&self) -> &'static str {
            "images"
        }

        fn run(&self, backend: &dyn super::Importer) -> Vec<super::AssetEdit> {
            use crate::util::{MediaFormat, detect_media_format, reencode_image_as_jpeg};

            let css_text = super::css_referenced_text(backend);
            let mut edits = Vec::new();
            for path in backend.list_assets() {
                let Ok(data) = backend.load_asset(path) else {
                    continue;
                };
                if data.len() < self.min_size {
                    continue;
                }
                let format = detect_media_format(path, &data);
                if !matches!(format, MediaFormat::Png | MediaFormat::Jpeg) {
                    continue;
                }
                let Some(jpeg) =
                    reencode_image_as_jpeg(&data, self.quality, Some(self.max_dimension))
                else {
                    continue;
                };
                if (jpeg.len() as f64) > (data.len() as f64) * (1.0 - MIN_SAVING) {
                    continue;
                }
                // Only a format change needs a rename (and reference
                // rewriting); a JPEG shrunk in place keeps its path.
                let new_path = if format == MediaFormat::Png {
                    let basename = path.rsplit('/').next().unwrap_or(path);
                    if !css_text.is_empty() && css_text.contains(basename) {
                        // CSS references this image by name; renaming would
                        // leave those url(...) references dangling.
                        continue;
                    }
                    Some(
                        match path
                            .strip_suffix(".png")
                            .or_else(|| path.strip_suffix(".PNG"))
                        {
                            Some(stem) => format!("{stem}.jpg"),
                            None => format!("{path}.jpg"),
                        },
                    )
                } else {
                    None
                };
                edits.push(super::AssetEdit {
                    path: path.clone(),
                    new_path,
                    data: jpeg,
                });
            }
            edits
        }
    }
}
