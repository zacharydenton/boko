//! HTML/XML utility functions for content extraction.
//!
//! Generic utilities for working with HTML elements, paths, and text content.

use crate::css::NodeRef;

/// Check if a tag is a block-level element that should become a Container
pub fn is_block_element(tag: &str) -> bool {
    matches!(
        tag,
        "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "hgroup"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "figure"
            | "figcaption"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "thead"
            | "tbody"
            | "main"
            | "address"
            | "pre"
            | "hr"
    )
}

/// Resolve a relative path against a base directory
/// e.g., resolve_relative_path("epub/text/", "../images/foo.png") -> "epub/images/foo.png"
pub fn resolve_relative_path(base_dir: &str, relative: &str) -> String {
    if !relative.starts_with("../") && !relative.starts_with("./") {
        // Not a relative path, just join
        return format!("{base_dir}{relative}");
    }

    // Split the base directory into components
    let mut components: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();

    let mut rel = relative;

    // Process ../ and ./
    while rel.starts_with("../") || rel.starts_with("./") {
        if rel.starts_with("../") {
            components.pop(); // Go up one directory
            rel = &rel[3..];
        } else if rel.starts_with("./") {
            rel = &rel[2..];
        }
    }

    // Join remaining components with the relative path
    if components.is_empty() {
        rel.to_string()
    } else {
        format!("{}/{}", components.join("/"), rel)
    }
}

/// Clean up text by normalizing whitespace
pub fn clean_text(text: &str) -> String {
    let decoded = decode_html_entities(text);

    // Preserve knowledge of leading/trailing whitespace for proper merging
    let has_leading_space = decoded.chars().next().is_some_and(|c| c.is_whitespace());
    let has_trailing_space = decoded
        .chars()
        .next_back()
        .is_some_and(|c| c.is_whitespace());

    // Normalize internal whitespace (collapse multiple whitespace to single space)
    let mut cleaned = String::new();
    let mut last_was_space = true;

    for ch in decoded.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                cleaned.push(' ');
                last_was_space = true;
            }
        } else {
            cleaned.push(ch);
            last_was_space = false;
        }
    }

    // Trim internal whitespace
    let trimmed = cleaned.trim();

    if trimmed.is_empty() {
        // Text is all whitespace (e.g., HTML source indentation) - return empty
        // Boundary whitespace is handled when there's actual content adjacent to it
        String::new()
    } else {
        // Restore boundary spaces for proper merging with sibling elements
        let mut result = String::new();
        if has_leading_space {
            result.push(' ');
        }
        result.push_str(trimmed);
        if has_trailing_space {
            result.push(' ');
        }
        result
    }
}

/// Decode common HTML entities
pub fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#8217;", "'")
        .replace("&#8220;", "\"")
        .replace("&#8221;", "\"")
        .replace("&#160;", " ")
        .replace("&nbsp;", " ")
}

/// Serialize a node and its children as an XML string.
/// Used for preserving MathML elements as raw XML for Kindle rendering.
pub fn serialize_node_as_xml(node: &NodeRef) -> String {
    use kuchiki::NodeData;

    let mut output = String::new();

    fn serialize_recursive(node: &NodeRef, output: &mut String) {
        match node.data() {
            NodeData::Element(elem) => {
                let name = elem.name.local.as_ref();
                output.push('<');
                output.push_str(name);

                // Add attributes (including xmlns for math element)
                for (key, value) in elem.attributes.borrow().map.iter() {
                    output.push(' ');
                    output.push_str(&key.local);
                    output.push_str("=\"");
                    // Escape attribute values
                    output.push_str(&value.value.replace('"', "&quot;"));
                    output.push('"');
                }

                let children: Vec<_> = node.children().collect();
                if children.is_empty() {
                    output.push_str("/>");
                } else {
                    output.push('>');
                    for child in children {
                        serialize_recursive(&child, output);
                    }
                    output.push_str("</");
                    output.push_str(name);
                    output.push('>');
                }
            }
            NodeData::Text(text) => {
                // Escape XML special characters
                let escaped = text
                    .borrow()
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                output.push_str(&escaped);
            }
            _ => {
                for child in node.children() {
                    serialize_recursive(&child, output);
                }
            }
        }
    }

    serialize_recursive(node, &mut output);
    output
}
