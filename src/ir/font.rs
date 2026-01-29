//! Font face definitions for @font-face rules.
//!
//! This module provides a representation of CSS @font-face rules,
//! which map font family names to font files with specific weight/style combinations.

use super::{FontStyle, FontWeight};

/// A parsed @font-face rule.
///
/// Maps a font family name to a font resource file with specific weight and style.
/// Used by KFX export to create font entities linking font_family to resource location.
#[derive(Debug, Clone)]
pub struct FontFace {
    /// The font family name (e.g., "Ubuntu", "UbuntuMono").
    pub font_family: String,
    /// The font weight (normal, bold, or numeric 100-900).
    pub font_weight: FontWeight,
    /// The font style (normal, italic, oblique).
    pub font_style: FontStyle,
    /// The source path to the font file (relative to the EPUB root).
    /// e.g., "fonts/Ubuntu-M.ttf" or "../fonts/Ubuntu-M.ttf"
    pub src: String,
}

impl FontFace {
    /// Create a new font face definition.
    pub fn new(
        font_family: impl Into<String>,
        font_weight: FontWeight,
        font_style: FontStyle,
        src: impl Into<String>,
    ) -> Self {
        Self {
            font_family: font_family.into(),
            font_weight,
            font_style,
            src: src.into(),
        }
    }
}
