//! CSS parsing for style extraction.
//!
//! This module provides CSS parsing capabilities for extracting styles
//! from EPUB stylesheets to apply to KFX output.
//!
//! # Module Structure
//!
//! - `types`: CSS value types and enums (CssValue, TextAlign, FontWeight, etc.)
//! - `style`: ParsedStyle struct with style merging and comparison
//! - `parsing`: CSS property parsing functions
//! - `stylesheet`: Stylesheet parsing and selector matching

mod parsing;
mod style;
mod stylesheet;
mod types;

#[cfg(test)]
mod tests;

// Re-export all public types for external use
pub use style::ParsedStyle;
pub use stylesheet::{NodeRef, Stylesheet};
pub use types::*;

// Re-export kuchiki types needed by external code
