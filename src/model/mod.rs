//! Core data model for ebook processing.
//!
//! This module contains:
//! - Book metadata and runtime handle
//! - Chapter representation (IR tree structure)
//! - Node types and semantic roles
//! - Semantic attributes (href, src, alt, etc.)
//! - Link representation for internal/external links
//! - Font face definitions

mod book;
mod chapter;
mod font;
mod links;
mod node;
mod resolved;
pub mod section_tree;
mod semantic;

// Re-export book types
pub use book::{
    Book, CollectionInfo, Contributor, Format, Landmark, LandmarkType, Metadata, Resource, TocEntry,
};

// Re-export chapter and iteration
pub use chapter::{Chapter, ChildIter, DfsIter};

// Re-export node types
pub use node::{Node, NodeId, Role, TextRange};

// Re-export semantic attributes
pub use semantic::SemanticMap;

// Re-export link types
pub use links::{AnchorTarget, GlobalNodeId, InternalLocation, Link, LinkTarget};

// Re-export resolved links
pub use resolved::ResolvedLinks;

// Re-export font types
pub use font::FontFace;

// Re-export section tree
pub use section_tree::{ContentBlock, SectionNode, SectionTree, extract_section_tree};
