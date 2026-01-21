//! Content types and extraction for KFX generation.
//!
//! This module contains:
//! - Content item types (Text, Image, Container)
//! - Chapter data structures
//! - Content chunking for large chapters
//! - XHTML content extraction

mod extraction;

pub use extraction::*;

use crate::css::ParsedStyle;

use super::symbols::sym;

/// List type for ordered/unordered lists
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListType {
    /// Ordered list (ol) - decimal numbered
    Ordered,
    /// Unordered list (ul) - bullet points
    Unordered,
}

/// An inline style run within a paragraph
/// Specifies that a range of characters has a different style
#[derive(Debug, Clone)]
pub struct StyleRun {
    /// Character offset within the text
    pub offset: usize,
    /// Number of characters this style applies to
    pub length: usize,
    /// The style to apply for this range
    pub style: ParsedStyle,
    /// Optional anchor href for hyperlinks in this range
    pub anchor_href: Option<String>,
    /// Optional element ID from inline element (e.g., <a id="noteref-1">)
    /// Used for anchor targets (back-links)
    pub element_id: Option<String>,
}

/// A content item - text, image, or nested container
#[derive(Debug, Clone)]
pub enum ContentItem {
    /// Text content with styling and optional inline style runs
    Text {
        text: String,
        style: ParsedStyle,
        /// Optional inline style runs for different character ranges
        inline_runs: Vec<StyleRun>,
        /// Optional anchor href for hyperlinks
        anchor_href: Option<String>,
        /// Optional HTML element ID (for TOC anchor targets)
        element_id: Option<String>,
        /// Whether this text is from a verse/poetry block (affects line break handling)
        is_verse: bool,
    },
    /// Image reference with optional styling
    Image {
        /// Path/href to the image resource (relative to EPUB structure)
        resource_href: String,
        style: ParsedStyle,
        /// Alt text for accessibility
        alt: Option<String>,
    },
    /// Container with nested content items (for block-level elements like sections, divs)
    Container {
        /// Style for the container itself
        style: ParsedStyle,
        /// Nested content items
        children: Vec<ContentItem>,
        /// Tag name for debugging/identification
        tag: String,
        /// Optional HTML element ID (for TOC anchor targets)
        element_id: Option<String>,
        /// List type if this is an ol/ul container
        list_type: Option<ListType>,
    },
}

impl ContentItem {
    /// Get the style for this content item
    pub fn style(&self) -> &ParsedStyle {
        match self {
            ContentItem::Text { style, .. } => style,
            ContentItem::Image { style, .. } => style,
            ContentItem::Container { style, .. } => style,
        }
    }

    /// Get flattened iterator over all leaf items (text and images)
    pub fn flatten(&self) -> Vec<&ContentItem> {
        match self {
            ContentItem::Text { .. } | ContentItem::Image { .. } => vec![self],
            ContentItem::Container { children, .. } => {
                children.iter().flat_map(|c| c.flatten()).collect()
            }
        }
    }

    /// Calculate total text size (for chunking)
    pub fn total_text_size(&self) -> usize {
        match self {
            ContentItem::Text { text, .. } => text.len(),
            ContentItem::Image { .. } => 1, // Images count as minimal size
            ContentItem::Container { children, .. } => {
                children.iter().map(|c| c.total_text_size()).sum()
            }
        }
    }

    /// Count total number of items including nested children (for EID calculation)
    pub fn count_items(&self) -> usize {
        match self {
            ContentItem::Text { .. } | ContentItem::Image { .. } => 1,
            ContentItem::Container { children, .. } => {
                1 + children.iter().map(|c| c.count_items()).sum::<usize>()
            }
        }
    }
}

/// Count total content items including nested containers
pub fn count_content_items(items: &[ContentItem]) -> usize {
    items.iter().map(|item| item.count_items()).sum()
}

/// Collect all referenced image hrefs from content items
pub fn collect_referenced_images(items: &[ContentItem]) -> std::collections::HashSet<String> {
    let mut hrefs = std::collections::HashSet::new();
    for item in items {
        collect_images_recursive(item, &mut hrefs);
    }
    hrefs
}

fn collect_images_recursive(item: &ContentItem, hrefs: &mut std::collections::HashSet<String>) {
    match item {
        ContentItem::Image { resource_href, .. } => {
            hrefs.insert(resource_href.clone());
        }
        ContentItem::Container { children, .. } => {
            for child in children {
                collect_images_recursive(child, hrefs);
            }
        }
        ContentItem::Text { .. } => {}
    }
}

/// Data for a single chapter/section
pub struct ChapterData {
    /// Unique identifier for this chapter
    pub id: String,
    /// Chapter title (for TOC)
    pub title: String,
    /// Content items for this chapter
    pub content: Vec<ContentItem>,
    /// Source XHTML path (for internal link targets)
    pub source_path: String,
}

/// A chunk of content (subset of a chapter)
pub struct ContentChunk {
    /// Unique ID for this chunk
    pub id: String,
    /// Content items for this chunk
    pub items: Vec<ContentItem>,
}

impl ChapterData {
    /// Split chapter into chunks that don't exceed MAX_CHUNK_SIZE characters
    pub fn into_chunks(self) -> Vec<ContentChunk> {
        let mut chunks = Vec::new();
        let mut current_items = Vec::new();
        let mut current_size = 0;
        let mut chunk_index = 0;

        for item in self.content.into_iter() {
            let item_size = item.total_text_size();

            // If adding this item would exceed chunk size, start a new chunk
            if current_size + item_size > sym::MAX_CHUNK_SIZE && !current_items.is_empty() {
                chunks.push(ContentChunk {
                    id: format!("{}-{}", self.id, chunk_index),
                    items: std::mem::take(&mut current_items),
                });
                chunk_index += 1;
                current_size = 0;
            }

            current_size += item_size;
            current_items.push(item);
        }

        // Push remaining items
        if !current_items.is_empty() {
            chunks.push(ContentChunk {
                id: format!("{}-{}", self.id, chunk_index),
                items: current_items,
            });
        }

        chunks
    }
}
