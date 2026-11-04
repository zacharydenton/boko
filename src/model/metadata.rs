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
    /// Raw resource bytes.
    pub data: Vec<u8>,
    /// MIME type (e.g. "image/jpeg"), normalized to a known static string.
    pub media_type: &'static str,
}

/// A contributor with optional role and sort name (EPUB `dc:contributor`).
#[derive(Debug, Clone, Default)]
pub struct Contributor {
    /// Display name of the contributor.
    pub name: String,
    /// Sort key (`file-as` refinement), e.g. "Doe, Jane".
    pub file_as: Option<String>,
    /// MARC relator code: "trl", "edt", "ill", etc.
    pub role: Option<String>,
}

/// Collection/series information (EPUB 3 `belongs-to-collection`).
#[derive(Debug, Clone)]
pub struct CollectionInfo {
    /// Collection or series name.
    pub name: String,
    /// "series" or "set"
    pub collection_type: Option<String>,
    /// group-position (1, 2, 3.5, etc.)
    pub position: Option<f64>,
}

/// Book metadata (Dublin Core + extensions)
///
/// Populated from the OPF `<metadata>` element for EPUB, or the
/// corresponding EXTH/entity records for MOBI/AZW3/KFX. String fields
/// default to empty and `Option` fields to `None` when a source book
/// omits them.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    /// Book title (`dc:title`).
    pub title: String,
    /// Author display names (`dc:creator`), in declaration order.
    pub authors: Vec<String>,
    /// Language tag such as "en" or "pt-BR" (`dc:language`).
    pub language: String,
    /// Unique identifier (`dc:identifier`), e.g. ISBN, UUID, or URI.
    pub identifier: String,
    /// Publisher name (`dc:publisher`).
    pub publisher: Option<String>,
    /// Description or blurb (`dc:description`); may contain HTML markup.
    pub description: Option<String>,
    /// Subject/genre keywords (`dc:subject`).
    pub subjects: Vec<String>,
    /// Publication date (`dc:date`), as written in the source.
    pub date: Option<String>,
    /// Copyright/license statement (`dc:rights`).
    pub rights: Option<String>,
    /// Path of the cover image resource within the book, if identified.
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
    /// Global reading direction from the OPF spine's
    /// `page-progression-direction` (`"ltr"`, `"rtl"`, or `"default"`).
    /// Right-to-left books (Arabic, Hebrew, vertical Japanese) lose their
    /// reading order without it.
    pub page_progression_direction: Option<String>,
}

impl Metadata {
    /// Whether the book reads right-to-left: explicit spine direction wins,
    /// followed by an explicit language script, then a language fallback.
    /// The fallback is a compatibility heuristic for EPUB 2 books, not a
    /// substitute for authored text direction. Kurdish defaults to Latin;
    /// Arabic-script Kurdish should use `ku-Arab` or Central Kurdish (`ckb`).
    pub fn is_rtl(&self) -> bool {
        match self.page_progression_direction.as_deref() {
            Some("rtl") => true,
            Some("ltr") => false,
            _ => {
                let mut subtags = self.language.trim().split(['-', '_']);
                let primary = subtags.next().unwrap_or("");
                if primary.len() < 2 {
                    return false;
                }
                // Do not mistake extension/private-use payloads for scripts.
                if let Some(script) = subtags
                    .take_while(|s| s.len() > 1)
                    .find(|s| s.len() == 4 && s.bytes().all(|b| b.is_ascii_alphabetic()))
                {
                    // RTL scripts from Unicode CLDR's scriptMetadata.txt:
                    // https://github.com/unicode-org/cldr/blob/main/common/properties/scriptMetadata.txt
                    return matches!(
                        script.to_ascii_lowercase().as_str(),
                        "adlm"
                            | "arab"
                            | "armi"
                            | "avst"
                            | "chrs"
                            | "cprt"
                            | "elym"
                            | "gara"
                            | "hatr"
                            | "hebr"
                            | "hung"
                            | "khar"
                            | "lydi"
                            | "mand"
                            | "mani"
                            | "mend"
                            | "merc"
                            | "mero"
                            | "narb"
                            | "nbat"
                            | "nkoo"
                            | "orkh"
                            | "ougr"
                            | "palm"
                            | "phli"
                            | "phlp"
                            | "phnx"
                            | "prti"
                            | "rohg"
                            | "samr"
                            | "sarb"
                            | "sidt"
                            | "sogd"
                            | "sogo"
                            | "syrc"
                            | "thaa"
                            | "yezi"
                    );
                }
                matches!(
                    primary.to_ascii_lowercase().as_str(),
                    "ar" | "ara"
                        | "arc"
                        | "ckb"
                        | "dv"
                        | "div"
                        | "fa"
                        | "fas"
                        | "per"
                        | "he"
                        | "heb"
                        | "iw"
                        | "ps"
                        | "pus"
                        | "sd"
                        | "snd"
                        | "syr"
                        | "ug"
                        | "uig"
                        | "ur"
                        | "urd"
                        | "yi"
                        | "yid"
                )
            }
        }
    }
}

/// A table of contents entry (hierarchical)
///
/// Built from the EPUB 3 nav document or EPUB 2 NCX (or the equivalent
/// Kindle TOC structures).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// Display label for the entry.
    pub title: String,
    /// Link target: a spine document path, optionally with a `#fragment`.
    pub href: String,
    /// Nested sub-entries (deeper TOC levels).
    pub children: Vec<TocEntry>,
    /// Play order for sorting (from NCX playOrder attribute)
    pub play_order: Option<usize>,
    /// Resolved target (set by `resolve_links()`)
    pub target: Option<AnchorTarget>,
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
    /// Create a leaf entry with the given title and href (no children,
    /// no play order, unresolved target).
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
