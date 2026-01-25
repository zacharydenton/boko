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
//! # Ok::<(), std::io::Error>(())
//! ```

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;

use crate::book::Book;
use crate::import::ChapterId;
use crate::ir::{IRChapter, NodeId, Role, StyleId, StylePool};

use super::{generate_css, synthesize_xhtml_document};

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
    pub fn merge(&mut self, chapter_idx: usize, chapter: &IRChapter) {
        // Ensure remaps vec is large enough
        while self.remaps.len() <= chapter_idx {
            self.remaps.push(HashMap::new());
        }

        let remap = &mut self.remaps[chapter_idx];

        // Merge each style from the chapter's pool
        for (local_id, style) in chapter.styles.iter() {
            let global_id = self.pool.intern(style.clone());
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
        self.remaps
            .iter()
            .flat_map(|m| m.values())
            .copied()
            .collect()
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
pub fn normalize_book(book: &mut Book) -> io::Result<NormalizedContent> {
    let spine: Vec<_> = book.spine().to_vec();

    // =========================================================================
    // Pass 1: Load all chapters and merge styles
    // =========================================================================

    let mut global_styles = GlobalStylePool::new();
    let mut ir_chapters: Vec<(ChapterId, String, Arc<IRChapter>)> = Vec::with_capacity(spine.len());

    for (idx, entry) in spine.iter().enumerate() {
        let source_path = book
            .source_id(entry.id)
            .unwrap_or("unknown.xhtml")
            .to_string();

        // Load chapter as IR (using cache for efficiency)
        let chapter = book.load_chapter_cached(entry.id)?;

        // Merge styles into global pool
        global_styles.merge(idx, &chapter);

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

    let mut chapters = Vec::with_capacity(ir_chapters.len());
    let mut all_assets = HashSet::new();

    for (idx, (chapter_id, source_path, ir)) in ir_chapters.iter().enumerate() {
        // Build remapped style map for this chapter
        let mut remapped_class_map: HashMap<StyleId, String> = HashMap::new();
        for (local_id, _) in ir.styles.iter() {
            let global_id = global_styles.remap(idx, local_id);
            if let Some(class_name) = css_artifact.class_map.get(&global_id) {
                remapped_class_map.insert(local_id, class_name.clone());
            }
        }

        // Extract title from first heading or use source path
        let title = extract_chapter_title(ir).unwrap_or_else(|| source_path.clone());

        // Synthesize XHTML document
        let result = synthesize_xhtml_document(ir, &remapped_class_map, &title, Some("style.css"));

        // Collect assets
        all_assets.extend(result.assets);

        chapters.push(ChapterContent {
            id: *chapter_id,
            source_path: source_path.clone(),
            document: result.body,
        });
    }

    Ok(NormalizedContent {
        styles: global_styles,
        chapters,
        assets: all_assets,
        css: css_artifact.stylesheet,
    })
}

/// Extract a title from the first heading in a chapter.
fn extract_chapter_title(ir: &IRChapter) -> Option<String> {
    for node_id in ir.iter_dfs() {
        if let Some(node) = ir.node(node_id) {
            if matches!(node.role, Role::Heading(_)) {
                // Collect text from heading's children
                let mut title = String::new();
                collect_text_recursive(ir, node_id, &mut title);
                if !title.is_empty() {
                    return Some(title.trim().to_string());
                }
            }
        }
    }
    None
}

/// Recursively collect text content from a node and its descendants.
fn collect_text_recursive(ir: &IRChapter, node_id: NodeId, buf: &mut String) {
    if let Some(node) = ir.node(node_id) {
        if node.role == Role::Text {
            buf.push_str(ir.text(node.text));
        }
    }

    for child_id in ir.children(node_id) {
        collect_text_recursive(ir, child_id, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{FontWeight, Node};

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
        let mut chapter1 = IRChapter::new();
        let mut bold = ComputedStyle::default();
        bold.font_weight = FontWeight::BOLD;
        let bold_id = chapter1.styles.intern(bold.clone());

        // Create second chapter with the same bold style
        let mut chapter2 = IRChapter::new();
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

        let mut chapter = IRChapter::new();
        let mut bold = ComputedStyle::default();
        bold.font_weight = FontWeight::BOLD;
        chapter.styles.intern(bold);

        global.merge(0, &chapter);

        let used = global.used_styles();
        assert!(!used.is_empty());
    }

    #[test]
    fn test_extract_chapter_title() {
        let mut chapter = IRChapter::new();

        // Add a heading with text
        let h1 = chapter.alloc_node(Node::new(Role::Heading(1)));
        chapter.append_child(NodeId::ROOT, h1);

        let text_range = chapter.append_text("Chapter One");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(h1, text_node);

        let title = extract_chapter_title(&chapter);
        assert_eq!(title, Some("Chapter One".to_string()));
    }

    #[test]
    fn test_extract_chapter_title_no_heading() {
        let chapter = IRChapter::new();
        let title = extract_chapter_title(&chapter);
        assert_eq!(title, None);
    }
}
