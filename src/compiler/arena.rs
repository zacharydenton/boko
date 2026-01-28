//! Arena-based DOM for HTML parsing.
//!
//! This module provides an efficient arena-allocated DOM tree that html5ever
//! can parse into. The arena layout enables fast traversal and selector matching.

use std::collections::HashMap;

use html5ever::{LocalName, Namespace, QualName};

/// Unique identifier for a node in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArenaNodeId(pub u32);

impl ArenaNodeId {
    /// Sentinel value for no node.
    pub const NONE: ArenaNodeId = ArenaNodeId(u32::MAX);

    /// Check if this is a valid node ID.
    pub fn is_some(&self) -> bool {
        self.0 != u32::MAX
    }

    /// Check if this is the sentinel value.
    pub fn is_none(&self) -> bool {
        self.0 == u32::MAX
    }
}

/// Node type in the arena DOM.
#[derive(Debug, Clone)]
pub enum ArenaNodeData {
    /// Document root.
    Document,
    /// Element with name and attributes.
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
        /// Pre-extracted id for fast matching.
        id: Option<String>,
        /// Pre-extracted classes for fast matching.
        classes: Vec<String>,
    },
    /// Text content.
    Text(String),
    /// Comment (ignored but needed for TreeSink).
    Comment(String),
    /// Document type declaration.
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
}

/// HTML attribute.
#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: QualName,
    pub value: String,
}

/// A node in the arena DOM.
#[derive(Debug)]
pub struct ArenaNode {
    pub data: ArenaNodeData,
    pub parent: ArenaNodeId,
    pub first_child: ArenaNodeId,
    pub last_child: ArenaNodeId,
    pub prev_sibling: ArenaNodeId,
    pub next_sibling: ArenaNodeId,
}

impl ArenaNode {
    /// Create a new node with the given data.
    fn new(data: ArenaNodeData) -> Self {
        Self {
            data,
            parent: ArenaNodeId::NONE,
            first_child: ArenaNodeId::NONE,
            last_child: ArenaNodeId::NONE,
            prev_sibling: ArenaNodeId::NONE,
            next_sibling: ArenaNodeId::NONE,
        }
    }
}

/// Arena-based DOM tree.
///
/// All nodes are stored in a contiguous vector for cache-friendly traversal.
/// Parent/child/sibling links use indices into this vector.
pub struct ArenaDom {
    /// All nodes in the arena.
    nodes: Vec<ArenaNode>,
    /// Document root ID.
    document: ArenaNodeId,
    /// Map from id attribute to node ID for fast lookup.
    id_map: HashMap<String, ArenaNodeId>,
}

impl ArenaDom {
    /// Create a new empty DOM with a document root.
    pub fn new() -> Self {
        let mut dom = Self {
            nodes: Vec::new(),
            document: ArenaNodeId::NONE,
            id_map: HashMap::new(),
        };
        dom.document = dom.alloc(ArenaNode::new(ArenaNodeData::Document));
        dom
    }

    /// Allocate a new node in the arena.
    fn alloc(&mut self, node: ArenaNode) -> ArenaNodeId {
        let id = ArenaNodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    /// Get the document root ID.
    pub fn document(&self) -> ArenaNodeId {
        self.document
    }

    /// Get a node by ID.
    pub fn get(&self, id: ArenaNodeId) -> Option<&ArenaNode> {
        if id.is_none() {
            return None;
        }
        self.nodes.get(id.0 as usize)
    }

    /// Get a mutable node by ID.
    pub fn get_mut(&mut self, id: ArenaNodeId) -> Option<&mut ArenaNode> {
        if id.is_none() {
            return None;
        }
        self.nodes.get_mut(id.0 as usize)
    }

    /// Create a new element node.
    pub fn create_element(&mut self, name: QualName, attrs: Vec<Attribute>) -> ArenaNodeId {
        // Pre-extract id and class for fast CSS matching
        let mut id = None;
        let mut classes = Vec::new();

        for attr in &attrs {
            if attr.name.local.as_ref() == "id" {
                id = Some(attr.value.clone());
            } else if attr.name.local.as_ref() == "class" {
                classes = attr
                    .value
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
            }
        }

        let node_id = self.alloc(ArenaNode::new(ArenaNodeData::Element {
            name,
            attrs,
            id: id.clone(),
            classes,
        }));

        // Register in id map
        if let Some(id_str) = id {
            self.id_map.insert(id_str, node_id);
        }

        node_id
    }

    /// Create a new text node.
    pub fn create_text(&mut self, text: String) -> ArenaNodeId {
        self.alloc(ArenaNode::new(ArenaNodeData::Text(text)))
    }

    /// Create a new comment node.
    pub fn create_comment(&mut self, text: String) -> ArenaNodeId {
        self.alloc(ArenaNode::new(ArenaNodeData::Comment(text)))
    }

    /// Create a doctype node.
    pub fn create_doctype(
        &mut self,
        name: String,
        public_id: String,
        system_id: String,
    ) -> ArenaNodeId {
        self.alloc(ArenaNode::new(ArenaNodeData::Doctype {
            name,
            public_id,
            system_id,
        }))
    }

    /// Append a child to a parent node.
    pub fn append(&mut self, parent: ArenaNodeId, child: ArenaNodeId) {
        // Get parent's last child
        let last_child = self
            .get(parent)
            .map(|n| n.last_child)
            .unwrap_or(ArenaNodeId::NONE);

        // Set child's parent and prev sibling
        if let Some(child_node) = self.get_mut(child) {
            child_node.parent = parent;
            child_node.prev_sibling = last_child;
        }

        // Update old last child's next sibling
        if last_child.is_some() {
            if let Some(last_node) = self.get_mut(last_child) {
                last_node.next_sibling = child;
            }
        }

        // Update parent
        if let Some(parent_node) = self.get_mut(parent) {
            if parent_node.first_child.is_none() {
                parent_node.first_child = child;
            }
            parent_node.last_child = child;
        }
    }

    /// Insert a node before a sibling.
    pub fn insert_before(&mut self, sibling: ArenaNodeId, new_node: ArenaNodeId) {
        let parent = self
            .get(sibling)
            .map(|n| n.parent)
            .unwrap_or(ArenaNodeId::NONE);
        let prev = self
            .get(sibling)
            .map(|n| n.prev_sibling)
            .unwrap_or(ArenaNodeId::NONE);

        // Set new node's links
        if let Some(new) = self.get_mut(new_node) {
            new.parent = parent;
            new.prev_sibling = prev;
            new.next_sibling = sibling;
        }

        // Update sibling's prev
        if let Some(sib) = self.get_mut(sibling) {
            sib.prev_sibling = new_node;
        }

        // Update prev's next (or parent's first_child)
        if prev.is_some() {
            if let Some(p) = self.get_mut(prev) {
                p.next_sibling = new_node;
            }
        } else if let Some(par) = self.get_mut(parent) {
            par.first_child = new_node;
        }
    }

    /// Append text to an existing text node, or create new if last child isn't text.
    pub fn append_text(&mut self, parent: ArenaNodeId, text: &str) {
        let last_child = self
            .get(parent)
            .map(|n| n.last_child)
            .unwrap_or(ArenaNodeId::NONE);

        // Try to append to existing text node
        if let Some(last) = self.get_mut(last_child) {
            if let ArenaNodeData::Text(ref mut existing) = last.data {
                existing.push_str(text);
                return;
            }
        }

        // Create new text node
        let text_node = self.create_text(text.to_string());
        self.append(parent, text_node);
    }

    /// Get node by id attribute.
    pub fn get_by_id(&self, id: &str) -> Option<ArenaNodeId> {
        self.id_map.get(id).copied()
    }

    /// Get the number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the DOM is empty (only has document root).
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Iterate over children of a node.
    pub fn children(&self, parent: ArenaNodeId) -> ChildrenIter<'_> {
        let first = self
            .get(parent)
            .map(|n| n.first_child)
            .unwrap_or(ArenaNodeId::NONE);
        ChildrenIter {
            dom: self,
            current: first,
        }
    }

    /// Find the first element matching a predicate (DFS).
    pub fn find<F>(&self, predicate: F) -> Option<ArenaNodeId>
    where
        F: Fn(&ArenaNode) -> bool,
    {
        let mut stack = vec![self.document];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get(id) {
                if predicate(node) {
                    return Some(id);
                }
                // Push children in reverse order for left-to-right traversal
                let mut children: Vec<_> = self.children(id).collect();
                children.reverse();
                stack.extend(children);
            }
        }
        None
    }

    /// Find element by tag name (first match).
    pub fn find_by_tag(&self, tag: &str) -> Option<ArenaNodeId> {
        self.find(|node| {
            if let ArenaNodeData::Element { name, .. } = &node.data {
                name.local.as_ref() == tag
            } else {
                false
            }
        })
    }
}

impl Default for ArenaDom {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator over children of a node.
pub struct ChildrenIter<'a> {
    dom: &'a ArenaDom,
    current: ArenaNodeId,
}

impl<'a> Iterator for ChildrenIter<'a> {
    type Item = ArenaNodeId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_none() {
            return None;
        }
        let id = self.current;
        self.current = self
            .dom
            .get(id)
            .map(|n| n.next_sibling)
            .unwrap_or(ArenaNodeId::NONE);
        Some(id)
    }
}

/// Convenience methods for element nodes.
impl ArenaDom {
    /// Get element's local name (tag).
    pub fn element_name(&self, id: ArenaNodeId) -> Option<&LocalName> {
        self.get(id).and_then(|n| match &n.data {
            ArenaNodeData::Element { name, .. } => Some(&name.local),
            _ => None,
        })
    }

    /// Get element's namespace.
    pub fn element_namespace(&self, id: ArenaNodeId) -> Option<&Namespace> {
        self.get(id).and_then(|n| match &n.data {
            ArenaNodeData::Element { name, .. } => Some(&name.ns),
            _ => None,
        })
    }

    /// Get an attribute value.
    pub fn get_attr(&self, id: ArenaNodeId, attr_name: &str) -> Option<&str> {
        self.get(id).and_then(|n| match &n.data {
            ArenaNodeData::Element { attrs, .. } => attrs
                .iter()
                .find(|a| a.name.local.as_ref() == attr_name)
                .map(|a| a.value.as_str()),
            _ => None,
        })
    }

    /// Get element's id attribute.
    pub fn element_id(&self, id: ArenaNodeId) -> Option<&str> {
        self.get(id).and_then(|n| match &n.data {
            ArenaNodeData::Element { id, .. } => id.as_deref(),
            _ => None,
        })
    }

    /// Get element's classes.
    pub fn element_classes(&self, id: ArenaNodeId) -> &[String] {
        static EMPTY: &[String] = &[];
        self.get(id)
            .and_then(|n| match &n.data {
                ArenaNodeData::Element { classes, .. } => Some(classes.as_slice()),
                _ => None,
            })
            .unwrap_or(EMPTY)
    }

    /// Check if node is an element.
    pub fn is_element(&self, id: ArenaNodeId) -> bool {
        self.get(id)
            .is_some_and(|n| matches!(n.data, ArenaNodeData::Element { .. }))
    }

    /// Check if node is a text node.
    pub fn is_text(&self, id: ArenaNodeId) -> bool {
        self.get(id)
            .is_some_and(|n| matches!(n.data, ArenaNodeData::Text(_)))
    }

    /// Get text content of a text node.
    pub fn text_content(&self, id: ArenaNodeId) -> Option<&str> {
        self.get(id).and_then(|n| match &n.data {
            ArenaNodeData::Text(s) => Some(s.as_str()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use html5ever::ns;

    use super::*;

    fn make_qname(local: &str) -> QualName {
        QualName::new(None, ns!(html), LocalName::from(local))
    }

    #[test]
    fn test_create_elements() {
        let mut dom = ArenaDom::new();

        let div = dom.create_element(
            make_qname("div"),
            vec![Attribute {
                name: make_qname("id"),
                value: "main".to_string(),
            }],
        );

        dom.append(dom.document(), div);

        assert_eq!(dom.element_name(div).unwrap().as_ref(), "div");
        assert_eq!(dom.element_id(div), Some("main"));
        assert_eq!(dom.get_by_id("main"), Some(div));
    }

    #[test]
    fn test_append_children() {
        let mut dom = ArenaDom::new();

        let parent = dom.create_element(make_qname("div"), vec![]);
        let child1 = dom.create_element(make_qname("p"), vec![]);
        let child2 = dom.create_element(make_qname("p"), vec![]);

        dom.append(dom.document(), parent);
        dom.append(parent, child1);
        dom.append(parent, child2);

        let children: Vec<_> = dom.children(parent).collect();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0], child1);
        assert_eq!(children[1], child2);
    }

    #[test]
    fn test_text_merging() {
        let mut dom = ArenaDom::new();

        let p = dom.create_element(make_qname("p"), vec![]);
        dom.append(dom.document(), p);

        dom.append_text(p, "Hello, ");
        dom.append_text(p, "World!");

        let children: Vec<_> = dom.children(p).collect();
        assert_eq!(children.len(), 1);
        assert_eq!(dom.text_content(children[0]), Some("Hello, World!"));
    }
}
