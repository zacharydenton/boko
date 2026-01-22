//! Style conversion from ParsedStyle to KFX ION.
//!
//! Contains the main add_all_styles() logic for converting CSS to KFX.

use std::collections::HashMap;

use crate::css::{CssValue, FontVariant, ParsedStyle, TextAlign};
use crate::kfx::ion::{encode_kfx_decimal, IonValue};

use super::{
    border_to_ion, break_value_to_symbol, radius_to_ion, spacing_to_ion, spacing_to_multiplier,
    ToKfxIon,
};
use crate::kfx::writer::fragment::KfxFragment;
use crate::kfx::writer::symbols::{sym, SymbolTable};

/// Check if a CssValue is non-zero (for layout property detection)
fn has_nonzero_value(val: &Option<CssValue>) -> bool {
    match val {
        None => false,
        Some(CssValue::Px(v)) => v.abs() > 0.001,
        Some(CssValue::Em(v)) => v.abs() > 0.001,
        Some(CssValue::Rem(v)) => v.abs() > 0.001,
        Some(CssValue::Percent(v)) => v.abs() > 0.001,
        Some(CssValue::Number(v)) => v.abs() > 0.001,
        Some(_) => true, // Other values like keywords count as non-zero
    }
}

/// Convert a ParsedStyle to KFX ION style struct
pub fn style_to_ion(
    style: &ParsedStyle,
    style_sym: u64,
    symtab: &mut SymbolTable,
) -> IonValue {
    let mut style_ion = HashMap::new();
    style_ion.insert(sym::STYLE_NAME, IonValue::Symbol(style_sym));

    // Detect special style types
    let is_image_style = style.is_image;
    let is_inline_style = style.is_inline;

    // Inline styles (for links/anchors) are minimal - just block type
    // Reference uses $127: $349 for inline elements
    if is_inline_style {
        style_ion.insert(
            sym::STYLE_BLOCK_TYPE,
            IonValue::Symbol(sym::BLOCK_TYPE_INLINE),
        );
        return IonValue::Struct(style_ion);
    }

    if !is_image_style {
        // Font family - use string value (verified via CSS mapping)
        if let Some(ref family) = style.font_family {
            let family_lower = family.to_lowercase();
            let family_str = match family_lower.as_str() {
                "serif" | "georgia" | "times" | "times new roman" => "serif".to_string(),
                "sans-serif" | "arial" | "helvetica" => "sans-serif".to_string(),
                "monospace" | "courier" | "courier new" => "monospace".to_string(),
                "cursive" => "cursive".to_string(),
                "fantasy" => "fantasy".to_string(),
                _ => family_lower.clone(),
            };
            style_ion.insert(sym::FONT_FAMILY, IonValue::String(family_str));
        }

        // Add STYLE_BLOCK_TYPE based on style content
        // Reference KFX behavior:
        // - BLOCK_TYPE_BLOCK ($383) only for styles with actual layout properties
        // - No BLOCK_TYPE at all for font-only styles (reference omits it)
        // - BLOCK_TYPE_INLINE ($349) only for explicit inline elements (handled above)
        // Note: We check for non-zero values since margin: 0 shouldn't count as a layout property
        let has_layout_props = has_nonzero_value(&style.margin_top)
            || has_nonzero_value(&style.margin_bottom)
            || has_nonzero_value(&style.margin_left)
            || has_nonzero_value(&style.margin_right)
            || has_nonzero_value(&style.text_indent)
            || style.width.is_some()
            || style.height.is_some()
            || style.max_width.is_some()
            || style.break_before.is_some()
            || style.break_after.is_some()
            || style.break_inside.is_some();

        // Only set BLOCK_TYPE when style has layout properties
        // Font-only styles don't get BLOCK_TYPE (matches reference behavior)
        if has_layout_props {
            style_ion.insert(sym::STYLE_BLOCK_TYPE, IonValue::Symbol(sym::BLOCK_TYPE_BLOCK));
        }

        // Language tag ($10) from xml:lang or lang attribute
        if let Some(ref lang) = style.lang {
            style_ion.insert(sym::LANGUAGE, IonValue::String(lang.clone()));
        }

        // Note: IMAGE_FIT baseline removed - reference KFX doesn't include it for text styles
    }

    // Font size
    add_font_size(&mut style_ion, style, is_image_style);

    // Text align
    if let Some(align) = style.text_align {
        let align_sym = match align {
            TextAlign::Left => sym::ALIGN_LEFT,
            TextAlign::Right => sym::ALIGN_RIGHT,
            TextAlign::Center => sym::ALIGN_CENTER,
            TextAlign::Justify => sym::ALIGN_JUSTIFY,
        };
        style_ion.insert(sym::TEXT_ALIGN, IonValue::Symbol(align_sym));
    }

    // Font weight
    add_font_weight(&mut style_ion, style);

    // Font style
    add_font_style(&mut style_ion, style);

    // Font variant
    if let Some(FontVariant::SmallCaps) = style.font_variant {
        style_ion.insert(
            sym::FONT_VARIANT,
            IonValue::Symbol(sym::FONT_VARIANT_SMALL_CAPS),
        );
    }

    // Margins: top/bottom use UNIT_MULTIPLIER (space-before/after), left/right use UNIT_PERCENT
    // This matches Kindle Previewer's output format
    if let Some(ref val) = style.margin_top {
        if let Some(ion_val) = spacing_to_multiplier(val) {
            style_ion.insert(sym::SPACE_BEFORE, ion_val);
        }
    }
    if let Some(ref val) = style.margin_bottom {
        if let Some(ion_val) = spacing_to_multiplier(val) {
            style_ion.insert(sym::SPACE_AFTER, ion_val);
        }
    }
    // Left/right margins use percent (unchanged)
    if let Some(ref val) = style.margin_left {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::MARGIN_LEFT, ion_val);
        }
    }
    if let Some(ref val) = style.margin_right {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::MARGIN_RIGHT, ion_val);
        }
    }

    // Width and height
    add_dimensions(&mut style_ion, style);

    // Image-specific properties
    if is_image_style {
        style_ion.insert(sym::IMAGE_FIT, IonValue::Symbol(sym::IMAGE_FIT_CONTAIN));
        style_ion.insert(sym::IMAGE_LAYOUT, IonValue::Symbol(sym::ALIGN_CENTER));
    }

    // Margin auto centering
    add_margin_auto_centering(&mut style_ion, style, is_image_style);

    // Text indent
    add_text_indent(&mut style_ion, style);

    // Line height
    add_line_height(&mut style_ion, style, is_image_style);

    // Colors
    if let Some(ref color) = style.color {
        if let Some(val) = color.to_kfx_ion() {
            style_ion.insert(sym::COLOR, val);
        }
    }
    if let Some(ref bg_color) = style.background_color {
        if let Some(val) = bg_color.to_kfx_ion() {
            style_ion.insert(sym::BACKGROUND_COLOR, val);
        }
    }

    // Borders - disabled: reference KFX doesn't include border styles
    // add_borders(&mut style_ion, style, symtab);

    // Vertical align
    add_vertical_align(&mut style_ion, style);

    // Letter/word spacing
    if let Some(ref spacing) = style.letter_spacing {
        if let Some(val) = spacing_to_ion(spacing) {
            style_ion.insert(sym::LETTER_SPACING, val);
        }
    }
    if let Some(ref spacing) = style.word_spacing {
        if let Some(val) = spacing_to_ion(spacing) {
            style_ion.insert(sym::WORD_SPACING, val);
        }
    }

    // White space
    if let Some(nowrap) = style.white_space_nowrap {
        style_ion.insert(sym::WHITE_SPACE_NOWRAP, IonValue::Bool(nowrap));
    }

    // Text decorations
    add_text_decorations(&mut style_ion, style);

    // Opacity
    if let Some(opacity) = style.opacity {
        let val = (opacity as f32) / 100.0;
        style_ion.insert(sym::OPACITY, IonValue::Decimal(encode_kfx_decimal(val)));
    }

    // Min/max dimensions
    add_min_max_dimensions(&mut style_ion, style);

    // Clear
    add_clear(&mut style_ion, style);

    // Word break
    add_word_break(&mut style_ion, style);

    // Overflow
    add_overflow(&mut style_ion, style);

    // Visibility
    add_visibility(&mut style_ion, style);

    // Break properties
    add_break_properties(&mut style_ion, style);

    // Border radius
    add_border_radius(&mut style_ion, style);

    IonValue::Struct(style_ion)
}

fn add_font_size(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle, is_image_style: bool) {
    // Images don't get font-size
    if is_image_style {
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

fn add_font_weight(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref weight) = style.font_weight {
        let weight_sym = if weight.is_bold() {
            sym::FONT_WEIGHT_BOLD
        } else {
            match weight {
                crate::css::FontWeight::Weight(100) => sym::FONT_WEIGHT_100,
                crate::css::FontWeight::Weight(200) => sym::FONT_WEIGHT_200,
                crate::css::FontWeight::Weight(300) => sym::FONT_WEIGHT_300,
                crate::css::FontWeight::Weight(400) => sym::FONT_WEIGHT_NORMAL,
                crate::css::FontWeight::Weight(500) => sym::FONT_WEIGHT_500,
                crate::css::FontWeight::Weight(600) => sym::FONT_WEIGHT_600,
                crate::css::FontWeight::Weight(700) => sym::FONT_WEIGHT_BOLD,
                crate::css::FontWeight::Weight(800) => sym::FONT_WEIGHT_800,
                crate::css::FontWeight::Weight(900) => sym::FONT_WEIGHT_900,
                crate::css::FontWeight::Weight(n) if *n < 400 => sym::FONT_WEIGHT_300,
                crate::css::FontWeight::Weight(n) if *n < 600 => sym::FONT_WEIGHT_500,
                crate::css::FontWeight::Weight(_) => sym::FONT_WEIGHT_BOLD,
                crate::css::FontWeight::Normal => sym::FONT_WEIGHT_NORMAL,
                crate::css::FontWeight::Bold => sym::FONT_WEIGHT_BOLD,
            }
        };
        // Include font-weight (Kindle Previewer includes normal explicitly)
        style_ion.insert(sym::FONT_WEIGHT, IonValue::Symbol(weight_sym));
    }
}

fn add_font_style(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(style_type) = style.font_style {
        let style_sym = match style_type {
            crate::css::FontStyle::Italic => sym::FONT_STYLE_ITALIC,
            crate::css::FontStyle::Oblique => sym::FONT_STYLE_OBLIQUE,
            crate::css::FontStyle::Normal => sym::FONT_STYLE_NORMAL,
        };
        // Include font-style (Kindle Previewer includes normal explicitly)
        style_ion.insert(sym::FONT_STYLE, IonValue::Symbol(style_sym));
    }
}

fn add_dimensions(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref width) = style.width {
        let width_val = match width {
            CssValue::Percent(pct) => {
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*pct)));
                Some(IonValue::Struct(s))
            }
            CssValue::Em(v) | CssValue::Rem(v) => {
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                Some(IonValue::Struct(s))
            }
            CssValue::Px(v) => {
                let pct = *v * 0.117;
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(pct)));
                Some(IonValue::Struct(s))
            }
            _ => None,
        };
        if let Some(val) = width_val {
            style_ion.insert(sym::STYLE_WIDTH, val);
        }
    }
    if let Some(ref height) = style.height {
        let height_val = match height {
            CssValue::Percent(pct) => {
                let mut s = HashMap::new();
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*pct)));
                Some(IonValue::Struct(s))
            }
            _ => height.to_kfx_ion(),
        };
        if let Some(val) = height_val {
            style_ion.insert(sym::STYLE_HEIGHT, val);
        }
    }
}

fn add_margin_auto_centering(
    style_ion: &mut HashMap<u64, IonValue>,
    style: &ParsedStyle,
    _is_image_style: bool,
) {
    // Note: Reference KFX doesn't include IMAGE_FIT/IMAGE_LAYOUT for margin:auto centering
    // on text styles. This function is kept for potential future use but currently does nothing
    // for text elements. Image styles get IMAGE_FIT/IMAGE_LAYOUT added separately.
    let _has_margin_auto_centering = matches!(
        (&style.margin_left, &style.margin_right),
        (Some(CssValue::Keyword(l)), Some(CssValue::Keyword(r)))
        if l == "auto" && r == "auto"
    );
    // Intentionally not adding IMAGE_FIT/IMAGE_LAYOUT for text styles
}

fn add_text_indent(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
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
            s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
            s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
            style_ion.insert(sym::TEXT_INDENT, IonValue::Struct(s));
        }
    }
}

/// Default line-height factor used by Kindle for normalization
const DEFAULT_LINE_HEIGHT: f32 = 1.2;

fn add_line_height(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle, is_image_style: bool) {
    // Images don't get line-height
    if is_image_style {
        return;
    }

    // Only add line-height if explicitly set (reference omits default 1.0)
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
            } else if (val - 1.0).abs() < 0.001 {
                // line-height is effectively 1.0 (default), skip
                None
            } else {
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
            }
        }
    }
}

fn add_borders(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle, symtab: &mut SymbolTable) {
    let border_top_sym = symtab.get_or_intern("border-top");
    let border_bottom_sym = symtab.get_or_intern("border-bottom");
    let border_left_sym = symtab.get_or_intern("border-left");
    let border_right_sym = symtab.get_or_intern("border-right");
    let border_style_sym = symtab.get_or_intern("border-style");
    let solid_sym = symtab.get_or_intern("solid");
    let dotted_sym = symtab.get_or_intern("dotted");
    let dashed_sym = symtab.get_or_intern("dashed");

    let borders = [
        (style.border_top.as_ref(), border_top_sym),
        (style.border_right.as_ref(), border_right_sym),
        (style.border_bottom.as_ref(), border_bottom_sym),
        (style.border_left.as_ref(), border_left_sym),
    ];

    for (border, sym) in borders {
        if let Some(b) = border {
            if let Some(val) = border_to_ion(b, solid_sym, dotted_sym, dashed_sym, border_style_sym) {
                style_ion.insert(sym, val);
            }
        }
    }
}

fn add_vertical_align(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(align) = style.vertical_align {
        use crate::css::VerticalAlign;
        let align_sym = match align {
            VerticalAlign::Baseline => sym::FONT_WEIGHT_NORMAL,
            VerticalAlign::Top => sym::VERTICAL_TOP,
            VerticalAlign::Middle => sym::ALIGN_CENTER,
            VerticalAlign::Bottom => sym::VERTICAL_BOTTOM,
            VerticalAlign::Super => sym::VERTICAL_SUPER,
            VerticalAlign::Sub => sym::VERTICAL_SUB,
            VerticalAlign::TextTop => sym::VERTICAL_TEXT_TOP,
            VerticalAlign::TextBottom => sym::VERTICAL_TEXT_BOTTOM,
        };
        if align != VerticalAlign::Baseline {
            style_ion.insert(sym::VERTICAL_ALIGN, IonValue::Symbol(align_sym));
        }
    }
}

fn add_text_decorations(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    let decorations = [
        (style.text_decoration_underline, sym::TEXT_DECORATION_UNDERLINE),
        (style.text_decoration_overline, sym::TEXT_DECORATION_OVERLINE),
        (style.text_decoration_line_through, sym::TEXT_DECORATION_LINE_THROUGH),
    ];

    for (enabled, sym) in decorations {
        if enabled {
            style_ion.insert(sym, IonValue::Symbol(sym::DECORATION_PRESENT));
        }
    }
}

fn add_min_max_dimensions(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref val) = style.min_width {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::MIN_WIDTH, ion_val);
        }
    }
    if let Some(ref val) = style.min_height {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::MIN_HEIGHT, ion_val);
        }
    }
    if let Some(ref val) = style.max_width {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::MAX_WIDTH, ion_val);
        }
    }
    if let Some(ref val) = style.max_height {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::STYLE_HEIGHT, ion_val);
        }
    }
}

fn add_clear(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(clear) = style.clear {
        use crate::css::Clear;
        let clear_sym = match clear {
            Clear::None => sym::TEXT_TRANSFORM_NONE,
            Clear::Left => sym::ALIGN_LEFT,
            Clear::Right => sym::ALIGN_RIGHT,
            Clear::Both => sym::CLEAR_BOTH,
        };
        if clear != Clear::None {
            style_ion.insert(sym::CLEAR, IonValue::Symbol(clear_sym));
        }
    }
}

fn add_word_break(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(word_break) = style.word_break {
        use crate::css::WordBreak;
        let break_sym = match word_break {
            WordBreak::Normal => sym::FONT_WEIGHT_NORMAL,
            WordBreak::BreakAll => sym::WORD_BREAK_ALL,
            WordBreak::KeepAll => sym::FONT_WEIGHT_NORMAL,
        };
        if word_break != WordBreak::Normal {
            style_ion.insert(sym::WORD_BREAK, IonValue::Symbol(break_sym));
        }
    }
}

fn add_overflow(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(overflow) = style.overflow {
        use crate::css::Overflow;
        if matches!(overflow, Overflow::Hidden | Overflow::Clip) {
            style_ion.insert(sym::OVERFLOW_CLIP, IonValue::Bool(true));
        }
    }
}

fn add_visibility(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(visibility) = style.visibility {
        use crate::css::Visibility;
        style_ion.insert(
            sym::VISIBILITY,
            IonValue::Bool(visibility == Visibility::Visible),
        );
    }
}

fn add_break_properties(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(break_val) = style.break_before {
        let break_sym = break_value_to_symbol(break_val);
        style_ion.insert(sym::BREAK_BEFORE, IonValue::Symbol(break_sym));
    }
    if let Some(break_val) = style.break_after {
        let break_sym = break_value_to_symbol(break_val);
        style_ion.insert(sym::BREAK_AFTER, IonValue::Symbol(break_sym));
    }
    if let Some(break_val) = style.break_inside {
        let break_sym = break_value_to_symbol(break_val);
        style_ion.insert(sym::BREAK_INSIDE, IonValue::Symbol(break_sym));
    }
}

fn add_border_radius(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    let radii = [
        (style.border_radius_tl.as_ref(), sym::BORDER_RADIUS_TL),
        (style.border_radius_tr.as_ref(), sym::BORDER_RADIUS_TR),
        (style.border_radius_br.as_ref(), sym::BORDER_RADIUS_BR),
        (style.border_radius_bl.as_ref(), sym::BORDER_RADIUS_BL),
    ];

    for (radius, sym) in radii {
        if let Some(r) = radius {
            if let Some(ion_val) = radius_to_ion(r) {
                style_ion.insert(sym, ion_val);
            }
        }
    }
}

/// Collect all unique styles from chapters and create style fragments
pub fn collect_and_create_styles(
    chapters: &[crate::kfx::writer::content::ChapterData],
    symtab: &mut SymbolTable,
    style_map: &mut HashMap<ParsedStyle, u64>,
) -> Vec<KfxFragment> {
    use crate::kfx::writer::content::ContentItem;
    use std::collections::HashSet;

    // Collect all unique styles, including from nested containers and inline runs
    fn collect_styles(item: &ContentItem, styles: &mut HashSet<ParsedStyle>) {
        styles.insert(item.style().clone());

        match item {
            ContentItem::Container { children, .. } => {
                for child in children {
                    collect_styles(child, styles);
                }
            }
            ContentItem::Text { inline_runs, .. } => {
                for run in inline_runs {
                    styles.insert(run.style.clone());
                }
            }
            ContentItem::Image { .. } => {}
        }
    }

    let mut unique_styles = HashSet::new();
    for chapter in chapters {
        for item in &chapter.content {
            collect_styles(item, &mut unique_styles);
        }
    }

    let mut fragments = Vec::new();

    for (i, style) in unique_styles.into_iter().enumerate() {
        let style_id = format!("style-{i}");
        let style_sym = symtab.get_or_intern(&style_id);

        let style_ion = style_to_ion(&style, style_sym, symtab);

        fragments.push(KfxFragment::new(sym::STYLE, &style_id, style_ion));

        style_map.insert(style, style_sym);
    }

    fragments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::ParsedStyle;

    #[test]
    fn test_text_style_no_image_fit_baseline() {
        // Text styles should NOT include IMAGE_FIT ($546) baseline property
        // Reference KFX doesn't include it for non-image styles
        let style = ParsedStyle {
            font_size: Some(crate::css::CssValue::Em(1.0)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        // Should NOT have IMAGE_FIT for text styles
        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };
        assert!(
            !ion_map.contains_key(&sym::IMAGE_FIT),
            "Text styles should not have IMAGE_FIT baseline property"
        );
    }

    #[test]
    fn test_image_style_has_image_fit() {
        // Image styles SHOULD include IMAGE_FIT
        let style = ParsedStyle {
            is_image: true,
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };
        assert!(
            ion_map.contains_key(&sym::IMAGE_FIT),
            "Image styles should have IMAGE_FIT property"
        );
    }

    #[test]
    fn test_line_height_divided_by_1_2() {
        // Kindle Previewer divides line-height values by 1.2 (default line-height factor)
        // CSS line-height: 1.5 → KFX line-height: 1.25 (1.5/1.2)
        // CSS line-height: 2.0 → KFX line-height: 1.666667 (2/1.2)
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            line_height: Some(crate::css::CssValue::Number(1.5)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Should have LINE_HEIGHT
        assert!(
            ion_map.contains_key(&sym::LINE_HEIGHT),
            "Style should have line-height"
        );

        // Extract the value
        let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
            Some(IonValue::Struct(s)) => s,
            _ => panic!("Expected line-height struct"),
        };

        let value = match lh_struct.get(&sym::VALUE) {
            Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
            _ => panic!("Expected decimal value"),
        };

        // Should be 1.5 / 1.2 = 1.25
        let expected = 1.5 / 1.2;
        assert!(
            (value - expected).abs() < 0.01,
            "line-height 1.5 should become {} in KFX, got {}",
            expected,
            value
        );
    }

    #[test]
    fn test_margin_top_divided_by_1_2() {
        // Kindle Previewer divides vertical em margins by 1.2 (default line-height factor)
        // CSS margin-top: 1em → KFX margin-top: 0.833333em (1/1.2)
        // CSS margin-top: 3em → KFX margin-top: 2.5em (3/1.2)
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            margin_top: Some(crate::css::CssValue::Em(3.0)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Should have SPACE_BEFORE (margin-top)
        assert!(
            ion_map.contains_key(&sym::SPACE_BEFORE),
            "Style should have margin-top (SPACE_BEFORE)"
        );

        // Extract the value
        let margin_struct = match ion_map.get(&sym::SPACE_BEFORE) {
            Some(IonValue::Struct(s)) => s,
            _ => panic!("Expected margin-top struct"),
        };

        let value = match margin_struct.get(&sym::VALUE) {
            Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
            _ => panic!("Expected decimal value"),
        };

        // Should be 3 / 1.2 = 2.5
        let expected = 3.0 / 1.2;
        assert!(
            (value - expected).abs() < 0.01,
            "margin-top 3em should become {} in KFX, got {}",
            expected,
            value
        );
    }

    #[test]
    fn test_line_height_rem_normalized_to_font_size() {
        // When line-height is in rem/em units, it should be normalized relative
        // to font-size but NOT divided by 1.2 (division only applies to unitless values).
        // Example: font-size: 0.875rem; line-height: 1.25rem
        // - line-height in em = 1.25 / 0.875 = 1.42857
        // - NO division by 1.2 because it's already in absolute units
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            font_size: Some(crate::css::CssValue::Rem(0.875)),
            line_height: Some(crate::css::CssValue::Rem(1.25)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Should have LINE_HEIGHT
        assert!(
            ion_map.contains_key(&sym::LINE_HEIGHT),
            "Style should have line-height"
        );

        let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
            Some(IonValue::Struct(s)) => s,
            _ => panic!("Expected line-height struct"),
        };

        let value = match lh_struct.get(&sym::VALUE) {
            Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
            _ => panic!("Expected decimal value"),
        };

        // line-height in em = 1.25 / 0.875 = 1.42857
        // NO division by 1.2 for absolute units
        let expected = 1.25 / 0.875;
        assert!(
            (value - expected).abs() < 0.01,
            "line-height 1.25rem with font-size 0.875rem should become {} in KFX, got {}",
            expected,
            value
        );
    }

    #[test]
    fn test_font_only_style_omits_block_type() {
        // Styles with only font formatting (no layout properties) should NOT
        // have BLOCK_TYPE at all. Reference KFX only sets BLOCK_TYPE on styles
        // with actual layout properties.
        let style = ParsedStyle {
            font_style: Some(crate::css::FontStyle::Italic),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Font-only styles should NOT have BLOCK_TYPE at all (matches reference)
        assert!(
            !ion_map.contains_key(&sym::STYLE_BLOCK_TYPE),
            "Font-only style should not have BLOCK_TYPE"
        );
    }

    #[test]
    fn test_layout_style_uses_block_block_type() {
        // Styles with layout properties (margins, text-indent) should use
        // BLOCK_TYPE_BLOCK ($383)
        let style = ParsedStyle {
            margin_top: Some(crate::css::CssValue::Em(1.0)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Should have STYLE_BLOCK_TYPE = BLOCK ($383)
        match ion_map.get(&sym::STYLE_BLOCK_TYPE) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(
                    *s,
                    sym::BLOCK_TYPE_BLOCK,
                    "Layout style should use BLOCK_TYPE_BLOCK ($383), got ${}",
                    s
                );
            }
            _ => panic!("Expected STYLE_BLOCK_TYPE symbol"),
        }
    }

    #[test]
    fn test_zero_margin_omits_block_type() {
        // Styles with margin: 0 should NOT have BLOCK_TYPE
        // because margin: 0 doesn't affect layout (same as font-only)
        let style = ParsedStyle {
            font_style: Some(crate::css::FontStyle::Italic),
            margin_top: Some(crate::css::CssValue::Px(0.0)),
            margin_bottom: Some(crate::css::CssValue::Px(0.0)),
            margin_left: Some(crate::css::CssValue::Px(0.0)),
            margin_right: Some(crate::css::CssValue::Px(0.0)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Should NOT have BLOCK_TYPE because margins are zero
        assert!(
            !ion_map.contains_key(&sym::STYLE_BLOCK_TYPE),
            "Style with margin: 0 should not have BLOCK_TYPE"
        );
    }

    #[test]
    fn test_em_width_does_not_add_max_width() {
        // Width in em units should NOT automatically add MAX_WIDTH
        // Reference KFX does not have MAX_WIDTH on these styles
        let style = ParsedStyle {
            width: Some(crate::css::CssValue::Em(10.0)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Should have STYLE_WIDTH but NOT MAX_WIDTH
        assert!(
            ion_map.contains_key(&sym::STYLE_WIDTH),
            "Style should have STYLE_WIDTH"
        );
        assert!(
            !ion_map.contains_key(&sym::MAX_WIDTH),
            "Width in em should not automatically add MAX_WIDTH"
        );
    }

    #[test]
    fn test_text_xs_style_has_line_height_in_ion() {
        // The text-xs pattern: font-size 0.75rem, line-height 1rem
        // Both should be present in the ION output
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            font_size: Some(crate::css::CssValue::Rem(0.75)),
            line_height: Some(crate::css::CssValue::Rem(1.0)),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("text-xs");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Check font-size is present
        assert!(
            ion_map.contains_key(&sym::FONT_SIZE),
            "text-xs style should have FONT_SIZE"
        );

        // Check line-height is present - THIS IS THE KEY ASSERTION
        assert!(
            ion_map.contains_key(&sym::LINE_HEIGHT),
            "text-xs style should have LINE_HEIGHT, but it's missing! ION keys: {:?}",
            ion_map.keys().collect::<Vec<_>>()
        );

        // Verify line-height value
        // line-height = 1rem / 0.75rem font-size = 1.33333
        let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
            Some(IonValue::Struct(s)) => s,
            _ => panic!("Expected line-height struct"),
        };

        let value = match lh_struct.get(&sym::VALUE) {
            Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
            _ => panic!("Expected decimal value"),
        };

        // line-height in em = 1.0 / 0.75 = 1.33333
        // NO division by 1.2 for absolute units (rem)
        let expected = 1.0 / 0.75;
        assert!(
            (value - expected).abs() < 0.01,
            "text-xs line-height should be ~{}, got {}",
            expected,
            value
        );
    }

    #[test]
    fn test_line_height_zero_normalizes_with_font_size() {
        // CSS pattern for sub/sup: font-size: 75%; line-height: 0
        // Kindle Previewer normalizes line-height: 0 to 1.0/font-size-ratio
        // This maintains vertical rhythm (smaller text gets larger line-height multiplier)
        // Example: font-size 75% -> line-height 1/0.75 = 1.33333
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            font_size: Some(crate::css::CssValue::Percent(75.0)),
            line_height: Some(crate::css::CssValue::Number(0.0)), // line-height: 0
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("sub-sup");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Should have LINE_HEIGHT (not skipped!)
        assert!(
            ion_map.contains_key(&sym::LINE_HEIGHT),
            "Style with line-height: 0 should have LINE_HEIGHT (normalized to 1.0/font-size). Keys: {:?}",
            ion_map.keys().collect::<Vec<_>>()
        );

        // Verify line-height value
        let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
            Some(IonValue::Struct(s)) => s,
            _ => panic!("Expected line-height struct"),
        };

        let value = match lh_struct.get(&sym::VALUE) {
            Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
            _ => panic!("Expected decimal value"),
        };

        // line-height 0 with font-size 75% normalizes to:
        // 1.0 / 0.75 = 1.33333
        // Then divided by 1.2 for KFX: 1.33333 / 1.2 = 1.11111
        let expected = (1.0 / 0.75) / 1.2; // = 1.11111
        assert!(
            (value - expected).abs() < 0.01,
            "line-height: 0 with font-size: 75% should become ~{} in KFX, got {}",
            expected,
            value
        );
    }
}
