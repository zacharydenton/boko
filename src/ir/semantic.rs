//! Sparse semantic attributes for IR nodes.
//!
//! Most nodes don't have href, src, or alt attributes.
//! Using HashMaps is more memory-efficient than `Option<String>` on every Node.
//!
//! String values are stored in a single contiguous buffer, with TextRange
//! references into that buffer. This avoids per-attribute String allocations.

use std::collections::HashMap;

use super::node::{NodeId, TextRange};

/// Sparse map for semantic attributes.
///
/// Stores attributes only for nodes that have them, saving memory
/// compared to storing `Option<String>` on every node.
///
/// All string values are stored in a single buffer, with TextRange
/// references. This eliminates per-attribute heap allocations.
#[derive(Debug, Default, Clone)]
pub struct SemanticMap {
    /// Contiguous buffer for all string attribute values.
    buffer: String,
    /// href attribute (for links).
    href: HashMap<NodeId, TextRange>,
    /// src attribute (for images).
    src: HashMap<NodeId, TextRange>,
    /// alt attribute (for images).
    alt: HashMap<NodeId, TextRange>,
    /// id attribute (for anchors).
    id: HashMap<NodeId, TextRange>,
    /// title attribute (for tooltips).
    title: HashMap<NodeId, TextRange>,
    /// lang attribute (for language).
    lang: HashMap<NodeId, TextRange>,
    /// epub:type attribute (for EPUB semantics).
    epub_type: HashMap<NodeId, TextRange>,
    /// WAI-ARIA role attribute.
    aria_role: HashMap<NodeId, TextRange>,
    /// datetime attribute (for `<time>` elements).
    datetime: HashMap<NodeId, TextRange>,
    /// start attribute (for ordered lists, ol@start).
    list_start: HashMap<NodeId, u32>,
    /// rowspan attribute (for table cells).
    row_span: HashMap<NodeId, u32>,
    /// colspan attribute (for table cells).
    col_span: HashMap<NodeId, u32>,
    /// Whether a table cell is a header cell (th vs td).
    is_header_cell: HashMap<NodeId, bool>,
    /// Programming language for code blocks.
    language: HashMap<NodeId, TextRange>,
}

impl SemanticMap {
    /// Create a new empty semantic map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a string to the buffer and return its TextRange.
    fn append(&mut self, s: &str) -> TextRange {
        let start = self.buffer.len() as u32;
        self.buffer.push_str(s);
        TextRange::new(start, s.len() as u32)
    }

    /// Get a string slice from a TextRange.
    fn get_str(&self, range: TextRange) -> &str {
        let start = range.start as usize;
        let end = (range.start + range.len) as usize;
        &self.buffer[start..end]
    }

    // --- href ---

    /// Set the href for a node.
    pub fn set_href(&mut self, node: NodeId, href: &str) {
        if !href.is_empty() {
            let range = self.append(href);
            self.href.insert(node, range);
        }
    }

    /// Get the href for a node.
    pub fn href(&self, node: NodeId) -> Option<&str> {
        self.href.get(&node).map(|r| self.get_str(*r))
    }

    // --- src ---

    /// Set the src for a node.
    pub fn set_src(&mut self, node: NodeId, src: &str) {
        if !src.is_empty() {
            let range = self.append(src);
            self.src.insert(node, range);
        }
    }

    /// Get the src for a node.
    pub fn src(&self, node: NodeId) -> Option<&str> {
        self.src.get(&node).map(|r| self.get_str(*r))
    }

    // --- alt ---

    /// Set the alt text for a node.
    pub fn set_alt(&mut self, node: NodeId, alt: &str) {
        if !alt.is_empty() {
            let range = self.append(alt);
            self.alt.insert(node, range);
        }
    }

    /// Get the alt text for a node.
    pub fn alt(&self, node: NodeId) -> Option<&str> {
        self.alt.get(&node).map(|r| self.get_str(*r))
    }

    // --- id ---

    /// Set the id for a node.
    pub fn set_id(&mut self, node: NodeId, id: &str) {
        if !id.is_empty() {
            let range = self.append(id);
            self.id.insert(node, range);
        }
    }

    /// Get the id for a node.
    pub fn id(&self, node: NodeId) -> Option<&str> {
        self.id.get(&node).map(|r| self.get_str(*r))
    }

    // --- title ---

    /// Set the title for a node.
    pub fn set_title(&mut self, node: NodeId, title: &str) {
        if !title.is_empty() {
            let range = self.append(title);
            self.title.insert(node, range);
        }
    }

    /// Get the title for a node.
    pub fn title(&self, node: NodeId) -> Option<&str> {
        self.title.get(&node).map(|r| self.get_str(*r))
    }

    // --- lang ---

    /// Set the language for a node.
    pub fn set_lang(&mut self, node: NodeId, lang: &str) {
        if !lang.is_empty() {
            let range = self.append(lang);
            self.lang.insert(node, range);
        }
    }

    /// Get the language for a node.
    pub fn lang(&self, node: NodeId) -> Option<&str> {
        self.lang.get(&node).map(|r| self.get_str(*r))
    }

    // --- epub:type ---

    /// Set the epub:type for a node.
    pub fn set_epub_type(&mut self, node: NodeId, epub_type: &str) {
        if !epub_type.is_empty() {
            let range = self.append(epub_type);
            self.epub_type.insert(node, range);
        }
    }

    /// Get the epub:type for a node.
    pub fn epub_type(&self, node: NodeId) -> Option<&str> {
        self.epub_type.get(&node).map(|r| self.get_str(*r))
    }

    // --- aria role ---

    /// Set the WAI-ARIA role for a node.
    pub fn set_aria_role(&mut self, node: NodeId, role: &str) {
        if !role.is_empty() {
            let range = self.append(role);
            self.aria_role.insert(node, range);
        }
    }

    /// Get the WAI-ARIA role for a node.
    pub fn aria_role(&self, node: NodeId) -> Option<&str> {
        self.aria_role.get(&node).map(|r| self.get_str(*r))
    }

    // --- datetime ---

    /// Set the datetime for a node (from `<time>` elements).
    pub fn set_datetime(&mut self, node: NodeId, datetime: &str) {
        if !datetime.is_empty() {
            let range = self.append(datetime);
            self.datetime.insert(node, range);
        }
    }

    /// Get the datetime for a node.
    pub fn datetime(&self, node: NodeId) -> Option<&str> {
        self.datetime.get(&node).map(|r| self.get_str(*r))
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
    pub fn set_language(&mut self, node: NodeId, language: &str) {
        if !language.is_empty() {
            let range = self.append(language);
            self.language.insert(node, range);
        }
    }

    /// Get the programming language for a code block.
    pub fn language(&self, node: NodeId) -> Option<&str> {
        self.language.get(&node).map(|r| self.get_str(*r))
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
    pub fn set_attr(&mut self, node: NodeId, name: &str, value: &str) -> bool {
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
    /// Note: This appends resolved values to the buffer (old values become
    /// unreachable but buffer space is not reclaimed).
    ///
    /// # Arguments
    ///
    /// * `resolver` - A function that takes a path and returns the resolved path
    pub fn resolve_paths<F>(&mut self, resolver: F)
    where
        F: Fn(&str) -> String,
    {
        // Resolve src attributes (images)
        // Collect updates first to avoid borrow conflicts
        let src_updates: Vec<_> = self
            .src
            .iter()
            .map(|(&node, &range)| {
                let old_value = self.get_str(range);
                (node, resolver(old_value))
            })
            .collect();

        for (node, new_value) in src_updates {
            let range = self.append(&new_value);
            self.src.insert(node, range);
        }

        // Resolve href attributes (links)
        // Note: Only resolve internal links, not external URLs
        let href_updates: Vec<_> = self
            .href
            .iter()
            .filter_map(|(&node, &range)| {
                let old_value = self.get_str(range);
                if !old_value.contains("://") && !old_value.starts_with("mailto:") {
                    Some((node, resolver(old_value)))
                } else {
                    None
                }
            })
            .collect();

        for (node, new_value) in href_updates {
            let range = self.append(&new_value);
            self.href.insert(node, range);
        }
    }
}
