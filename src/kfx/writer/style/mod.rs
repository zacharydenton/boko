//! CSS to KFX style conversion.
//!
//! This module handles converting CSS properties to KFX ION format,
//! including the ToKfxIon trait and specialized property modules.
//!
//! # Axis-Specific Unit Conversion
//!
//! KFX uses different unit systems for vertical and horizontal spacing:
//!
//! - **Vertical (top/bottom)**: Uses `UNIT_MULTIPLIER` ($310), values normalized
//!   by dividing by 1.2 (default line-height). CSS `margin-top: 1em` becomes
//!   `{$306: $310, $307: 0.833}` (1.0 / 1.2 ≈ 0.833).
//!
//! - **Horizontal (left/right)**: Uses `UNIT_PERCENT` ($314) directly.
//!   CSS `margin-left: 5%` becomes `{$306: $314, $307: 5.0}`.
//!
//! This matches Kindle Previewer's output format, where vertical spacing is
//! relative to line-height and horizontal spacing is relative to container width.
//!
//! See [`add_margins`], [`add_padding`], and [`spacing_to_multiplier`] for details.

mod conversion;
pub mod font;
pub mod layout;
pub mod spacing;

pub use conversion::*;

use std::collections::HashMap;

use crate::css::{Border, BorderStyle, Color, CssValue};
use crate::kfx::ion::{IonValue, encode_kfx_decimal};

use super::symbols::sym;

/// Trait for converting CSS values to KFX ION representation
pub trait ToKfxIon {
    /// Convert to KFX ION value, returning None if the value should be omitted
    fn to_kfx_ion(&self) -> Option<IonValue>;
}

// ============================================================================
// Unit Value Construction
// ============================================================================

/// Build a KFX unit-value struct: {$306: unit_symbol, $307: decimal_value}
/// Returns None if value is effectively zero (abs < 0.001)
fn unit_value(unit_sym: u64, value: f32) -> Option<IonValue> {
    if value.abs() < 0.001 {
        return None;
    }
    let mut s = HashMap::new();
    s.insert(sym::UNIT, IonValue::Symbol(unit_sym));
    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(value)));
    Some(IonValue::Struct(s))
}

/// Build a KFX unit-value struct, always (even for zero values)
/// Used when zero is a meaningful value that should be output
pub fn unit_value_always(unit_sym: u64, value: f32) -> IonValue {
    let mut s = HashMap::new();
    s.insert(sym::UNIT, IonValue::Symbol(unit_sym));
    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(value)));
    IonValue::Struct(s)
}

// ============================================================================
// CssValue → KFX ION Conversion
// ============================================================================

/// Convert a CssValue to Ion for margins/padding
/// Format: {$306: unit_symbol, $307: decimal_value}
impl ToKfxIon for CssValue {
    fn to_kfx_ion(&self) -> Option<IonValue> {
        match self {
            // Relative units converted to percent
            CssValue::Px(v) => unit_value(sym::UNIT_PERCENT, *v * 0.117),
            CssValue::Em(v) | CssValue::Rem(v) => unit_value(sym::UNIT_PERCENT, *v * 3.125),
            CssValue::Percent(v) => unit_value(sym::UNIT_PERCENT, *v),
            CssValue::Number(v) => unit_value(sym::UNIT_MULTIPLIER, *v),

            // Viewport units
            CssValue::Vw(v) => unit_value(sym::UNIT_VW, *v),
            CssValue::Vh(v) => unit_value(sym::UNIT_VH, *v),
            CssValue::Vmin(v) => unit_value(sym::UNIT_VMIN, *v),
            CssValue::Vmax(v) => unit_value(sym::UNIT_VMAX, *v),

            // Font-relative units
            CssValue::Ch(v) => unit_value(sym::UNIT_CH, *v),
            CssValue::Ex(v) => unit_value(sym::UNIT_EX, *v),

            // Physical units
            CssValue::Cm(v) => unit_value(sym::UNIT_CM, *v),
            CssValue::Mm(v) => unit_value(sym::UNIT_MM, *v),
            CssValue::In(v) => unit_value(sym::UNIT_IN, *v),
            CssValue::Pt(v) => unit_value(sym::UNIT_PX, *v), // pt uses px symbol

            _ => None,
        }
    }
}

impl ToKfxIon for Color {
    fn to_kfx_ion(&self) -> Option<IonValue> {
        match self {
            Color::Rgba(r, g, b, _a) => {
                // Serialize as ARGB integer 0xFFRRGGBB (alpha=255 for opaque)
                // Reference KFX uses this format for text colors
                let val = (0xFFi64 << 24) | ((*r as i64) << 16) | ((*g as i64) << 8) | (*b as i64);
                Some(IonValue::Int(val))
            }
            _ => None,
        }
    }
}

/// Convert border to ION with style, width, and color
pub fn border_to_ion(
    border: &Border,
    solid_sym: u64,
    dotted_sym: u64,
    dashed_sym: u64,
    border_style_sym: u64,
) -> Option<IonValue> {
    if border.style == BorderStyle::None || border.style == BorderStyle::Hidden {
        return None;
    }

    // Skip borders with 0 width
    if let Some(ref w) = border.width {
        let is_zero = match w {
            CssValue::Px(v) => v.abs() < 0.001,
            CssValue::Em(v) | CssValue::Rem(v) => v.abs() < 0.001,
            CssValue::Percent(v) => v.abs() < 0.001,
            CssValue::Number(v) => v.abs() < 0.001,
            _ => false,
        };
        if is_zero {
            return None;
        }
    }

    let mut b_struct = HashMap::new();

    // Style
    let style_sym = match border.style {
        BorderStyle::Solid => solid_sym,
        BorderStyle::Dotted => dotted_sym,
        BorderStyle::Dashed => dashed_sym,
        // Fallback to solid for others
        _ => solid_sym,
    };
    b_struct.insert(border_style_sym, IonValue::Symbol(style_sym));

    // Width
    if let Some(ref w) = border.width {
        if let Some(val) = w.to_kfx_ion() {
            b_struct.insert(sym::VALUE, val);
        }
    } else {
        // Default width (1px) - use structure format
        let mut val = HashMap::new();
        val.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PX));
        val.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(1.0)));
        b_struct.insert(sym::VALUE, IonValue::Struct(val));
    }

    // Color
    if let Some(ref c) = border.color {
        if let Some(val) = c.to_kfx_ion() {
            b_struct.insert(sym::COLOR, val);
        }
    } else {
        // Default to black (ARGB format with alpha=255)
        b_struct.insert(sym::COLOR, IonValue::Int(0xFF000000u32 as i64));
    }

    Some(IonValue::Struct(b_struct))
}

/// Convert border radius value to ION with px units
pub fn radius_to_ion(val: &CssValue) -> Option<IonValue> {
    let px_val = match val {
        CssValue::Px(v) => Some(*v * 0.45), // Convert to KFX px
        CssValue::Em(v) | CssValue::Rem(v) => Some(*v * 16.0 * 0.45), // em to px
        CssValue::Percent(v) => Some(*v * 45.0), // percent to px approximation
        _ => None,
    };
    // Note: radius values are always output even if zero, so we use unit_value_always
    // but filter None from the match above
    px_val.map(|v| unit_value_always(sym::UNIT_PX, v))
}

// ============================================================================
// Four-Sided Property Helpers
// ============================================================================

/// Symbols for four-sided properties (margin, padding, border, radius)
pub struct FourSidedSyms {
    pub top: u64,
    pub right: u64,
    pub bottom: u64,
    pub left: u64,
}

/// Symbol constants for margin properties
pub const MARGIN_SYMS: FourSidedSyms = FourSidedSyms {
    top: sym::SPACE_BEFORE,
    right: sym::MARGIN_RIGHT,
    bottom: sym::SPACE_AFTER,
    left: sym::MARGIN_LEFT,
};

/// Symbol constants for padding properties
pub const PADDING_SYMS: FourSidedSyms = FourSidedSyms {
    top: sym::PADDING_TOP,
    right: sym::PADDING_RIGHT,
    bottom: sym::PADDING_BOTTOM,
    left: sym::PADDING_LEFT,
};

/// Symbol constants for border radius properties
pub const BORDER_RADIUS_SYMS: FourSidedSyms = FourSidedSyms {
    top: sym::BORDER_RADIUS_TL,
    right: sym::BORDER_RADIUS_TR,
    bottom: sym::BORDER_RADIUS_BR,
    left: sym::BORDER_RADIUS_BL,
};

/// Add a spacing property with axis-specific conversion.
///
/// Vertical (top/bottom) uses UNIT_MULTIPLIER (normalized by line-height).
/// Horizontal (left/right) uses UNIT_PERCENT.
fn add_spacing_property(
    style: &mut HashMap<u64, IonValue>,
    value: Option<&CssValue>,
    sym: u64,
    is_vertical: bool,
) {
    if let Some(val) = value {
        let ion_val = if is_vertical {
            spacing_to_multiplier(val)
        } else {
            val.to_kfx_ion()
        };
        if let Some(v) = ion_val {
            style.insert(sym, v);
        }
    }
}

/// Add all four margin properties with axis-specific conversion
pub fn add_margins(
    style: &mut HashMap<u64, IonValue>,
    top: Option<&CssValue>,
    right: Option<&CssValue>,
    bottom: Option<&CssValue>,
    left: Option<&CssValue>,
) {
    add_spacing_property(style, top, MARGIN_SYMS.top, true);
    add_spacing_property(style, right, MARGIN_SYMS.right, false);
    add_spacing_property(style, bottom, MARGIN_SYMS.bottom, true);
    add_spacing_property(style, left, MARGIN_SYMS.left, false);
}

/// Add all four padding properties with axis-specific conversion
pub fn add_padding(
    style: &mut HashMap<u64, IonValue>,
    top: Option<&CssValue>,
    right: Option<&CssValue>,
    bottom: Option<&CssValue>,
    left: Option<&CssValue>,
) {
    add_spacing_property(style, top, PADDING_SYMS.top, true);
    add_spacing_property(style, right, PADDING_SYMS.right, false);
    add_spacing_property(style, bottom, PADDING_SYMS.bottom, true);
    add_spacing_property(style, left, PADDING_SYMS.left, false);
}

/// Add all four border radius properties
pub fn add_border_radius(
    style: &mut HashMap<u64, IonValue>,
    top_left: Option<&CssValue>,
    top_right: Option<&CssValue>,
    bottom_right: Option<&CssValue>,
    bottom_left: Option<&CssValue>,
) {
    let corners = [
        (top_left, BORDER_RADIUS_SYMS.top),
        (top_right, BORDER_RADIUS_SYMS.right),
        (bottom_right, BORDER_RADIUS_SYMS.bottom),
        (bottom_left, BORDER_RADIUS_SYMS.left),
    ];
    for (value, sym) in corners {
        if let Some(val) = value
            && let Some(ion) = radius_to_ion(val)
        {
            style.insert(sym, ion);
        }
    }
}

/// Convert spacing value (letter-spacing, word-spacing) to em units
pub fn spacing_to_ion(spacing: &CssValue) -> Option<IonValue> {
    let em_val: Option<f32> = match spacing {
        CssValue::Em(v) | CssValue::Rem(v) => Some(*v),
        CssValue::Px(v) => Some(*v * 0.45), // px to em approximation based on mapping
        CssValue::Keyword(k) if k == "normal" => Some(0.0),
        _ => None,
    };
    // Spacing values always output (including zero for "normal")
    em_val.map(|val| unit_value_always(sym::UNIT_EM, val))
}

/// Default line-height factor used by Kindle for normalization
const DEFAULT_LINE_HEIGHT: f32 = 1.2;

/// Convert margin-top/bottom to UNIT_MULTIPLIER format for space-before/space-after
///
/// Vertical spacing (margin-top/bottom, padding-top/bottom) is normalized relative to
/// line-height (divided by 1.2) and uses UNIT_MULTIPLIER. This matches Kindle Previewer's
/// output format and converts CSS margins (relative to font-size) to KFX multipliers
/// (relative to line-height).
pub fn spacing_to_multiplier(spacing: &CssValue) -> Option<IonValue> {
    let css_val: Option<f32> = match spacing {
        CssValue::Em(v) | CssValue::Rem(v) => (*v).abs().ge(&0.001).then_some(*v),
        CssValue::Px(v) => {
            let em = *v / 16.0; // Convert px to em (16px = 1em)
            em.abs().ge(&0.001).then_some(em)
        }
        CssValue::Percent(v) => {
            let mult = *v / 100.0; // Percent of line-height as multiplier
            mult.abs().ge(&0.001).then_some(mult)
        }
        CssValue::Number(v) => (*v).abs().ge(&0.001).then_some(*v),
        _ => None,
    };
    // Kindle Previewer divides vertical margins by 1.2 (default line-height factor)
    css_val.and_then(|val| unit_value(sym::UNIT_MULTIPLIER, val / DEFAULT_LINE_HEIGHT))
}

/// Convert break property value to symbol
pub fn break_value_to_symbol(break_val: crate::css::BreakValue) -> u64 {
    use crate::css::BreakValue;
    match break_val {
        BreakValue::Auto => sym::BREAK_AUTO, // $383
        BreakValue::Avoid | BreakValue::AvoidPage | BreakValue::AvoidColumn => sym::BREAK_AVOID, // $353
        BreakValue::Page | BreakValue::Left | BreakValue::Right | BreakValue::Column => {
            sym::BREAK_ALWAYS // $352
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_uses_argb_format() {
        // Reference KFX uses ARGB format 0xFFRRGGBB for colors
        // This ensures alpha channel is set to 255 (opaque)
        let red = Color::Rgba(255, 0, 0, 255);
        let ion = red.to_kfx_ion().expect("Should produce ION value");

        match ion {
            IonValue::Int(val) => {
                // Should be 0xFFFF0000 = 4294901760
                assert_eq!(val, 0xFFFF0000u32 as i64, "Red color should be 0xFFFF0000");
            }
            _ => panic!("Expected Int value"),
        }
    }

    #[test]
    fn test_color_black_uses_argb_format() {
        // Black should be 0xFF000000, not 0x00000000
        let black = Color::Rgba(0, 0, 0, 255);
        let ion = black.to_kfx_ion().expect("Should produce ION value");

        match ion {
            IonValue::Int(val) => {
                // Should be 0xFF000000 = 4278190080
                assert_eq!(
                    val, 0xFF000000u32 as i64,
                    "Black color should be 0xFF000000"
                );
            }
            _ => panic!("Expected Int value"),
        }
    }

    #[test]
    fn test_color_preserves_rgb_values() {
        // Test that RGB values are correctly encoded in ARGB format
        let color = Color::Rgba(0x12, 0x34, 0x56, 255);
        let ion = color.to_kfx_ion().expect("Should produce ION value");

        match ion {
            IonValue::Int(val) => {
                // Should be 0xFF123456
                let expected = 0xFF123456i64;
                assert_eq!(val, expected, "Color #123456 should be 0xFF123456");
            }
            _ => panic!("Expected Int value"),
        }
    }

    #[test]
    fn test_vertical_spacing_uses_multiplier() {
        // Vertical spacing (margin-top/bottom) uses UNIT_MULTIPLIER
        // and is normalized by dividing by 1.2 (default line-height)
        let margin = CssValue::Em(1.2); // 1.2em
        let ion = spacing_to_multiplier(&margin).expect("Should produce ION value");

        match ion {
            IonValue::Struct(s) => {
                // Unit should be UNIT_MULTIPLIER ($310)
                match s.get(&sym::UNIT) {
                    Some(IonValue::Symbol(unit)) => {
                        assert_eq!(
                            *unit,
                            sym::UNIT_MULTIPLIER,
                            "Vertical spacing should use UNIT_MULTIPLIER ($310)"
                        );
                    }
                    _ => panic!("Expected Symbol for unit"),
                }
            }
            _ => panic!("Expected Struct value"),
        }
    }

    #[test]
    fn test_horizontal_spacing_uses_percent() {
        // Horizontal spacing (margin-left/right) uses UNIT_PERCENT directly
        let margin = CssValue::Percent(5.0); // 5%
        let ion = margin.to_kfx_ion().expect("Should produce ION value");

        match ion {
            IonValue::Struct(s) => {
                // Unit should be UNIT_PERCENT ($314)
                match s.get(&sym::UNIT) {
                    Some(IonValue::Symbol(unit)) => {
                        assert_eq!(
                            *unit,
                            sym::UNIT_PERCENT,
                            "Horizontal spacing should use UNIT_PERCENT ($314)"
                        );
                    }
                    _ => panic!("Expected Symbol for unit"),
                }
            }
            _ => panic!("Expected Struct value"),
        }
    }

    #[test]
    fn test_add_margins_uses_axis_specific_units() {
        // Test that add_margins applies correct units per axis
        let mut style = HashMap::new();

        // Add margins with same CSS value for all sides
        let margin = CssValue::Em(1.0);
        add_margins(
            &mut style,
            Some(&margin), // top
            Some(&margin), // right
            Some(&margin), // bottom
            Some(&margin), // left
        );

        // Vertical (top/bottom) should use UNIT_MULTIPLIER
        if let Some(IonValue::Struct(top)) = style.get(&MARGIN_SYMS.top) {
            match top.get(&sym::UNIT) {
                Some(IonValue::Symbol(unit)) => {
                    assert_eq!(
                        *unit,
                        sym::UNIT_MULTIPLIER,
                        "margin-top should use UNIT_MULTIPLIER"
                    );
                }
                _ => panic!("Expected Symbol for margin-top unit"),
            }
        } else {
            panic!("Expected Struct for margin-top");
        }

        // Horizontal (right/left) should use UNIT_PERCENT
        if let Some(IonValue::Struct(right)) = style.get(&MARGIN_SYMS.right) {
            match right.get(&sym::UNIT) {
                Some(IonValue::Symbol(unit)) => {
                    assert_eq!(
                        *unit,
                        sym::UNIT_PERCENT,
                        "margin-right should use UNIT_PERCENT"
                    );
                }
                _ => panic!("Expected Symbol for margin-right unit"),
            }
        } else {
            panic!("Expected Struct for margin-right");
        }
    }
}
