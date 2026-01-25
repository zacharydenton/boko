//! Sparse semantic attributes for IR nodes.
//!
//! Most nodes don't have href, src, or alt attributes.
//! Using HashMaps is more memory-efficient than Option<String> on every Node.

use std::collections::HashMap;

use super::node::NodeId;

/// Sparse map for semantic attributes.
///
/// Stores attributes only for nodes that have them, saving memory
/// compared to storing Option<String> on every node.
#[derive(Debug, Default, Clone)]
pub struct SemanticMap {
    /// href attribute (for links).
    href: HashMap<NodeId, String>,
    /// src attribute (for images).
    src: HashMap<NodeId, String>,
    /// alt attribute (for images).
    alt: HashMap<NodeId, String>,
    /// id attribute (for anchors).
    id: HashMap<NodeId, String>,
    /// title attribute (for tooltips).
    title: HashMap<NodeId, String>,
    /// lang attribute (for language).
    lang: HashMap<NodeId, String>,
}

impl SemanticMap {
    /// Create a new empty semantic map.
    pub fn new() -> Self {
        Self::default()
    }

    // --- href ---

    /// Set the href for a node.
    pub fn set_href(&mut self, node: NodeId, href: String) {
        if !href.is_empty() {
            self.href.insert(node, href);
        }
    }

    /// Get the href for a node.
    pub fn href(&self, node: NodeId) -> Option<&str> {
        self.href.get(&node).map(|s| s.as_str())
    }

    // --- src ---

    /// Set the src for a node.
    pub fn set_src(&mut self, node: NodeId, src: String) {
        if !src.is_empty() {
            self.src.insert(node, src);
        }
    }

    /// Get the src for a node.
    pub fn src(&self, node: NodeId) -> Option<&str> {
        self.src.get(&node).map(|s| s.as_str())
    }

    // --- alt ---

    /// Set the alt text for a node.
    pub fn set_alt(&mut self, node: NodeId, alt: String) {
        if !alt.is_empty() {
            self.alt.insert(node, alt);
        }
    }

    /// Get the alt text for a node.
    pub fn alt(&self, node: NodeId) -> Option<&str> {
        self.alt.get(&node).map(|s| s.as_str())
    }

    // --- id ---

    /// Set the id for a node.
    pub fn set_id(&mut self, node: NodeId, id: String) {
        if !id.is_empty() {
            self.id.insert(node, id);
        }
    }

    /// Get the id for a node.
    pub fn id(&self, node: NodeId) -> Option<&str> {
        self.id.get(&node).map(|s| s.as_str())
    }

    // --- title ---

    /// Set the title for a node.
    pub fn set_title(&mut self, node: NodeId, title: String) {
        if !title.is_empty() {
            self.title.insert(node, title);
        }
    }

    /// Get the title for a node.
    pub fn title(&self, node: NodeId) -> Option<&str> {
        self.title.get(&node).map(|s| s.as_str())
    }

    // --- lang ---

    /// Set the language for a node.
    pub fn set_lang(&mut self, node: NodeId, lang: String) {
        if !lang.is_empty() {
            self.lang.insert(node, lang);
        }
    }

    /// Get the language for a node.
    pub fn lang(&self, node: NodeId) -> Option<&str> {
        self.lang.get(&node).map(|s| s.as_str())
    }

    /// Get the total number of stored attributes.
    pub fn len(&self) -> usize {
        self.href.len()
            + self.src.len()
            + self.alt.len()
            + self.id.len()
            + self.title.len()
            + self.lang.len()
    }

    /// Check if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Resolve all `src` and `href` paths using the provided resolver function.
    ///
    /// This is used to canonicalize relative paths (e.g., `../images/photo.jpg`)
    /// to absolute archive paths (e.g., `OEBPS/images/photo.jpg`).
    ///
    /// # Arguments
    ///
    /// * `resolver` - A function that takes a path and returns the resolved path
    pub fn resolve_paths<F>(&mut self, resolver: F)
    where
        F: Fn(&str) -> String,
    {
        // Resolve src attributes (images)
        for value in self.src.values_mut() {
            *value = resolver(value);
        }

        // Resolve href attributes (links)
        // Note: Only resolve internal links, not external URLs
        for value in self.href.values_mut() {
            if !value.contains("://") && !value.starts_with("mailto:") {
                *value = resolver(value);
            }
        }
    }
}
