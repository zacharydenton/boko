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
//! | Format   | Read | Write |
//! |----------|------|-------|
//! | KFX      | ✓    | ✓     |
//! | AZW3     | ✓    | ✓     |
//! | EPUB     | ✓    | ✓     |
//! | MOBI     | ✓    | -     |
//! | Markdown | -    | ✓     |
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
//! # Ok::<(), boko::Error>(())
//! ```

mod book;
pub(crate) mod dom;
pub mod error;
pub mod export;
pub mod import;
pub(crate) mod io;
pub(crate) mod markdown;
pub mod model;
mod resolved;
pub mod style;

pub(crate) mod epub;
/// KFX format internals (Ion codec, container layout, symbol tables).
///
/// Exposed for boko's own tooling (`boko kfx-dump`, fuzz targets, fixture
/// generators) — **not** part of the stable API; contents may change in any
/// release without a major-version bump.
#[doc(hidden)]
pub mod kfx;
pub(crate) mod mobi;

pub(crate) mod util;

#[cfg(feature = "wasm")]
pub mod wasm;

// Error handling
pub use error::{Error, Result};

// Primary exports from model
pub use model::{
    Book, Chapter, ContentBlock, Format, Metadata, Node, NodeId, Resource, Role, SectionNode,
    SectionTree, SemanticMap, TextRange, TocEntry, extract_section_tree,
};

// Primary exports from style
pub use style::{ComputedStyle, ListStyleType, Origin, StyleId, StylePool, Stylesheet, ToCss};

// Primary exports from dom
pub use dom::compile_html;

// Primary exports from other modules
pub use export::{
    Azw3Config, Azw3Exporter, EpubConfig, EpubExporter, Exporter, KfxExporter, MarkdownConfig,
    MarkdownExporter,
};
pub use import::{ChapterId, Importer, SpineEntry};
pub use io::{ByteSource, FileSource};
