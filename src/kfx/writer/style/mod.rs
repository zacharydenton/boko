//! CSS to KFX style conversion.
//!
//! This module handles converting CSS properties to KFX ION format,
//! including the ToKfxIon trait and FourSided property abstraction.

mod conversion;

pub use conversion::*;

use std::collections::HashMap;

use crate::css::{Border, BorderStyle, Color, CssValue};
use crate::kfx::ion::{encode_kfx_decimal, IonValue};

use super::symbols::sym;

/// Trait for converting CSS values to KFX ION representation
pub trait ToKfxIon {
    /// Convert to KFX ION value, returning None if the value should be omitted
    fn to_kfx_ion(&self) -> Option<IonValue>;
}

/// Convert a CssValue to Ion for margins/padding
/// Format: {$306: unit_symbol, $307: decimal_value}
impl ToKfxIon for CssValue {
    fn to_kfx_ion(&self) -> Option<IonValue> {
        match self {
            CssValue::Px(v) => {
                if v.abs() < 0.001 {
                    return None;
                }
                // Convert px to percent (approximate: 1px ~ 0.117% based on mapping)
                let pct = *v * 0.117;
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(pct)));
                Some(IonValue::Struct(s))
            }
            CssValue::Em(v) | CssValue::Rem(v) => {
                if v.abs() < 0.001 {
                    return None;
                }
                // Convert em to percent (3.125% per 1em based on mapping)
                let pct = *v * 3.125;
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(pct)));
                Some(IonValue::Struct(s))
            }
            CssValue::Percent(v) => {
                if v.abs() < 0.001 {
                    return None;
                }
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                Some(IonValue::Struct(s))
            }
            CssValue::Number(v) => {
                if v.abs() < 0.001 {
                    return None;
                }
                // Unitless number - use multiplier
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_MULTIPLIER));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                Some(IonValue::Struct(s))
            }
            _ => None,
        }
    }
}

impl ToKfxIon for Color {
    fn to_kfx_ion(&self) -> Option<IonValue> {
        match self {
            Color::Rgba(r, g, b, _a) => {
                // Serialize as integer 0x00RRGGBB
                let val = ((*r as i64) << 16) | ((*g as i64) << 8) | (*b as i64);
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
        // Default to black
        b_struct.insert(sym::COLOR, IonValue::Int(0));
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
    px_val.map(|v| {
        let mut s = HashMap::new();
        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PX));
        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(v)));
        IonValue::Struct(s)
    })
}

/// Symbols for four-sided properties (margin, padding, border, radius)
pub struct FourSidedSyms {
    pub top: u64,
    pub right: u64,
    pub bottom: u64,
    pub left: u64,
}

/// Four-sided property abstraction for margin, padding, border, radius
pub struct FourSided<'a, T> {
    pub top: Option<&'a T>,
    pub right: Option<&'a T>,
    pub bottom: Option<&'a T>,
    pub left: Option<&'a T>,
}

impl<'a, T: ToKfxIon> FourSided<'a, T> {
    /// Add all four sides to a style struct
    pub fn add_to_style(&self, style: &mut HashMap<u64, IonValue>, syms: &FourSidedSyms) {
        if let Some(v) = self.top.and_then(|t| t.to_kfx_ion()) {
            style.insert(syms.top, v);
        }
        if let Some(v) = self.right.and_then(|r| r.to_kfx_ion()) {
            style.insert(syms.right, v);
        }
        if let Some(v) = self.bottom.and_then(|b| b.to_kfx_ion()) {
            style.insert(syms.bottom, v);
        }
        if let Some(v) = self.left.and_then(|l| l.to_kfx_ion()) {
            style.insert(syms.left, v);
        }
    }
}

/// Convert spacing value (letter-spacing, word-spacing) to em units
pub fn spacing_to_ion(spacing: &CssValue) -> Option<IonValue> {
    let em_val: Option<f32> = match spacing {
        CssValue::Em(v) | CssValue::Rem(v) => Some(*v),
        CssValue::Px(v) => Some(*v * 0.45 / 1.0), // px to em approximation based on mapping
        CssValue::Keyword(k) if k == "normal" => Some(0.0),
        _ => None,
    };
    em_val.map(|val| {
        let mut s = HashMap::new();
        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
        IonValue::Struct(s)
    })
}

/// Convert margin-top/bottom to UNIT_MULTIPLIER format for space-before/space-after
/// Reference KFX uses UNIT_MULTIPLIER with em values for vertical spacing
pub fn spacing_to_multiplier(spacing: &CssValue) -> Option<IonValue> {
    let multiplier_val: Option<f32> = match spacing {
        CssValue::Em(v) | CssValue::Rem(v) => {
            if v.abs() < 0.001 {
                None
            } else {
                Some(*v)
            }
        }
        CssValue::Px(v) => {
            let em = *v / 16.0; // Convert px to em (16px = 1em)
            if em.abs() < 0.001 {
                None
            } else {
                Some(em)
            }
        }
        CssValue::Percent(v) => {
            // Percent of line-height, approximate as multiplier
            let mult = *v / 100.0;
            if mult.abs() < 0.001 {
                None
            } else {
                Some(mult)
            }
        }
        CssValue::Number(v) => {
            if v.abs() < 0.001 {
                None
            } else {
                Some(*v)
            }
        }
        _ => None,
    };
    multiplier_val.map(|val| {
        let mut s = HashMap::new();
        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_MULTIPLIER));
        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
        IonValue::Struct(s)
    })
}

/// Convert break property value to symbol
pub fn break_value_to_symbol(break_val: crate::css::BreakValue) -> u64 {
    use crate::css::BreakValue;
    match break_val {
        BreakValue::Auto => sym::BLOCK_TYPE_BLOCK, // $383
        BreakValue::Avoid | BreakValue::AvoidPage | BreakValue::AvoidColumn => sym::BREAK_AVOID, // $353
        _ => sym::BLOCK_TYPE_BLOCK, // Default to auto
    }
}
