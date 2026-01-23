//! Style conversion from ParsedStyle to KFX ION.
//!
//! Contains the main add_all_styles() logic for converting CSS to KFX.

use std::collections::HashMap;

use crate::css::{
    BorderCollapse, BoxSizing, ColumnCount, CssFloat, CssValue, FontVariant, Hyphens, LineBreak,
    ListStylePosition, ListStyleType, ParsedStyle, RubyAlign, RubyMerge, RubyPosition, TextAlign,
    TextCombineUpright, TextDecorationLineStyle, TextEmphasisStyle, TextOrientation, UnicodeBidi,
    WritingMode,
};
use crate::kfx::ion::{IonValue, encode_kfx_decimal};

use super::{ToKfxIon, spacing_to_ion};
use crate::kfx::writer::fragment::KfxFragment;
use crate::kfx::writer::symbols::{SymbolTable, sym};

/// Convert a ParsedStyle to KFX ION style struct
pub fn style_to_ion(style: &ParsedStyle, style_sym: u64, _symtab: &mut SymbolTable) -> IonValue {
    let mut style_ion = HashMap::new();
    style_ion.insert(sym::STYLE_NAME, IonValue::Symbol(style_sym));

    // Detect special style types
    let is_image_style = style.is_image;
    let is_inline_style = style.is_inline;

    // Font and text properties (inline-safe: font-family, size, weight, style, color, decorations)
    // These apply to both block and inline styles
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
    }

    // Font properties (size, weight, style) - inline-safe
    super::font::add_all(&mut style_ion, style, is_image_style);

    // Inline styles skip block-level properties (text-align, margins, padding, etc.)
    if is_inline_style {
        // Add text decorations (underline, etc.) - these ARE inline properties
        add_text_decorations(&mut style_ion, style);

        // Add vertical-align - inline property
        add_vertical_align(&mut style_ion, style);

        // Add line-height if specified - inline property
        if let Some(ref lh) = style.line_height
            && let Some(val) = lh.to_kfx_ion()
        {
            style_ion.insert(sym::LINE_HEIGHT, val);
        }

        // Add letter/word spacing - inline properties
        if let Some(ref spacing) = style.letter_spacing
            && let Some(val) = spacing_to_ion(spacing)
        {
            style_ion.insert(sym::LETTER_SPACING, val);
        }
        if let Some(ref spacing) = style.word_spacing
            && let Some(val) = spacing_to_ion(spacing)
        {
            style_ion.insert(sym::WORD_SPACING, val);
        }

        return IonValue::Struct(style_ion);
    }

    // Block-level properties below this point

    // Text align - skip left (default value)
    if let Some(align) = style.text_align {
        // Only output non-default alignments
        if align != TextAlign::Left {
            let align_sym = match align {
                TextAlign::Left => sym::ALIGN_LEFT, // Won't reach this
                TextAlign::Right => sym::ALIGN_RIGHT,
                TextAlign::Center => sym::ALIGN_CENTER,
                TextAlign::Justify => sym::ALIGN_JUSTIFY,
            };
            style_ion.insert(sym::TEXT_ALIGN, IonValue::Symbol(align_sym));
        }
    }

    // Font variant
    if let Some(FontVariant::SmallCaps) = style.font_variant {
        style_ion.insert(
            sym::FONT_VARIANT,
            IonValue::Symbol(sym::FONT_VARIANT_SMALL_CAPS),
        );
    }

    // Margins and padding: vertical (top/bottom) use UNIT_MULTIPLIER, horizontal use UNIT_PERCENT
    // See add_margins/add_padding in mod.rs for axis-specific conversion details
    super::add_margins(
        &mut style_ion,
        style.margin_top.as_ref(),
        style.margin_right.as_ref(),
        style.margin_bottom.as_ref(),
        style.margin_left.as_ref(),
    );
    super::add_padding(
        &mut style_ion,
        style.padding_top.as_ref(),
        style.padding_right.as_ref(),
        style.padding_bottom.as_ref(),
        style.padding_left.as_ref(),
    );

    // Width and height
    super::layout::add_dimensions(&mut style_ion, style);

    // Image-specific properties
    if is_image_style {
        style_ion.insert(sym::IMAGE_FIT, IonValue::Symbol(sym::IMAGE_FIT_CONTAIN));
        style_ion.insert(sym::IMAGE_LAYOUT, IonValue::Symbol(sym::ALIGN_CENTER));
    }

    // Text spacing (indent, line-height)
    super::spacing::add_all(&mut style_ion, style, is_image_style);

    // Colors
    if let Some(ref color) = style.color
        && let Some(val) = color.to_kfx_ion()
    {
        style_ion.insert(sym::COLOR, val);
    }
    if let Some(ref bg_color) = style.background_color
        && let Some(val) = bg_color.to_kfx_ion()
    {
        style_ion.insert(sym::BACKGROUND_COLOR, val);
    }

    // Borders
    add_borders(&mut style_ion, style);

    // Vertical align
    add_vertical_align(&mut style_ion, style);

    // Letter/word spacing
    if let Some(ref spacing) = style.letter_spacing
        && let Some(val) = spacing_to_ion(spacing)
    {
        style_ion.insert(sym::LETTER_SPACING, val);
    }
    if let Some(ref spacing) = style.word_spacing
        && let Some(val) = spacing_to_ion(spacing)
    {
        style_ion.insert(sym::WORD_SPACING, val);
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

    // Layout properties (min/max dimensions, clear, word-break, overflow, visibility, break)
    super::layout::add_min_max_dimensions(&mut style_ion, style);
    super::layout::add_clear(&mut style_ion, style);
    super::layout::add_word_break(&mut style_ion, style);
    super::layout::add_overflow(&mut style_ion, style);
    super::layout::add_visibility(&mut style_ion, style);
    super::layout::add_break_properties(&mut style_ion, style);

    // Border radius
    add_border_radius_props(&mut style_ion, style);

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

    // Table border-spacing
    add_border_spacing(&mut style_ion, style);

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

    // CSS hyphens property ($127)
    add_hyphens(&mut style_ion, style, is_image_style, is_inline_style);

    // CSS box-sizing property ($546)
    add_box_sizing(&mut style_ion, style);

    // CSS unicode-bidi property ($674)
    add_unicode_bidi(&mut style_ion, style);

    // CSS line-break property ($780)
    add_line_break(&mut style_ion, style);

    // CSS text-orientation property ($706)
    add_text_orientation(&mut style_ion, style);

    // Table properties
    add_border_collapse(&mut style_ion, style);
    add_border_spacing(&mut style_ion, style);

    // P2 Phase 2: Layout hints for semantic elements
    add_layout_hints(&mut style_ion, style);

    IonValue::Struct(style_ion)
}

// ============================================================================
// Border and Decoration Functions
// ============================================================================

fn add_borders(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    use crate::css::BorderStyle;

    // Helper to convert BorderStyle to KFX symbol
    fn border_style_to_symbol(bs: BorderStyle) -> Option<u64> {
        match bs {
            BorderStyle::None | BorderStyle::Hidden => None, // Don't output none/hidden
            BorderStyle::Solid => Some(sym::BORDER_STYLE_SOLID),
            BorderStyle::Dotted => Some(sym::BORDER_STYLE_DOTTED),
            BorderStyle::Dashed => Some(sym::BORDER_STYLE_DASHED),
            BorderStyle::Double => Some(sym::BORDER_STYLE_DOUBLE),
            BorderStyle::Groove => Some(sym::BORDER_STYLE_GROOVE),
            BorderStyle::Ridge => Some(sym::BORDER_STYLE_RIDGE),
            BorderStyle::Inset => Some(sym::BORDER_STYLE_INSET),
            BorderStyle::Outset => Some(sym::BORDER_STYLE_OUTSET),
        }
    }

    // Helper to convert color to KFX value
    fn color_to_ion(color: &crate::css::Color) -> Option<IonValue> {
        use crate::css::Color;
        match color {
            Color::Rgba(r, g, b, _) => {
                let rgb = ((*r as u32) << 16) | ((*g as u32) << 8) | (*b as u32);
                Some(IonValue::Int(rgb as i64))
            }
            Color::Current | Color::Transparent => None, // Don't output these
        }
    }

    // Border sides: (border, width_sym, style_sym, color_sym)
    let borders = [
        (
            style.border_top.as_ref(),
            sym::BORDER_TOP_WIDTH,
            sym::BORDER_TOP_STYLE,
            sym::BORDER_TOP_COLOR,
        ),
        (
            style.border_right.as_ref(),
            sym::BORDER_RIGHT_WIDTH,
            sym::BORDER_RIGHT_STYLE,
            sym::BORDER_RIGHT_COLOR,
        ),
        (
            style.border_bottom.as_ref(),
            sym::BORDER_BOTTOM_WIDTH,
            sym::BORDER_BOTTOM_STYLE,
            sym::BORDER_BOTTOM_COLOR,
        ),
        (
            style.border_left.as_ref(),
            sym::BORDER_LEFT_WIDTH,
            sym::BORDER_LEFT_STYLE,
            sym::BORDER_LEFT_COLOR,
        ),
    ];

    for (border, width_sym, style_sym, color_sym) in borders {
        if let Some(b) = border
            && let Some(style_val) = border_style_to_symbol(b.style)
        {
            // Output width if present
            if let Some(ref width) = b.width
                && let Some(width_ion) = width.to_kfx_ion()
            {
                style_ion.insert(width_sym, width_ion);
            }

            // Output style
            style_ion.insert(style_sym, IonValue::Symbol(style_val));

            // Output color if present
            if let Some(ref color) = b.color
                && let Some(color_ion) = color_to_ion(color)
            {
                style_ion.insert(color_sym, color_ion);
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

// ============================================================================
// Border Radius
// ============================================================================

fn add_border_radius_props(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    super::add_border_radius(
        style_ion,
        style.border_radius_tl.as_ref(),
        style.border_radius_tr.as_ref(),
        style.border_radius_br.as_ref(),
        style.border_radius_bl.as_ref(),
    );
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
    if let Some(ref color) = style.text_emphasis_color
        && let Some(val) = color.to_kfx_ion()
    {
        style_ion.insert(sym::TEXT_EMPHASIS_COLOR, val);
    }
}

// P2 Phase 2: Border collapse
// Note: Uses boolean values per yj_to_epub_properties.py: False=separate, True=collapse
// Only outputs for collapse (non-default) since separate is the CSS default
fn add_border_collapse(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    if let Some(collapse) = style.border_collapse
        && collapse == BorderCollapse::Collapse
    {
        // True = collapse
        style_ion.insert(sym::BORDER_COLLAPSE, IonValue::Bool(true));
    }
    // Don't output separate (false) since it's the CSS default
}

// Table border-spacing
// Uses $456 for vertical and $457 for horizontal (per yj_to_epub_properties.py)
fn add_border_spacing(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    use crate::kfx::writer::style::ToKfxIon;

    if let Some(ref horizontal) = style.border_spacing_horizontal
        && let Some(ion_val) = horizontal.to_kfx_ion()
    {
        style_ion.insert(sym::BORDER_SPACING_HORIZONTAL, ion_val);
    }
    if let Some(ref vertical) = style.border_spacing_vertical
        && let Some(ion_val) = vertical.to_kfx_ion()
    {
        style_ion.insert(sym::BORDER_SPACING_VERTICAL, ion_val);
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

/// CSS hyphens property ($127) for text hyphenation control
fn add_hyphens(
    style_ion: &mut HashMap<u64, IonValue>,
    style: &ParsedStyle,
    is_image_style: bool,
    is_inline_style: bool,
) {
    // Images and inline styles don't get hyphens
    if is_image_style || is_inline_style {
        return;
    }

    // Only output hyphens if CSS explicitly specifies it
    if let Some(hyphens) = style.hyphens {
        let hyphens_sym = match hyphens {
            Hyphens::None => sym::HYPHENS_NONE,     // $349
            Hyphens::Manual => sym::HYPHENS_MANUAL, // $384
            Hyphens::Auto => sym::HYPHENS_AUTO,     // $383
        };
        style_ion.insert(sym::HYPHENS, IonValue::Symbol(hyphens_sym));
    }
}

/// CSS box-sizing property ($546)
fn add_box_sizing(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Only output box-sizing if CSS explicitly specifies it
    // Note: content-box is the CSS default, so we could skip it,
    // but Kindle Previewer includes border-box explicitly when specified
    if let Some(box_sizing) = style.box_sizing {
        let box_sizing_sym = match box_sizing {
            BoxSizing::ContentBox => sym::BOX_SIZING_CONTENT_BOX, // $377
            BoxSizing::BorderBox => sym::BOX_SIZING_BORDER_BOX,   // $378
            BoxSizing::PaddingBox => sym::BOX_SIZING_PADDING_BOX, // $379
        };
        style_ion.insert(sym::BOX_SIZING, IonValue::Symbol(box_sizing_sym));
    }
}

/// CSS unicode-bidi property ($674) for bidirectional text control
fn add_unicode_bidi(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Only output unicode-bidi if CSS explicitly specifies it
    // Normal is the default and should not be output
    if let Some(bidi) = style.unicode_bidi {
        let bidi_sym = match bidi {
            UnicodeBidi::Normal => return,             // Default, don't output
            UnicodeBidi::Embed => sym::BIDI_EMBED,     // $675
            UnicodeBidi::Isolate => sym::BIDI_ISOLATE, // $676
            UnicodeBidi::BidiOverride => sym::BIDI_OVERRIDE, // $677
            UnicodeBidi::IsolateOverride => sym::BIDI_ISOLATE_OVERRIDE, // $678
            UnicodeBidi::Plaintext => sym::BIDI_PLAINTEXT, // $679
        };
        style_ion.insert(sym::UNICODE_BIDI, IonValue::Symbol(bidi_sym));
    }
}

/// CSS line-break property ($780) for CJK line breaking rules
fn add_line_break(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Only output line-break if CSS explicitly specifies it
    // Auto is the default and should not be output
    if let Some(lb) = style.line_break {
        let lb_sym = match lb {
            LineBreak::Auto => return,                       // Default, don't output
            LineBreak::Normal => sym::FONT_WEIGHT_NORMAL,    // $350 (shared symbol)
            LineBreak::Loose => sym::LINE_BREAK_LOOSE,       // $781
            LineBreak::Strict => sym::LINE_BREAK_STRICT,     // $782
            LineBreak::Anywhere => sym::LINE_BREAK_ANYWHERE, // $783
        };
        style_ion.insert(sym::LINE_BREAK, IonValue::Symbol(lb_sym));
    }
}

/// CSS text-orientation property ($706) for vertical writing mode
fn add_text_orientation(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    // Only output text-orientation if CSS explicitly specifies it
    // Mixed is the default and should not be output
    if let Some(orient) = style.text_orientation {
        let orient_sym = match orient {
            TextOrientation::Mixed => return, // Default, don't output
            TextOrientation::Upright => sym::TEXT_ORIENTATION_UPRIGHT, // $779
            TextOrientation::Sideways => sym::TEXT_ORIENTATION_SIDEWAYS, // $778
        };
        style_ion.insert(sym::TEXT_ORIENTATION, IonValue::Symbol(orient_sym));
    }
}

// P2 Phase 2: Layout hints for semantic elements
fn add_layout_hints(style_ion: &mut HashMap<u64, IonValue>, style: &ParsedStyle) {
    let mut hints = Vec::new();

    // Add heading hint for h1-h6 elements
    if style.is_heading {
        hints.push(IonValue::Symbol(sym::LAYOUT_HINT_HEADING));
    }
    // Add figure hint for <figure> elements
    if style.is_figure {
        hints.push(IonValue::Symbol(sym::LAYOUT_HINT_FIGURE));
    }
    // Add caption hint for <figcaption> elements
    if style.is_caption {
        hints.push(IonValue::Symbol(sym::LAYOUT_HINT_CAPTION));
    }

    // $761: [hint, ...] - layout hints list
    if !hints.is_empty() {
        style_ion.insert(sym::LAYOUT_HINTS, IonValue::List(hints));
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
