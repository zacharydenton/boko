//! Font property conversion (font-size, font-weight, font-style).

use std::collections::HashMap;

use crate::css::{CssValue, FontStyle, FontWeight, ParsedStyle};
use crate::kfx::ion::{IonValue, encode_kfx_decimal};
use crate::kfx::writer::symbols::sym;

/// Add all font properties to the style ION struct
pub fn add_all(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle, is_image: bool) {
    add_font_size(style_ion, style, is_image);
    add_font_weight(style_ion, style);
    add_font_style(style_ion, style);
}

/// Add font-size property
fn add_font_size(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle, is_image: bool) {
    // Images don't get font-size
    if is_image {
        return;
    }

    // Only add font-size if explicitly set (reference omits default 1em)
    if let Some(ref size) = style.font_size {
        let size_val: f32 = match size {
            CssValue::Em(v) | CssValue::Rem(v) => *v,
            CssValue::Percent(v) => *v / 100.0,
            CssValue::Keyword(k) => match k.as_str() {
                "smaller" => 0.833333,
                "larger" => 1.2,
                "xx-small" => 0.5625,
                "x-small" => 0.625,
                "small" => 0.8125,
                "medium" => 1.0,
                "large" => 1.125,
                "x-large" => 1.5,
                "xx-large" => 2.0,
                _ => return, // Unknown keyword, skip
            },
            CssValue::Px(v) => *v / 16.0, // Assume 16px = 1em
            _ => return,
        };

        // Skip if value is effectively 1.0 (default)
        if (size_val - 1.0).abs() < 0.001 {
            return;
        }

        let mut s = HashMap::new();
        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM_FONTSIZE));
        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(size_val)));
        style_ion.insert(sym::FONT_SIZE, IonValue::Struct(s));
    }
}

/// Add font-weight property
fn add_font_weight(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref weight) = style.font_weight {
        let weight_sym = if weight.is_bold() {
            sym::FONT_WEIGHT_BOLD
        } else {
            match weight {
                FontWeight::Weight(100) => sym::FONT_WEIGHT_100,
                FontWeight::Weight(200) => sym::FONT_WEIGHT_200,
                FontWeight::Weight(300) => sym::FONT_WEIGHT_300,
                FontWeight::Weight(400) => sym::FONT_WEIGHT_NORMAL,
                FontWeight::Weight(500) => sym::FONT_WEIGHT_500,
                FontWeight::Weight(600) => sym::FONT_WEIGHT_600,
                FontWeight::Weight(700) => sym::FONT_WEIGHT_BOLD,
                FontWeight::Weight(800) => sym::FONT_WEIGHT_800,
                FontWeight::Weight(900) => sym::FONT_WEIGHT_900,
                FontWeight::Weight(n) if *n < 400 => sym::FONT_WEIGHT_300,
                FontWeight::Weight(n) if *n < 600 => sym::FONT_WEIGHT_500,
                FontWeight::Weight(_) => sym::FONT_WEIGHT_BOLD,
                FontWeight::Normal => sym::FONT_WEIGHT_NORMAL,
                FontWeight::Bold => sym::FONT_WEIGHT_BOLD,
            }
        };
        // Include font-weight (Kindle Previewer includes normal explicitly)
        style_ion.insert(sym::FONT_WEIGHT, IonValue::Symbol(weight_sym));
    }
}

/// Add font-style property
fn add_font_style(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(style_type) = style.font_style {
        let style_sym = match style_type {
            FontStyle::Italic => sym::FONT_STYLE_ITALIC,
            FontStyle::Oblique => sym::FONT_STYLE_OBLIQUE,
            FontStyle::Normal => sym::FONT_STYLE_NORMAL,
        };
        // Include font-style (Kindle Previewer includes normal explicitly)
        style_ion.insert(sym::FONT_STYLE, IonValue::Symbol(style_sym));
    }
}
