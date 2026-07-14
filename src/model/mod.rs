//! Core data model for ebook processing.
//!
//! This module contains:
//! - Book metadata and format types (pure data)
//! - Chapter representation (IR tree structure)
//! - Node types and semantic roles
//! - Semantic attributes (href, src, alt, etc.)
//! - Link representation for internal/external links
//! - Font face definitions
//!
//! The `Book` runtime handle lives in `crate::book` (re-exported here for
//! backwards compatibility), keeping this module free of importer/exporter
//! dependencies.

mod chapter;
mod font;
mod links;
mod metadata;
mod node;
pub mod section_tree;
mod semantic;

// Re-export pure book data types
pub use metadata::{
    CollectionInfo, Contributor, Format, Landmark, LandmarkType, Metadata, Resource, TocEntry,
};

// Re-export the Book runtime handle (moved to crate::book; kept here so
// `boko::model::Book` remains a valid path)
pub use crate::book::Book;

// Re-export chapter and iteration
pub use chapter::{Chapter, ChildIter, DfsIter};

// Re-export node types
pub use node::{Node, NodeId, Role, TextRange};

// Re-export semantic attributes
pub use semantic::SemanticMap;

// Re-export link types
pub use links::{AnchorTarget, ChapterId, GlobalNodeId, InternalLocation, Link, LinkTarget};

// Re-export resolved links (moved to crate::resolved; kept here so
// `boko::model::ResolvedLinks` remains a valid path)
pub use crate::resolved::ResolvedLinks;

// Re-export font types
pub use font::FontFace;

// Re-export section tree
pub use section_tree::{ContentBlock, SectionNode, SectionTree, extract_section_tree};
