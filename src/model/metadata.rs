//! Pure, format-agnostic data types for ebooks.
//!
//! This module provides `Format`, `Metadata`, `TocEntry`, `Resource`,
//! `Landmark`, and related value types. The `Book` runtime handle
//! (which dispatches to importers/exporters) lives in `crate::book`.

use std::path::Path;

use crate::model::AnchorTarget;

/// Ebook file format.
///
/// `#[non_exhaustive]`: new formats can be added without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// EPUB format (EPUB 2 or 3)
    Epub,
    /// AZW3/KF8 format (modern Kindle)
    Azw3,
    /// MOBI format (legacy Kindle)
    Mobi,
    /// KFX format (Kindle Format 10)
    Kfx,
    /// Markdown (export only)
    Markdown,
}

/// A resource (image, font, CSS, etc.) with its data and media type.
#[derive(Debug, Clone)]
pub struct Resource {
    pub data: Vec<u8>,
    pub media_type: &'static str,
}

/// A contributor with optional role and sort name.
#[derive(Debug, Clone, Default)]
pub struct Contributor {
    pub name: String,
    pub file_as: Option<String>,
    /// MARC relator code: "trl", "edt", "ill", etc.
    pub role: Option<String>,
}

/// Collection/series information.
#[derive(Debug, Clone)]
pub struct CollectionInfo {
    pub name: String,
    /// "series" or "set"
    pub collection_type: Option<String>,
    /// group-position (1, 2, 3.5, etc.)
    pub position: Option<f64>,
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
    /// dcterms:modified timestamp
    pub modified_date: Option<String>,
    /// dc:contributor with roles (translators, editors, illustrators, etc.)
    pub contributors: Vec<Contributor>,
    /// file-as for title (sort key)
    pub title_sort: Option<String>,
    /// file-as for first author (sort key)
    pub author_sort: Option<String>,
    /// belongs-to-collection (series info)
    pub collection: Option<CollectionInfo>,
}

/// A table of contents entry (hierarchical)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub title: String,
    pub href: String,
    pub children: Vec<TocEntry>,
    /// Play order for sorting (from NCX playOrder attribute)
    pub play_order: Option<usize>,
    /// Resolved target (set by `resolve_links()`)
    pub target: Option<AnchorTarget>,
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

/// Type of landmark in a book's navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LandmarkType {
    /// Cover page (image)
    Cover,
    /// Title page
    TitlePage,
    /// Table of contents
    Toc,
    /// Start reading location (where the book opens)
    StartReading,
    /// Beginning of body/main content
    BodyMatter,
    /// Front matter (preface, introduction, etc.)
    FrontMatter,
    /// Back matter (appendix, index, etc.)
    BackMatter,
    /// Acknowledgements
    Acknowledgements,
    /// Bibliography
    Bibliography,
    /// Glossary
    Glossary,
    /// Index
    Index,
    /// Preface
    Preface,
    /// Endnotes/Footnotes
    Endnotes,
    /// List of illustrations
    Loi,
    /// List of tables
    Lot,
}

/// A landmark navigation entry.
///
/// Landmarks identify structural locations in a book (cover, start of content,
/// endnotes, etc.) used for navigation and reader features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landmark {
    /// Type of landmark
    pub landmark_type: LandmarkType,
    /// Target href (file path with optional fragment)
    pub href: String,
    /// Display label
    pub label: String,
}

impl Format {
    /// Detect format from file extension.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        path.as_ref()
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| match ext.to_lowercase().as_str() {
                "epub" => Some(Format::Epub),
                "azw3" => Some(Format::Azw3),
                "mobi" | "azw" => Some(Format::Mobi),
                "kfx" => Some(Format::Kfx),
                "md" | "txt" => Some(Format::Markdown),
                _ => None,
            })
    }

    /// Whether this format can be used for input/import.
    pub fn can_import(&self) -> bool {
        matches!(
            self,
            Format::Epub | Format::Azw3 | Format::Mobi | Format::Kfx
        )
    }

    /// Whether this format can be used for output/export.
    pub fn can_export(&self) -> bool {
        !matches!(self, Format::Mobi)
    }
}

impl TocEntry {
    pub fn new(title: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            href: href.into(),
            children: Vec::new(),
            play_order: None,
            target: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_path_recognises_known_extensions() {
        assert_eq!(Format::from_path("book.epub"), Some(Format::Epub));
        assert_eq!(Format::from_path("book.azw3"), Some(Format::Azw3));
        assert_eq!(Format::from_path("book.mobi"), Some(Format::Mobi));
        assert_eq!(Format::from_path("book.azw"), Some(Format::Mobi));
        assert_eq!(Format::from_path("book.kfx"), Some(Format::Kfx));
        assert_eq!(Format::from_path("notes.md"), Some(Format::Markdown));
        assert_eq!(Format::from_path("book.AZW"), Some(Format::Mobi));
        assert_eq!(Format::from_path("book.unknown"), None);
        assert_eq!(Format::from_path("no_extension"), None);
    }
}
