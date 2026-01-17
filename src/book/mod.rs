//! Core data types for representing ebooks.
//!
//! This module provides format-agnostic types that serve as the intermediate
//! representation between different ebook formats (EPUB, MOBI, AZW3).

use std::collections::HashMap;
use std::io;
use std::path::Path;

/// Ebook file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// EPUB format (EPUB 2 or 3)
    Epub,
    /// AZW3/KF8 format (modern Kindle)
    Azw3,
    /// MOBI format (legacy Kindle). Writes as KF8/AZW3.
    Mobi,
    /// KFX/KF10 format (latest Kindle). Read-only.
    Kfx,
}

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

impl Format {
    /// Detect format from file extension.
    ///
    /// Returns `None` if the extension is not recognized.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        path.as_ref()
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| match ext.to_lowercase().as_str() {
                "epub" => Some(Format::Epub),
                "azw3" => Some(Format::Azw3),
                "mobi" => Some(Format::Mobi),
                "kfx" => Some(Format::Kfx),
                _ => None,
            })
    }
}

impl Book {
    /// Create a new empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open an ebook file, auto-detecting the format from the file extension.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::Book;
    ///
    /// let book = Book::open("input.epub")?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let format = Format::from_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown file format: {}", path.display()),
            )
        })?;
        Self::open_format(path, format)
    }

    /// Open an ebook file with an explicit format.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::{Book, Format};
    ///
    /// let book = Book::open_format("input.bin", Format::Epub)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn open_format(path: impl AsRef<Path>, format: Format) -> io::Result<Self> {
        match format {
            Format::Epub => crate::epub::read_epub(path),
            Format::Azw3 | Format::Mobi => crate::mobi::read_mobi(path),
            Format::Kfx => crate::kfx::read_kfx(path),
        }
    }

    /// Save the book to a file, auto-detecting the format from the file extension.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::Book;
    ///
    /// let book = Book::open("input.epub")?;
    /// book.save("output.azw3")?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let format = Format::from_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown file format: {}", path.display()),
            )
        })?;
        self.save_format(path, format)
    }

    /// Save the book to a file with an explicit format.
    ///
    /// Note: KFX format is read-only. Attempting to save as KFX will return an error.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use boko::{Book, Format};
    ///
    /// let book = Book::open("input.epub")?;
    /// book.save_format("output.bin", Format::Azw3)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn save_format(&self, path: impl AsRef<Path>, format: Format) -> io::Result<()> {
        match format {
            Format::Epub => crate::epub::write_epub(self, path),
            Format::Azw3 | Format::Mobi => crate::mobi::write_mobi(self, path),
            Format::Kfx => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "KFX format is read-only; save as EPUB or AZW3 instead",
            )),
        }
    }

    /// Add a resource to the book
    pub fn add_resource(
        &mut self,
        href: impl Into<String>,
        data: Vec<u8>,
        media_type: impl Into<String>,
    ) {
        self.resources.insert(
            href.into(),
            Resource {
                data,
                media_type: media_type.into(),
            },
        );
    }

    /// Get a resource by href
    pub fn get_resource(&self, href: &str) -> Option<&Resource> {
        self.resources.get(href)
    }

    /// Add a spine item
    pub fn add_spine_item(
        &mut self,
        id: impl Into<String>,
        href: impl Into<String>,
        media_type: impl Into<String>,
    ) {
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
