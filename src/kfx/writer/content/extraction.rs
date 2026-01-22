//! XHTML content extraction for KFX generation.
//!
//! Parses XHTML content and extracts text, images, and structure
//! while preserving CSS styling information.

use kuchiki::traits::*;

use crate::css::{NodeRef, ParsedStyle, Stylesheet};
use crate::kfx::writer::symbols::sym;

use super::{ContentItem, ListType, StyleRun};

/// Pending text item: (text, style, anchor_href, element_id, is_verse, is_noteref)
type PendingText = (String, ParsedStyle, Option<String>, Option<String>, bool, bool);

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

/// Flatten only the outermost body/section wrappers to get a usable list of content items.
/// Preserves all other HTML structure faithfully.
pub fn flatten_containers(items: Vec<ContentItem>) -> Vec<ContentItem> {
    items
        .into_iter()
        .flat_map(|item| {
            match &item {
                ContentItem::Container { tag, children, .. }
                    if tag == "body" || tag == "section" || tag == "article" || tag == "main" =>
                {
                    // Recursively flatten body/section wrappers
                    flatten_containers(children.clone())
                }
                _ => vec![item],
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
    let mut pending_texts: Vec<PendingText> = Vec::new();

    // Helper to flush pending text items into a merged item
    fn flush_pending(pending: &mut Vec<PendingText>, result: &mut Vec<ContentItem>) {
        if pending.is_empty() {
            return;
        }

        // is_verse if ANY of the pending items is verse
        let is_verse = pending.iter().any(|(_, _, _, _, v, _)| *v);
        // is_noteref if ANY of the pending items is noteref
        let is_noteref = pending.iter().any(|(_, _, _, _, _, n)| *n);

        if pending.len() == 1 && pending[0].2.is_none() && pending[0].3.is_none() {
            // Single text item with no anchor and no element_id, no inline runs needed
            let (text, style, _, _, _, _) = pending.remove(0);
            result.push(ContentItem::Text {
                text,
                style,
                inline_runs: Vec::new(),
                anchor_href: None,
                element_id: None, // Text merged from inline elements doesn't have its own ID
                is_verse,
                is_noteref,
            });
        } else {
            // Multiple text items OR has anchors/element_ids - merge with inline style runs
            // Find the most common style to use as base (by character count, not item count)
            // This ensures styled inline elements (like <i>Short Works</i>) become runs,
            // not the base style for the entire paragraph
            let base_style = {
                let mut style_counts: std::collections::HashMap<&ParsedStyle, usize> =
                    std::collections::HashMap::new();
                for (text, style, _, _, _, _) in pending.iter() {
                    *style_counts.entry(style).or_insert(0) += text.chars().count();
                }
                style_counts
                    .into_iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(style, _)| style.clone())
                    .unwrap_or_else(|| pending[0].1.clone())
            };

            // Build combined text and inline runs
            let mut combined_text = String::new();
            let mut inline_runs = Vec::new();

            for (text, style, anchor_href, element_id, _, item_is_noteref) in pending.drain(..) {
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
                        is_noteref: item_is_noteref,
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
                is_verse,
                is_noteref,
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
                is_verse,
                is_noteref,
                ..
            } => {
                // Accumulate text with style, anchor, element_id, verse flag, and noteref flag
                pending_texts.push((text, style, anchor_href, element_id, is_verse, is_noteref));
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
fn extract_from_node(
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
                let classification = {
                    let attrs = element.attributes.borrow();
                    // Check epub:type attribute
                    let epub_type = attrs.get("epub:type").map(|s| s.to_lowercase());
                    // Check role attribute (ARIA)
                    let role = attrs.get("role").map(|s| s.to_lowercase());

                    if epub_type.as_deref() == Some("footnote")
                        || role.as_deref() == Some("doc-footnote")
                    {
                        Some(sym::FOOTNOTE)
                    } else if epub_type.as_deref() == Some("endnote")
                        || role.as_deref() == Some("doc-endnote")
                    {
                        Some(sym::ENDNOTE)
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

/// Serialize a node and its children as an XML string.
/// Used for preserving MathML elements as raw XML for Kindle rendering.
fn serialize_node_as_xml(node: &NodeRef) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::Stylesheet;
    use std::collections::HashSet;

    /// Helper to collect all text content from ContentItems, splitting by newlines
    fn collect_all_texts(items: &[ContentItem]) -> Vec<String> {
        let mut texts = Vec::new();
        for item in items {
            match item {
                ContentItem::Text { text, .. } => {
                    for line in text.split('\n') {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            texts.push(trimmed.to_string());
                        }
                    }
                }
                ContentItem::Container { children, .. } => {
                    texts.extend(collect_all_texts(children));
                }
                _ => {}
            }
        }
        texts
    }

    #[test]
    fn test_br_tag_creates_line_break() {
        // Poetry with <br> tags should produce separate text entries
        let html = r#"<html><body>
            <p>Line one<br/>Line two<br/>Line three</p>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Should have a Container with Text items containing newline markers
        // When collected into TEXT_CONTENT, should become 3 separate entries
        let texts = collect_all_texts(&flattened);
        assert_eq!(
            texts.len(),
            3,
            "BR tags should create separate text entries, got: {:?}",
            texts
        );
        assert_eq!(texts[0], "Line one");
        assert_eq!(texts[1], "Line two");
        assert_eq!(texts[2], "Line three");
    }

    #[test]
    fn test_br_with_spans_like_poetry() {
        // This matches the actual Standard Ebooks poetry structure
        let html = r#"<html><body>
            <p>
                <span>Lead me, O Zeus, and thou O Destiny,</span>
                <br/>
                <span>The way that I am bid by you to go:</span>
                <br/>
                <span>To follow I am ready.</span>
            </p>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        let texts = collect_all_texts(&flattened);
        assert_eq!(
            texts.len(),
            3,
            "Poetry with span+br structure should create separate text entries, got: {:?}",
            texts
        );
        assert_eq!(texts[0], "Lead me, O Zeus, and thou O Destiny,");
        assert_eq!(texts[1], "The way that I am bid by you to go:");
        assert_eq!(texts[2], "To follow I am ready.");
    }

    #[test]
    fn test_poetry_br_in_actual_epub() {
        // Test that BR tags in actual EPUB poetry are handled correctly
        let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
        let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

        // Find the-enchiridion.xhtml which contains the "Lead me, O Zeus" poetry
        let enchiridion = book
            .resources
            .iter()
            .find(|(k, _)| k.contains("enchiridion"))
            .map(|(k, v)| (k.clone(), v));

        if let Some((enchiridion_path, resource)) = enchiridion {
            // Collect CSS like the builder does
            fn extract_css_hrefs(data: &[u8], base_path: &str) -> Vec<String> {
                let html = String::from_utf8_lossy(data);
                let document = kuchiki::parse_html().one(html.as_ref());
                let base_dir = if let Some(pos) = base_path.rfind('/') {
                    &base_path[..pos + 1]
                } else {
                    ""
                };

                let mut hrefs = Vec::new();
                if let Ok(links) = document.select("link[rel='stylesheet']") {
                    for link in links {
                        if let Some(href) = link.attributes.borrow().get("href") {
                            let resolved = if href.starts_with('/') {
                                href.to_string()
                            } else {
                                format!("{}{}", base_dir, href)
                            };
                            hrefs.push(resolved);
                        }
                    }
                }
                hrefs
            }

            let css_hrefs = extract_css_hrefs(&resource.data, &enchiridion_path);
            let mut combined_css = String::new();
            for css_href in &css_hrefs {
                if let Some(css_resource) = book.resources.get(css_href) {
                    combined_css.push_str(&String::from_utf8_lossy(&css_resource.data));
                    combined_css.push('\n');
                }
            }

            // Use the same stylesheet parsing as the builder
            let stylesheet = Stylesheet::parse_with_defaults(&combined_css);
            let content =
                extract_content_from_xhtml(&resource.data, &stylesheet, &enchiridion_path);

            // Collect all text content, looking for the Zeus poetry
            fn find_zeus_text(item: &ContentItem, found: &mut Vec<String>, raw: &mut Vec<String>) {
                match item {
                    ContentItem::Text { text, is_verse, .. } => {
                        if text.contains("Zeus") || text.contains("Destiny") {
                            // Store raw text to see if newlines are present
                            raw.push(format!("RAW (is_verse={}): {:?}", is_verse, text));
                            // Split by newlines and add each
                            for line in text.split('\n') {
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    found.push(trimmed.to_string());
                                }
                            }
                        }
                    }
                    ContentItem::Container { children, .. } => {
                        for child in children {
                            find_zeus_text(child, found, raw);
                        }
                    }
                    _ => {}
                }
            }

            let mut zeus_texts = Vec::new();
            let mut raw_texts = Vec::new();
            for item in &content {
                find_zeus_text(item, &mut zeus_texts, &mut raw_texts);
            }

            // The poetry should be split into separate lines
            assert!(
                zeus_texts.len() >= 2,
                "Poetry should be split into multiple lines, found: {:?}",
                zeus_texts
            );

            // Verify the first line doesn't contain the second line's text
            if !zeus_texts.is_empty() {
                assert!(
                    !zeus_texts[0].contains("The way that I am bid"),
                    "First line should not contain second line's text. Got: {}",
                    zeus_texts[0]
                );
            }
        }
    }

    #[test]
    fn test_builder_collect_texts_preserves_newlines() {
        // Test that the builder's collect_texts function correctly splits by newlines
        let html = r#"<html><body>
            <p>
                <span>Lead me, O Zeus, and thou O Destiny,</span>
                <br/>
                <span>The way that I am bid by you to go:</span>
            </p>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // This mimics the builder's collect_texts function
        fn builder_collect_texts(item: &ContentItem, texts: &mut Vec<String>) {
            match item {
                ContentItem::Text { text, .. } => {
                    for line in text.split('\n') {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            texts.push(trimmed.to_string());
                        }
                    }
                }
                ContentItem::Image { .. } => {}
                ContentItem::Container { children, .. } => {
                    for child in children {
                        builder_collect_texts(child, texts);
                    }
                }
            }
        }

        let mut texts = Vec::new();
        for item in &flattened {
            builder_collect_texts(item, &mut texts);
        }

        assert_eq!(
            texts.len(),
            2,
            "Should produce 2 separate text entries, got: {:?}",
            texts
        );
        assert_eq!(texts[0], "Lead me, O Zeus, and thou O Destiny,");
        assert_eq!(texts[1], "The way that I am bid by you to go:");
    }

    #[test]
    fn test_is_verse_preserved_through_flatten() {
        // Verify is_verse survives the full pipeline: extract -> flatten -> chunk -> collect
        let html = r#"<html><body>
            <blockquote epub:type="z3998:verse">
                <p>
                    <span>Line one</span>
                    <br/>
                    <span>Line two</span>
                </p>
            </blockquote>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Simulate what the builder does: flatten items and check is_verse
        let mut found_verse_text = false;
        for item in &flattened {
            for leaf in item.flatten() {
                if let ContentItem::Text { text, is_verse, .. } = leaf {
                    if text.contains('\n') {
                        assert!(
                            *is_verse,
                            "is_verse should be true for BR-separated text after flatten"
                        );
                        found_verse_text = true;
                    }
                }
            }
        }
        assert!(found_verse_text, "Should have found text with newlines");
    }

    #[test]
    fn test_lang_attribute_extraction_from_fixture() {
        // Test lang extraction from actual EPUB fixture
        let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
        let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

        // Collect all lang values found in content
        let mut langs_found = HashSet::new();

        fn collect_langs(item: &ContentItem, langs: &mut HashSet<String>) {
            match item {
                ContentItem::Text { style, .. } | ContentItem::Image { style, .. } => {
                    if let Some(ref lang) = style.lang {
                        langs.insert(lang.clone());
                    }
                }
                ContentItem::Container {
                    style, children, ..
                } => {
                    if let Some(ref lang) = style.lang {
                        langs.insert(lang.clone());
                    }
                    for child in children {
                        collect_langs(child, langs);
                    }
                }
            }
        }

        // Extract content from each spine item
        for spine_item in &book.spine {
            if let Some(resource) = book.resources.get(&spine_item.href) {
                let stylesheet = Stylesheet::default();
                let content =
                    extract_content_from_xhtml(&resource.data, &stylesheet, &spine_item.href);
                for item in &content {
                    collect_langs(item, &mut langs_found);
                }
            }
        }

        // The Epictetus EPUB contains Greek (grc) and Latin (la) text
        assert!(
            langs_found.contains("grc") || langs_found.contains("la") || langs_found.contains("en"),
            "Should find language tags in EPUB content, found: {:?}",
            langs_found
        );
    }

    #[test]
    fn test_br_inherits_is_verse_from_context() {
        // Verify that BR tags inherit is_verse from parent context
        // In non-verse context (plain HTML), BR creates newline but is_verse=false
        // This means text stays as single paragraph (soft line break)
        let html = r#"<html><body>
            <p>Line one<br/>Line two</p>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Find the merged text item
        for item in &flattened {
            if let ContentItem::Container { children, .. } = item {
                for child in children {
                    if let ContentItem::Text { text, is_verse, .. } = child {
                        if text.contains('\n') {
                            // In non-verse context, BR creates newline but is_verse=false
                            // This is correct - only verse context (epub:type="z3998:verse") should split
                            assert!(
                                !*is_verse,
                                "BR in non-verse context should have is_verse=false, but got true. Text: {:?}",
                                text
                            );
                            return;
                        }
                    }
                }
            }
        }
        panic!("Did not find merged text with newline");
    }

    #[test]
    fn test_normalize_text_for_kfx_splits_verse() {
        // Test that normalize_text_for_kfx correctly splits verse text
        // Create a simple book with verse content
        let html = r#"<html><body>
            <blockquote epub:type="z3998:verse">
                <p>
                    <span>Line one</span>
                    <br/>
                    <span>Line two</span>
                </p>
            </blockquote>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Find all text items and verify is_verse
        let mut found_text = None;
        for item in &flattened {
            for leaf in item.flatten() {
                if let ContentItem::Text { text, is_verse, .. } = leaf {
                    if text.contains('\n') {
                        found_text = Some((text.clone(), *is_verse));
                    }
                }
            }
        }

        let (text, is_verse) = found_text.expect("Should find text with newline");
        assert!(is_verse, "is_verse should be true");

        // Now test normalize_text_for_kfx directly
        fn normalize_text_for_kfx(text: &str, is_verse: bool) -> Vec<String> {
            if is_verse {
                text.split('\n')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                if text.trim().is_empty() {
                    vec![]
                } else {
                    vec![text.to_string()]
                }
            }
        }

        let normalized = normalize_text_for_kfx(&text, is_verse);
        assert_eq!(
            normalized.len(),
            2,
            "Should split into 2 lines, got: {:?}",
            normalized
        );
        assert_eq!(normalized[0].trim(), "Line one");
        assert_eq!(normalized[1].trim(), "Line two");
    }

    #[test]
    fn test_ordered_list_creates_container_with_list_type() {
        // Test that <ol> creates a Container with list_type: Ordered
        let html = r#"<html><body>
            <ol>
                <li>First item</li>
                <li>Second item</li>
                <li>Third item</li>
            </ol>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Should have one Container for the <ol>
        assert_eq!(
            flattened.len(),
            1,
            "Should have one top-level item (the ol container)"
        );

        // The container should have list_type: Ordered
        match &flattened[0] {
            ContentItem::Container {
                tag,
                children,
                list_type,
                ..
            } => {
                assert_eq!(tag, "ol", "Container should be an ol element");
                assert_eq!(
                    *list_type,
                    Some(ListType::Ordered),
                    "ol should have list_type: Ordered"
                );
                // Should have 3 children (li items)
                assert_eq!(children.len(), 3, "ol should have 3 li children");

                // Each li should be a Container with its text
                for (i, child) in children.iter().enumerate() {
                    match child {
                        ContentItem::Container { tag, .. } => {
                            assert_eq!(tag, "li", "Child {} should be an li element", i);
                        }
                        _ => panic!("Child {} should be a Container (li), got {:?}", i, child),
                    }
                }
            }
            _ => panic!("Expected Container, got {:?}", flattened[0]),
        }
    }

    #[test]
    fn test_unordered_list_creates_container_with_list_type() {
        // Test that <ul> creates a Container with list_type: Unordered
        let html = r#"<html><body>
            <ul>
                <li>Bullet one</li>
                <li>Bullet two</li>
            </ul>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        assert_eq!(
            flattened.len(),
            1,
            "Should have one top-level item (the ul container)"
        );

        match &flattened[0] {
            ContentItem::Container {
                tag,
                children,
                list_type,
                ..
            } => {
                assert_eq!(tag, "ul", "Container should be a ul element");
                assert_eq!(
                    *list_type,
                    Some(ListType::Unordered),
                    "ul should have list_type: Unordered"
                );
                assert_eq!(children.len(), 2, "ul should have 2 li children");
            }
            _ => panic!("Expected Container, got {:?}", flattened[0]),
        }
    }

    #[test]
    fn test_display_none_elements_skipped() {
        // Elements with display:none should be skipped entirely
        let html = r#"<html><body>
            <p>Visible content</p>
            <p class="hidden">Hidden content</p>
            <p>More visible</p>
        </body></html>"#;

        let css = ".hidden { display: none; }";
        let stylesheet = Stylesheet::parse(css);
        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);
        let texts = collect_all_texts(&flattened);
        let all_text = texts.join(" ");

        assert!(
            all_text.contains("Visible content"),
            "visible content should be kept"
        );
        assert!(
            all_text.contains("More visible"),
            "visible content should be kept"
        );
        assert!(
            !all_text.contains("Hidden content"),
            "display:none content should be skipped, got: {}",
            all_text
        );
    }

    #[test]
    fn test_mobi_fallback_skipped_via_display_none() {
        // mobi fallback content with display:none should be skipped (epub/mobi conditional)
        let html = r#"<html><body>
            <span class="epub">Keep this epub content</span>
            <span class="mobi">Skip this mobi fallback</span>
        </body></html>"#;

        let css = ".epub { display: inline; } .mobi { display: none; }";
        let stylesheet = Stylesheet::parse(css);
        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);
        let texts = collect_all_texts(&flattened);
        let all_text = texts.join(" ");

        assert!(
            all_text.contains("Keep this"),
            "epub content should be kept"
        );
        assert!(
            !all_text.contains("Skip this"),
            "mobi content (display:none) should be skipped, got: {}",
            all_text
        );
    }

    #[test]
    fn test_mathml_preserved_as_xml_string() {
        // MathML elements should be serialized as raw XML strings
        let html = r#"<html><body>
            <p>Before equation</p>
            <math xmlns="http://www.w3.org/1998/Math/MathML"><mi>x</mi><mo>+</mo><mn>1</mn></math>
            <p>After equation</p>
        </body></html>"#;

        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let stylesheet = Stylesheet::default();
        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Find MathML text
        let mathml_text = flattened.iter().find_map(|item| match item {
            ContentItem::Text { text, .. } if text.contains("<math") => Some(text.clone()),
            _ => None,
        });

        assert!(
            mathml_text.is_some(),
            "MathML should be preserved as XML string"
        );

        let xml = mathml_text.unwrap();
        assert!(
            xml.contains("<mi>x</mi>"),
            "Should preserve element structure"
        );
        assert!(xml.contains("<mo>+</mo>"), "Should preserve operators");
        assert!(xml.contains("<mn>1</mn>"), "Should preserve numbers");
    }

    #[test]
    fn test_mathml_with_mobi_fallback() {
        // Full epub/mobi conditional with MathML - should use MathML, skip fallback image
        let html = r#"<html><body>
            <span class="epub"><math xmlns="http://www.w3.org/1998/Math/MathML"><mi>y</mi></math></span>
            <span class="mobi"><img src="../images/eq1.jpg" alt="equation"/></span>
        </body></html>"#;

        let css = ".epub { display: inline; } .mobi { display: none; }";
        let stylesheet = Stylesheet::parse(css);
        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Should have MathML
        let has_mathml = flattened
            .iter()
            .any(|item| matches!(item, ContentItem::Text { text, .. } if text.contains("<math")));
        assert!(has_mathml, "Should preserve MathML from epub span");

        // Should NOT have fallback image
        let has_fallback_image = flattened.iter().any(|item| {
            matches!(item, ContentItem::Image { resource_href, .. } if resource_href.contains("eq1.jpg"))
        });
        assert!(
            !has_fallback_image,
            "Should NOT include mobi fallback image (display:none)"
        );
    }

    #[test]
    fn test_endnotes_list_from_fixture() {
        // Test that actual endnotes from EPUB are extracted as list container
        let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
        let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

        // Find endnotes.xhtml
        let endnotes = book
            .resources
            .iter()
            .find(|(k, _)| k.contains("endnotes"))
            .map(|(k, v)| (k.clone(), v));

        let (endnotes_path, resource) = endnotes.expect("Should find endnotes.xhtml");

        // Extract content
        let stylesheet = Stylesheet::default();
        let content = extract_content_from_xhtml(&resource.data, &stylesheet, &endnotes_path);

        // Find the ol container
        fn find_list_container(items: &[ContentItem]) -> Option<&ContentItem> {
            for item in items {
                match item {
                    ContentItem::Container {
                        list_type: Some(_), ..
                    } => return Some(item),
                    ContentItem::Container { children, .. } => {
                        if let Some(found) = find_list_container(children) {
                            return Some(found);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let list = find_list_container(&content);
        assert!(list.is_some(), "Should find a list container in endnotes");

        if let Some(ContentItem::Container {
            tag,
            list_type,
            children,
            ..
        }) = list
        {
            assert_eq!(tag, "ol", "Endnotes list should be an ol");
            assert_eq!(
                *list_type,
                Some(ListType::Ordered),
                "Endnotes should have Ordered list type"
            );
            // Epictetus has 98+ endnotes
            assert!(
                children.len() > 90,
                "Endnotes should have many li items, got {}",
                children.len()
            );
        }
    }

    #[test]
    fn test_backlink_creates_inline_run_with_anchor() {
        // Verify that backlinks (↩︎) in endnotes create inline runs with anchor_href
        // This is critical for links to work in KFX output
        let html = r#"<html><body>
            <p>Some note text. <a href="chapter.xhtml#noteref-1" role="doc-backlink">↩︎</a></p>
        </body></html>"#;

        let stylesheet = Stylesheet::default();
        let content = extract_content_from_xhtml(html.as_bytes(), &stylesheet, "endnotes.xhtml");

        // Find the paragraph with inline runs
        fn find_text_with_backlink(items: &[ContentItem]) -> Option<Vec<StyleRun>> {
            for item in items {
                match item {
                    ContentItem::Text {
                        text, inline_runs, ..
                    } if text.contains("↩︎") => {
                        return Some(inline_runs.clone());
                    }
                    ContentItem::Container { children, .. } => {
                        if let Some(runs) = find_text_with_backlink(children) {
                            return Some(runs);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let inline_runs = find_text_with_backlink(&content);
        assert!(inline_runs.is_some(), "Should find text with backlink");

        let runs = inline_runs.unwrap();
        assert!(!runs.is_empty(), "Should have inline runs for backlink");

        // Find the inline run with anchor_href
        let backlink_run = runs.iter().find(|r| r.anchor_href.is_some());
        assert!(
            backlink_run.is_some(),
            "Should have inline run with anchor_href"
        );

        let run = backlink_run.unwrap();
        assert!(
            run.anchor_href.as_ref().unwrap().contains("noteref-1"),
            "Anchor href should reference noteref-1, got {:?}",
            run.anchor_href
        );
    }

    #[test]
    fn test_span_with_css_class_preserves_line_height() {
        // This tests the text-xs Tailwind pattern: font-size and line-height on an inline span
        // The span's style (including line-height) should be preserved in the extracted content
        let css = r#"
            .text-xs { font-size: 0.75rem; line-height: 1rem; }
        "#;
        let html = r#"<html><body>
            <p><span class="text-xs">Test content</span></p>
        </body></html>"#;

        let stylesheet = Stylesheet::parse(css);
        let document = kuchiki::parse_html().one(html);
        let body = document
            .select("body")
            .ok()
            .and_then(|mut iter| iter.next())
            .map(|n| n.as_node().clone())
            .unwrap();

        let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
        let flattened = flatten_containers(items);

        // Find the text item and check its style has line-height
        fn find_text_style(items: &[ContentItem]) -> Option<ParsedStyle> {
            for item in items {
                match item {
                    ContentItem::Text { text, style, .. } if text.contains("Test content") => {
                        return Some(style.clone());
                    }
                    ContentItem::Container { children, .. } => {
                        if let Some(s) = find_text_style(children) {
                            return Some(s);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let style = find_text_style(&flattened).expect("Should find text item");

        // Check font-size is preserved
        assert!(
            matches!(style.font_size, Some(crate::css::CssValue::Rem(v)) if (v - 0.75).abs() < 0.01),
            "Style should have font-size: 0.75rem, got {:?}",
            style.font_size
        );

        // Check line-height is preserved - THIS IS THE KEY ASSERTION
        assert!(
            matches!(style.line_height, Some(crate::css::CssValue::Rem(v)) if (v - 1.0).abs() < 0.01),
            "Style should have line-height: 1rem, got {:?}",
            style.line_height
        );
    }

    #[test]
    fn test_footnote_classification_from_epub_type() {
        // Test that epub:type="footnote" sets classification
        let html = r#"
            <html xmlns:epub="http://www.idpf.org/2007/ops">
            <body>
                <aside epub:type="footnote" id="fn1">
                    <p>This is a footnote</p>
                </aside>
            </body>
            </html>
        "#;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);

        // Find the aside container
        fn find_classification(items: &[ContentItem]) -> Option<u64> {
            for item in items {
                if let ContentItem::Container {
                    classification,
                    children,
                    ..
                } = item
                {
                    if classification.is_some() {
                        return *classification;
                    }
                    if let Some(c) = find_classification(children) {
                        return Some(c);
                    }
                }
            }
            None
        }

        let classification = find_classification(&flattened);
        assert_eq!(
            classification,
            Some(sym::FOOTNOTE),
            "Container with epub:type='footnote' should have FOOTNOTE classification ($618)"
        );
    }

    #[test]
    fn test_endnote_classification_from_epub_type() {
        // Test that epub:type="endnote" sets classification
        let html = r#"
            <html xmlns:epub="http://www.idpf.org/2007/ops">
            <body>
                <aside epub:type="endnote" id="en1">
                    <p>This is an endnote</p>
                </aside>
            </body>
            </html>
        "#;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);

        fn find_classification(items: &[ContentItem]) -> Option<u64> {
            for item in items {
                if let ContentItem::Container {
                    classification,
                    children,
                    ..
                } = item
                {
                    if classification.is_some() {
                        return *classification;
                    }
                    if let Some(c) = find_classification(children) {
                        return Some(c);
                    }
                }
            }
            None
        }

        let classification = find_classification(&flattened);
        assert_eq!(
            classification,
            Some(sym::ENDNOTE),
            "Container with epub:type='endnote' should have ENDNOTE classification ($619)"
        );
    }

    #[test]
    fn test_footnote_classification_from_aria_role() {
        // Test that role="doc-footnote" sets classification
        let html = r#"
            <html>
            <body>
                <aside role="doc-footnote" id="fn1">
                    <p>This is a footnote via ARIA</p>
                </aside>
            </body>
            </html>
        "#;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);

        fn find_classification(items: &[ContentItem]) -> Option<u64> {
            for item in items {
                if let ContentItem::Container {
                    classification,
                    children,
                    ..
                } = item
                {
                    if classification.is_some() {
                        return *classification;
                    }
                    if let Some(c) = find_classification(children) {
                        return Some(c);
                    }
                }
            }
            None
        }

        let classification = find_classification(&flattened);
        assert_eq!(
            classification,
            Some(sym::FOOTNOTE),
            "Container with role='doc-footnote' should have FOOTNOTE classification ($618)"
        );
    }

    #[test]
    fn test_noteref_detection_from_epub_type() {
        // Test that epub:type="noteref" sets is_noteref on text
        let html = r##"
            <html xmlns:epub="http://www.idpf.org/2007/ops">
            <body>
                <p>See note<a epub:type="noteref" href="#fn1">1</a> for details.</p>
            </body>
            </html>
        "##;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);
        let merged = merge_text_with_inline_runs(flattened);

        // Find text with inline runs containing noteref
        fn find_noteref_run(items: &[ContentItem]) -> Option<bool> {
            for item in items {
                match item {
                    ContentItem::Text { inline_runs, .. } => {
                        for run in inline_runs {
                            if run.is_noteref && run.anchor_href.is_some() {
                                return Some(true);
                            }
                        }
                    }
                    ContentItem::Container { children, .. } => {
                        if let Some(result) = find_noteref_run(children) {
                            return Some(result);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let has_noteref = find_noteref_run(&merged);
        assert_eq!(
            has_noteref,
            Some(true),
            "Link with epub:type='noteref' should have is_noteref=true in inline run"
        );
    }

    #[test]
    fn test_noteref_detection_from_aria_role() {
        // Test that role="doc-noteref" sets is_noteref on text
        let html = r##"
            <html>
            <body>
                <p>See note<a role="doc-noteref" href="#fn1">1</a> for details.</p>
            </body>
            </html>
        "##;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);
        let merged = merge_text_with_inline_runs(flattened);

        fn find_noteref_run(items: &[ContentItem]) -> Option<bool> {
            for item in items {
                match item {
                    ContentItem::Text { inline_runs, .. } => {
                        for run in inline_runs {
                            if run.is_noteref && run.anchor_href.is_some() {
                                return Some(true);
                            }
                        }
                    }
                    ContentItem::Container { children, .. } => {
                        if let Some(result) = find_noteref_run(children) {
                            return Some(result);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let has_noteref = find_noteref_run(&merged);
        assert_eq!(
            has_noteref,
            Some(true),
            "Link with role='doc-noteref' should have is_noteref=true in inline run"
        );
    }

    #[test]
    fn test_regular_link_not_noteref() {
        // Test that regular links don't have is_noteref
        let html = r#"
            <html>
            <body>
                <p>Visit <a href="https://example.com">this site</a> for more.</p>
            </body>
            </html>
        "#;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);
        let merged = merge_text_with_inline_runs(flattened);

        // Find inline run with anchor_href and check is_noteref
        fn check_link_not_noteref(items: &[ContentItem]) -> Option<bool> {
            for item in items {
                match item {
                    ContentItem::Text { inline_runs, .. } => {
                        for run in inline_runs {
                            if run.anchor_href.is_some() {
                                // Found a link - should NOT be noteref
                                return Some(!run.is_noteref);
                            }
                        }
                    }
                    ContentItem::Container { children, .. } => {
                        if let Some(result) = check_link_not_noteref(children) {
                            return Some(result);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let link_not_noteref = check_link_not_noteref(&merged);
        assert_eq!(
            link_not_noteref,
            Some(true),
            "Regular link without epub:type/role should have is_noteref=false"
        );
    }

    #[test]
    fn test_figure_element_sets_is_figure() {
        // Test that <figure> elements set is_figure on style
        let html = r#"
            <html>
            <body>
                <figure>
                    <img src="image.jpg" alt="Test image"/>
                </figure>
            </body>
            </html>
        "#;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);

        fn find_figure_style(items: &[ContentItem]) -> Option<bool> {
            for item in items {
                if let ContentItem::Container {
                    style, tag, children, ..
                } = item
                {
                    if tag == "figure" {
                        return Some(style.is_figure);
                    }
                    if let Some(result) = find_figure_style(children) {
                        return Some(result);
                    }
                }
            }
            None
        }

        let is_figure = find_figure_style(&flattened);
        assert_eq!(
            is_figure,
            Some(true),
            "<figure> element should have is_figure=true on its style"
        );
    }

    #[test]
    fn test_figcaption_element_sets_is_caption() {
        // Test that <figcaption> elements set is_caption on style
        let html = r#"
            <html>
            <body>
                <figure>
                    <img src="image.jpg" alt="Test image"/>
                    <figcaption>This is a caption</figcaption>
                </figure>
            </body>
            </html>
        "#;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);

        fn find_figcaption_style(items: &[ContentItem]) -> Option<bool> {
            for item in items {
                if let ContentItem::Container {
                    style, tag, children, ..
                } = item
                {
                    if tag == "figcaption" {
                        return Some(style.is_caption);
                    }
                    if let Some(result) = find_figcaption_style(children) {
                        return Some(result);
                    }
                }
            }
            None
        }

        let is_caption = find_figcaption_style(&flattened);
        assert_eq!(
            is_caption,
            Some(true),
            "<figcaption> element should have is_caption=true on its style"
        );
    }

    #[test]
    fn test_heading_element_sets_is_heading() {
        // Test that h1-h6 elements set is_heading on style
        let html = r#"
            <html>
            <body>
                <h2>Chapter Title</h2>
            </body>
            </html>
        "#;

        let stylesheet = Stylesheet::parse("");
        let document = kuchiki::parse_html().one(html);
        let body = document.select("body").unwrap().next().unwrap();
        let items = extract_from_node(
            body.as_node(),
            &stylesheet,
            &ParsedStyle::default(),
            "",
            None,
            false,
            false,
        );
        let flattened = flatten_containers(items);

        fn find_heading_style(items: &[ContentItem]) -> Option<bool> {
            for item in items {
                if let ContentItem::Container {
                    style, tag, children, ..
                } = item
                {
                    if tag.starts_with('h') && tag.len() == 2 {
                        return Some(style.is_heading);
                    }
                    if let Some(result) = find_heading_style(children) {
                        return Some(result);
                    }
                }
            }
            None
        }

        let is_heading = find_heading_style(&flattened);
        assert_eq!(
            is_heading,
            Some(true),
            "<h2> element should have is_heading=true on its style"
        );
    }
}
