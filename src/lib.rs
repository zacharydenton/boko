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
//! The simplest way to convert ebooks:
//!
//! ```no_run
//! use boko::Book;
//!
//! // Convert EPUB to AZW3
//! let book = Book::open("input.epub")?;
//! book.save("output.azw3")?;
//!
//! // Convert AZW3/MOBI to EPUB
//! let book = Book::open("input.azw3")?;
//! book.save("output.epub")?;
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! For explicit format control (Format: Epub, Azw3, Mobi):
//!
//! ```no_run
//! use boko::{Book, Format};
//!
//! let book = Book::open_format("input.bin", Format::Mobi)?;
//! book.save_format("output.bin", Format::Azw3)?;
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Free functions are also available:
//!
//! ```no_run
//! use boko::{read_epub, write_mobi};
//!
//! let book = read_epub("input.epub")?;
//! write_mobi(&book, "output.azw3")?;
//! # Ok::<(), std::io::Error>(())
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
pub mod kfx;
pub mod mobi;
pub(crate) mod util;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use book::{Book, Format, Metadata, Resource, SpineItem, TocEntry};
pub use epub::{read_epub, read_epub_from_reader, write_epub, write_epub_to_writer};
pub use kfx::{read_kfx, read_kfx_from_reader, write_kfx, write_kfx_to_writer};
pub use mobi::{read_mobi, read_mobi_from_reader, write_mobi, write_mobi_to_writer};
