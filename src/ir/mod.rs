//! Intermediate Representation (IR) for normalized ebook content.
//!
//! The IR provides a format-agnostic tree structure for ebook chapters:
//! - Nodes with semantic roles (paragraphs, headings, links, etc.)
//! - Interned styles via StylePool
//! - Sparse semantic attributes (href, src, alt)
//! - Universal link representation (handles both EPUB IDs and Kindle offsets)
//! - Global text buffer with range references
//!
//! # Example
//!
//! ```
//! use boko::ir::{IRChapter, Node, NodeId, Role};
//!
//! // IRChapter is produced by compile_html()
//! // Here we show manual construction for illustration:
//! let mut chapter = IRChapter::new();
//! let root = chapter.root();
//! assert_eq!(chapter.node(root).unwrap().role, Role::Root);
//! ```

mod links;
mod node;
mod semantic;
mod style;

pub use links::{InternalLocation, Link, LinkTarget};
pub use node::{Node, NodeId, Role, TextRange};
pub use semantic::SemanticMap;
pub use style::{
    BorderStyle, BoxSizing, BreakValue, Color, ComputedStyle, DecorationStyle, Display, Float,
    FontStyle, FontVariant, FontWeight, Hyphens, Length, ListStylePosition, ListStyleType, StyleId,
    StylePool, TextAlign, TextTransform, ToCss, Visibility,
};

/// A chapter's content in normalized IR form.
///
/// The IR tree uses a parent-pointer / first-child / next-sibling representation
/// for efficient traversal and minimal memory overhead.
#[derive(Debug, Clone)]
pub struct IRChapter {
    /// All nodes in the tree (index 0 is always the root).
    nodes: Vec<Node>,
    /// Style pool with deduplication.
    pub styles: StylePool,
    /// Sparse semantic attributes (href, src, alt, id).
    pub semantics: SemanticMap,
    /// Global text buffer (nodes reference ranges into this).
    text: String,
}

impl Default for IRChapter {
    fn default() -> Self {
        Self::new()
    }
}

impl IRChapter {
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

    /// Append a child node to a parent.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        // Set the child's parent
        if let Some(child_node) = self.nodes.get_mut(child.0 as usize) {
            child_node.parent = Some(parent);
        }

        // Find the last child of parent and append
        if let Some(parent_node) = self.nodes.get(parent.0 as usize) {
            if let Some(first_child) = parent_node.first_child {
                // Find last sibling
                let mut current = first_child;
                while let Some(node) = self.nodes.get(current.0 as usize) {
                    if let Some(next) = node.next_sibling {
                        current = next;
                    } else {
                        break;
                    }
                }
                // Append as next sibling of last child
                if let Some(last_node) = self.nodes.get_mut(current.0 as usize) {
                    last_node.next_sibling = Some(child);
                }
            } else {
                // No children yet, set as first child
                if let Some(parent_node) = self.nodes.get_mut(parent.0 as usize) {
                    parent_node.first_child = Some(child);
                }
            }
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

    /// Iterate over all nodes in depth-first order.
    pub fn iter_dfs(&self) -> DfsIter<'_> {
        DfsIter {
            chapter: self,
            stack: vec![NodeId::ROOT],
        }
    }
}

/// Iterator over children of a node.
pub struct ChildIter<'a> {
    chapter: &'a IRChapter,
    current: Option<NodeId>,
}

impl<'a> Iterator for ChildIter<'a> {
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

/// Depth-first iterator over all nodes.
pub struct DfsIter<'a> {
    chapter: &'a IRChapter,
    stack: Vec<NodeId>,
}

impl<'a> Iterator for DfsIter<'a> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.stack.pop()?;

        // Push children in reverse order so they're visited left-to-right
        let mut children: Vec<NodeId> = self.chapter.children(current).collect();
        children.reverse();
        self.stack.extend(children);

        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chapter_creation() {
        let chapter = IRChapter::new();
        assert_eq!(chapter.node_count(), 1);
        assert_eq!(chapter.root(), NodeId::ROOT);

        let root = chapter.node(NodeId::ROOT).unwrap();
        assert_eq!(root.role, Role::Root);
        assert!(root.parent.is_none());
    }

    #[test]
    fn test_text_buffer() {
        let mut chapter = IRChapter::new();

        let range1 = chapter.append_text("Hello, ");
        let range2 = chapter.append_text("World!");

        assert_eq!(chapter.text(range1), "Hello, ");
        assert_eq!(chapter.text(range2), "World!");
        assert_eq!(chapter.text_buffer(), "Hello, World!");
    }

    #[test]
    fn test_node_tree() {
        let mut chapter = IRChapter::new();

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
        let mut chapter = IRChapter::new();

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

        semantics.set_href(node, "https://example.com".to_string());
        semantics.set_alt(node, "An image".to_string());

        assert_eq!(semantics.href(node), Some("https://example.com"));
        assert_eq!(semantics.alt(node), Some("An image"));
        assert_eq!(semantics.src(node), None);
    }
}
