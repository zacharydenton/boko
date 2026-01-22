//! Style conversion from ParsedStyle to KFX ION.
//!
//! Contains the main add_all_styles() logic for converting CSS to KFX.

use std::collections::HashMap;

use crate::css::{
    BorderCollapse, ColumnCount, CssFloat, CssValue, FontVariant, ListStylePosition, ListStyleType,
    ParsedStyle, RubyAlign, RubyMerge, RubyPosition, TextAlign, TextCombineUpright,
    TextDecorationLineStyle, TextEmphasisStyle, WritingMode,
};
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
        // P1: Additional units
        Some(CssValue::Vw(v)) => v.abs() > 0.001,
        Some(CssValue::Vh(v)) => v.abs() > 0.001,
        Some(CssValue::Vmin(v)) => v.abs() > 0.001,
        Some(CssValue::Vmax(v)) => v.abs() > 0.001,
        Some(CssValue::Ch(v)) => v.abs() > 0.001,
        Some(CssValue::Ex(v)) => v.abs() > 0.001,
        Some(CssValue::Cm(v)) => v.abs() > 0.001,
        Some(CssValue::Mm(v)) => v.abs() > 0.001,
        Some(CssValue::In(v)) => v.abs() > 0.001,
        Some(CssValue::Pt(v)) => v.abs() > 0.001,
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

    // Inline styles (for links/anchors) are minimal
    // Note: $127 is hyphens property, not block type - don't output unless hyphens is specified
    if is_inline_style {
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

        // Language tag ($10) from xml:lang or lang attribute
        if let Some(ref lang) = style.lang {
            style_ion.insert(sym::LANGUAGE, IonValue::String(lang.clone()));
        }

        // Note: $127 is hyphens property (not block type as previously thought)
        // Only output when CSS hyphens is explicitly specified
        // Reference KFX doesn't add hyphens as a default for all styles
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

    // Padding: top/bottom use UNIT_MULTIPLIER (like margin top/bottom)
    if let Some(ref val) = style.padding_top {
        if let Some(ion_val) = spacing_to_multiplier(val) {
            style_ion.insert(sym::PADDING_TOP, ion_val);
        }
    }
    if let Some(ref val) = style.padding_bottom {
        if let Some(ion_val) = spacing_to_multiplier(val) {
            style_ion.insert(sym::PADDING_BOTTOM, ion_val);
        }
    }
    // Padding left/right use their own symbols ($53, $55)
    if let Some(ref val) = style.padding_left {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::PADDING_LEFT, ion_val);
        }
    }
    if let Some(ref val) = style.padding_right {
        if let Some(ion_val) = val.to_kfx_ion() {
            style_ion.insert(sym::PADDING_RIGHT, ion_val);
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

    // Text transform
    if let Some(transform) = style.text_transform {
        use crate::css::TextTransform;
        let sym_val = match transform {
            TextTransform::None => sym::TEXT_TRANSFORM_NONE,
            TextTransform::Uppercase => sym::TEXT_TRANSFORM_UPPERCASE,
            TextTransform::Lowercase => sym::TEXT_TRANSFORM_LOWERCASE,
            TextTransform::Capitalize => sym::TEXT_TRANSFORM_CAPITALIZE,
        };
        style_ion.insert(sym::TEXT_TRANSFORM, IonValue::Symbol(sym_val));
    }

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

    // P1: List style properties
    add_list_style(&mut style_ion, style);

    // P2: Writing mode
    add_writing_mode(&mut style_ion, style);

    // P4: Shadow properties
    add_shadows(&mut style_ion, style);

    // P1 Phase 2: Ruby annotation properties
    add_ruby_properties(&mut style_ion, style);

    // P1 Phase 2: Text emphasis properties
    add_text_emphasis(&mut style_ion, style);

    // P2 Phase 2: Border collapse
    add_border_collapse(&mut style_ion, style);

    // P1 Phase 2: Drop cap
    add_drop_cap(&mut style_ion, style);

    // P2 Phase 2: Transform properties
    add_transform(&mut style_ion, style);

    // P2 Phase 2: Baseline-shift
    add_baseline_shift(&mut style_ion, style);

    // P2 Phase 2: Column layout
    add_column_count(&mut style_ion, style);

    // P2 Phase 2: Float property
    add_float(&mut style_ion, style);

    // P2 Phase 2: Layout hints for semantic elements
    add_layout_hints(&mut style_ion, style);

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
            // P1: Handle viewport and other new units via ToKfxIon trait
            _ => width.to_kfx_ion(),
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
            // Reference uses percent for negative values (hanging indent), em for positive
            if val < 0.0 {
                // Convert em to percent: 1em = 3.125%
                let percent_val = val * 3.125;
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(percent_val)));
            } else {
                s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
                s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
            }
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
    // Get the line style (if any)
    // Per yj_to_epub_properties.py: decoration properties use $328-$331 as values
    // $328=solid, $329=double, $330=dashed, $331=dotted
    let line_style = style.text_decoration_line_style;

    // Helper to get decoration style symbol
    let style_sym = |style: Option<TextDecorationLineStyle>| -> u64 {
        match style {
            Some(TextDecorationLineStyle::Dashed) => sym::TEXT_DECORATION_STYLE_DASHED,
            Some(TextDecorationLineStyle::Dotted) => sym::TEXT_DECORATION_STYLE_DOTTED,
            Some(TextDecorationLineStyle::Double) => sym::TEXT_DECORATION_STYLE_DOUBLE,
            Some(TextDecorationLineStyle::Solid) | None => sym::DECORATION_PRESENT, // $328 = solid
        }
    };

    // Handle underline with optional line style
    if style.text_decoration_underline {
        style_ion.insert(
            sym::TEXT_DECORATION_UNDERLINE,
            IonValue::Symbol(style_sym(line_style)),
        );
    }

    // Handle overline with optional line style
    if style.text_decoration_overline {
        style_ion.insert(
            sym::TEXT_DECORATION_OVERLINE,
            IonValue::Symbol(style_sym(line_style)),
        );
    }

    // Handle line-through with optional line style
    if style.text_decoration_line_through {
        style_ion.insert(
            sym::TEXT_DECORATION_LINE_THROUGH,
            IonValue::Symbol(style_sym(line_style)),
        );
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

// P1: List style properties
fn add_list_style(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // List style type ($100)
    if let Some(list_type) = style.list_style_type {
        let type_sym = match list_type {
            ListStyleType::Disc => sym::LIST_TYPE_DISC,
            ListStyleType::Circle => sym::LIST_TYPE_CIRCLE,
            ListStyleType::Square => sym::LIST_TYPE_SQUARE,
            ListStyleType::Decimal => sym::LIST_TYPE_DECIMAL,
            ListStyleType::DecimalLeadingZero => sym::LIST_TYPE_DECIMAL, // Fallback
            ListStyleType::LowerAlpha => sym::LIST_TYPE_LOWER_ALPHA,
            ListStyleType::UpperAlpha => sym::LIST_TYPE_UPPER_ALPHA,
            ListStyleType::LowerRoman => sym::LIST_TYPE_LOWER_ROMAN,
            ListStyleType::UpperRoman => sym::LIST_TYPE_UPPER_ROMAN,
            ListStyleType::LowerGreek => sym::LIST_TYPE_LOWER_ALPHA, // Fallback
            ListStyleType::None => sym::LIST_TYPE_NONE,
        };
        style_ion.insert(sym::LIST_TYPE, IonValue::Symbol(type_sym));
    }

    // List style position ($551)
    if let Some(position) = style.list_style_position {
        let pos_sym = match position {
            ListStylePosition::Inside => sym::LIST_POSITION_INSIDE,
            ListStylePosition::Outside => sym::LIST_POSITION_OUTSIDE,
        };
        style_ion.insert(sym::LIST_POSITION, IonValue::Symbol(pos_sym));
    }
}

// P2: Writing mode
fn add_writing_mode(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Writing mode ($560)
    if let Some(mode) = style.writing_mode {
        let mode_sym = match mode {
            WritingMode::HorizontalTb => sym::WRITING_MODE_HORIZONTAL_TB,
            WritingMode::VerticalRl => sym::WRITING_MODE_VERTICAL_RL,
            WritingMode::VerticalLr => sym::WRITING_MODE_VERTICAL_LR,
        };
        // Only output if not default horizontal-tb
        if mode != WritingMode::HorizontalTb {
            style_ion.insert(sym::WRITING_MODE, IonValue::Symbol(mode_sym));
        }
    }

    // Text combine upright ($561)
    if let Some(combine) = style.text_combine_upright {
        match combine {
            TextCombineUpright::All => {
                style_ion.insert(sym::TEXT_COMBINE_UPRIGHT, IonValue::Bool(true));
            }
            TextCombineUpright::Digits(n) if n > 0 => {
                style_ion.insert(sym::TEXT_COMBINE_UPRIGHT, IonValue::Int(n as i64));
            }
            _ => {}
        }
    }
}

// P4: Shadow properties
fn add_shadows(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Box shadow ($496) - store as string for now (simplified)
    if let Some(ref shadow) = style.box_shadow {
        style_ion.insert(sym::BOX_SHADOW, IonValue::String(shadow.clone()));
    }

    // Text shadow ($497) - store as string for now (simplified)
    if let Some(ref shadow) = style.text_shadow {
        style_ion.insert(sym::TEXT_SHADOW, IonValue::String(shadow.clone()));
    }
}

// P1 Phase 2: Ruby annotation properties
fn add_ruby_properties(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Ruby position ($762)
    if let Some(position) = style.ruby_position {
        let pos_sym = match position {
            RubyPosition::Over => sym::RUBY_POSITION_OVER,
            RubyPosition::Under => sym::RUBY_POSITION_UNDER,
        };
        // Only output if not default (over)
        if position != RubyPosition::Over {
            style_ion.insert(sym::RUBY_POSITION, IonValue::Symbol(pos_sym));
        }
    }

    // Ruby align ($766)
    if let Some(align) = style.ruby_align {
        let align_sym = match align {
            RubyAlign::Center => sym::RUBY_ALIGN_CENTER,
            RubyAlign::Start => sym::RUBY_ALIGN_START,
            RubyAlign::SpaceAround => sym::RUBY_ALIGN_SPACE_AROUND,
            RubyAlign::SpaceBetween => sym::RUBY_ALIGN_SPACE_BETWEEN,
        };
        // Only output if not default (center)
        if align != RubyAlign::Center {
            style_ion.insert(sym::RUBY_ALIGN, IonValue::Symbol(align_sym));
        }
    }

    // Ruby merge ($764)
    if let Some(merge) = style.ruby_merge {
        let merge_sym = match merge {
            RubyMerge::Separate => sym::RUBY_MERGE_SEPARATE,
            RubyMerge::Collapse => sym::RUBY_MERGE_COLLAPSE,
        };
        // Only output if not default (separate)
        if merge != RubyMerge::Separate {
            style_ion.insert(sym::RUBY_MERGE, IonValue::Symbol(merge_sym));
        }
    }
}

// P1 Phase 2: Text emphasis properties
fn add_text_emphasis(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Text emphasis style ($717)
    if let Some(emphasis) = style.text_emphasis_style {
        let style_sym = match emphasis {
            TextEmphasisStyle::None => return, // Don't output None
            TextEmphasisStyle::Filled => sym::TEXT_EMPHASIS_FILLED,
            TextEmphasisStyle::Open => sym::TEXT_EMPHASIS_OPEN,
            TextEmphasisStyle::Dot => sym::TEXT_EMPHASIS_DOT,
            TextEmphasisStyle::Circle => sym::TEXT_EMPHASIS_CIRCLE,
            TextEmphasisStyle::FilledCircle => sym::TEXT_EMPHASIS_FILLED_CIRCLE,
            TextEmphasisStyle::OpenCircle => sym::TEXT_EMPHASIS_OPEN_CIRCLE,
            TextEmphasisStyle::FilledDot => sym::TEXT_EMPHASIS_FILLED_DOT,
            TextEmphasisStyle::OpenDot => sym::TEXT_EMPHASIS_OPEN_DOT,
            TextEmphasisStyle::DoubleCircle => sym::TEXT_EMPHASIS_DOUBLE_CIRCLE,
            TextEmphasisStyle::FilledDoubleCircle => sym::TEXT_EMPHASIS_FILLED_DOUBLE_CIRCLE,
            TextEmphasisStyle::OpenDoubleCircle => sym::TEXT_EMPHASIS_OPEN_DOUBLE_CIRCLE,
            TextEmphasisStyle::Triangle => sym::TEXT_EMPHASIS_TRIANGLE,
            TextEmphasisStyle::FilledTriangle => sym::TEXT_EMPHASIS_FILLED_TRIANGLE,
            TextEmphasisStyle::OpenTriangle => sym::TEXT_EMPHASIS_OPEN_TRIANGLE,
            TextEmphasisStyle::Sesame => sym::TEXT_EMPHASIS_SESAME,
            TextEmphasisStyle::FilledSesame => sym::TEXT_EMPHASIS_FILLED_SESAME,
            TextEmphasisStyle::OpenSesame => sym::TEXT_EMPHASIS_OPEN_SESAME,
        };
        style_ion.insert(sym::TEXT_EMPHASIS_STYLE, IonValue::Symbol(style_sym));
    }

    // Text emphasis color ($718)
    if let Some(ref color) = style.text_emphasis_color {
        if let Some(val) = color.to_kfx_ion() {
            style_ion.insert(sym::TEXT_EMPHASIS_COLOR, val);
        }
    }
}

// P2 Phase 2: Border collapse
// Note: Uses boolean values per yj_to_epub_properties.py: False=separate, True=collapse
fn add_border_collapse(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(collapse) = style.border_collapse {
        // Only output if not default (separate)
        if collapse != BorderCollapse::Separate {
            // True = collapse
            style_ion.insert(sym::BORDER_COLLAPSE, IonValue::Bool(true));
        }
    }
}

// P1 Phase 2: Drop cap
fn add_drop_cap(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(drop_cap) = style.drop_cap {
        if drop_cap.lines > 0 {
            style_ion.insert(sym::DROP_CAP_LINES, IonValue::Int(drop_cap.lines as i64));
        }
        if drop_cap.chars > 0 {
            style_ion.insert(sym::DROP_CAP_CHARS, IonValue::Int(drop_cap.chars as i64));
        }
    }
}

// P2 Phase 2: Transform properties
fn add_transform(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Transform matrix
    if let Some(ref transform) = style.transform {
        // Skip identity transforms
        if !transform.is_identity() {
            // KFX stores transform as a 6-element list [a, b, c, d, tx, ty]
            let matrix_list: Vec<IonValue> = transform
                .matrix
                .iter()
                .map(|&v| IonValue::Decimal(encode_kfx_decimal(v)))
                .collect();
            style_ion.insert(sym::TRANSFORM, IonValue::List(matrix_list));
        }
    }

    // Transform origin
    if let Some(ref origin) = style.transform_origin {
        // Transform-origin uses a struct with $59 (x/left) and $58 (y/top)
        let mut origin_struct = HashMap::new();

        // X position (left)
        if let Some(ref x) = origin.x {
            match x {
                CssValue::Percent(v) => {
                    origin_struct.insert(59, IonValue::Decimal(encode_kfx_decimal(*v)));
                }
                CssValue::Px(v) => {
                    origin_struct.insert(59, IonValue::Decimal(encode_kfx_decimal(*v)));
                }
                _ => {}
            }
        }

        // Y position (top)
        if let Some(ref y) = origin.y {
            match y {
                CssValue::Percent(v) => {
                    origin_struct.insert(58, IonValue::Decimal(encode_kfx_decimal(*v)));
                }
                CssValue::Px(v) => {
                    origin_struct.insert(58, IonValue::Decimal(encode_kfx_decimal(*v)));
                }
                _ => {}
            }
        }

        if !origin_struct.is_empty() {
            style_ion.insert(sym::TRANSFORM_ORIGIN, IonValue::Struct(origin_struct));
        }
    }
}

// P2 Phase 2: Baseline-shift
fn add_baseline_shift(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(ref shift) = style.baseline_shift {
        // Baseline-shift is a numeric value (typically in em or percent)
        match shift {
            CssValue::Em(v) => {
                // Convert em to decimal value
                style_ion.insert(
                    sym::BASELINE_SHIFT,
                    IonValue::Decimal(encode_kfx_decimal(*v)),
                );
            }
            CssValue::Percent(v) => {
                // Convert percent to decimal (e.g., 50% = 0.5)
                style_ion.insert(
                    sym::BASELINE_SHIFT,
                    IonValue::Decimal(encode_kfx_decimal(*v / 100.0)),
                );
            }
            CssValue::Px(v) => {
                // Pixels need conversion - assume ~16px base font
                style_ion.insert(
                    sym::BASELINE_SHIFT,
                    IonValue::Decimal(encode_kfx_decimal(*v / 16.0)),
                );
            }
            _ => {}
        }
    }
}

// P2 Phase 2: Column layout
fn add_column_count(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(count) = style.column_count {
        match count {
            ColumnCount::Auto => {
                // auto uses $383 (shared with hyphens, break values)
                style_ion.insert(sym::COLUMN_COUNT, IonValue::Symbol(sym::COLUMN_COUNT_AUTO));
            }
            ColumnCount::Count(n) => {
                // Numeric column count
                style_ion.insert(sym::COLUMN_COUNT, IonValue::Int(n as i64));
            }
        }
    }
}

// P2 Phase 2: Float property
fn add_float(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(float) = style.float {
        // Only output non-none values
        let float_sym = match float {
            CssFloat::None => return, // Don't output float: none
            CssFloat::Left => sym::FLOAT_LEFT,
            CssFloat::Right => sym::FLOAT_RIGHT,
            CssFloat::SnapBlock => sym::FLOAT_SNAP_BLOCK,
        };
        style_ion.insert(sym::FLOAT, IonValue::Symbol(float_sym));
    }
}

// P2 Phase 2: Layout hints for semantic elements
fn add_layout_hints(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Add heading hint for h1-h6 elements
    if style.is_heading {
        // $761: [$760] - layout hints list containing heading hint
        style_ion.insert(
            sym::LAYOUT_HINTS,
            IonValue::List(vec![IonValue::Symbol(sym::LAYOUT_HINT_HEADING)]),
        );
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

        // Font-only styles should NOT have HYPHENS at all (matches reference)
        // Note: $127 is CSS hyphens property, not block type
        assert!(
            !ion_map.contains_key(&sym::HYPHENS),
            "Font-only style should not have HYPHENS"
        );
    }

    #[test]
    fn test_layout_style_does_not_have_hyphens() {
        // Layout styles should NOT have $127 (hyphens) by default
        // $127 is only output when CSS hyphens property is explicitly set
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

        // Should NOT have $127 (hyphens) unless CSS explicitly sets it
        assert!(
            !ion_map.contains_key(&sym::HYPHENS),
            "Layout style should not have HYPHENS by default"
        );
    }

    #[test]
    fn test_zero_margin_omits_hyphens() {
        // Styles with margin: 0 should NOT have HYPHENS
        // $127 is CSS hyphens property, not block type indicator
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

        // Should NOT have HYPHENS
        assert!(
            !ion_map.contains_key(&sym::HYPHENS),
            "Style should not have HYPHENS by default"
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

    // P1 Tests: List style properties
    #[test]
    fn test_list_style_type_decimal() {
        let style = ParsedStyle {
            list_style_type: Some(crate::css::ListStyleType::Decimal),
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
            ion_map.contains_key(&sym::LIST_TYPE),
            "Style should have LIST_TYPE"
        );
        match ion_map.get(&sym::LIST_TYPE) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::LIST_TYPE_DECIMAL, "Expected decimal list type");
            }
            _ => panic!("Expected symbol for LIST_TYPE"),
        }
    }

    #[test]
    fn test_list_style_type_disc() {
        let style = ParsedStyle {
            list_style_type: Some(crate::css::ListStyleType::Disc),
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
            ion_map.contains_key(&sym::LIST_TYPE),
            "Style should have LIST_TYPE"
        );
        match ion_map.get(&sym::LIST_TYPE) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::LIST_TYPE_DISC, "Expected disc list type");
            }
            _ => panic!("Expected symbol for LIST_TYPE"),
        }
    }

    #[test]
    fn test_list_style_position_inside() {
        let style = ParsedStyle {
            list_style_position: Some(crate::css::ListStylePosition::Inside),
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
            ion_map.contains_key(&sym::LIST_POSITION),
            "Style should have LIST_POSITION"
        );
        match ion_map.get(&sym::LIST_POSITION) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::LIST_POSITION_INSIDE, "Expected inside position");
            }
            _ => panic!("Expected symbol for LIST_POSITION"),
        }
    }

    // P1 Tests: Additional CSS units
    #[test]
    fn test_viewport_width_unit() {
        let style = ParsedStyle {
            width: Some(crate::css::CssValue::Vw(50.0)),
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
            ion_map.contains_key(&sym::STYLE_WIDTH),
            "Style should have STYLE_WIDTH"
        );
    }

    #[test]
    fn test_viewport_height_unit() {
        let style = ParsedStyle {
            height: Some(crate::css::CssValue::Vh(100.0)),
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
            ion_map.contains_key(&sym::STYLE_HEIGHT),
            "Style should have STYLE_HEIGHT"
        );
    }

    // P2 Tests: Writing mode
    #[test]
    fn test_writing_mode_vertical_rl() {
        let style = ParsedStyle {
            writing_mode: Some(crate::css::WritingMode::VerticalRl),
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
            ion_map.contains_key(&sym::WRITING_MODE),
            "Style should have WRITING_MODE"
        );
        match ion_map.get(&sym::WRITING_MODE) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(
                    *s,
                    sym::WRITING_MODE_VERTICAL_RL,
                    "Expected vertical-rl writing mode"
                );
            }
            _ => panic!("Expected symbol for WRITING_MODE"),
        }
    }

    #[test]
    fn test_writing_mode_horizontal_tb_not_output() {
        // Horizontal-tb is the default and should not be output
        let style = ParsedStyle {
            writing_mode: Some(crate::css::WritingMode::HorizontalTb),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        // Default horizontal-tb should not be output
        assert!(
            !ion_map.contains_key(&sym::WRITING_MODE),
            "Default horizontal-tb should not output WRITING_MODE"
        );
    }

    // P4 Tests: Shadow properties
    #[test]
    fn test_box_shadow() {
        let style = ParsedStyle {
            box_shadow: Some("2px 2px 4px #000".to_string()),
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
            ion_map.contains_key(&sym::BOX_SHADOW),
            "Style should have BOX_SHADOW"
        );
    }

    #[test]
    fn test_text_shadow() {
        let style = ParsedStyle {
            text_shadow: Some("1px 1px 2px black".to_string()),
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
            ion_map.contains_key(&sym::TEXT_SHADOW),
            "Style should have TEXT_SHADOW"
        );
    }

    // P1 Phase 2 Tests: Ruby annotations
    #[test]
    fn test_ruby_position_under() {
        let style = ParsedStyle {
            ruby_position: Some(crate::css::RubyPosition::Under),
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
            ion_map.contains_key(&sym::RUBY_POSITION),
            "Style should have RUBY_POSITION"
        );
        match ion_map.get(&sym::RUBY_POSITION) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::RUBY_POSITION_UNDER, "Expected ruby-position: under");
            }
            _ => panic!("Expected symbol for RUBY_POSITION"),
        }
    }

    #[test]
    fn test_ruby_position_over_not_output() {
        // Over is the default and should not be output
        let style = ParsedStyle {
            ruby_position: Some(crate::css::RubyPosition::Over),
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
            !ion_map.contains_key(&sym::RUBY_POSITION),
            "Default ruby-position: over should not output"
        );
    }

    #[test]
    fn test_ruby_align_space_between() {
        let style = ParsedStyle {
            ruby_align: Some(crate::css::RubyAlign::SpaceBetween),
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
            ion_map.contains_key(&sym::RUBY_ALIGN),
            "Style should have RUBY_ALIGN"
        );
        match ion_map.get(&sym::RUBY_ALIGN) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(
                    *s,
                    sym::RUBY_ALIGN_SPACE_BETWEEN,
                    "Expected ruby-align: space-between"
                );
            }
            _ => panic!("Expected symbol for RUBY_ALIGN"),
        }
    }

    #[test]
    fn test_ruby_merge_collapse() {
        let style = ParsedStyle {
            ruby_merge: Some(crate::css::RubyMerge::Collapse),
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
            ion_map.contains_key(&sym::RUBY_MERGE),
            "Style should have RUBY_MERGE"
        );
        match ion_map.get(&sym::RUBY_MERGE) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::RUBY_MERGE_COLLAPSE, "Expected ruby-merge: collapse");
            }
            _ => panic!("Expected symbol for RUBY_MERGE"),
        }
    }

    // P1 Phase 2 Tests: Text emphasis
    #[test]
    fn test_text_emphasis_style_filled_circle() {
        let style = ParsedStyle {
            text_emphasis_style: Some(crate::css::TextEmphasisStyle::FilledCircle),
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
            ion_map.contains_key(&sym::TEXT_EMPHASIS_STYLE),
            "Style should have TEXT_EMPHASIS_STYLE"
        );
        match ion_map.get(&sym::TEXT_EMPHASIS_STYLE) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(
                    *s,
                    sym::TEXT_EMPHASIS_FILLED_CIRCLE,
                    "Expected text-emphasis-style: filled circle"
                );
            }
            _ => panic!("Expected symbol for TEXT_EMPHASIS_STYLE"),
        }
    }

    #[test]
    fn test_text_emphasis_style_none_not_output() {
        let style = ParsedStyle {
            text_emphasis_style: Some(crate::css::TextEmphasisStyle::None),
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
            !ion_map.contains_key(&sym::TEXT_EMPHASIS_STYLE),
            "text-emphasis-style: none should not output"
        );
    }

    #[test]
    fn test_text_emphasis_color() {
        let style = ParsedStyle {
            text_emphasis_color: Some(crate::css::Color::Rgba(255, 0, 0, 255)),
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
            ion_map.contains_key(&sym::TEXT_EMPHASIS_COLOR),
            "Style should have TEXT_EMPHASIS_COLOR"
        );
    }

    // P2 Phase 2 Tests: Border collapse
    #[test]
    fn test_border_collapse() {
        let style = ParsedStyle {
            border_collapse: Some(crate::css::BorderCollapse::Collapse),
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
            ion_map.contains_key(&sym::BORDER_COLLAPSE),
            "Style should have BORDER_COLLAPSE"
        );
        // Per yj_to_epub_properties.py: True = collapse
        match ion_map.get(&sym::BORDER_COLLAPSE) {
            Some(IonValue::Bool(true)) => {}
            other => panic!("Expected Bool(true) for BORDER_COLLAPSE, got {:?}", other),
        }
    }

    #[test]
    fn test_border_collapse_separate_not_output() {
        // Separate is the default and should not be output
        let style = ParsedStyle {
            border_collapse: Some(crate::css::BorderCollapse::Separate),
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
            !ion_map.contains_key(&sym::BORDER_COLLAPSE),
            "Default border-collapse: separate should not output"
        );
    }

    // P1 Phase 2 Tests: Drop cap
    #[test]
    fn test_drop_cap() {
        let style = ParsedStyle {
            drop_cap: Some(crate::css::DropCap { lines: 3, chars: 1 }),
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
            ion_map.contains_key(&sym::DROP_CAP_LINES),
            "Style should have DROP_CAP_LINES"
        );
        assert!(
            ion_map.contains_key(&sym::DROP_CAP_CHARS),
            "Style should have DROP_CAP_CHARS"
        );

        match ion_map.get(&sym::DROP_CAP_LINES) {
            Some(IonValue::Int(n)) => {
                assert_eq!(*n, 3, "Expected drop cap lines = 3");
            }
            _ => panic!("Expected int for DROP_CAP_LINES"),
        }
        match ion_map.get(&sym::DROP_CAP_CHARS) {
            Some(IonValue::Int(n)) => {
                assert_eq!(*n, 1, "Expected drop cap chars = 1");
            }
            _ => panic!("Expected int for DROP_CAP_CHARS"),
        }
    }

    // P2 Phase 2 Tests: Text decoration line style
    #[test]
    fn test_text_decoration_underline_dashed() {
        let style = ParsedStyle {
            text_decoration_underline: true,
            text_decoration_line_style: Some(crate::css::TextDecorationLineStyle::Dashed),
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
            ion_map.contains_key(&sym::TEXT_DECORATION_UNDERLINE),
            "Style should have TEXT_DECORATION_UNDERLINE"
        );
        // Per yj_to_epub_properties.py: $330 = dashed line style
        match ion_map.get(&sym::TEXT_DECORATION_UNDERLINE) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(
                    *s,
                    sym::TEXT_DECORATION_STYLE_DASHED,
                    "Expected dashed style symbol ($330)"
                );
            }
            _ => panic!("Expected symbol for TEXT_DECORATION_UNDERLINE"),
        }
    }

    #[test]
    fn test_text_decoration_line_through_double() {
        let style = ParsedStyle {
            text_decoration_line_through: true,
            text_decoration_line_style: Some(crate::css::TextDecorationLineStyle::Double),
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
            ion_map.contains_key(&sym::TEXT_DECORATION_LINE_THROUGH),
            "Style should have TEXT_DECORATION_LINE_THROUGH"
        );
        // Per yj_to_epub_properties.py: $329 = double line style
        match ion_map.get(&sym::TEXT_DECORATION_LINE_THROUGH) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(
                    *s,
                    sym::TEXT_DECORATION_STYLE_DOUBLE,
                    "Expected double style symbol ($329)"
                );
            }
            _ => panic!("Expected symbol for TEXT_DECORATION_LINE_THROUGH"),
        }
    }

    // P2 Phase 2 Tests: Transform properties
    #[test]
    fn test_transform_translate() {
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            transform: Some(crate::css::Transform::translate(10.0, 20.0)),
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
            ion_map.contains_key(&sym::TRANSFORM),
            "Style should have TRANSFORM"
        );

        match ion_map.get(&sym::TRANSFORM) {
            Some(IonValue::List(list)) => {
                assert_eq!(list.len(), 6, "Transform should have 6 elements");
                // translate(10, 20) = [1, 0, 0, 1, 10, 20]
                if let IonValue::Decimal(bytes) = &list[4] {
                    let tx = decode_kfx_decimal(bytes);
                    assert!((tx - 10.0).abs() < 0.001, "Expected tx=10, got {}", tx);
                }
                if let IonValue::Decimal(bytes) = &list[5] {
                    let ty = decode_kfx_decimal(bytes);
                    assert!((ty - 20.0).abs() < 0.001, "Expected ty=20, got {}", ty);
                }
            }
            _ => panic!("Expected list for TRANSFORM"),
        }
    }

    #[test]
    fn test_transform_scale() {
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            transform: Some(crate::css::Transform::scale(2.0, 0.5)),
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
            ion_map.contains_key(&sym::TRANSFORM),
            "Style should have TRANSFORM"
        );

        match ion_map.get(&sym::TRANSFORM) {
            Some(IonValue::List(list)) => {
                assert_eq!(list.len(), 6, "Transform should have 6 elements");
                // scale(2, 0.5) = [2, 0, 0, 0.5, 0, 0]
                if let IonValue::Decimal(bytes) = &list[0] {
                    let sx = decode_kfx_decimal(bytes);
                    assert!((sx - 2.0).abs() < 0.001, "Expected sx=2, got {}", sx);
                }
                if let IonValue::Decimal(bytes) = &list[3] {
                    let sy = decode_kfx_decimal(bytes);
                    assert!((sy - 0.5).abs() < 0.001, "Expected sy=0.5, got {}", sy);
                }
            }
            _ => panic!("Expected list for TRANSFORM"),
        }
    }

    #[test]
    fn test_transform_identity_omitted() {
        // Identity transform should be omitted from output
        let style = ParsedStyle {
            transform: Some(crate::css::Transform::default()), // Identity
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
            !ion_map.contains_key(&sym::TRANSFORM),
            "Identity transform should be omitted"
        );
    }

    // P2 Phase 2 Tests: Baseline-shift
    #[test]
    fn test_baseline_shift_em() {
        use crate::kfx::ion::decode_kfx_decimal;

        let style = ParsedStyle {
            baseline_shift: Some(crate::css::CssValue::Em(0.5)),
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
            ion_map.contains_key(&sym::BASELINE_SHIFT),
            "Style should have BASELINE_SHIFT"
        );

        match ion_map.get(&sym::BASELINE_SHIFT) {
            Some(IonValue::Decimal(bytes)) => {
                let shift = decode_kfx_decimal(bytes);
                assert!(
                    (shift - 0.5).abs() < 0.001,
                    "Expected baseline-shift 0.5em, got {}",
                    shift
                );
            }
            _ => panic!("Expected decimal for BASELINE_SHIFT"),
        }
    }

    // P2 Phase 2 Tests: Column count
    #[test]
    fn test_column_count_numeric() {
        let style = ParsedStyle {
            column_count: Some(crate::css::ColumnCount::Count(3)),
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
            ion_map.contains_key(&sym::COLUMN_COUNT),
            "Style should have COLUMN_COUNT"
        );

        match ion_map.get(&sym::COLUMN_COUNT) {
            Some(IonValue::Int(n)) => {
                assert_eq!(*n, 3, "Expected column-count 3");
            }
            _ => panic!("Expected int for COLUMN_COUNT"),
        }
    }

    #[test]
    fn test_column_count_auto() {
        let style = ParsedStyle {
            column_count: Some(crate::css::ColumnCount::Auto),
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
            ion_map.contains_key(&sym::COLUMN_COUNT),
            "Style should have COLUMN_COUNT"
        );

        match ion_map.get(&sym::COLUMN_COUNT) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(
                    *s,
                    sym::COLUMN_COUNT_AUTO,
                    "Expected column-count auto ($383)"
                );
            }
            _ => panic!("Expected symbol for COLUMN_COUNT auto"),
        }
    }

    // P2 Phase 2 Tests: Float property
    #[test]
    fn test_float_left() {
        let style = ParsedStyle {
            float: Some(crate::css::CssFloat::Left),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        assert!(ion_map.contains_key(&sym::FLOAT), "Style should have FLOAT");

        match ion_map.get(&sym::FLOAT) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::FLOAT_LEFT, "Expected float: left ($59)");
            }
            _ => panic!("Expected symbol for FLOAT"),
        }
    }

    #[test]
    fn test_float_right() {
        let style = ParsedStyle {
            float: Some(crate::css::CssFloat::Right),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        assert!(ion_map.contains_key(&sym::FLOAT), "Style should have FLOAT");

        match ion_map.get(&sym::FLOAT) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::FLOAT_RIGHT, "Expected float: right ($61)");
            }
            _ => panic!("Expected symbol for FLOAT"),
        }
    }

    #[test]
    fn test_float_snap_block() {
        let style = ParsedStyle {
            float: Some(crate::css::CssFloat::SnapBlock),
            ..Default::default()
        };

        let mut symtab = SymbolTable::new();
        let style_sym = symtab.get_or_intern("test-style");
        let ion = style_to_ion(&style, style_sym, &mut symtab);

        let ion_map = match ion {
            IonValue::Struct(map) => map,
            _ => panic!("Expected struct"),
        };

        assert!(ion_map.contains_key(&sym::FLOAT), "Style should have FLOAT");

        match ion_map.get(&sym::FLOAT) {
            Some(IonValue::Symbol(s)) => {
                assert_eq!(*s, sym::FLOAT_SNAP_BLOCK, "Expected float: snap-block ($786)");
            }
            _ => panic!("Expected symbol for FLOAT"),
        }
    }

    #[test]
    fn test_float_none_not_output() {
        // float: none should not be output (default behavior)
        let style = ParsedStyle {
            float: Some(crate::css::CssFloat::None),
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
            !ion_map.contains_key(&sym::FLOAT),
            "float: none should not be output"
        );
    }

    // P2 Phase 2 Tests: Layout hints
    #[test]
    fn test_heading_style_has_layout_hints() {
        let style = ParsedStyle {
            is_heading: true,
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
            ion_map.contains_key(&sym::LAYOUT_HINTS),
            "Heading style should have LAYOUT_HINTS ($761)"
        );

        match ion_map.get(&sym::LAYOUT_HINTS) {
            Some(IonValue::List(list)) => {
                assert_eq!(list.len(), 1, "Layout hints should have 1 element");
                match &list[0] {
                    IonValue::Symbol(s) => {
                        assert_eq!(
                            *s,
                            sym::LAYOUT_HINT_HEADING,
                            "Expected heading hint ($760)"
                        );
                    }
                    _ => panic!("Expected symbol in layout hints list"),
                }
            }
            _ => panic!("Expected list for LAYOUT_HINTS"),
        }
    }

    #[test]
    fn test_non_heading_style_no_layout_hints() {
        let style = ParsedStyle {
            is_heading: false,
            font_weight: Some(crate::css::FontWeight::Bold),
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
            !ion_map.contains_key(&sym::LAYOUT_HINTS),
            "Non-heading style should not have LAYOUT_HINTS"
        );
    }
}
