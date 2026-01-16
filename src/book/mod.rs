use std::collections::HashMap;

/// Intermediate representation of an ebook.
/// Format-agnostic structure that EPUB, MOBI, and AZW3 can convert to/from.
#[derive(Debug, Clone, Default)]
pub struct Book {
    pub metadata: Metadata,
    pub spine: Vec<SpineItem>,
    pub toc: Vec<TocEntry>,
    pub resources: HashMap<String, Resource>,
}

/// Book metadata (Dublin Core + extensions)
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub title: String,
    pub authors: Vec<String>,
    pub language: String,
    pub identifier: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub date: Option<String>,
    pub rights: Option<String>,
    pub cover_image: Option<String>,
}

/// An item in the reading order (spine)
#[derive(Debug, Clone)]
pub struct SpineItem {
    pub id: String,
    pub href: String,
    pub media_type: String,
    pub linear: bool,
}

/// A table of contents entry (hierarchical)
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TocEntry {
    pub title: String,
    pub href: String,
    pub children: Vec<TocEntry>,
    /// Play order for sorting (from NCX playOrder attribute)
    pub play_order: Option<usize>,
}

impl Ord for TocEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.play_order.cmp(&other.play_order)
    }
}

impl PartialOrd for TocEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A resource (content document, image, CSS, font, etc.)
#[derive(Debug, Clone)]
pub struct Resource {
    pub data: Vec<u8>,
    pub media_type: String,
}

impl Book {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a resource to the book
    pub fn add_resource(&mut self, href: impl Into<String>, data: Vec<u8>, media_type: impl Into<String>) {
        self.resources.insert(href.into(), Resource {
            data,
            media_type: media_type.into(),
        });
    }

    /// Get a resource by href
    pub fn get_resource(&self, href: &str) -> Option<&Resource> {
        self.resources.get(href)
    }

    /// Add a spine item
    pub fn add_spine_item(&mut self, id: impl Into<String>, href: impl Into<String>, media_type: impl Into<String>) {
        self.spine.push(SpineItem {
            id: id.into(),
            href: href.into(),
            media_type: media_type.into(),
            linear: true,
        });
    }
}

impl Metadata {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.authors.push(author.into());
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    pub fn with_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.identifier = identifier.into();
        self
    }
}

impl TocEntry {
    pub fn new(title: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            href: href.into(),
            children: Vec::new(),
            play_order: None,
        }
    }

    pub fn with_child(mut self, child: TocEntry) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_play_order(mut self, order: usize) -> Self {
        self.play_order = Some(order);
        self
    }
}
