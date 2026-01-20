//! XHTML content extraction for KFX generation.
//!
//! Parses XHTML content and extracts text, images, and structure
//! while preserving CSS styling information.

use kuchiki::traits::*;

use crate::css::{NodeRef, ParsedStyle, Stylesheet};

use super::{ContentItem, StyleRun};

/// Check if a tag is a block-level element that should become a Container
fn is_block_element(tag: &str) -> bool {
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
    )
}

/// Check if a container tag represents a structural wrapper that can be flattened.
/// Structural elements like <section>, <div>, <article> are just grouping wrappers
/// and their children should be promoted to the parent level.
fn is_structural_container(tag: &str) -> bool {
    matches!(tag, "section" | "div" | "article" | "main" | "body")
}

/// Check if a container tag represents a semantic element that should be preserved.
/// Semantic elements like <header>, <footer>, <figure> should never be flattened
/// or unwrapped, even with a single child.
fn is_semantic_container(tag: &str) -> bool {
    matches!(
        tag,
        "header"
            | "footer"
            | "nav"
            | "aside"
            | "figure"
            | "figcaption"
            | "blockquote"
            | "hgroup"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

/// Flatten unnecessary container nesting
/// - Structural containers (section, div, article) are completely flattened - children promoted
/// - Semantic containers (header, footer, figure) are always preserved as containers
/// - Generic containers (p, span) with a single block child are unwrapped (child promoted)
pub fn flatten_containers(items: Vec<ContentItem>) -> Vec<ContentItem> {
    items
        .into_iter()
        .flat_map(|item| {
            match item {
                ContentItem::Container {
                    children,
                    style,
                    tag,
                    element_id,
                } => {
                    // First, recursively flatten children
                    let flattened_children = flatten_containers(children);

                    // Structural containers (section, div, article) are flattened -
                    // their children are promoted to the parent level.
                    // This matches the reference KFX structure where <section> doesn't
                    // create an extra container layer.
                    // IMPORTANT: Preserve element_id by propagating it to the first child
                    // (used for TOC navigation with fragment IDs)
                    if is_structural_container(&tag) {
                        if let Some(id) = element_id {
                            // Propagate element_id to first child that can have it
                            let mut children = flattened_children;
                            if let Some(first) = children.first_mut() {
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
                            return children;
                        }
                        return flattened_children;
                    }

                    // Semantic containers (header, footer, figure) are always preserved,
                    // even with a single child. They represent meaningful structure.
                    if is_semantic_container(&tag) {
                        return vec![ContentItem::Container {
                            children: flattened_children,
                            style,
                            tag,
                            element_id,
                        }];
                    }

                    // For generic containers (p, span, etc.), apply single-child unwrapping

                    // If container has single child that's a Container or Text, unwrap it
                    // (unless the container has meaningful style that would be lost)
                    if flattened_children.len() == 1 {
                        let child = flattened_children.into_iter().next().unwrap();
                        match child {
                            // Single Text child - the container (like <p>) becomes the Text
                            // The style from the container should be on the Text
                            ContentItem::Text {
                                text,
                                inline_runs,
                                anchor_href,
                                style: child_style,
                                element_id: child_element_id,
                            } => {
                                // Merge container's style with child's style
                                let mut merged_style = style;
                                merged_style.merge(&child_style);
                                // Prefer container's element_id, fall back to child's
                                let merged_element_id = element_id.or(child_element_id);
                                return vec![ContentItem::Text {
                                    text,
                                    style: merged_style,
                                    inline_runs,
                                    anchor_href,
                                    element_id: merged_element_id,
                                }];
                            }
                            // Single Container child - flatten if container has default style
                            ContentItem::Container {
                                children: inner_children,
                                style: inner_style,
                                tag: inner_tag,
                                element_id: inner_element_id,
                            } => {
                                // Keep the inner container, but with merged style
                                let mut merged_style = style;
                                merged_style.merge(&inner_style);
                                // Prefer outer element_id, fall back to inner
                                let merged_element_id = element_id.or(inner_element_id);
                                return vec![ContentItem::Container {
                                    children: inner_children,
                                    style: merged_style,
                                    tag: inner_tag,
                                    element_id: merged_element_id,
                                }];
                            }
                            // Single Image child - unwrap, keeping the image
                            other => return vec![other],
                        }
                    }

                    // Multiple children - keep container but with flattened children
                    vec![ContentItem::Container {
                        children: flattened_children,
                        style,
                        tag,
                        element_id,
                    }]
                }
                // Non-containers pass through unchanged
                other => vec![other],
            }
        })
        .collect()
}

/// Merge consecutive Text items into a single Text item with inline style runs
/// This combines text spans that have different inline styles (bold, italic, etc.)
/// into a single paragraph with style runs specifying which ranges have which styles.
/// Anchor hrefs and inline element IDs are tracked in the inline runs.
pub fn merge_text_with_inline_runs(items: Vec<ContentItem>) -> Vec<ContentItem> {
    if items.is_empty() {
        return items;
    }

    let mut result = Vec::new();
    // Track pending texts: (text, style, anchor_href, element_id)
    let mut pending_texts: Vec<(String, ParsedStyle, Option<String>, Option<String>)> = Vec::new();

    // Helper to flush pending text items into a merged item
    fn flush_pending(
        pending: &mut Vec<(String, ParsedStyle, Option<String>, Option<String>)>,
        result: &mut Vec<ContentItem>,
    ) {
        if pending.is_empty() {
            return;
        }

        if pending.len() == 1 && pending[0].2.is_none() && pending[0].3.is_none() {
            // Single text item with no anchor and no element_id, no inline runs needed
            let (text, style, _, _) = pending.remove(0);
            result.push(ContentItem::Text {
                text,
                style,
                inline_runs: Vec::new(),
                anchor_href: None,
                element_id: None, // Text merged from inline elements doesn't have its own ID
            });
        } else {
            // Multiple text items OR has anchors/element_ids - merge with inline style runs
            // Find the most common style to use as base (or use first item's style)
            let base_style = pending[0].1.clone();

            // Build combined text and inline runs
            let mut combined_text = String::new();
            let mut inline_runs = Vec::new();

            for (text, style, anchor_href, element_id) in pending.drain(..) {
                let offset = combined_text.chars().count();
                let length = text.chars().count();

                // Determine if we need an inline run
                let style_differs = style != base_style;
                let has_anchor = anchor_href.is_some();
                let has_element_id = element_id.is_some();

                if style_differs || has_anchor || has_element_id {
                    // Determine the style for this run:
                    // - If style differs from base (bold, italic, etc.), use the full style
                    // - If only anchor/element_id differs (plain link), use a minimal inline style
                    let run_style = if !style_differs && (has_anchor || has_element_id) {
                        // Anchor-only run: create minimal inline style
                        // This matches reference behavior where links use $127: $349 only
                        ParsedStyle {
                            is_inline: true,
                            ..Default::default()
                        }
                    } else {
                        // Style differs: use the actual style
                        style
                    };

                    inline_runs.push(StyleRun {
                        offset,
                        length,
                        style: run_style,
                        anchor_href,
                        element_id,
                    });
                }

                combined_text.push_str(&text);
            }

            result.push(ContentItem::Text {
                text: combined_text,
                style: base_style,
                inline_runs,
                anchor_href: None, // Anchors are now in inline_runs
                element_id: None,  // Merged text doesn't have element ID
            });
        }
    }

    for item in items {
        match item {
            ContentItem::Text {
                text,
                style,
                anchor_href,
                element_id,
                ..
            } => {
                // Accumulate text with style, anchor, and element_id for inline anchor targets
                pending_texts.push((text, style, anchor_href, element_id));
            }
            other => {
                // Non-text item: flush any pending texts first
                flush_pending(&mut pending_texts, &mut result);
                result.push(other);
            }
        }
    }

    // Flush any remaining pending texts
    flush_pending(&mut pending_texts, &mut result);

    result
}

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

    let items = extract_from_node(&body, stylesheet, &ParsedStyle::default(), base_dir, None);
    // Flatten unnecessary container nesting (section wrappers, paragraph wrappers, etc.)
    flatten_containers(items)
}

/// Extract content from a node, preserving hierarchy for block elements
/// Returns the extracted content items for this node and its descendants
fn extract_from_node(
    node: &NodeRef,
    stylesheet: &Stylesheet,
    parent_style: &ParsedStyle,
    base_dir: &str,
    anchor_href: Option<&str>, // Current anchor href context (from parent <a>)
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

            // Skip hidden elements (display:none, position:absolute with large negative offset)
            if computed_style.is_hidden() {
                return vec![];
            }

            // Extract element ID for anchor targets (used in TOC navigation)
            let element_id = element.attributes.borrow().get("id").map(|s| s.to_string());

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

            // Determine anchor_href for children:
            // If this is an <a> element, extract its href; otherwise pass through parent's
            let child_anchor_href = if tag_name == "a" {
                element.attributes.borrow().get("href").map(|href| {
                    // Resolve relative href to full path (matches section_eids keys)
                    // External URLs (http/https) are kept as-is
                    if href.starts_with("http://") || href.starts_with("https://") {
                        href.to_string()
                    } else {
                        resolve_relative_path(base_dir, href)
                    }
                })
            } else {
                anchor_href.map(|s| s.to_string())
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
                ));
            }

            // Block elements become Containers with their children nested
            if is_block_element(tag_name) && !children.is_empty() {
                // Merge consecutive text items with inline style runs
                let merged_children = merge_text_with_inline_runs(children);
                return vec![ContentItem::Container {
                    style: direct_with_inline,
                    children: merged_children,
                    tag: tag_name.to_string(),
                    element_id,
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
                ));
            }
            children
        }
    }
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
fn clean_text(text: &str) -> String {
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
fn decode_html_entities(text: &str) -> String {
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
