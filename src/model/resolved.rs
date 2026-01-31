//! Book-level link resolution.
//!
//! This module provides [`ResolvedLinks`], which resolves all internal `href` attributes
//! in a book to their targets and builds reverse mappings for efficient lookup.

use std::collections::HashMap;
use std::sync::Arc;

use crate::import::ChapterId;
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, Role};

/// Book-level link resolution result with forward and reverse mappings.
///
/// This struct is produced by [`Book::resolve_links()`] and provides:
/// - Forward lookup: given a link node, find its target
/// - Reverse lookup: given a target node, find all links pointing to it
/// - Broken link detection: links that couldn't be resolved
///
/// # Example
///
/// ```ignore
/// let mut book = Book::open("input.epub")?;
/// let resolved = book.resolve_links()?;
///
/// // Forward lookup
/// let link_node = GlobalNodeId::new(ChapterId(0), NodeId(5));
/// if let Some(target) = resolved.get(link_node) {
///     println!("Link points to {:?}", target);
/// }
///
/// // Reverse lookup
/// let target_node = GlobalNodeId::new(ChapterId(1), NodeId(23));
/// if resolved.is_internal_target(target_node) {
///     println!("Node is targeted by {} links", resolved.links_to(target_node).len());
/// }
/// ```
#[derive(Debug, Default)]
pub struct ResolvedLinks {
    /// Source link node → resolved target
    links: HashMap<GlobalNodeId, AnchorTarget>,

    /// Reverse: target node → source link nodes
    /// Enables O(1) "is this node a link target?" during traversal
    internal_targets: HashMap<GlobalNodeId, Vec<GlobalNodeId>>,

    /// Reverse: chapter start → source link nodes
    chapter_targets: HashMap<ChapterId, Vec<GlobalNodeId>>,

    /// Broken links: (source node, unresolved href)
    broken: Vec<(GlobalNodeId, String)>,
}

impl ResolvedLinks {
    /// Create a new empty ResolvedLinks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the resolved target for a link node.
    pub fn get(&self, source: GlobalNodeId) -> Option<&AnchorTarget> {
        self.links.get(&source)
    }

    /// Check if a node is an internal link target.
    pub fn is_internal_target(&self, node: GlobalNodeId) -> bool {
        self.internal_targets.contains_key(&node)
    }

    /// Check if a chapter start is a link target.
    pub fn is_chapter_target(&self, chapter: ChapterId) -> bool {
        self.chapter_targets.contains_key(&chapter)
    }

    /// Get all links pointing to a specific node.
    ///
    /// Returns an empty slice if no links point to this node.
    pub fn links_to(&self, target: GlobalNodeId) -> &[GlobalNodeId] {
        self.internal_targets
            .get(&target)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all links pointing to a chapter start.
    ///
    /// Returns an empty slice if no links point to this chapter.
    pub fn links_to_chapter(&self, chapter: ChapterId) -> &[GlobalNodeId] {
        self.chapter_targets
            .get(&chapter)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all broken links.
    ///
    /// Returns tuples of (source node, unresolved href).
    pub fn broken_links(&self) -> &[(GlobalNodeId, String)] {
        &self.broken
    }

    /// Iterate all resolved links.
    pub fn iter(&self) -> impl Iterator<Item = (GlobalNodeId, &AnchorTarget)> {
        self.links.iter().map(|(&k, v)| (k, v))
    }

    /// Get the total number of resolved links.
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// Check if there are no resolved links.
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

/// Builder for constructing ResolvedLinks during resolution.
pub(crate) struct ResolvedLinksBuilder {
    resolved: ResolvedLinks,
}

impl ResolvedLinksBuilder {
    pub fn new() -> Self {
        Self {
            resolved: ResolvedLinks::new(),
        }
    }

    /// Add a resolved internal link (to a specific node).
    pub fn add_internal(&mut self, source: GlobalNodeId, target: GlobalNodeId) {
        self.resolved
            .links
            .insert(source, AnchorTarget::Internal(target));
        self.resolved
            .internal_targets
            .entry(target)
            .or_default()
            .push(source);
    }

    /// Add a resolved chapter link (to chapter start).
    pub fn add_chapter(&mut self, source: GlobalNodeId, chapter: ChapterId) {
        self.resolved
            .links
            .insert(source, AnchorTarget::Chapter(chapter));
        self.resolved
            .chapter_targets
            .entry(chapter)
            .or_default()
            .push(source);
    }

    /// Add an external link.
    pub fn add_external(&mut self, source: GlobalNodeId, url: String) {
        self.resolved
            .links
            .insert(source, AnchorTarget::External(url));
    }

    /// Add a broken link.
    pub fn add_broken(&mut self, source: GlobalNodeId, href: String) {
        self.resolved.broken.push((source, href));
    }

    /// Finish building and return the ResolvedLinks.
    pub fn build(self) -> ResolvedLinks {
        self.resolved
    }
}

/// Resolve all links in a book.
///
/// This is the main resolution algorithm that:
/// 1. Loads all chapters
/// 2. Calls importer's index_anchors() to build format-specific anchor maps
/// 3. Walks all chapters, finds Link nodes, resolves via importer
/// 4. Builds reverse maps for efficient lookup
pub(crate) fn resolve_book_links(
    book: &mut crate::model::Book,
) -> std::io::Result<ResolvedLinks> {
    let mut builder = ResolvedLinksBuilder::new();

    // Step 1: Load all chapters
    let spine: Vec<_> = book.spine().to_vec();
    let mut chapters: Vec<(ChapterId, Arc<Chapter>)> = Vec::new();

    for entry in &spine {
        let chapter = book.load_chapter_cached(entry.id)?;
        chapters.push((entry.id, chapter));
    }

    // Step 2: Let the importer build format-specific anchor maps
    book.index_anchors(&chapters);

    // Step 3: Walk all chapters, find Link nodes, resolve via importer
    for (chapter_id, chapter) in &chapters {
        for node_id in chapter.iter_dfs() {
            let node = match chapter.node(node_id) {
                Some(n) => n,
                None => continue,
            };

            // Only process Link nodes
            if node.role != Role::Link {
                continue;
            }

            // Get the href attribute
            let href = match chapter.semantics.href(node_id) {
                Some(h) => h,
                None => continue,
            };

            let source = GlobalNodeId::new(*chapter_id, node_id);

            // Resolve via importer's format-specific logic
            match book.resolve_href(*chapter_id, href) {
                Some(AnchorTarget::Internal(target)) => {
                    builder.add_internal(source, target);
                }
                Some(AnchorTarget::Chapter(target_chapter)) => {
                    builder.add_chapter(source, target_chapter);
                }
                Some(AnchorTarget::External(url)) => {
                    builder.add_external(source, url);
                }
                None => {
                    builder.add_broken(source, href.to_string());
                }
            }
        }
    }

    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeId;

    #[test]
    fn test_resolved_links_empty() {
        let resolved = ResolvedLinks::new();
        assert!(resolved.is_empty());
        assert_eq!(resolved.len(), 0);
        assert!(resolved.broken_links().is_empty());
    }

    #[test]
    fn test_builder_internal_link() {
        let mut builder = ResolvedLinksBuilder::new();

        let source = GlobalNodeId::new(ChapterId(0), NodeId(5));
        let target = GlobalNodeId::new(ChapterId(1), NodeId(23));

        builder.add_internal(source, target);

        let resolved = builder.build();

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved.get(source),
            Some(&AnchorTarget::Internal(target))
        );
        assert!(resolved.is_internal_target(target));
        assert_eq!(resolved.links_to(target), &[source]);
    }

    #[test]
    fn test_builder_chapter_link() {
        let mut builder = ResolvedLinksBuilder::new();

        let source = GlobalNodeId::new(ChapterId(0), NodeId(10));
        let target_chapter = ChapterId(2);

        builder.add_chapter(source, target_chapter);

        let resolved = builder.build();

        assert_eq!(
            resolved.get(source),
            Some(&AnchorTarget::Chapter(target_chapter))
        );
        assert_eq!(resolved.links_to_chapter(target_chapter), &[source]);
    }

    #[test]
    fn test_builder_external_link() {
        let mut builder = ResolvedLinksBuilder::new();

        let source = GlobalNodeId::new(ChapterId(0), NodeId(15));
        let url = "https://example.com".to_string();

        builder.add_external(source, url.clone());

        let resolved = builder.build();

        assert_eq!(
            resolved.get(source),
            Some(&AnchorTarget::External(url))
        );
    }

    #[test]
    fn test_builder_broken_link() {
        let mut builder = ResolvedLinksBuilder::new();

        let source = GlobalNodeId::new(ChapterId(0), NodeId(20));
        let href = "nonexistent.xhtml#missing".to_string();

        builder.add_broken(source, href.clone());

        let resolved = builder.build();

        assert_eq!(resolved.broken_links(), &[(source, href)]);
    }

    #[test]
    fn test_multiple_links_to_same_target() {
        let mut builder = ResolvedLinksBuilder::new();

        let target = GlobalNodeId::new(ChapterId(1), NodeId(100));
        let source1 = GlobalNodeId::new(ChapterId(0), NodeId(5));
        let source2 = GlobalNodeId::new(ChapterId(2), NodeId(10));

        builder.add_internal(source1, target);
        builder.add_internal(source2, target);

        let resolved = builder.build();

        assert!(resolved.is_internal_target(target));
        let links = resolved.links_to(target);
        assert_eq!(links.len(), 2);
        assert!(links.contains(&source1));
        assert!(links.contains(&source2));
    }

    #[test]
    fn test_is_chapter_target() {
        let mut builder = ResolvedLinksBuilder::new();

        let source = GlobalNodeId::new(ChapterId(0), NodeId(5));
        let target_chapter = ChapterId(2);

        builder.add_chapter(source, target_chapter);

        let resolved = builder.build();

        assert!(resolved.is_chapter_target(target_chapter));
        assert!(!resolved.is_chapter_target(ChapterId(99)));
    }
}
