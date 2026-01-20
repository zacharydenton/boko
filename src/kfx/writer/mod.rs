//! KFX writer module - converts Book to KFX format.
//!
//! This module is organized into focused submodules:
//! - `symbols`: Symbol IDs and symbol table management
//! - `fragment`: KFX fragment representation
//! - `content`: Content types, extraction, and chunking
//! - `style`: CSS to KFX style conversion
//! - `navigation`: TOC, landmarks, and anchor handling
//! - `position`: EID calculation and position maps
//! - `resources`: Image and font resource handling
//! - `serialization`: Binary container format
//! - `builder`: Main orchestration

pub mod builder;
pub mod content;
pub mod fragment;
pub mod navigation;
pub mod position;
pub mod resources;
pub mod serialization;
pub mod style;
pub mod symbols;

// Re-export the public API
pub use builder::{write_kfx, write_kfx_to_writer, KfxBookBuilder};
pub use content::{ChapterData, ContentChunk, ContentItem, StyleRun};
pub use fragment::KfxFragment;
pub use symbols::{sym, SymbolTable};
