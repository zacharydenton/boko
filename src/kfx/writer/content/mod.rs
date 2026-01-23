//! Content types and extraction for KFX generation.
//!
//! This module contains:
//! - Content item types (Text, Image, Container)
//! - Chapter data structures
//! - Content chunking for large chapters
//! - XHTML content extraction
//! - Content validation (P3 improvement)
//!
//! Submodules:
//! - `extraction`: Core XHTML→ContentItem conversion
//! - `html_utils`: HTML/XML utility functions
//! - `merging`: Post-processing (flatten, merge inline runs)

mod extraction;
pub(crate) mod html_utils;
pub(crate) mod merging;

pub use extraction::*;
pub use merging::{flatten_containers, merge_text_with_inline_runs};

use crate::css::ParsedStyle;

use super::symbols::sym;

// =============================================================================
// P3: Content Validation Constants
// =============================================================================

/// Maximum size for a single content fragment in bytes (P3 improvement)
/// Based on kfxinput's MAX_CONTENT_FRAGMENT_SIZE = 8192
pub const MAX_CONTENT_FRAGMENT_SIZE: usize = 8192;

/// Unexpected characters that should be detected and warned about (P3 improvement)
/// These are control characters, interlinear annotations, and non-characters
/// Based on kfxinput's UNEXPECTED_CHARACTERS list
pub static UNEXPECTED_CHARACTERS: &[char] = &[
    // C0 control characters (except common whitespace)
    '\u{0000}', '\u{0001}', '\u{0002}', '\u{0003}', '\u{0004}', '\u{0005}', '\u{0006}', '\u{0007}',
    '\u{0008}', '\u{000B}', '\u{000C}', '\u{000E}', '\u{000F}', '\u{0010}', '\u{0011}', '\u{0012}',
    '\u{0013}', '\u{0014}', '\u{0015}', '\u{0016}', '\u{0017}', '\u{0018}', '\u{0019}', '\u{001A}',
    '\u{001B}', '\u{001C}', '\u{001D}', '\u{001E}', '\u{001F}', '\u{007F}',
    // C1 control characters
    '\u{0080}', '\u{0081}', '\u{0082}', '\u{0083}', '\u{0084}', '\u{0085}', '\u{0086}', '\u{0087}',
    '\u{0088}', '\u{0089}', '\u{008A}', '\u{008B}', '\u{008C}', '\u{008D}', '\u{008E}', '\u{008F}',
    '\u{0090}', '\u{0091}', '\u{0092}', '\u{0093}', '\u{0094}', '\u{0095}', '\u{0096}', '\u{0097}',
    '\u{0098}', '\u{0099}', '\u{009A}', '\u{009B}', '\u{009C}', '\u{009D}', '\u{009E}', '\u{009F}',
    // Arabic letter mark (invisible directional control)
    '\u{061C}', // Invisible separator
    '\u{2063}', // Interlinear annotation anchors
    '\u{FFF9}', '\u{FFFA}', '\u{FFFB}', // Noncharacters
    '\u{FFFE}', '\u{FFFF}',
];

// =============================================================================
// P3: Content Validation Functions
// =============================================================================

/// Check if a string contains any unexpected characters (P3 improvement)
/// Returns a list of (character, position) for any unexpected characters found
pub fn find_unexpected_characters(text: &str) -> Vec<(char, usize)> {
    let mut unexpected = Vec::new();
    for (pos, c) in text.char_indices() {
        if UNEXPECTED_CHARACTERS.contains(&c) {
            unexpected.push((c, pos));
        }
    }
    unexpected
}

/// Remove unexpected characters from a string (P3 improvement)
/// Returns the cleaned string
pub fn clean_unexpected_characters(text: &str) -> String {
    text.chars()
        .filter(|c| !UNEXPECTED_CHARACTERS.contains(c))
        .collect()
}

/// Validate content fragment size (P3 improvement)
/// Returns true if the content is within acceptable size limits
pub fn validate_fragment_size(content: &str) -> bool {
    content.len() <= MAX_CONTENT_FRAGMENT_SIZE
}

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
    /// Whether this link is a noteref (reference to a footnote/endnote)
    /// When true, $616: $617 is added to inline style runs for popup behavior
    pub is_noteref: bool,
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
        /// Whether this text is from a noteref link (triggers popup footnotes)
        is_noteref: bool,
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
        /// Table cell colspan attribute
        colspan: Option<u32>,
        /// Table cell rowspan attribute
        rowspan: Option<u32>,
        /// Content classification for footnotes/endnotes ($615 value)
        /// - Some(sym::FOOTNOTE) for footnote containers
        /// - Some(sym::ENDNOTE) for endnote containers
        classification: Option<u64>,
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

    /// Check if this container has nested containers in its children.
    /// Used to determine if a list item needs complex (flattened) handling.
    pub fn has_nested_containers(&self) -> bool {
        match self {
            ContentItem::Container { children, .. } => children
                .iter()
                .any(|c| matches!(c, ContentItem::Container { .. })),
            _ => false,
        }
    }

    /// Count items when nested containers are flattened (for complex list items).
    /// This matches the behavior of build_nested_paragraphs in the builder.
    /// Intermediate containers don't get EIDs - only text and images do.
    pub fn count_flattened_items(&self) -> usize {
        match self {
            ContentItem::Text { text, is_verse, .. } => {
                // Text items may become multiple entries (split by newlines in verse)
                if *is_verse {
                    text.split('\n').filter(|s| !s.trim().is_empty()).count()
                } else if !text.trim().is_empty() {
                    1
                } else {
                    0
                }
            }
            ContentItem::Image { .. } => 1,
            ContentItem::Container { children, .. } => {
                // Flattened: don't count container itself, just its flattened children
                children.iter().map(|c| c.count_flattened_items()).sum()
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

// =============================================================================
// P3: Content Validation Tests
// =============================================================================

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn test_find_unexpected_null_character() {
        let text = "Hello\0World";
        let unexpected = find_unexpected_characters(text);
        assert_eq!(unexpected.len(), 1);
        assert_eq!(unexpected[0], ('\0', 5));
    }

    #[test]
    fn test_find_unexpected_c1_control() {
        let text = "Hello\u{0085}World"; // NEL (Next Line)
        let unexpected = find_unexpected_characters(text);
        assert_eq!(unexpected.len(), 1);
        assert_eq!(unexpected[0], ('\u{0085}', 5));
    }

    #[test]
    fn test_find_no_unexpected_characters() {
        let text = "Hello World! This is normal text with 日本語 and émojis 🎉";
        let unexpected = find_unexpected_characters(text);
        assert!(unexpected.is_empty());
    }

    #[test]
    fn test_clean_unexpected_characters() {
        let text = "Hello\0World\u{001F}!";
        let cleaned = clean_unexpected_characters(text);
        assert_eq!(cleaned, "HelloWorld!");
    }

    #[test]
    fn test_validate_fragment_size_within_limit() {
        let small_content = "a".repeat(1000);
        assert!(validate_fragment_size(&small_content));
    }

    #[test]
    fn test_validate_fragment_size_at_limit() {
        let at_limit = "a".repeat(MAX_CONTENT_FRAGMENT_SIZE);
        assert!(validate_fragment_size(&at_limit));
    }

    #[test]
    fn test_validate_fragment_size_exceeds_limit() {
        let too_large = "a".repeat(MAX_CONTENT_FRAGMENT_SIZE + 1);
        assert!(!validate_fragment_size(&too_large));
    }

    #[test]
    fn test_interlinear_annotation_chars_detected() {
        // These are used in Japanese ruby/furigana but shouldn't appear in final text
        let text = "Test\u{FFF9}annotation\u{FFFA}separator\u{FFFB}end";
        let unexpected = find_unexpected_characters(text);
        assert_eq!(unexpected.len(), 3);
    }
}
