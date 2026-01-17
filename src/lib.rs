//! # boko
//!
//! A fast, lightweight library for reading and writing EPUB and MOBI/AZW3 ebooks.
//!
//! ## Features
//!
//! - Read and write EPUB 2/3 files
//! - Read and write MOBI/AZW3 (KF8) files
//! - Convert between formats via intermediate [`Book`] representation
//! - Preserves metadata, table of contents, images, fonts, and CSS
//!
//! ## Quick Start
//!
//! ```no_run
//! use boko::{read_epub, write_mobi, read_mobi, write_epub};
//!
//! // Convert EPUB to AZW3
//! let book = read_epub("input.epub").unwrap();
//! write_mobi(&book, "output.azw3").unwrap();
//!
//! // Convert AZW3 to EPUB
//! let book = read_mobi("input.azw3").unwrap();
//! write_epub(&book, "output.epub").unwrap();
//! ```
//!
//! ## Working with Books
//!
//! The [`Book`] struct is the central data type, representing an ebook in a
//! format-agnostic way:
//!
//! ```
//! use boko::{Book, Metadata, TocEntry};
//!
//! let mut book = Book::new();
//! book.metadata = Metadata::new("My Book")
//!     .with_author("Author Name")
//!     .with_language("en");
//!
//! // Add content
//! book.add_resource("chapter1.xhtml", b"<html>...</html>".to_vec(), "application/xhtml+xml");
//! book.add_spine_item("ch1", "chapter1.xhtml", "application/xhtml+xml");
//!
//! // Add table of contents
//! book.toc.push(TocEntry::new("Chapter 1", "chapter1.xhtml"));
//! ```

pub mod book;
pub mod epub;
pub mod mobi;
pub(crate) mod util;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use book::{Book, Metadata, Resource, SpineItem, TocEntry};
pub use epub::{read_epub, write_epub};
pub use mobi::{read_mobi, write_mobi};
