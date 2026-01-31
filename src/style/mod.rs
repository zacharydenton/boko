//! Style system for CSS property types, computed styles, and cascade.
//!
//! This module contains:
//! - CSS property types (Color, Length, Display, etc.)
//! - ComputedStyle and StylePool for style management
//! - Declaration parsing and stylesheet handling
//! - CSS cascade implementation

mod cascade;
mod declaration;
pub(crate) mod parse;
mod properties;
mod types;

// Re-export the ToCss trait
pub trait ToCss {
    /// Write this value as CSS to the buffer.
    fn to_css(&self, buf: &mut String);

    /// Convert to a CSS string (convenience method).
    fn to_css_string(&self) -> String {
        let mut buf = String::new();
        self.to_css(&mut buf);
        buf
    }
}

// Re-export property types
pub use properties::{
    BorderCollapse, BorderStyle, BoxSizing, BreakValue, Clear, Color, DecorationStyle, Display,
    Float, FontStyle, FontVariant, FontWeight, Hyphens, Length, ListStylePosition, ListStyleType,
    OverflowWrap, TextAlign, TextTransform, VerticalAlign, Visibility, WhiteSpace, WordBreak,
};

// Re-export core style types
pub use types::{ComputedStyle, StyleId, StylePool};

// Re-export declaration type (kept minimal)
pub use declaration::Declaration;

// Re-export stylesheet types from parse module
pub use parse::{CssRule, Origin, Specificity, Stylesheet, TextDecorationValue};

// Re-export cascade function
pub use cascade::compute_styles;

// Re-export macro for internal use
#[allow(unused_imports)]
pub(crate) use properties::enum_property;
