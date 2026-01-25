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
    /// epub:type attribute (for EPUB semantics).
    epub_type: HashMap<NodeId, String>,
    /// WAI-ARIA role attribute.
    aria_role: HashMap<NodeId, String>,
    /// datetime attribute (for <time> elements).
    datetime: HashMap<NodeId, String>,
    /// start attribute (for ordered lists, ol@start).
    list_start: HashMap<NodeId, u32>,
    /// rowspan attribute (for table cells).
    row_span: HashMap<NodeId, u32>,
    /// colspan attribute (for table cells).
    col_span: HashMap<NodeId, u32>,
    /// Whether a table cell is a header cell (th vs td).
    is_header_cell: HashMap<NodeId, bool>,
    /// Programming language for code blocks.
    language: HashMap<NodeId, String>,
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

    // --- epub:type ---

    /// Set the epub:type for a node.
    pub fn set_epub_type(&mut self, node: NodeId, epub_type: String) {
        if !epub_type.is_empty() {
            self.epub_type.insert(node, epub_type);
        }
    }

    /// Get the epub:type for a node.
    pub fn epub_type(&self, node: NodeId) -> Option<&str> {
        self.epub_type.get(&node).map(|s| s.as_str())
    }

    // --- aria role ---

    /// Set the WAI-ARIA role for a node.
    pub fn set_aria_role(&mut self, node: NodeId, role: String) {
        if !role.is_empty() {
            self.aria_role.insert(node, role);
        }
    }

    /// Get the WAI-ARIA role for a node.
    pub fn aria_role(&self, node: NodeId) -> Option<&str> {
        self.aria_role.get(&node).map(|s| s.as_str())
    }

    // --- datetime ---

    /// Set the datetime for a node (from `<time>` elements).
    pub fn set_datetime(&mut self, node: NodeId, datetime: String) {
        if !datetime.is_empty() {
            self.datetime.insert(node, datetime);
        }
    }

    /// Get the datetime for a node.
    pub fn datetime(&self, node: NodeId) -> Option<&str> {
        self.datetime.get(&node).map(|s| s.as_str())
    }

    // --- list_start ---

    /// Set the start number for an ordered list (from `<ol start="N">`).
    pub fn set_list_start(&mut self, node: NodeId, start: u32) {
        if start != 1 {
            self.list_start.insert(node, start);
        }
    }

    /// Get the start number for an ordered list.
    /// Returns None if not set (defaults to 1).
    pub fn list_start(&self, node: NodeId) -> Option<u32> {
        self.list_start.get(&node).copied()
    }

    // --- row_span ---

    /// Set the rowspan for a table cell.
    pub fn set_row_span(&mut self, node: NodeId, span: u32) {
        if span > 1 {
            self.row_span.insert(node, span);
        }
    }

    /// Get the rowspan for a table cell.
    /// Returns None if not set (defaults to 1).
    pub fn row_span(&self, node: NodeId) -> Option<u32> {
        self.row_span.get(&node).copied()
    }

    // --- col_span ---

    /// Set the colspan for a table cell.
    pub fn set_col_span(&mut self, node: NodeId, span: u32) {
        if span > 1 {
            self.col_span.insert(node, span);
        }
    }

    /// Get the colspan for a table cell.
    /// Returns None if not set (defaults to 1).
    pub fn col_span(&self, node: NodeId) -> Option<u32> {
        self.col_span.get(&node).copied()
    }

    // --- is_header_cell ---

    /// Set whether a table cell is a header cell (th vs td).
    pub fn set_header_cell(&mut self, node: NodeId, is_header: bool) {
        if is_header {
            self.is_header_cell.insert(node, true);
        }
    }

    /// Check if a table cell is a header cell.
    pub fn is_header_cell(&self, node: NodeId) -> bool {
        self.is_header_cell.get(&node).copied().unwrap_or(false)
    }

    // --- language ---

    /// Set the programming language for a code block.
    pub fn set_language(&mut self, node: NodeId, language: String) {
        if !language.is_empty() {
            self.language.insert(node, language);
        }
    }

    /// Get the programming language for a code block.
    pub fn language(&self, node: NodeId) -> Option<&str> {
        self.language.get(&node).map(|s| s.as_str())
    }

    // --- Generic access ---

    /// Get an attribute by name.
    ///
    /// This provides uniform access to semantic attributes, useful for
    /// exporters that need to query multiple attributes dynamically.
    ///
    /// # Supported attribute names
    ///
    /// - `"href"` - Link target
    /// - `"src"` - Image source
    /// - `"alt"` - Alternative text
    /// - `"id"` - Element ID
    /// - `"title"` - Tooltip text
    /// - `"lang"` - Language code
    /// - `"epub:type"` - EPUB semantic type
    /// - `"role"` - WAI-ARIA role
    /// - `"datetime"` - Machine-readable date
    ///
    /// # Example
    ///
    /// ```
    /// use boko::ir::{SemanticMap, NodeId};
    ///
    /// let mut semantics = SemanticMap::new();
    /// let node = NodeId(1);
    ///
    /// semantics.set_attr(node, "href", "https://example.com");
    /// assert_eq!(semantics.get_attr(node, "href"), Some("https://example.com"));
    /// ```
    pub fn get_attr(&self, node: NodeId, name: &str) -> Option<&str> {
        match name {
            "href" => self.href(node),
            "src" => self.src(node),
            "alt" => self.alt(node),
            "id" => self.id(node),
            "title" => self.title(node),
            "lang" | "xml:lang" => self.lang(node),
            "epub:type" => self.epub_type(node),
            "role" => self.aria_role(node),
            "datetime" => self.datetime(node),
            _ => None,
        }
    }

    /// Set an attribute by name.
    ///
    /// Returns `true` if the attribute name was recognized, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use boko::ir::{SemanticMap, NodeId};
    ///
    /// let mut semantics = SemanticMap::new();
    /// let node = NodeId(1);
    ///
    /// assert!(semantics.set_attr(node, "alt", "A photo"));
    /// assert!(!semantics.set_attr(node, "unknown", "value")); // Unrecognized
    /// ```
    pub fn set_attr(&mut self, node: NodeId, name: &str, value: impl Into<String>) -> bool {
        let value = value.into();
        match name {
            "href" => {
                self.set_href(node, value);
                true
            }
            "src" => {
                self.set_src(node, value);
                true
            }
            "alt" => {
                self.set_alt(node, value);
                true
            }
            "id" => {
                self.set_id(node, value);
                true
            }
            "title" => {
                self.set_title(node, value);
                true
            }
            "lang" | "xml:lang" => {
                self.set_lang(node, value);
                true
            }
            "epub:type" => {
                self.set_epub_type(node, value);
                true
            }
            "role" => {
                self.set_aria_role(node, value);
                true
            }
            "datetime" => {
                self.set_datetime(node, value);
                true
            }
            _ => false,
        }
    }

    /// Get the total number of stored attributes.
    pub fn len(&self) -> usize {
        self.href.len()
            + self.src.len()
            + self.alt.len()
            + self.id.len()
            + self.title.len()
            + self.lang.len()
            + self.epub_type.len()
            + self.aria_role.len()
            + self.datetime.len()
            + self.list_start.len()
            + self.row_span.len()
            + self.col_span.len()
            + self.is_header_cell.len()
            + self.language.len()
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
