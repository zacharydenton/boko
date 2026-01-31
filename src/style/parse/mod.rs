//! CSS parsing utilities.

pub(crate) mod border;
pub(crate) mod box_model;
pub(crate) mod font;
pub(crate) mod keywords;
pub(crate) mod values;

mod stylesheet;

// Public types only
pub use stylesheet::{CssRule, Origin, Specificity, Stylesheet};
pub use values::TextDecorationValue;
