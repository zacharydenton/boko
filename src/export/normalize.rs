//! Normalized export pipeline.
//!
//! This module provides functionality for transforming ebooks through the IR layer
//! to produce clean, consistent output. It merges styles from all chapters into a
//! unified stylesheet and synthesizes normalized XHTML.
//!
//! # Two-Pass Export Flow
//!
//! 1. **Pass 1**: Load all chapters as IR, merge styles into GlobalStylePool
//! 2. **Pass 2**: Generate unified CSS, synthesize XHTML per chapter with remapped styles
//!
//! # Example
//!
//! ```no_run
//! use boko::Book;
//! use boko::export::normalize_book;
//!
//! let mut book = Book::open("input.epub")?;
//! let content = normalize_book(&mut book)?;
//!
//! // content.css contains the unified stylesheet
//! // content.chapters contains synthesized XHTML documents
//! // content.assets contains all referenced asset paths
//! # Ok::<(), boko::Error>(())
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::import::ChapterId;
use crate::model::{Book, Chapter, NodeId, Role};
use crate::style::{StyleId, StylePool};

use super::{generate_css, synthesize_xhtml_document_with_class_list};

/// Collects styles from all chapters into a unified pool.
///
/// When merging styles from multiple chapters, identical styles are deduplicated
/// and assigned the same global StyleId. Each chapter's local StyleIds are remapped
/// to global IDs for consistent class names across the book.
#[derive(Debug)]
pub struct GlobalStylePool {
    /// The unified style pool containing all unique styles.
    pool: StylePool,
    /// Maps (chapter_idx, local_StyleId) -> global_StyleId
    remaps: Vec<HashMap<StyleId, StyleId>>,
}

impl Default for GlobalStylePool {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalStylePool {
    /// Create a new empty global style pool.
    pub fn new() -> Self {
        Self {
            pool: StylePool::new(),
            remaps: Vec::new(),
        }
    }

    /// Merge styles from a chapter into the global pool.
    ///
    /// This method:
    /// 1. Iterates over all styles in the chapter's pool
    /// 2. Interns each style into the global pool (deduplicating identical styles)
    /// 3. Records the mapping from local to global StyleId
    ///
    /// # Arguments
    ///
    /// * `chapter_idx` - Index of the chapter (used for remap lookups)
    /// * `chapter` - The IR chapter containing styles to merge
    pub fn merge(&mut self, chapter_idx: usize, chapter: &Chapter) {
        // Ensure remaps vec is large enough
        while self.remaps.len() <= chapter_idx {
            self.remaps.push(HashMap::new());
        }

        let remap = &mut self.remaps[chapter_idx];

        // Merge each style from the chapter's pool
        for (local_id, style) in chapter.styles.iter() {
            let global_id = self.pool.intern_ref(style);
            remap.insert(local_id, global_id);
        }
    }

    /// Remap a local StyleId to its global equivalent.
    ///
    /// # Arguments
    ///
    /// * `chapter_idx` - Index of the chapter the style belongs to
    /// * `local_id` - The local StyleId from that chapter
    ///
    /// # Returns
    ///
    /// The global StyleId, or the default style if not found.
    pub fn remap(&self, chapter_idx: usize, local_id: StyleId) -> StyleId {
        self.remaps
            .get(chapter_idx)
            .and_then(|m| m.get(&local_id))
            .copied()
            .unwrap_or(StyleId::DEFAULT)
    }

    /// Get a reference to the unified style pool.
    pub fn pool(&self) -> &StylePool {
        &self.pool
    }

    /// Get all used style IDs across all chapters.
    pub fn used_styles(&self) -> Vec<StyleId> {
        let mut set = HashSet::new();
        for map in &self.remaps {
            set.extend(map.values().copied());
        }
        let mut styles: Vec<StyleId> = set.into_iter().collect();
        styles.sort_by_key(|s| s.0);
        styles
    }
}

/// Content for a single normalized chapter.
#[derive(Debug, Clone)]
pub struct ChapterContent {
    /// Chapter identifier.
    pub id: ChapterId,
    /// Original source path within the ebook.
    pub source_path: String,
    /// Complete synthesized XHTML document.
    pub document: String,
}

/// Result of normalizing all chapters in a book.
#[derive(Debug)]
pub struct NormalizedContent {
    /// The global style pool with merged styles.
    pub styles: GlobalStylePool,
    /// Normalized chapters with synthesized XHTML.
    pub chapters: Vec<ChapterContent>,
    /// All asset paths referenced across chapters.
    pub assets: HashSet<String>,
    /// The unified CSS stylesheet.
    pub css: String,
    /// Maps each chapter's original source path to its emitted `chapter_{i}.xhtml`.
    pub source_to_output: HashMap<String, String>,
    /// Maps an element/anchor id to the emitted file that defines it. Lets
    /// callers turn a bare `#anchor` (as KFX TOCs use) or a cross-chapter link
    /// into a `chapter_{i}.xhtml#anchor` reference that actually resolves.
    pub anchor_to_output: HashMap<String, String>,
}

impl NormalizedContent {
    /// Rewrite a TOC tree's hrefs to target the emitted chapter files.
    pub fn rewrite_toc(&self, toc: &[crate::model::TocEntry]) -> Vec<crate::model::TocEntry> {
        toc.iter().map(|e| self.rewrite_toc_entry(e)).collect()
    }

    fn rewrite_toc_entry(&self, entry: &crate::model::TocEntry) -> crate::model::TocEntry {
        let mut out = entry.clone();
        out.href = rewrite_href(
            &self.source_to_output,
            &self.anchor_to_output,
            None,
            &entry.href,
        );
        out.children = entry
            .children
            .iter()
            .map(|c| self.rewrite_toc_entry(c))
            .collect();
        out
    }
}

/// Resolve a link/TOC href that referenced an original source path or a bare
/// anchor fragment into one targeting the emitted `chapter_{i}.xhtml` files.
///
/// `base_source` is the source path of the document the href appears in (used to
/// resolve relative file references); pass `None` for book-global hrefs like TOC
/// entries.
fn rewrite_href(
    source_to_output: &HashMap<String, String>,
    anchor_to_output: &HashMap<String, String>,
    base_source: Option<&str>,
    href: &str,
) -> String {
    // Leave external and non-navigational links untouched.
    if href.is_empty() || href.contains("://") || href.starts_with("mailto:") {
        return href.to_string();
    }

    let (file, frag) = match href.split_once('#') {
        Some((f, fr)) => (f, Some(fr)),
        None => (href, None),
    };

    let output = if file.is_empty() {
        // Bare "#frag": find the chapter that defines the anchor; otherwise
        // assume it targets the current document.
        frag.and_then(|fr| anchor_to_output.get(fr).cloned())
            .or_else(|| base_source.and_then(|b| source_to_output.get(b).cloned()))
    } else {
        let resolved = match base_source {
            Some(b) => crate::dom::resolve_path(b, file),
            None => file.to_string(),
        };
        source_to_output
            .get(&resolved)
            .or_else(|| source_to_output.get(file))
            .cloned()
    };

    match (output, frag) {
        (Some(o), Some(fr)) => format!("{o}#{fr}"),
        (Some(o), None) => o,
        // Unknown target: keep a same-document fragment as-is; leave other
        // unresolved hrefs unchanged rather than inventing a target.
        (None, Some(fr)) if file.is_empty() => format!("#{fr}"),
        _ => href.to_string(),
    }
}

/// Rewrite the `href="…"` attributes inside a synthesized document so internal
/// links point at the emitted chapter files.
fn rewrite_document_hrefs(
    doc: &str,
    base_source: &str,
    source_to_output: &HashMap<String, String>,
    anchor_to_output: &HashMap<String, String>,
) -> String {
    const NEEDLE: &str = " href=\"";
    if !doc.contains(NEEDLE) {
        return doc.to_string();
    }
    let mut out = String::with_capacity(doc.len());
    let mut rest = doc;
    while let Some(pos) = rest.find(NEEDLE) {
        let (before, after) = rest.split_at(pos + NEEDLE.len());
        out.push_str(before);
        if let Some(end) = after.find('"') {
            let rewritten = rewrite_href(
                source_to_output,
                anchor_to_output,
                Some(base_source),
                &after[..end],
            );
            out.push_str(&rewritten);
            rest = &after[end..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Normalize all chapters in a book through the IR pipeline.
///
/// This is the main entry point for normalized export. It:
/// 1. Loads each chapter as IR
/// 2. Merges all styles into a global pool
/// 3. Generates a unified CSS stylesheet
/// 4. Synthesizes XHTML for each chapter with remapped styles
/// 5. Collects all asset references
///
/// # Arguments
///
/// * `book` - Mutable reference to the book to normalize
///
/// # Returns
///
/// A `NormalizedContent` containing all normalized data ready for export.
pub fn normalize_book(book: &Book) -> crate::Result<NormalizedContent> {
    let spine = book.spine();

    // =========================================================================
    // Pass 1: Load all chapters and merge styles
    // =========================================================================

    let mut global_styles = GlobalStylePool::new();
    let mut ir_chapters: Vec<(ChapterId, String, Arc<Chapter>)> = Vec::with_capacity(spine.len());
    // Link-rewrite maps: original source path / anchor id -> emitted filename.
    let mut source_to_output: HashMap<String, String> = HashMap::new();
    let mut anchor_to_output: HashMap<String, String> = HashMap::new();

    // Compile every spine chapter up front as one batch — importers with
    // thread-safe IO (EPUB) parallelize the HTML parse + cascade + IR
    // transform across chapters, which dominates cold conversion.
    let spine_ids: Vec<ChapterId> = spine.iter().map(|e| e.id).collect();
    let loaded = book.load_chapters_cached(&spine_ids)?;

    for ((idx, entry), chapter) in spine.iter().enumerate().zip(loaded) {
        let source_path = book
            .source_id(entry.id)
            .unwrap_or("unknown.xhtml")
            .to_string();

        // Merge styles into global pool
        global_styles.merge(idx, &chapter);

        // Record where this chapter and its anchors will live in the output, so
        // TOC entries and internal links can be remapped from the original
        // source paths / bare `#anchor`s to the emitted `chapter_{i}.xhtml`.
        let output_name = format!("chapter_{idx}.xhtml");
        source_to_output.insert(source_path.clone(), output_name.clone());
        for node_id in chapter.iter_dfs() {
            if let Some(id) = chapter.semantics.id(node_id) {
                anchor_to_output
                    .entry(id.to_string())
                    .or_insert_with(|| output_name.clone());
            }
        }

        ir_chapters.push((entry.id, source_path, chapter));
    }

    // =========================================================================
    // Generate unified CSS
    // =========================================================================

    let used_styles = global_styles.used_styles();
    let css_artifact = generate_css(global_styles.pool(), &used_styles);

    // =========================================================================
    // Pass 2: Synthesize XHTML with remapped styles
    // =========================================================================

    // Each chapter's synthesis reads only shared immutable state, so this
    // fans out across chapters when the `parallel` feature is on.
    let synthesize_one = |(idx, (chapter_id, source_path, ir)): (
        usize,
        &(ChapterId, String, Arc<Chapter>),
    )|
     -> (ChapterContent, HashSet<String>) {
        // Build remapped style map for this chapter
        let mut remapped_class_list: Vec<Option<&str>> = vec![None; ir.styles.len()];
        for (local_id, _) in ir.styles.iter() {
            let global_id = global_styles.remap(idx, local_id);
            if let Some(class_name) = css_artifact.class_name_fast(global_id) {
                let slot = remapped_class_list
                    .get_mut(local_id.0 as usize)
                    .expect("style id out of bounds");
                *slot = Some(class_name);
            }
        }

        // Extract title from first heading or use source path
        let title = extract_chapter_title(ir).unwrap_or_else(|| source_path.clone());

        // Synthesize XHTML document
        let result = synthesize_xhtml_document_with_class_list(
            ir,
            &remapped_class_list,
            &title,
            Some("style.css"),
        );

        // Rewrite internal links to target the emitted chapter files.
        let document = rewrite_document_hrefs(
            &result.body,
            source_path,
            &source_to_output,
            &anchor_to_output,
        );

        (
            ChapterContent {
                id: *chapter_id,
                source_path: source_path.clone(),
                document,
            },
            result.assets,
        )
    };

    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    let synthesized: Vec<(ChapterContent, HashSet<String>)> = {
        use rayon::prelude::*;
        ir_chapters
            .par_iter()
            .enumerate()
            .map(synthesize_one)
            .collect()
    };
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    let synthesized: Vec<(ChapterContent, HashSet<String>)> =
        ir_chapters.iter().enumerate().map(synthesize_one).collect();

    let mut chapters = Vec::with_capacity(synthesized.len());
    let mut all_assets = HashSet::new();
    for (content, assets) in synthesized {
        all_assets.extend(assets);
        chapters.push(content);
    }

    Ok(NormalizedContent {
        styles: global_styles,
        chapters,
        assets: all_assets,
        css: css_artifact.stylesheet,
        source_to_output,
        anchor_to_output,
    })
}

/// Extract a title from the first heading in a chapter.
fn extract_chapter_title(ir: &Chapter) -> Option<String> {
    for node_id in ir.iter_dfs() {
        if let Some(node) = ir.node(node_id)
            && matches!(node.role, Role::Heading(_))
        {
            // Collect text from heading's children
            let mut title = String::new();
            collect_text_recursive(ir, node_id, &mut title);
            if !title.is_empty() {
                return Some(title.trim().to_string());
            }
        }
    }
    None
}

/// Recursively collect text content from a node and its descendants.
fn collect_text_recursive(ir: &Chapter, node_id: NodeId, buf: &mut String) {
    if let Some(node) = ir.node(node_id)
        && node.role == Role::Text
    {
        buf.push_str(ir.text(node.text));
    }

    for child_id in ir.children(node_id) {
        collect_text_recursive(ir, child_id, buf);
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::model::Node;
    use crate::style::{ComputedStyle, FontWeight};

    #[test]
    fn test_global_style_pool_new() {
        let pool = GlobalStylePool::new();
        assert_eq!(pool.pool().len(), 1); // Default style
        assert!(pool.remaps.is_empty());
    }

    #[test]
    fn test_global_style_pool_merge() {
        let mut global = GlobalStylePool::new();

        // Create first chapter with a bold style
        let mut chapter1 = Chapter::new();
        let mut bold = ComputedStyle::default();
        bold.font_weight = FontWeight::BOLD;
        let bold_id = chapter1.styles.intern(bold.clone());

        // Create second chapter with the same bold style
        let mut chapter2 = Chapter::new();
        let bold_id2 = chapter2.styles.intern(bold);

        // Merge both chapters
        global.merge(0, &chapter1);
        global.merge(1, &chapter2);

        // Both should map to the same global ID
        let global_id1 = global.remap(0, bold_id);
        let global_id2 = global.remap(1, bold_id2);
        assert_eq!(global_id1, global_id2);

        // Global pool should have 2 styles (default + bold)
        assert_eq!(global.pool().len(), 2);
    }

    #[test]
    fn test_global_style_pool_remap_unknown() {
        let global = GlobalStylePool::new();

        // Unknown chapter/style should return default
        let result = global.remap(999, StyleId(999));
        assert_eq!(result, StyleId::DEFAULT);
    }

    #[test]
    fn test_global_style_pool_used_styles() {
        let mut global = GlobalStylePool::new();

        let mut chapter = Chapter::new();
        let mut bold = ComputedStyle::default();
        bold.font_weight = FontWeight::BOLD;
        chapter.styles.intern(bold);

        global.merge(0, &chapter);

        let used = global.used_styles();
        assert!(!used.is_empty());
    }

    #[test]
    fn test_extract_chapter_title() {
        let mut chapter = Chapter::new();

        // Add a heading with text
        let h1 = chapter.alloc_node(Node::new(Role::Heading(1)));
        chapter.append_child(NodeId::ROOT, h1);

        let text_range = chapter.append_text("Chapter One");
        let mut text_node = Node::new(Role::Text);
        text_node.text = text_range;
        let text_id = chapter.alloc_node(text_node);
        chapter.append_child(h1, text_id);

        let title = extract_chapter_title(&chapter);
        assert_eq!(title, Some("Chapter One".to_string()));
    }

    #[test]
    fn test_extract_chapter_title_no_heading() {
        let chapter = Chapter::new();
        let title = extract_chapter_title(&chapter);
        assert_eq!(title, None);
    }

    #[test]
    fn rewrite_href_maps_anchors_and_paths() {
        let source_to_output =
            HashMap::from([("text/ch2.xhtml".to_string(), "chapter_1.xhtml".to_string())]);
        let anchor_to_output = HashMap::from([("sec3".to_string(), "chapter_1.xhtml".to_string())]);
        let rw = |href: &str| rewrite_href(&source_to_output, &anchor_to_output, None, href);

        // Bare "#anchor" (as KFX TOCs use) -> the chapter that defines it.
        assert_eq!(rw("#sec3"), "chapter_1.xhtml#sec3");
        // Source path, with and without a fragment -> emitted file.
        assert_eq!(rw("text/ch2.xhtml#x"), "chapter_1.xhtml#x");
        assert_eq!(rw("text/ch2.xhtml"), "chapter_1.xhtml");
        // Unknown anchor stays a same-document fragment; externals untouched.
        assert_eq!(rw("#missing"), "#missing");
        assert_eq!(rw("https://example.com/a"), "https://example.com/a");
    }
}
