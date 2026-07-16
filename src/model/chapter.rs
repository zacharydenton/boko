//! Chapter representation for normalized ebook content.
//!
//! The Chapter (formerly IRChapter) provides a format-agnostic tree structure
//! for ebook chapters:
//! - Nodes with semantic roles (paragraphs, headings, links, etc.)
//! - Interned styles via StylePool
//! - Sparse semantic attributes (href, src, alt)
//! - Universal link representation (handles both EPUB IDs and Kindle offsets)
//! - Global text buffer with range references

use super::node::{Node, NodeId, Role, TextRange};
use super::semantic::SemanticMap;
use crate::style::StylePool;

/// A chapter's content in normalized IR form.
///
/// The IR tree uses a parent-pointer / first-child / next-sibling representation
/// for efficient traversal and minimal memory overhead.
#[derive(Debug, Clone)]
pub struct Chapter {
    /// All nodes in the tree (index 0 is always the root).
    nodes: Vec<Node>,
    /// Style pool with deduplication.
    pub styles: StylePool,
    /// Sparse semantic attributes (href, src, alt, id).
    pub semantics: SemanticMap,
    /// Global text buffer (nodes reference ranges into this).
    text: String,
}

impl Default for Chapter {
    fn default() -> Self {
        Self::new()
    }
}

impl Chapter {
    /// Create a new empty chapter with a root node.
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::new(Role::Root)],
            styles: StylePool::new(),
            semantics: SemanticMap::new(),
            text: String::new(),
        }
    }

    /// Get the root node ID.
    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }

    /// Get a node by ID.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0 as usize)
    }

    /// Get a mutable node by ID.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.0 as usize)
    }

    /// Get the number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Allocate a new node and return its ID.
    pub fn alloc_node(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    /// Append text to the global buffer and return the range.
    pub fn append_text(&mut self, text: &str) -> TextRange {
        let start = self.text.len() as u32;
        self.text.push_str(text);
        TextRange::new(start, text.len() as u32)
    }

    /// Append text with HTML whitespace normalization (runs of whitespace
    /// collapse to a single space) and return the range.
    ///
    /// Normalizes directly into the global buffer, so already-normalized text
    /// (the common case) is a single `memcpy` and never allocates an
    /// intermediate `String`.
    pub fn append_text_normalized(&mut self, text: &str) -> TextRange {
        let start = self.text.len() as u32;
        if is_normalized_whitespace(text) {
            self.text.push_str(text);
        } else {
            self.text.reserve(text.len());
            let mut prev_was_whitespace = false;
            for c in text.chars() {
                if c.is_whitespace() {
                    if !prev_was_whitespace {
                        self.text.push(' ');
                        prev_was_whitespace = true;
                    }
                } else {
                    self.text.push(c);
                    prev_was_whitespace = false;
                }
            }
        }
        TextRange::new(start, self.text.len() as u32 - start)
    }

    /// Get text from a range.
    pub fn text(&self, range: TextRange) -> &str {
        let start = range.start as usize;
        let end = (range.start + range.len) as usize;
        &self.text[start..end]
    }

    /// Get the entire text buffer.
    pub fn text_buffer(&self) -> &str {
        &self.text
    }

    /// Append a child node to a parent. O(1) via the parent's `last_child`
    /// pointer (previously O(children) per call, i.e. O(n²) to build a node
    /// with n siblings).
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        // Set the child's parent.
        if let Some(child_node) = self.nodes.get_mut(child.0 as usize) {
            child_node.parent = Some(parent);
        }

        let last = match self.nodes.get(parent.0 as usize) {
            Some(p) => p.last_child,
            None => return,
        };
        match last {
            // Append after the current last child.
            Some(last_id) => {
                if let Some(last_node) = self.nodes.get_mut(last_id.0 as usize) {
                    last_node.next_sibling = Some(child);
                }
            }
            // First child.
            None => {
                if let Some(parent_node) = self.nodes.get_mut(parent.0 as usize) {
                    parent_node.first_child = Some(child);
                }
            }
        }
        if let Some(parent_node) = self.nodes.get_mut(parent.0 as usize) {
            parent_node.last_child = Some(child);
        }
    }

    /// Iterate over children of a node.
    pub fn children(&self, parent: NodeId) -> ChildIter<'_> {
        let first_child = self
            .nodes
            .get(parent.0 as usize)
            .and_then(|n| n.first_child);
        ChildIter {
            chapter: self,
            current: first_child,
        }
    }

    /// Iterate over all nodes in depth-first (preorder) order.
    pub fn iter_dfs(&self) -> DfsIter<'_> {
        DfsIter {
            chapter: self,
            next: Some(NodeId::ROOT),
        }
    }
}

/// Whether `text` is already whitespace-normalized: ASCII-only with no
/// whitespace other than single interior spaces. The check is conservative —
/// any non-ASCII byte falls back to the char-by-char normalization path,
/// which handles Unicode whitespace exactly as before.
fn is_normalized_whitespace(text: &str) -> bool {
    let mut prev_space = false;
    for &b in text.as_bytes() {
        match b {
            b' ' => {
                if prev_space {
                    return false;
                }
                prev_space = true;
            }
            b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r' => return false,
            _ if b >= 0x80 => return false,
            _ => prev_space = false,
        }
    }
    true
}

/// Iterator over children of a node.
pub struct ChildIter<'a> {
    chapter: &'a Chapter,
    current: Option<NodeId>,
}

impl Iterator for ChildIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        self.current = self
            .chapter
            .nodes
            .get(current.0 as usize)
            .and_then(|n| n.next_sibling);
        Some(current)
    }
}

/// Depth-first (preorder) iterator over all nodes.
///
/// Allocation-free: the first-child/next-sibling/parent links let it walk the
/// tree with a single cursor instead of a heap stack, so a full traversal
/// (every export path runs several) makes zero allocations.
pub struct DfsIter<'a> {
    chapter: &'a Chapter,
    next: Option<NodeId>,
}

impl Iterator for DfsIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next?;

        // Descend to the first child; else move to the next sibling; else
        // climb through ancestors until one has a next sibling.
        self.next = self.chapter.node(current).and_then(|node| {
            if let Some(child) = node.first_child {
                return Some(child);
            }
            let mut n = node;
            loop {
                if let Some(sib) = n.next_sibling {
                    return Some(sib);
                }
                n = self.chapter.node(n.parent?)?;
            }
        });

        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{ComputedStyle, FontWeight};

    #[test]
    fn test_chapter_creation() {
        let chapter = Chapter::new();
        assert_eq!(chapter.node_count(), 1);
        assert_eq!(chapter.root(), NodeId::ROOT);

        let root = chapter.node(NodeId::ROOT).unwrap();
        assert_eq!(root.role, Role::Root);
        assert!(root.parent.is_none());
    }

    #[test]
    fn test_text_buffer() {
        let mut chapter = Chapter::new();

        let range1 = chapter.append_text("Hello, ");
        let range2 = chapter.append_text("World!");

        assert_eq!(chapter.text(range1), "Hello, ");
        assert_eq!(chapter.text(range2), "World!");
        assert_eq!(chapter.text_buffer(), "Hello, World!");
    }

    #[test]
    fn test_node_tree() {
        let mut chapter = Chapter::new();

        let para = chapter.alloc_node(Node::new(Role::Text));
        chapter.append_child(NodeId::ROOT, para);

        let text_range = chapter.append_text("Test content");
        let text_node = chapter.alloc_node(Node::text(text_range));
        chapter.append_child(para, text_node);

        // Verify structure
        let children: Vec<_> = chapter.children(NodeId::ROOT).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], para);

        let text_children: Vec<_> = chapter.children(para).collect();
        assert_eq!(text_children.len(), 1);
        assert_eq!(chapter.node(text_children[0]).unwrap().role, Role::Text);
    }

    #[test]
    fn test_dfs_iteration() {
        let mut chapter = Chapter::new();

        let para1 = chapter.alloc_node(Node::new(Role::Text));
        let para2 = chapter.alloc_node(Node::new(Role::Text));
        chapter.append_child(NodeId::ROOT, para1);
        chapter.append_child(NodeId::ROOT, para2);

        let range = chapter.append_text("Text");
        let text = chapter.alloc_node(Node::text(range));
        chapter.append_child(para1, text);

        let nodes: Vec<_> = chapter.iter_dfs().collect();
        assert_eq!(nodes.len(), 4); // root, para1, text, para2
        assert_eq!(nodes[0], NodeId::ROOT);
        assert_eq!(nodes[1], para1);
        assert_eq!(nodes[2], text);
        assert_eq!(nodes[3], para2);
    }

    #[test]
    fn test_style_interning() {
        let mut pool = StylePool::new();

        let style1 = ComputedStyle {
            font_weight: FontWeight::BOLD,
            ..Default::default()
        };
        let style2 = ComputedStyle {
            font_weight: FontWeight::BOLD,
            ..Default::default()
        };
        let style3 = ComputedStyle {
            font_weight: FontWeight::NORMAL,
            ..Default::default()
        };

        let id1 = pool.intern(style1);
        let id2 = pool.intern(style2);
        let id3 = pool.intern(style3);

        // Same style should get same ID
        assert_eq!(id1, id2);
        // Different style should get different ID
        assert_ne!(id1, id3);
        // Pool should have 3 styles (default + 2 unique)
        assert_eq!(pool.len(), 3);
    }

    #[test]
    fn test_semantic_map() {
        let mut semantics = SemanticMap::new();
        let node = NodeId(1);

        semantics.set_href(node, "https://example.com");
        semantics.set_alt(node, "An image");

        assert_eq!(semantics.href(node), Some("https://example.com"));
        assert_eq!(semantics.alt(node), Some("An image"));
        assert_eq!(semantics.src(node), None);
    }
}
