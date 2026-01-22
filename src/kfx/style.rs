//! KFX style to CSS conversion.
//!
//! This module provides functions to parse KFX style ION data into ParsedStyle,
//! enabling bidirectional conversion between KFX and CSS.

use crate::css::{
    BreakValue, CssValue, FontStyle, FontVariant, FontWeight, ParsedStyle, TextAlign,
    VerticalAlign,
};
use crate::kfx::ion::IonValue;
use crate::kfx::writer::symbols::sym;

/// Parse a KFX style fragment ION value into a ParsedStyle.
/// This enables KFX → CSS conversion for KFX → EPUB export.
pub fn kfx_style_to_parsed(ion: &IonValue) -> ParsedStyle {
    let mut style = ParsedStyle::default();

    let map = match ion {
        IonValue::Struct(m) => m,
        IonValue::Annotated(_, inner) => {
            if let IonValue::Struct(m) = inner.as_ref() {
                m
            } else {
                return style;
            }
        }
        _ => return style,
    };

    // Font family ($11)
    if let Some(IonValue::String(s)) = map.get(&sym::FONT_FAMILY) {
        style.font_family = Some(s.clone());
    }

    // Font size ($16)
    if let Some(val) = map.get(&sym::FONT_SIZE) {
        style.font_size = parse_value_with_unit(val, true);
    }

    // Font weight ($13)
    if let Some(val) = map.get(&sym::FONT_WEIGHT) {
        style.font_weight = parse_font_weight(val);
    }

    // Font style ($12)
    if let Some(val) = map.get(&sym::FONT_STYLE) {
        style.font_style = parse_font_style(val);
    }

    // Font variant ($583)
    if let Some(val) = map.get(&sym::FONT_VARIANT) {
        style.font_variant = parse_font_variant(val);
    }

    // Text align ($34)
    if let Some(val) = map.get(&sym::TEXT_ALIGN) {
        style.text_align = parse_text_align(val);
    }

    // Text indent ($36)
    if let Some(val) = map.get(&sym::TEXT_INDENT) {
        style.text_indent = parse_value_with_unit(val, false);
    }

    // Line height ($42)
    if let Some(val) = map.get(&sym::LINE_HEIGHT) {
        style.line_height = parse_value_with_unit(val, false);
    }

    // Margins
    if let Some(val) = map.get(&sym::SPACE_BEFORE) {
        style.margin_top = parse_value_with_unit(val, false);
    }
    if let Some(val) = map.get(&sym::SPACE_AFTER) {
        style.margin_bottom = parse_value_with_unit(val, false);
    }
    if let Some(val) = map.get(&sym::MARGIN_LEFT) {
        style.margin_left = parse_value_with_unit(val, false);
    }
    if let Some(val) = map.get(&sym::MARGIN_RIGHT) {
        style.margin_right = parse_value_with_unit(val, false);
    }

    // Color ($19)
    if let Some(IonValue::Int(v)) = map.get(&sym::COLOR) {
        let r = ((v >> 16) & 0xFF) as u8;
        let g = ((v >> 8) & 0xFF) as u8;
        let b = (v & 0xFF) as u8;
        style.color = Some(crate::css::Color::Rgba(r, g, b, 255));
    }

    // Background color ($21)
    if let Some(IonValue::Int(v)) = map.get(&sym::BACKGROUND_COLOR) {
        let r = ((v >> 16) & 0xFF) as u8;
        let g = ((v >> 8) & 0xFF) as u8;
        let b = (v & 0xFF) as u8;
        style.background_color = Some(crate::css::Color::Rgba(r, g, b, 255));
    }

    // Dimensions
    if let Some(val) = map.get(&sym::STYLE_WIDTH) {
        style.width = parse_value_with_unit(val, false);
    }
    if let Some(val) = map.get(&sym::STYLE_HEIGHT) {
        style.height = parse_value_with_unit(val, false);
    }
    if let Some(val) = map.get(&sym::MAX_WIDTH) {
        style.max_width = parse_value_with_unit(val, false);
    }
    if let Some(val) = map.get(&sym::MIN_WIDTH) {
        style.min_width = parse_value_with_unit(val, false);
    }
    if let Some(val) = map.get(&sym::MIN_HEIGHT) {
        style.min_height = parse_value_with_unit(val, false);
    }

    // Vertical align ($44)
    if let Some(val) = map.get(&sym::VERTICAL_ALIGN) {
        style.vertical_align = parse_vertical_align(val);
    }

    // White space nowrap ($45)
    if let Some(IonValue::Bool(b)) = map.get(&sym::WHITE_SPACE_NOWRAP) {
        style.white_space_nowrap = Some(*b);
    }

    // Text decorations
    if let Some(IonValue::Symbol(s)) = map.get(&sym::TEXT_DECORATION_UNDERLINE) {
        if *s == sym::DECORATION_PRESENT {
            style.text_decoration_underline = true;
        }
    }
    if let Some(IonValue::Symbol(s)) = map.get(&sym::TEXT_DECORATION_LINE_THROUGH) {
        if *s == sym::DECORATION_PRESENT {
            style.text_decoration_line_through = true;
        }
    }
    if let Some(IonValue::Symbol(s)) = map.get(&sym::TEXT_DECORATION_OVERLINE) {
        if *s == sym::DECORATION_PRESENT {
            style.text_decoration_overline = true;
        }
    }

    // Break properties
    if let Some(val) = map.get(&sym::BREAK_BEFORE) {
        style.break_before = parse_break_value(val);
    }
    if let Some(val) = map.get(&sym::BREAK_AFTER) {
        style.break_after = parse_break_value(val);
    }
    if let Some(val) = map.get(&sym::BREAK_INSIDE) {
        style.break_inside = parse_break_value(val);
    }

    // Letter spacing ($32)
    if let Some(val) = map.get(&sym::LETTER_SPACING) {
        style.letter_spacing = parse_value_with_unit(val, false);
    }

    // Word spacing ($33)
    if let Some(val) = map.get(&sym::WORD_SPACING) {
        style.word_spacing = parse_value_with_unit(val, false);
    }

    // Opacity ($72)
    if let Some(val) = map.get(&sym::OPACITY) {
        if let Some(f) = extract_decimal(val) {
            style.opacity = Some((f * 100.0) as u8);
        }
    }

    // Language ($10)
    if let Some(IonValue::String(s)) = map.get(&sym::LANGUAGE) {
        style.lang = Some(s.clone());
    }

    style
}

/// Parse a KFX value struct {$306: unit, $307: value} into CssValue
fn parse_value_with_unit(ion: &IonValue, is_font_size: bool) -> Option<CssValue> {
    let map = match ion {
        IonValue::Struct(m) => m,
        _ => return None,
    };

    let unit_sym = match map.get(&sym::UNIT) {
        Some(IonValue::Symbol(s)) => *s,
        _ => return None,
    };

    let value = extract_decimal(map.get(&sym::VALUE)?)?;

    match unit_sym {
        sym::UNIT_EM | sym::UNIT_EM_FONTSIZE => Some(CssValue::Em(value)),
        sym::UNIT_PERCENT => Some(CssValue::Percent(value)),
        sym::UNIT_PX => Some(CssValue::Px(value)),
        sym::UNIT_MULTIPLIER => {
            // UNIT_MULTIPLIER is unitless for line-height, em for margins
            if is_font_size {
                Some(CssValue::Em(value))
            } else {
                Some(CssValue::Number(value))
            }
        }
        _ => None,
    }
}

/// Extract a decimal/float value from ION
fn extract_decimal(ion: &IonValue) -> Option<f32> {
    match ion {
        IonValue::Int(i) => Some(*i as f32),
        IonValue::Decimal(bytes) => Some(decode_kfx_decimal(bytes)),
        _ => None,
    }
}

/// Decode a KFX decimal value (coefficient + exponent)
fn decode_kfx_decimal(bytes: &[u8]) -> f32 {
    if bytes.is_empty() {
        return 0.0;
    }

    // Simple decoding: first byte is exponent (signed), rest is coefficient (signed varint)
    let exp = bytes[0] as i8 as i32;

    // Decode coefficient as signed varint
    let mut coef: i64 = 0;
    let mut shift = 0;
    for &byte in &bytes[1..] {
        coef |= ((byte & 0x7F) as i64) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
    }

    // Handle negative coefficient (sign bit)
    if bytes.len() > 1 && bytes[bytes.len() - 1] & 0x40 != 0 {
        coef = -coef;
    }

    coef as f32 * 10f32.powi(exp)
}

/// Parse font weight from KFX symbol
fn parse_font_weight(ion: &IonValue) -> Option<FontWeight> {
    let sym_id = match ion {
        IonValue::Symbol(s) => *s,
        _ => return None,
    };

    match sym_id {
        sym::FONT_WEIGHT_NORMAL => Some(FontWeight::Normal),
        sym::FONT_WEIGHT_BOLD => Some(FontWeight::Bold),
        sym::FONT_WEIGHT_100 => Some(FontWeight::Weight(100)),
        sym::FONT_WEIGHT_200 => Some(FontWeight::Weight(200)),
        sym::FONT_WEIGHT_300 => Some(FontWeight::Weight(300)),
        sym::FONT_WEIGHT_500 => Some(FontWeight::Weight(500)),
        sym::FONT_WEIGHT_600 => Some(FontWeight::Weight(600)),
        sym::FONT_WEIGHT_800 => Some(FontWeight::Weight(800)),
        sym::FONT_WEIGHT_900 => Some(FontWeight::Weight(900)),
        _ => None,
    }
}

/// Parse font style from KFX symbol
fn parse_font_style(ion: &IonValue) -> Option<FontStyle> {
    let sym_id = match ion {
        IonValue::Symbol(s) => *s,
        _ => return None,
    };

    match sym_id {
        sym::FONT_STYLE_NORMAL => Some(FontStyle::Normal),
        sym::FONT_STYLE_ITALIC => Some(FontStyle::Italic),
        sym::FONT_STYLE_OBLIQUE => Some(FontStyle::Oblique),
        _ => None,
    }
}

/// Parse font variant from KFX symbol
fn parse_font_variant(ion: &IonValue) -> Option<FontVariant> {
    let sym_id = match ion {
        IonValue::Symbol(s) => *s,
        _ => return None,
    };

    match sym_id {
        sym::FONT_VARIANT_NORMAL => Some(FontVariant::Normal),
        sym::FONT_VARIANT_SMALL_CAPS => Some(FontVariant::SmallCaps),
        _ => None,
    }
}

/// Parse text align from KFX symbol
fn parse_text_align(ion: &IonValue) -> Option<TextAlign> {
    let sym_id = match ion {
        IonValue::Symbol(s) => *s,
        _ => return None,
    };

    match sym_id {
        sym::ALIGN_LEFT => Some(TextAlign::Left),
        sym::ALIGN_RIGHT => Some(TextAlign::Right),
        sym::ALIGN_CENTER => Some(TextAlign::Center),
        sym::ALIGN_JUSTIFY => Some(TextAlign::Justify),
        _ => None,
    }
}

/// Parse vertical align from KFX symbol
fn parse_vertical_align(ion: &IonValue) -> Option<VerticalAlign> {
    let sym_id = match ion {
        IonValue::Symbol(s) => *s,
        _ => return None,
    };

    match sym_id {
        sym::FONT_WEIGHT_NORMAL => Some(VerticalAlign::Baseline), // $350 = baseline
        sym::VERTICAL_TOP => Some(VerticalAlign::Top),
        sym::VERTICAL_BOTTOM => Some(VerticalAlign::Bottom),
        sym::VERTICAL_SUPER => Some(VerticalAlign::Super),
        sym::VERTICAL_SUB => Some(VerticalAlign::Sub),
        sym::VERTICAL_TEXT_TOP => Some(VerticalAlign::TextTop),
        sym::VERTICAL_TEXT_BOTTOM => Some(VerticalAlign::TextBottom),
        sym::ALIGN_CENTER => Some(VerticalAlign::Middle), // $320 = middle
        _ => None,
    }
}

/// Parse break value from KFX symbol
fn parse_break_value(ion: &IonValue) -> Option<BreakValue> {
    let sym_id = match ion {
        IonValue::Symbol(s) => *s,
        _ => return None,
    };

    match sym_id {
        sym::BREAK_AUTO => Some(BreakValue::Auto),   // $383 = auto
        sym::BREAK_AVOID => Some(BreakValue::Avoid), // $353 = avoid
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_parse_simple_style() {
        let mut map = HashMap::new();
        map.insert(sym::FONT_WEIGHT, IonValue::Symbol(sym::FONT_WEIGHT_BOLD));
        map.insert(sym::FONT_STYLE, IonValue::Symbol(sym::FONT_STYLE_ITALIC));
        map.insert(sym::TEXT_ALIGN, IonValue::Symbol(sym::ALIGN_CENTER));

        let style = kfx_style_to_parsed(&IonValue::Struct(map));

        assert_eq!(style.font_weight, Some(FontWeight::Bold));
        assert_eq!(style.font_style, Some(FontStyle::Italic));
        assert_eq!(style.text_align, Some(TextAlign::Center));
    }

    #[test]
    fn test_parse_value_with_unit() {
        let mut val_struct = HashMap::new();
        val_struct.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
        val_struct.insert(sym::VALUE, IonValue::Int(2));

        let result = parse_value_with_unit(&IonValue::Struct(val_struct), false);
        assert!(matches!(result, Some(CssValue::Em(v)) if (v - 2.0).abs() < 0.01));
    }

    #[test]
    fn test_to_css_string() {
        let mut style = ParsedStyle::default();
        style.font_weight = Some(FontWeight::Bold);
        style.font_style = Some(FontStyle::Italic);
        style.text_align = Some(TextAlign::Center);
        style.margin_top = Some(CssValue::Em(2.0));

        let css = style.to_css_string();
        assert!(css.contains("font-weight: bold"));
        assert!(css.contains("font-style: italic"));
        assert!(css.contains("text-align: center"));
        assert!(css.contains("margin-top: 2em"));
    }

    #[test]
    fn test_kfx_to_css_roundtrip() {
        // Create a KFX style ION structure
        let mut map = HashMap::new();
        map.insert(sym::FONT_WEIGHT, IonValue::Symbol(sym::FONT_WEIGHT_BOLD));
        map.insert(sym::FONT_STYLE, IonValue::Symbol(sym::FONT_STYLE_ITALIC));
        map.insert(sym::TEXT_ALIGN, IonValue::Symbol(sym::ALIGN_CENTER));
        map.insert(sym::FONT_VARIANT, IonValue::Symbol(sym::FONT_VARIANT_SMALL_CAPS));
        map.insert(sym::WHITE_SPACE_NOWRAP, IonValue::Bool(true));
        map.insert(sym::BREAK_INSIDE, IonValue::Symbol(sym::BREAK_AVOID));

        // Add margin with value struct
        let mut margin_val = HashMap::new();
        margin_val.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
        margin_val.insert(sym::VALUE, IonValue::Int(2));
        map.insert(sym::SPACE_BEFORE, IonValue::Struct(margin_val));

        // KFX → ParsedStyle
        let parsed = kfx_style_to_parsed(&IonValue::Struct(map));

        // ParsedStyle → CSS string
        let css = parsed.to_css_string();

        // Verify CSS contains expected properties
        assert!(css.contains("font-weight: bold"), "CSS: {}", css);
        assert!(css.contains("font-style: italic"), "CSS: {}", css);
        assert!(css.contains("text-align: center"), "CSS: {}", css);
        assert!(css.contains("font-variant: small-caps"), "CSS: {}", css);
        assert!(css.contains("white-space: nowrap"), "CSS: {}", css);
        assert!(css.contains("break-inside: avoid"), "CSS: {}", css);
        assert!(css.contains("margin-top: 2em"), "CSS: {}", css);

        // CSS string → ParsedStyle (via Stylesheet parser)
        let reparsed = crate::css::Stylesheet::parse_inline_style(&css);

        // Verify roundtrip preserves values
        assert_eq!(reparsed.font_weight, Some(FontWeight::Bold));
        assert_eq!(reparsed.font_style, Some(FontStyle::Italic));
        assert_eq!(reparsed.text_align, Some(TextAlign::Center));
        assert_eq!(reparsed.font_variant, Some(FontVariant::SmallCaps));
        assert_eq!(reparsed.white_space_nowrap, Some(true));
        assert_eq!(reparsed.break_inside, Some(BreakValue::Avoid));
        assert!(matches!(reparsed.margin_top, Some(CssValue::Em(v)) if (v - 2.0).abs() < 0.01));
    }
}
