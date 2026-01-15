//! HTML parsing and manipulation using html5ever
//!
//! Provides utilities for:
//! - Parsing HTML/XHTML content
//! - Extracting elements (body, head, etc.)
//! - Serializing back to XHTML
//! - Cleaning up and normalizing HTML

use std::collections::HashSet;
use std::default::Default;
use std::rc::Rc;

use html5ever::parse_document;
use html5ever::serialize::{serialize, SerializeOpts, TraversalScope};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{ns, Attribute, ParseOpts, QualName};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};

/// Parse HTML content into a DOM tree
pub fn parse_html(html: &str) -> RcDom {
    let opts = ParseOpts {
        tree_builder: TreeBuilderOpts {
            drop_doctype: false,
            ..Default::default()
        },
        ..Default::default()
    };

    parse_document(RcDom::default(), opts)
        .from_utf8()
        .one(html.as_bytes())
}

/// Parse a fragment of HTML (not a full document)
pub fn parse_fragment(html: &str) -> RcDom {
    // Wrap in a minimal document structure for parsing
    let wrapped = format!(
        "<!DOCTYPE html><html><head></head><body>{}</body></html>",
        html
    );
    parse_html(&wrapped)
}

/// Serialize a DOM tree back to HTML string
pub fn serialize_html(dom: &RcDom) -> String {
    let mut bytes = Vec::new();
    let document: SerializableHandle = dom.document.clone().into();

    serialize(&mut bytes, &document, SerializeOpts::default()).expect("serialization failed");

    String::from_utf8(bytes).unwrap_or_default()
}

/// Serialize a node and its children to HTML string
pub fn serialize_node(handle: &Handle) -> String {
    let mut bytes = Vec::new();
    let serializable: SerializableHandle = handle.clone().into();

    let opts = SerializeOpts {
        traversal_scope: TraversalScope::IncludeNode,
        ..Default::default()
    };

    serialize(&mut bytes, &serializable, opts).expect("serialization failed");

    String::from_utf8(bytes).unwrap_or_default()
}

/// Find elements by local name in a DOM tree
pub fn find_elements_by_name(handle: &Handle, name: &str) -> Vec<Handle> {
    let mut results = Vec::new();
    find_elements_recursive(handle, name, &mut results);
    results
}

fn find_elements_recursive(handle: &Handle, name: &str, results: &mut Vec<Handle>) {
    if let NodeData::Element { name: ref qname, .. } = handle.data {
        if qname.local.as_ref() == name {
            results.push(handle.clone());
        }
    }

    for child in handle.children.borrow().iter() {
        find_elements_recursive(child, name, results);
    }
}

/// Get the first element with the given local name
pub fn find_first_element(handle: &Handle, name: &str) -> Option<Handle> {
    if let NodeData::Element { name: ref qname, .. } = handle.data {
        if qname.local.as_ref() == name {
            return Some(handle.clone());
        }
    }

    for child in handle.children.borrow().iter() {
        if let Some(found) = find_first_element(child, name) {
            return Some(found);
        }
    }

    None
}

/// Extract body content from an HTML document
pub fn extract_body_content(html: &str) -> String {
    let dom = parse_html(html);

    if let Some(body) = find_first_element(&dom.document, "body") {
        let mut content = String::new();
        for child in body.children.borrow().iter() {
            content.push_str(&serialize_node(child));
        }
        content
    } else {
        html.to_string()
    }
}

/// Get text content from a node (ignoring tags)
pub fn get_text_content(handle: &Handle) -> String {
    let mut text = String::new();
    get_text_recursive(handle, &mut text);
    text
}

fn get_text_recursive(handle: &Handle, text: &mut String) {
    match handle.data {
        NodeData::Text { ref contents } => {
            text.push_str(&contents.borrow());
        }
        NodeData::Element { .. } => {
            for child in handle.children.borrow().iter() {
                get_text_recursive(child, text);
            }
        }
        _ => {}
    }
}

/// Get an attribute value from an element
pub fn get_attribute(handle: &Handle, attr_name: &str) -> Option<String> {
    if let NodeData::Element { ref attrs, .. } = handle.data {
        for attr in attrs.borrow().iter() {
            if attr.name.local.as_ref() == attr_name {
                return Some(attr.value.to_string());
            }
        }
    }
    None
}

/// Set an attribute on an element
pub fn set_attribute(handle: &Handle, attr_name: &str, value: &str) {
    if let NodeData::Element { ref attrs, .. } = handle.data {
        let mut attrs_mut = attrs.borrow_mut();

        // Check if attribute exists
        for attr in attrs_mut.iter_mut() {
            if attr.name.local.as_ref() == attr_name {
                attr.value = value.into();
                return;
            }
        }

        // Add new attribute
        attrs_mut.push(Attribute {
            name: QualName::new(None, ns!(), attr_name.into()),
            value: value.into(),
        });
    }
}

/// Remove an attribute from an element
pub fn remove_attribute(handle: &Handle, attr_name: &str) {
    if let NodeData::Element { ref attrs, .. } = handle.data {
        attrs.borrow_mut().retain(|attr| attr.name.local.as_ref() != attr_name);
    }
}

/// Clean HTML by removing unnecessary elements and attributes
pub fn clean_html(html: &str) -> String {
    let dom = parse_html(html);
    clean_dom(&dom.document);
    serialize_html(&dom)
}

fn clean_dom(handle: &Handle) {
    // Remove script and style tags (they'll be empty or external in ebooks)
    let to_remove: Vec<Handle> = handle
        .children
        .borrow()
        .iter()
        .filter(|child| {
            if let NodeData::Element { ref name, .. } = child.data {
                let local = name.local.as_ref();
                local == "script" || local == "noscript"
            } else {
                false
            }
        })
        .cloned()
        .collect();

    // Actually remove the nodes
    for node in to_remove {
        handle.children.borrow_mut().retain(|c| !Rc::ptr_eq(c, &node));
    }

    // Clean attributes on all elements
    if let NodeData::Element { ref attrs, .. } = handle.data {
        let removable_attrs: HashSet<&str> = [
            "onclick",
            "onload",
            "onerror",
            "onmouseover",
            "onmouseout",
            "onfocus",
            "onblur",
            "data-react-checksum",
            "data-reactid",
        ]
        .into_iter()
        .collect();

        attrs.borrow_mut().retain(|attr| {
            let name = attr.name.local.as_ref();
            !removable_attrs.contains(name) && !name.starts_with("data-")
        });
    }

    // Recurse into children
    for child in handle.children.borrow().iter() {
        clean_dom(child);
    }
}

/// Convert HTML to XHTML (self-closing tags, namespace, etc.)
pub fn html_to_xhtml(html: &str) -> String {
    let dom = parse_html(html);

    // Add XHTML namespace to html element
    if let Some(html_elem) = find_first_element(&dom.document, "html") {
        set_attribute(&html_elem, "xmlns", "http://www.w3.org/1999/xhtml");
    }

    // Serialize and fix up
    let mut result = serialize_html(&dom);

    // Add XML declaration if not present
    if !result.starts_with("<?xml") {
        result = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", result);
    }

    result
}

/// Wrap content in a basic XHTML document structure
pub fn wrap_in_xhtml(content: &str, title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
<title>{}</title>
</head>
<body>
{}
</body>
</html>"#,
        escape_xml(title),
        content
    )
}

/// Escape XML special characters
pub fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Find all elements with a specific attribute
pub fn find_elements_with_attribute(handle: &Handle, attr_name: &str) -> Vec<Handle> {
    let mut results = Vec::new();
    find_with_attr_recursive(handle, attr_name, &mut results);
    results
}

fn find_with_attr_recursive(handle: &Handle, attr_name: &str, results: &mut Vec<Handle>) {
    if let NodeData::Element { ref attrs, .. } = handle.data {
        if attrs.borrow().iter().any(|a| a.name.local.as_ref() == attr_name) {
            results.push(handle.clone());
        }
    }

    for child in handle.children.borrow().iter() {
        find_with_attr_recursive(child, attr_name, results);
    }
}

/// Extract all links from an HTML document
pub fn extract_links(html: &str) -> Vec<(String, String)> {
    let dom = parse_html(html);
    let mut links = Vec::new();

    let anchors = find_elements_by_name(&dom.document, "a");
    for anchor in anchors {
        if let Some(href) = get_attribute(&anchor, "href") {
            let text = get_text_content(&anchor);
            links.push((href, text.trim().to_string()));
        }
    }

    links
}

/// Extract all image sources from an HTML document
pub fn extract_images(html: &str) -> Vec<String> {
    let dom = parse_html(html);
    let mut images = Vec::new();

    let img_elements = find_elements_by_name(&dom.document, "img");
    for img in img_elements {
        if let Some(src) = get_attribute(&img, "src") {
            images.push(src);
        }
    }

    images
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_serialize() {
        let html = "<html><head><title>Test</title></head><body><p>Hello</p></body></html>";
        let dom = parse_html(html);
        let output = serialize_html(&dom);
        assert!(output.contains("<p>Hello</p>"));
    }

    #[test]
    fn test_extract_body() {
        let html = "<html><body><p>Content</p></body></html>";
        let body = extract_body_content(html);
        assert!(body.contains("<p>Content</p>"));
        assert!(!body.contains("<html>"));
    }

    #[test]
    fn test_get_text_content() {
        let html = "<p>Hello <strong>World</strong></p>";
        let dom = parse_html(html);
        let p = find_first_element(&dom.document, "p").unwrap();
        let text = get_text_content(&p);
        assert_eq!(text.trim(), "Hello World");
    }

    #[test]
    fn test_extract_links() {
        let html = r#"<html><body><a href="page1.html">Page 1</a><a href="page2.html">Page 2</a></body></html>"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0, "page1.html");
    }

    #[test]
    fn test_clean_html() {
        let html = r#"<html><body><script>alert('x')</script><p onclick="foo()">Text</p></body></html>"#;
        let cleaned = clean_html(html);
        assert!(!cleaned.contains("<script>"));
        assert!(!cleaned.contains("onclick"));
        assert!(cleaned.contains("<p>"));
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("<test>"), "&lt;test&gt;");
        assert_eq!(escape_xml("A & B"), "A &amp; B");
    }
}
