//! # boko
//!
//! A high-performance, format-agnostic ebook processing engine.
//!
//! ## Architecture
//!
//! Boko uses an **Importer** architecture for reading ebooks:
//! - `Book` is the runtime handle that wraps format-specific backends
//! - `Importer` trait defines the interface for format backends
//! - Lazy loading via `ByteSource` for efficient random access
//!
//! ## Supported Formats
//!
//! | Format | Read | Write |
//! |--------|------|-------|
//! | EPUB   | ✓    | TODO  |
//! | AZW3   | ✓    | TODO  |
//! | MOBI   | ✓    | TODO  |
//!
//! ## Quick Start
//!
//! ```no_run
//! use boko::Book;
//!
//! let mut book = Book::open("input.epub")?;
//! println!("Title: {}", book.metadata().title);
//!
//! // Iterate chapters (collect spine first to avoid borrow issues)
//! let spine: Vec<_> = book.spine().to_vec();
//! for entry in spine {
//!     let content = book.load_raw(entry.id)?;
//!     println!("Chapter: {} bytes", content.len());
//! }
//! # Ok::<(), std::io::Error>(())
//! ```

pub mod book;
pub mod import;
pub mod io;

pub mod epub;
pub mod kfx;
pub mod mobi;

#[cfg(feature = "wasm")]
pub mod wasm;

// Primary exports
pub use book::{Book, Format, Metadata, TocEntry};
pub use import::{ChapterId, Importer, SpineEntry};
pub use io::{ByteSource, FileSource};
