//! html5ever TreeSink implementation for ArenaDom.

use std::cell::RefCell;

use html5ever::tendril::StrTendril;
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute as Html5Attribute, QualName};

use std::rc::Rc;

use super::arena::{ArenaDom, ArenaNodeId, Attribute};

/// Handle used by TreeSink to reference nodes.
#[derive(Debug, Clone)]
pub struct NodeHandle {
    pub id: ArenaNodeId,
    name: Option<Rc<QualName>>,
}

impl PartialEq for NodeHandle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for NodeHandle {}

impl Default for NodeHandle {
    fn default() -> Self {
        NodeHandle {
            id: ArenaNodeId::NONE,
            name: None,
        }
    }
}

/// TreeSink implementation that builds an ArenaDom.
///
/// Uses interior mutability (RefCell) because html5ever's TreeSink trait
/// requires methods to take `&self` but we need to mutate the DOM.
pub struct ArenaSink {
    dom: RefCell<ArenaDom>,
    quirks_mode: RefCell<QuirksMode>,
}

impl Default for ArenaSink {
    fn default() -> Self {
        Self::new()
    }
}

impl ArenaSink {
    pub fn new() -> Self {
        Self {
            dom: RefCell::new(ArenaDom::new()),
            quirks_mode: RefCell::new(QuirksMode::NoQuirks),
        }
    }

    /// Consume the sink and return the DOM.
    pub fn into_dom(self) -> ArenaDom {
        self.dom.into_inner()
    }
}

impl TreeSink for ArenaSink {
    type Handle = NodeHandle;
    type Output = Self;
    type ElemName<'a>
        = &'a QualName
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        self
    }

    fn parse_error(&self, _msg: std::borrow::Cow<'static, str>) {
        // Ignore parse errors - be lenient like browsers
    }

    fn get_document(&self) -> Self::Handle {
        NodeHandle {
            id: self.dom.borrow().document(),
            name: None,
        }
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        static EMPTY: QualName = QualName {
            prefix: None,
            ns: html5ever::ns!(),
            local: html5ever::local_name!(""),
        };

        target.name.as_deref().unwrap_or(&EMPTY)
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Html5Attribute>,
        _flags: ElementFlags,
    ) -> Self::Handle {
        let converted_attrs: Vec<Attribute> = attrs
            .into_iter()
            .map(|a| Attribute {
                name: a.name,
                value: a.value.to_string(),
            })
            .collect();

        let name_rc = Rc::new(name.clone());
        let id = self.dom.borrow_mut().create_element(name, converted_attrs);
        NodeHandle {
            id,
            name: Some(name_rc),
        }
    }

    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        let id = self.dom.borrow_mut().create_comment(text.to_string());
        NodeHandle { id, name: None }
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        // Processing instructions - create as comment
        let id = self.dom.borrow_mut().create_comment(String::new());
        NodeHandle { id, name: None }
    }

    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        let mut dom = self.dom.borrow_mut();
        match child {
            NodeOrText::AppendNode(node) => {
                dom.append(parent.id, node.id);
            }
            NodeOrText::AppendText(text) => {
                dom.append_text(parent.id, &text);
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        // If element has parent, append there; otherwise use prev_element
        let parent = self.dom.borrow().get(element.id).map(|n| n.parent);
        if let Some(parent) = parent
            && parent.is_some()
        {
            let mut dom = self.dom.borrow_mut();
            match child {
                NodeOrText::AppendNode(node) => {
                    dom.append(parent, node.id);
                }
                NodeOrText::AppendText(text) => {
                    dom.append_text(parent, &text);
                }
            }
            return;
        }
        self.append(prev_element, child);
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let mut dom = self.dom.borrow_mut();
        let doc = dom.document();
        let doctype = dom.create_doctype(
            name.to_string(),
            public_id.to_string(),
            system_id.to_string(),
        );
        dom.append(doc, doctype);
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        // For templates, just return the target itself
        // A full implementation would track template contents separately
        target.clone()
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x.id == y.id
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        *self.quirks_mode.borrow_mut() = mode;
    }

    fn append_before_sibling(&self, sibling: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        let mut dom = self.dom.borrow_mut();
        match new_node {
            NodeOrText::AppendNode(node) => {
                dom.insert_before(sibling.id, node.id);
            }
            NodeOrText::AppendText(text) => {
                let text_node = dom.create_text(text.to_string());
                dom.insert_before(sibling.id, text_node);
            }
        }
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Html5Attribute>) {
        let converted: Vec<Attribute> = attrs
            .into_iter()
            .map(|a| Attribute {
                name: a.name,
                value: a.value.to_string(),
            })
            .collect();
        self.dom
            .borrow_mut()
            .add_attrs_if_missing(target.id, converted);
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        let mut dom = self.dom.borrow_mut();

        let (parent, prev, next) = {
            let node = match dom.get(target.id) {
                Some(n) => n,
                None => return,
            };
            (node.parent, node.prev_sibling, node.next_sibling)
        };

        // Update prev sibling's next pointer
        if prev.is_some() {
            if let Some(p) = dom.get_mut(prev) {
                p.next_sibling = next;
            }
        } else if parent.is_some() {
            // Was first child
            if let Some(p) = dom.get_mut(parent) {
                p.first_child = next;
            }
        }

        // Update next sibling's prev pointer
        if next.is_some() {
            if let Some(n) = dom.get_mut(next) {
                n.prev_sibling = prev;
            }
        } else if parent.is_some() {
            // Was last child
            if let Some(p) = dom.get_mut(parent) {
                p.last_child = prev;
            }
        }

        // Clear the removed node's links
        if let Some(target_node) = dom.get_mut(target.id) {
            target_node.parent = ArenaNodeId::NONE;
            target_node.prev_sibling = ArenaNodeId::NONE;
            target_node.next_sibling = ArenaNodeId::NONE;
        }
    }

    fn reparent_children(&self, node: &Self::Handle, new_parent: &Self::Handle) {
        // Collect children first to avoid borrow issues
        let children: Vec<_> = self.dom.borrow().children(node.id).collect();

        {
            let mut dom = self.dom.borrow_mut();
            for child in &children {
                // Remove from old parent
                if let Some(c) = dom.get_mut(*child) {
                    c.parent = ArenaNodeId::NONE;
                    c.prev_sibling = ArenaNodeId::NONE;
                    c.next_sibling = ArenaNodeId::NONE;
                }
            }

            // Clear old parent's children
            if let Some(n) = dom.get_mut(node.id) {
                n.first_child = ArenaNodeId::NONE;
                n.last_child = ArenaNodeId::NONE;
            }
        }

        // Append to new parent
        let mut dom = self.dom.borrow_mut();
        for child in children {
            dom.append(new_parent.id, child);
        }
    }
}

#[cfg(test)]
mod tests {
    use html5ever::driver::ParseOpts;
    use html5ever::parse_document;
    use html5ever::tendril::TendrilSink;

    use super::*;

    fn parse_html(html: &str) -> ArenaDom {
        let sink = ArenaSink::new();
        let result = parse_document(sink, ParseOpts::default())
            .from_utf8()
            .one(html.as_bytes());
        result.into_dom()
    }

    #[test]
    fn test_basic_parse() {
        let dom = parse_html("<html><body><p>Hello</p></body></html>");

        // Should have document + html + head + body + p + text
        assert!(dom.len() > 3);

        // Find the p element
        let p = dom.find_by_tag("p").expect("should find p");
        assert_eq!(dom.element_name(p).unwrap().as_ref(), "p");

        // Check text content
        let text_id = dom.children(p).next().expect("p should have child");
        assert_eq!(dom.text_content(text_id), Some("Hello"));
    }

    #[test]
    fn test_attributes() {
        let dom = parse_html(r#"<div id="main" class="container header">Content</div>"#);

        let div = dom.find_by_tag("div").expect("should find div");
        assert_eq!(dom.element_id(div), Some("main"));

        let classes = dom.element_classes(div);
        assert!(classes.contains(&"container".to_string()));
        assert!(classes.contains(&"header".to_string()));
    }

    #[test]
    fn test_nested_structure() {
        let dom = parse_html(
            r#"
            <div>
                <p>First</p>
                <p>Second</p>
            </div>
        "#,
        );

        let div = dom.find_by_tag("div").expect("should find div");
        let children: Vec<_> = dom.children(div).collect();

        // Should have two p children (whitespace text nodes may also exist)
        let p_children: Vec<_> = children
            .iter()
            .filter(|&&c| dom.element_name(c).is_some_and(|n| n.as_ref() == "p"))
            .collect();
        assert_eq!(p_children.len(), 2);
    }
}
