//! Content item post-processing and merging.
//!
//! This module handles:
//! - Flattening unnecessary container wrappers
//! - Merging consecutive text items with inline style runs

use crate::css::ParsedStyle;

use super::{ContentItem, StyleRun};

/// Pending text item: (text, style, anchor_href, element_id, is_verse, is_noteref)
type PendingText = (
    String,
    ParsedStyle,
    Option<String>,
    Option<String>,
    bool,
    bool,
);

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
                    // - If style differs from base (bold, italic, etc.), use inline-only properties
                    // - If only anchor/element_id differs (plain link), use a minimal inline style
                    let run_style = if !style_differs && (has_anchor || has_element_id) {
                        // Anchor-only run: create minimal inline style
                        // This matches reference behavior where links use $127: $349 only
                        ParsedStyle {
                            is_inline: true,
                            ..Default::default()
                        }
                    } else {
                        // Style differs: convert to inline-only (strips block-level properties)
                        // This prevents margins, text-align, etc. from appearing in inline runs
                        style.to_inline_only()
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
