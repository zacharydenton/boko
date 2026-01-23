//! Spacing property conversion (text-indent, line-height).

use std::collections::HashMap;

use crate::css::{CssValue, ParsedStyle};
use crate::kfx::ion::{IonValue, encode_kfx_decimal};
use crate::kfx::writer::symbols::sym;

/// Default line-height factor used by Kindle for normalization
const DEFAULT_LINE_HEIGHT: f32 = 1.2;

/// Add text spacing properties to the style ION struct
pub fn add_all(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle, is_image: bool) {
    add_text_indent(style_ion, style);
    add_line_height(style_ion, style, is_image);
}

/// Add text-indent property
pub fn add_text_indent(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref indent) = style.text_indent {
        let em_val: Option<f32> = match indent {
            CssValue::Em(v) | CssValue::Rem(v) => Some(*v),
            CssValue::Px(v) => Some(*v / 16.0),
            CssValue::Percent(v) => Some(*v / 100.0),
            _ => None,
        };
        if let Some(val) = em_val {
            // Skip if value is effectively 0 (default)
            if val.abs() < 0.001 {
                return;
            }

            let mut s = HashMap::new();
            // Reference uses percent for negative values (hanging indent), em for positive
            if val < 0.0 {
                // Convert em to percent: 1em = 3.125%
                let percent_val = val * 3.125;
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(
                    sym::VALUE,
                    IonValue::Decimal(encode_kfx_decimal(percent_val)),
                );
            } else {
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
            }
            style_ion.insert(sym::TEXT_INDENT, IonValue::Struct(s));
        }
    }
}

/// Add line-height property with complex normalization logic
pub fn add_line_height(
    style_ion: &mut HashMap<u64, IonValue>,
    style: &ParsedStyle,
    is_image: bool,
) {
    // Images don't get line-height
    if is_image {
        return;
    }

    // Reference adds LINE_HEIGHT: 1 for styles with vertical-align or headings
    // even when CSS doesn't explicitly set line-height
    let needs_default_line_height = style.vertical_align.is_some() || style.is_heading;

    if let Some(ref height) = style.line_height {
        // Get font-size ratio for normalization (percent or em/rem)
        let font_size_rem: Option<f32> = style.font_size.as_ref().and_then(|fs| match fs {
            CssValue::Rem(v) => Some(*v),
            _ => None,
        });

        // Get font-size as a ratio (for normalizing line-height: 0)
        let font_size_ratio: Option<f32> = style.font_size.as_ref().and_then(|fs| match fs {
            CssValue::Percent(v) => Some(*v / 100.0),
            CssValue::Em(v) | CssValue::Rem(v) => Some(*v),
            _ => None,
        });

        // Track if this is a unitless line-height (Number/Percent)
        // vs absolute units (em/rem/px) - only unitless values need division by 1.2
        let (css_val, is_unitless): (Option<f32>, bool) = match height {
            CssValue::Number(v) => (Some(*v), true),
            CssValue::Percent(v) => (Some(*v / 100.0), true),
            CssValue::Em(v) => (Some(*v), false),
            CssValue::Rem(v) => {
                // Normalize rem line-height relative to rem font-size
                // CSS: font-size: 0.875rem; line-height: 1.25rem
                // -> line-height in em = 1.25 / 0.875 = 1.42857
                let normalized = if let Some(fs_rem) = font_size_rem {
                    *v / fs_rem
                } else {
                    // No font-size in rem, use line-height as-is
                    *v
                };
                (Some(normalized), false)
            }
            CssValue::Px(v) => (Some(*v / 16.0), false),
            _ => (None, false),
        };

        if let Some(val) = css_val {
            // Handle special case: line-height: 0 with a font-size ratio
            // CSS pattern for sub/sup: font-size: 75%; line-height: 0
            // Kindle normalizes this to line-height = 1.0 / font-size-ratio
            // This maintains vertical rhythm (smaller text gets larger line-height multiplier)
            let normalized_val = if val.abs() < 0.001 {
                // line-height is effectively 0
                if let Some(fs_ratio) = font_size_ratio {
                    if fs_ratio > 0.001 {
                        // Normalize to 1.0 / font-size-ratio
                        Some(1.0 / fs_ratio)
                    } else {
                        None // Skip if font-size is also 0
                    }
                } else {
                    None // No font-size ratio, skip line-height: 0
                }
            } else {
                // Output line-height even if 1.0 - reference KFX includes it
                // for styles with vertical-align, headings, etc.
                // Normal case: use the value as-is
                Some(val)
            };

            if let Some(final_val) = normalized_val {
                // Only divide by 1.2 for unitless line-height values (CSS multipliers)
                // Absolute units (em/rem/px) have already been normalized to em and
                // don't need the conversion to KFX multiplier space
                // Note: normalized line-height: 0 is treated as unitless since we computed the ratio
                let kfx_val = if is_unitless || val.abs() < 0.001 {
                    final_val / DEFAULT_LINE_HEIGHT
                } else {
                    final_val
                };

                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_MULTIPLIER));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(kfx_val)));
                style_ion.insert(sym::LINE_HEIGHT, IonValue::Struct(s));
                return; // Already set, don't add default
            }
        }
    }

    // Add default LINE_HEIGHT: 1 for styles with vertical-align or headings
    if needs_default_line_height {
        let mut s = HashMap::new();
        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_MULTIPLIER));
        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(1.0)));
        style_ion.insert(sym::LINE_HEIGHT, IonValue::Struct(s));
    }
}
