//! XHTML content extraction for KFX generation.
//!
//! Parses XHTML content and extracts text, images, and structure
//! while preserving CSS styling information.

use kuchiki::traits::*;

use crate::css::{NodeRef, ParsedStyle, Stylesheet};
use crate::kfx::writer::symbols::sym;

use super::html_utils::{clean_text, is_block_element, resolve_relative_path, serialize_node_as_xml};
use super::merging::{flatten_containers, merge_text_with_inline_runs};
use super::{ContentItem, ListType};

/// Extract CSS stylesheet hrefs from XHTML <link> tags in document order
/// `base_path` is the path of the XHTML file, used to resolve relative CSS paths
pub fn extract_css_hrefs_from_xhtml(data: &[u8], base_path: &str) -> Vec<String> {
    let html = String::from_utf8_lossy(data);
    let document = kuchiki::parse_html().one(html.as_ref());

    // Get the directory part of the base path for resolving relative paths
    let base_dir = if let Some(pos) = base_path.rfind('/') {
        &base_path[..pos + 1]
    } else {
        ""
    };

    let mut css_hrefs = Vec::new();

    // Find all <link> elements with rel="stylesheet"
    for link in document.select("link").unwrap() {
        let node = link.as_node();
        if let Some(element) = node.as_element() {
            let attrs = element.attributes.borrow();
            // Check if this is a stylesheet link
            if attrs.get("rel").is_some_and(|r| r.contains("stylesheet"))
                && let Some(href) = attrs.get("href")
            {
                // Resolve relative path to absolute path within EPUB
                let resolved = resolve_relative_path(base_dir, href);
                css_hrefs.push(resolved);
            }
        }
    }

    css_hrefs
}

/// Extract content items (text and images) from XHTML, preserving styles and hierarchy
/// `base_path` is the path of the XHTML file within the EPUB, used to resolve relative paths
pub fn extract_content_from_xhtml(
    data: &[u8],
    stylesheet: &Stylesheet,
    base_path: &str,
) -> Vec<ContentItem> {
    let html = String::from_utf8_lossy(data);

    // Get the directory part of the base path for resolving relative paths
    let base_dir = if let Some(pos) = base_path.rfind('/') {
        &base_path[..pos + 1]
    } else {
        ""
    };

    // Parse HTML with kuchiki for proper DOM-based CSS selector matching
    let document = kuchiki::parse_html().one(html.as_ref());

    // Find the body element (or root if no body)
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap_or_else(|| document.clone());

    let items = extract_from_node(
        &body,
        stylesheet,
        &ParsedStyle::default(),
        base_dir,
        None,
        false, // is_verse
        false, // is_noteref
    );
    // Flatten unnecessary container nesting (section wrappers, paragraph wrappers, etc.)
    flatten_containers(items)
}

/// Extract content from a node, preserving hierarchy for block elements
/// Returns the extracted content items for this node and its descendants
pub(crate) fn extract_from_node(
    node: &NodeRef,
    stylesheet: &Stylesheet,
    parent_style: &ParsedStyle,
    base_dir: &str,
    anchor_href: Option<&str>, // Current anchor href context (from parent <a>)
    is_verse: bool,            // Whether we're inside a z3998:verse block
    is_noteref: bool,          // Whether we're inside a noteref link (for popup footnotes)
) -> Vec<ContentItem> {
    use kuchiki::NodeData;

    match node.data() {
        NodeData::Element(element) => {
            let tag_name = element.name.local.as_ref();

            // Skip non-content tags
            if matches!(tag_name, "script" | "style" | "head" | "title" | "svg") {
                return vec![];
            }

            // Get direct style (only rules matching this element, no CSS inheritance)
            // KFX has its own style inheritance, so we only output direct styles
            let element_ref = node.clone().into_element_ref().unwrap();
            let direct_style = stylesheet.get_direct_style_for_element(&element_ref);

            // Also compute full style for hidden element detection and DOM traversal
            let mut computed_style = parent_style.clone();
            computed_style.merge(&direct_style);

            // Apply inline style (highest specificity) to both
            let mut direct_with_inline = direct_style.clone();
            if let Some(style_attr) = element.attributes.borrow().get("style") {
                let inline = Stylesheet::parse_inline_style(style_attr);
                direct_with_inline.merge(&inline);
                computed_style.merge(&inline);
            }

            // Mark heading elements (h1-h6) for layout hints
            if matches!(tag_name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                direct_with_inline.is_heading = true;
            }
            // Mark figure elements for layout hints
            if tag_name == "figure" {
                direct_with_inline.is_figure = true;
            }
            // Mark figcaption elements for layout hints
            if tag_name == "figcaption" {
                direct_with_inline.is_caption = true;
            }

            // Skip hidden elements (display: none) - handles epub/mobi conditional content
            // Exception: BR elements are structural (line breaks) and should never be skipped
            // even if CSS sets display:none (common in Standard Ebooks verse styling)
            if computed_style.is_hidden() && tag_name != "br" {
                return vec![];
            }

            // Extract element ID for anchor targets (used in TOC navigation)
            let element_id = element.attributes.borrow().get("id").map(|s| s.to_string());

            // Extract lang attribute and merge into styles
            // Language cascades to children via computed_style
            if let Some(lang) = element.attributes.borrow().get("lang") {
                let lang = lang.to_string();
                direct_with_inline.lang = Some(lang.clone());
                computed_style.lang = Some(lang);
            }

            // Handle image elements specially
            if tag_name == "img" {
                let attrs = element.attributes.borrow();
                if let Some(src) = attrs.get("src") {
                    // Resolve relative path to absolute path within EPUB
                    let resolved_path = resolve_relative_path(base_dir, src);
                    // Use direct style (not computed) - KFX handles inheritance
                    let mut image_style = direct_with_inline.clone();
                    image_style.is_image = true;
                    // Extract alt text for accessibility ($584)
                    let alt = attrs.get("alt").map(|s| s.to_string());
                    return vec![ContentItem::Image {
                        resource_href: resolved_path,
                        style: image_style,
                        alt,
                    }];
                }
                return vec![]; // img is self-closing, no children to process
            }

            // Handle <br> elements - create line break for poetry/verse
            // BR inherits verse context from parent - only creates paragraph break if in verse
            // In non-verse contexts (like colophon), BR creates a soft line break within the paragraph
            if tag_name == "br" {
                return vec![ContentItem::Text {
                    text: "\n".to_string(),
                    style: parent_style.clone(),
                    inline_runs: Vec::new(),
                    anchor_href: anchor_href.map(|s| s.to_string()),
                    element_id: None,
                    is_verse, // Inherit verse context - only splits in verse blocks
                    is_noteref,
                }];
            }

            // Handle <math> elements - serialize as raw XML string for Kindle MathML rendering
            if tag_name == "math" {
                let xml_string = serialize_node_as_xml(node);
                return vec![ContentItem::Text {
                    text: xml_string,
                    style: parent_style.clone(),
                    inline_runs: Vec::new(),
                    anchor_href: None,
                    element_id,
                    is_verse: false,
                    is_noteref: false, // Math elements aren't noterefs
                }];
            }

            // Handle <hr> elements - horizontal rule (self-closing, no children)
            // Creates an empty container with tag "hr" for KFX content type $596
            if tag_name == "hr" {
                return vec![ContentItem::Container {
                    style: direct_with_inline,
                    children: Vec::new(),
                    tag: "hr".to_string(),
                    element_id,
                    list_type: None,
                    colspan: None,
                    rowspan: None,
                    classification: None,
                }];
            }

            // Check if this element starts a verse block (z3998:verse)
            let child_is_verse = is_verse || {
                let attrs = element.attributes.borrow();
                attrs
                    .get("epub:type")
                    .map(|t| t.contains("z3998:verse"))
                    .unwrap_or(false)
            };

            // Determine anchor_href for children:
            // If this is an <a> element, extract its href; otherwise pass through parent's
            let (child_anchor_href, child_is_noteref) = if tag_name == "a" {
                let attrs = element.attributes.borrow();
                let href = attrs.get("href").map(|href| {
                    // Resolve relative href to full path (matches section_eids keys)
                    // External URLs (http/https/mailto) are kept as-is
                    if href.starts_with("http://")
                        || href.starts_with("https://")
                        || href.starts_with("mailto:")
                    {
                        href.to_string()
                    } else {
                        resolve_relative_path(base_dir, href)
                    }
                });
                // Check if this link is a noteref (triggers popup footnotes)
                let noteref = attrs
                    .get("epub:type")
                    .map(|t| t.contains("noteref"))
                    .unwrap_or(false)
                    || attrs
                        .get("role")
                        .map(|r| r == "doc-noteref")
                        .unwrap_or(false);
                (href, noteref || is_noteref)
            } else {
                (anchor_href.map(|s| s.to_string()), is_noteref)
            };

            // Extract children with anchor context
            let mut children = Vec::new();
            for child in node.children() {
                children.extend(extract_from_node(
                    &child,
                    stylesheet,
                    &computed_style,
                    base_dir,
                    child_anchor_href.as_deref(),
                    child_is_verse,
                    child_is_noteref,
                ));
            }

            // Block elements become Containers with their children nested
            if is_block_element(tag_name) && !children.is_empty() {
                // Merge consecutive text items with inline style runs
                let merged_children = merge_text_with_inline_runs(children);

                // Detect list type for ol/ul elements
                let list_type = match tag_name {
                    "ol" => Some(ListType::Ordered),
                    "ul" => Some(ListType::Unordered),
                    _ => None,
                };

                // Extract colspan/rowspan for table cells (td/th)
                let (colspan, rowspan) = if tag_name == "td" || tag_name == "th" {
                    let attrs = element.attributes.borrow();
                    let colspan = attrs.get("colspan").and_then(|v| v.parse::<u32>().ok());
                    let rowspan = attrs.get("rowspan").and_then(|v| v.parse::<u32>().ok());
                    (colspan, rowspan)
                } else {
                    (None, None)
                };

                // Detect footnote/endnote classification for popup support
                // Note: epub:type can have multiple space-separated values (e.g., "endnote footnote")
                let classification = {
                    let attrs = element.attributes.borrow();
                    // Check epub:type attribute (may contain multiple space-separated values)
                    let epub_type = attrs.get("epub:type").map(|s| s.to_lowercase());
                    // Check role attribute (ARIA)
                    let role = attrs.get("role").map(|s| s.to_lowercase());

                    // Check for endnote first since "endnote footnote" should classify as endnote
                    if epub_type.as_ref().map(|t| t.contains("endnote")).unwrap_or(false)
                        || role.as_deref() == Some("doc-endnote")
                    {
                        Some(sym::ENDNOTE)
                    } else if epub_type.as_ref().map(|t| t.contains("footnote")).unwrap_or(false)
                        || role.as_deref() == Some("doc-footnote")
                    {
                        Some(sym::FOOTNOTE)
                    } else {
                        None
                    }
                };

                return vec![ContentItem::Container {
                    style: direct_with_inline,
                    children: merged_children,
                    tag: tag_name.to_string(),
                    element_id,
                    list_type,
                    colspan,
                    rowspan,
                    classification,
                }];
            }

            // Non-block elements (span, a, em, strong, etc.) pass through children
            // IMPORTANT: Propagate element_id to first child if this inline element has an ID
            // This handles cases like <a id="noteref-1">2</a> where the anchor tag has an ID
            // that needs to be preserved for back-links
            if let Some(id) = element_id
                && let Some(first) = children.first_mut()
            {
                match first {
                    ContentItem::Text {
                        element_id: child_id,
                        ..
                    } => {
                        if child_id.is_none() {
                            *child_id = Some(id);
                        }
                    }
                    ContentItem::Container {
                        element_id: child_id,
                        ..
                    } => {
                        if child_id.is_none() {
                            *child_id = Some(id);
                        }
                    }
                    _ => {}
                }
            }
            children
        }
        NodeData::Text(text) => {
            let text_content = text.borrow();
            let cleaned = clean_text(&text_content);
            if !cleaned.is_empty() {
                vec![ContentItem::Text {
                    text: cleaned,
                    style: parent_style.clone(),
                    inline_runs: Vec::new(),
                    anchor_href: anchor_href.map(|s| s.to_string()),
                    element_id: None, // Text nodes don't have IDs (parent block does)
                    is_verse,
                    is_noteref,
                }]
            } else {
                vec![]
            }
        }
        _ => {
            // Process children for document/doctype/etc nodes
            let mut children = Vec::new();
            for child in node.children() {
                children.extend(extract_from_node(
                    &child,
                    stylesheet,
                    parent_style,
                    base_dir,
                    anchor_href,
                    is_verse,
                    is_noteref,
                ));
            }
            children
        }
    }
}

#[cfg(test)]
mod tests;
