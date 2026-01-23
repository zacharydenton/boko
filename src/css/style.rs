//! ParsedStyle struct and implementations.
//!
//! Contains the main CSS style representation used throughout the library.

use super::types::*;

/// Parsed CSS style properties
/// Note: Custom Hash/Eq implementation excludes image_width_px and image_height_px
/// since they don't affect KFX style output and would cause duplicate styles.
#[derive(Debug, Clone, Default)]
pub struct ParsedStyle {
    pub font_family: Option<String>,
    pub font_size: Option<CssValue>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_variant: Option<FontVariant>,
    pub text_transform: Option<TextTransform>,
    pub text_align: Option<TextAlign>,
    pub text_indent: Option<CssValue>,
    pub line_height: Option<CssValue>,
    pub margin_top: Option<CssValue>,
    pub margin_bottom: Option<CssValue>,
    pub margin_left: Option<CssValue>,
    pub margin_right: Option<CssValue>,
    pub padding_top: Option<CssValue>,
    pub padding_bottom: Option<CssValue>,
    pub padding_left: Option<CssValue>,
    pub padding_right: Option<CssValue>,
    pub color: Option<Color>,
    pub background_color: Option<Color>,
    pub border_top: Option<Border>,
    pub border_bottom: Option<Border>,
    pub border_left: Option<Border>,
    pub border_right: Option<Border>,
    pub display: Option<Display>,
    pub position: Option<Position>,
    pub left: Option<CssValue>,
    pub width: Option<CssValue>,
    pub height: Option<CssValue>,
    pub min_width: Option<CssValue>,
    pub min_height: Option<CssValue>,
    pub max_width: Option<CssValue>,
    pub max_height: Option<CssValue>,
    pub vertical_align: Option<VerticalAlign>,
    pub clear: Option<Clear>,
    pub word_break: Option<WordBreak>,
    pub overflow: Option<Overflow>,
    pub visibility: Option<Visibility>,
    pub break_before: Option<BreakValue>,
    pub break_after: Option<BreakValue>,
    pub break_inside: Option<BreakValue>,
    pub border_radius_tl: Option<CssValue>,
    pub border_radius_tr: Option<CssValue>,
    pub border_radius_br: Option<CssValue>,
    pub border_radius_bl: Option<CssValue>,
    pub letter_spacing: Option<CssValue>,
    pub word_spacing: Option<CssValue>,
    pub white_space_nowrap: Option<bool>,
    pub text_decoration_underline: bool,
    pub text_decoration_overline: bool,
    pub text_decoration_line_through: bool,
    pub text_decoration_line_style: Option<TextDecorationLineStyle>,
    pub opacity: Option<u8>,
    pub is_image: bool,
    pub is_inline: bool,
    pub is_heading: bool,
    pub is_figure: bool,
    pub is_caption: bool,
    pub image_width_px: Option<u32>,
    pub image_height_px: Option<u32>,
    pub lang: Option<String>,
    pub list_style_type: Option<ListStyleType>,
    pub list_style_position: Option<ListStylePosition>,
    pub writing_mode: Option<WritingMode>,
    pub text_combine_upright: Option<TextCombineUpright>,
    pub box_shadow: Option<String>,
    pub text_shadow: Option<String>,
    pub ruby_position: Option<RubyPosition>,
    pub ruby_align: Option<RubyAlign>,
    pub ruby_merge: Option<RubyMerge>,
    pub text_emphasis_style: Option<TextEmphasisStyle>,
    pub text_emphasis_color: Option<Color>,
    pub border_collapse: Option<BorderCollapse>,
    pub border_spacing_horizontal: Option<CssValue>,
    pub border_spacing_vertical: Option<CssValue>,
    pub drop_cap: Option<DropCap>,
    pub transform: Option<Transform>,
    pub transform_origin: Option<TransformOrigin>,
    pub baseline_shift: Option<CssValue>,
    pub column_count: Option<ColumnCount>,
    pub float: Option<CssFloat>,
    pub hyphens: Option<Hyphens>,
    pub box_sizing: Option<BoxSizing>,
    pub unicode_bidi: Option<UnicodeBidi>,
    pub line_break: Option<LineBreak>,
    pub text_orientation: Option<TextOrientation>,
}

impl ParsedStyle {
    /// Merge another style into this one (other takes precedence)
    pub fn merge(&mut self, other: &ParsedStyle) {
        if other.font_family.is_some() {
            self.font_family.clone_from(&other.font_family);
        }
        if other.font_size.is_some() {
            self.font_size.clone_from(&other.font_size);
        }
        if other.font_weight.is_some() {
            self.font_weight = other.font_weight;
        }
        if other.font_style.is_some() {
            self.font_style = other.font_style;
        }
        if other.font_variant.is_some() {
            self.font_variant = other.font_variant;
        }
        if other.text_transform.is_some() {
            self.text_transform = other.text_transform;
        }
        if other.text_align.is_some() {
            self.text_align = other.text_align;
        }
        if other.text_indent.is_some() {
            self.text_indent.clone_from(&other.text_indent);
        }
        if other.line_height.is_some() {
            self.line_height.clone_from(&other.line_height);
        }
        if other.margin_top.is_some() {
            self.margin_top.clone_from(&other.margin_top);
        }
        if other.margin_bottom.is_some() {
            self.margin_bottom.clone_from(&other.margin_bottom);
        }
        if other.margin_left.is_some() {
            self.margin_left.clone_from(&other.margin_left);
        }
        if other.margin_right.is_some() {
            self.margin_right.clone_from(&other.margin_right);
        }
        if other.padding_top.is_some() {
            self.padding_top.clone_from(&other.padding_top);
        }
        if other.padding_bottom.is_some() {
            self.padding_bottom.clone_from(&other.padding_bottom);
        }
        if other.padding_left.is_some() {
            self.padding_left.clone_from(&other.padding_left);
        }
        if other.padding_right.is_some() {
            self.padding_right.clone_from(&other.padding_right);
        }
        if other.color.is_some() {
            self.color.clone_from(&other.color);
        }
        if other.background_color.is_some() {
            self.background_color.clone_from(&other.background_color);
        }
        if other.border_top.is_some() {
            self.border_top.clone_from(&other.border_top);
        }
        if other.border_bottom.is_some() {
            self.border_bottom.clone_from(&other.border_bottom);
        }
        if other.border_left.is_some() {
            self.border_left.clone_from(&other.border_left);
        }
        if other.border_right.is_some() {
            self.border_right.clone_from(&other.border_right);
        }
        if other.display.is_some() {
            self.display = other.display;
        }
        if other.position.is_some() {
            self.position = other.position;
        }
        if other.left.is_some() {
            self.left.clone_from(&other.left);
        }
        if other.width.is_some() {
            self.width.clone_from(&other.width);
        }
        if other.height.is_some() {
            self.height.clone_from(&other.height);
        }
        if other.min_width.is_some() {
            self.min_width.clone_from(&other.min_width);
        }
        if other.min_height.is_some() {
            self.min_height.clone_from(&other.min_height);
        }
        if other.max_width.is_some() {
            self.max_width.clone_from(&other.max_width);
        }
        if other.max_height.is_some() {
            self.max_height.clone_from(&other.max_height);
        }
        if other.vertical_align.is_some() {
            self.vertical_align = other.vertical_align;
        }
        if other.clear.is_some() {
            self.clear = other.clear;
        }
        if other.word_break.is_some() {
            self.word_break = other.word_break;
        }
        if other.overflow.is_some() {
            self.overflow = other.overflow;
        }
        if other.visibility.is_some() {
            self.visibility = other.visibility;
        }
        if other.break_before.is_some() {
            self.break_before = other.break_before;
        }
        if other.break_after.is_some() {
            self.break_after = other.break_after;
        }
        if other.break_inside.is_some() {
            self.break_inside = other.break_inside;
        }
        if other.border_radius_tl.is_some() {
            self.border_radius_tl.clone_from(&other.border_radius_tl);
        }
        if other.border_radius_tr.is_some() {
            self.border_radius_tr.clone_from(&other.border_radius_tr);
        }
        if other.border_radius_br.is_some() {
            self.border_radius_br.clone_from(&other.border_radius_br);
        }
        if other.border_radius_bl.is_some() {
            self.border_radius_bl.clone_from(&other.border_radius_bl);
        }
        if other.letter_spacing.is_some() {
            self.letter_spacing.clone_from(&other.letter_spacing);
        }
        if other.word_spacing.is_some() {
            self.word_spacing.clone_from(&other.word_spacing);
        }
        if other.white_space_nowrap.is_some() {
            self.white_space_nowrap = other.white_space_nowrap;
        }
        if other.text_decoration_underline {
            self.text_decoration_underline = true;
        }
        if other.text_decoration_overline {
            self.text_decoration_overline = true;
        }
        if other.text_decoration_line_through {
            self.text_decoration_line_through = true;
        }
        if other.text_decoration_line_style.is_some() {
            self.text_decoration_line_style = other.text_decoration_line_style;
        }
        if other.opacity.is_some() {
            self.opacity = other.opacity;
        }
        if other.is_image {
            self.is_image = true;
        }
        if other.is_inline {
            self.is_inline = true;
        }
        if other.is_heading {
            self.is_heading = true;
        }
        if other.image_width_px.is_some() {
            self.image_width_px = other.image_width_px;
        }
        if other.image_height_px.is_some() {
            self.image_height_px = other.image_height_px;
        }
        if other.lang.is_some() {
            self.lang.clone_from(&other.lang);
        }
        if other.list_style_type.is_some() {
            self.list_style_type = other.list_style_type;
        }
        if other.list_style_position.is_some() {
            self.list_style_position = other.list_style_position;
        }
        if other.writing_mode.is_some() {
            self.writing_mode = other.writing_mode;
        }
        if other.text_combine_upright.is_some() {
            self.text_combine_upright = other.text_combine_upright;
        }
        if other.box_shadow.is_some() {
            self.box_shadow.clone_from(&other.box_shadow);
        }
        if other.text_shadow.is_some() {
            self.text_shadow.clone_from(&other.text_shadow);
        }
        if other.ruby_position.is_some() {
            self.ruby_position = other.ruby_position;
        }
        if other.ruby_align.is_some() {
            self.ruby_align = other.ruby_align;
        }
        if other.ruby_merge.is_some() {
            self.ruby_merge = other.ruby_merge;
        }
        if other.text_emphasis_style.is_some() {
            self.text_emphasis_style = other.text_emphasis_style;
        }
        if other.text_emphasis_color.is_some() {
            self.text_emphasis_color
                .clone_from(&other.text_emphasis_color);
        }
        if other.border_collapse.is_some() {
            self.border_collapse = other.border_collapse;
        }
        if other.border_spacing_horizontal.is_some() {
            self.border_spacing_horizontal
                .clone_from(&other.border_spacing_horizontal);
        }
        if other.border_spacing_vertical.is_some() {
            self.border_spacing_vertical
                .clone_from(&other.border_spacing_vertical);
        }
        if other.drop_cap.is_some() {
            self.drop_cap = other.drop_cap;
        }
        if other.transform.is_some() {
            self.transform.clone_from(&other.transform);
        }
        if other.transform_origin.is_some() {
            self.transform_origin.clone_from(&other.transform_origin);
        }
        if other.baseline_shift.is_some() {
            self.baseline_shift.clone_from(&other.baseline_shift);
        }
        if other.column_count.is_some() {
            self.column_count = other.column_count;
        }
        if other.float.is_some() {
            self.float = other.float;
        }
        if other.hyphens.is_some() {
            self.hyphens = other.hyphens;
        }
        if other.box_sizing.is_some() {
            self.box_sizing = other.box_sizing;
        }
        if other.unicode_bidi.is_some() {
            self.unicode_bidi = other.unicode_bidi;
        }
        if other.line_break.is_some() {
            self.line_break = other.line_break;
        }
        if other.text_orientation.is_some() {
            self.text_orientation = other.text_orientation;
        }
    }

    /// Check if this style indicates the element is hidden/invisible
    pub fn is_hidden(&self) -> bool {
        if self.display == Some(Display::None) {
            return true;
        }

        if self.position == Some(Position::Absolute)
            && let Some(ref left) = self.left
        {
            match left {
                CssValue::Em(v) if *v < -100.0 => return true,
                CssValue::Px(v) if *v < -1000.0 => return true,
                _ => {}
            }
        }

        false
    }

    /// Check if this style has any properties set
    pub fn is_empty(&self) -> bool {
        self.font_family.is_none()
            && self.font_size.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
            && self.font_variant.is_none()
            && self.text_transform.is_none()
            && self.text_align.is_none()
            && self.text_indent.is_none()
            && self.line_height.is_none()
            && self.letter_spacing.is_none()
            && self.word_spacing.is_none()
            && self.white_space_nowrap.is_none()
            && self.margin_top.is_none()
            && self.margin_bottom.is_none()
            && self.margin_left.is_none()
            && self.margin_right.is_none()
            && self.padding_top.is_none()
            && self.padding_bottom.is_none()
            && self.padding_left.is_none()
            && self.padding_right.is_none()
            && self.color.is_none()
            && self.background_color.is_none()
            && self.border_top.is_none()
            && self.border_bottom.is_none()
            && self.border_left.is_none()
            && self.border_right.is_none()
            && self.border_radius_tl.is_none()
            && self.border_radius_tr.is_none()
            && self.border_radius_br.is_none()
            && self.border_radius_bl.is_none()
            && self.display.is_none()
            && self.position.is_none()
            && self.left.is_none()
            && self.visibility.is_none()
            && self.overflow.is_none()
            && self.float.is_none()
            && self.clear.is_none()
            && self.width.is_none()
            && self.height.is_none()
            && self.min_width.is_none()
            && self.min_height.is_none()
            && self.max_width.is_none()
            && self.max_height.is_none()
            && self.vertical_align.is_none()
            && self.break_before.is_none()
            && self.break_after.is_none()
            && self.break_inside.is_none()
            && self.word_break.is_none()
            && !self.text_decoration_underline
            && !self.text_decoration_overline
            && !self.text_decoration_line_through
            && self.text_decoration_line_style.is_none()
            && self.opacity.is_none()
            && self.list_style_type.is_none()
            && self.list_style_position.is_none()
            && self.hyphens.is_none()
            && self.box_sizing.is_none()
            && self.unicode_bidi.is_none()
            && self.line_break.is_none()
            && self.text_orientation.is_none()
            && self.border_collapse.is_none()
            && self.border_spacing_horizontal.is_none()
            && self.border_spacing_vertical.is_none()
    }

    /// Convert this style to a CSS declaration string.
    pub fn to_css_string(&self) -> String {
        let mut props = Vec::new();

        if let Some(ref family) = self.font_family {
            props.push(format!("font-family: {}", family));
        }
        if let Some(ref size) = self.font_size {
            props.push(format!("font-size: {}", css_value_to_string(size)));
        }
        if let Some(weight) = self.font_weight {
            let val = match weight {
                FontWeight::Normal => "normal",
                FontWeight::Bold => "bold",
                FontWeight::Weight(w) => return format!("font-weight: {}", w),
            };
            props.push(format!("font-weight: {}", val));
        }
        if let Some(style) = self.font_style {
            let val = match style {
                FontStyle::Normal => "normal",
                FontStyle::Italic => "italic",
                FontStyle::Oblique => "oblique",
            };
            props.push(format!("font-style: {}", val));
        }
        if let Some(variant) = self.font_variant
            && variant == FontVariant::SmallCaps
        {
            props.push("font-variant: small-caps".to_string());
        }
        if let Some(align) = self.text_align {
            let val = match align {
                TextAlign::Left => "left",
                TextAlign::Right => "right",
                TextAlign::Center => "center",
                TextAlign::Justify => "justify",
            };
            props.push(format!("text-align: {}", val));
        }
        if let Some(transform) = self.text_transform {
            let val = match transform {
                TextTransform::None => "none",
                TextTransform::Uppercase => "uppercase",
                TextTransform::Lowercase => "lowercase",
                TextTransform::Capitalize => "capitalize",
            };
            props.push(format!("text-transform: {}", val));
        }
        if let Some(ref indent) = self.text_indent {
            props.push(format!("text-indent: {}", css_value_to_string(indent)));
        }
        if let Some(ref lh) = self.line_height {
            props.push(format!("line-height: {}", css_value_to_string(lh)));
        }
        if let Some(ref v) = self.margin_top {
            props.push(format!("margin-top: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.margin_right {
            props.push(format!("margin-right: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.margin_bottom {
            props.push(format!("margin-bottom: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.margin_left {
            props.push(format!("margin-left: {}", css_value_to_string(v)));
        }
        if let Some(ref color) = self.color
            && let Some(css) = color_to_css(color)
        {
            props.push(format!("color: {}", css));
        }
        if let Some(ref color) = self.background_color
            && let Some(css) = color_to_css(color)
        {
            props.push(format!("background-color: {}", css));
        }
        if let Some(ref v) = self.width {
            props.push(format!("width: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.height {
            props.push(format!("height: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.max_width {
            props.push(format!("max-width: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.max_height {
            props.push(format!("max-height: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.min_width {
            props.push(format!("min-width: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.min_height {
            props.push(format!("min-height: {}", css_value_to_string(v)));
        }
        if let Some(valign) = self.vertical_align {
            let val = match valign {
                VerticalAlign::Baseline => "baseline",
                VerticalAlign::Top => "top",
                VerticalAlign::Middle => "middle",
                VerticalAlign::Bottom => "bottom",
                VerticalAlign::Super => "super",
                VerticalAlign::Sub => "sub",
                VerticalAlign::TextTop => "text-top",
                VerticalAlign::TextBottom => "text-bottom",
            };
            props.push(format!("vertical-align: {}", val));
        }
        if self.white_space_nowrap == Some(true) {
            props.push("white-space: nowrap".to_string());
        }
        if self.text_decoration_underline {
            props.push("text-decoration: underline".to_string());
        }
        if self.text_decoration_line_through {
            props.push("text-decoration: line-through".to_string());
        }
        if self.text_decoration_overline {
            props.push("text-decoration: overline".to_string());
        }
        if let Some(brk) = self.break_before
            && brk != BreakValue::Auto
        {
            props.push(format!("break-before: {}", break_value_to_css(brk)));
        }
        if let Some(brk) = self.break_after
            && brk != BreakValue::Auto
        {
            props.push(format!("break-after: {}", break_value_to_css(brk)));
        }
        if let Some(brk) = self.break_inside
            && brk != BreakValue::Auto
        {
            props.push(format!("break-inside: {}", break_value_to_css(brk)));
        }
        if let Some(ref v) = self.letter_spacing {
            props.push(format!("letter-spacing: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.word_spacing {
            props.push(format!("word-spacing: {}", css_value_to_string(v)));
        }
        if let Some(opacity) = self.opacity {
            let val = opacity as f32 / 100.0;
            props.push(format!("opacity: {}", val));
        }

        props.join("; ")
    }

    /// Create an inline-only version of this style, keeping only inline-appropriate properties.
    /// Block-level properties (text-align, margins, padding, etc.) are excluded.
    /// Used for inline style runs in KFX where block properties shouldn't appear.
    pub fn to_inline_only(&self) -> ParsedStyle {
        ParsedStyle {
            // Inline font/text properties
            font_family: self.font_family.clone(),
            font_size: self.font_size.clone(),
            font_weight: self.font_weight,
            font_style: self.font_style,
            font_variant: self.font_variant,
            text_transform: self.text_transform,
            color: self.color.clone(),
            background_color: self.background_color.clone(),
            line_height: self.line_height.clone(),
            letter_spacing: self.letter_spacing.clone(),
            word_spacing: self.word_spacing.clone(),
            vertical_align: self.vertical_align,
            text_decoration_underline: self.text_decoration_underline,
            text_decoration_overline: self.text_decoration_overline,
            text_decoration_line_through: self.text_decoration_line_through,
            text_decoration_line_style: self.text_decoration_line_style,
            opacity: self.opacity,
            baseline_shift: self.baseline_shift.clone(),
            unicode_bidi: self.unicode_bidi,
            text_emphasis_style: self.text_emphasis_style,
            text_emphasis_color: self.text_emphasis_color.clone(),
            lang: self.lang.clone(),
            // Mark as inline style
            is_inline: true,
            // Block-level properties excluded (use defaults)
            ..Default::default()
        }
    }
}

/// Convert CssValue to CSS string representation
pub fn css_value_to_string(val: &CssValue) -> String {
    match val {
        CssValue::Px(v) => format!("{}px", format_number(*v)),
        CssValue::Em(v) => format!("{}em", format_number(*v)),
        CssValue::Rem(v) => format!("{}rem", format_number(*v)),
        CssValue::Percent(v) => format!("{}%", format_number(*v)),
        CssValue::Number(v) => format_number(*v),
        CssValue::Keyword(k) => k.clone(),
        CssValue::Vw(v) => format!("{}vw", format_number(*v)),
        CssValue::Vh(v) => format!("{}vh", format_number(*v)),
        CssValue::Vmin(v) => format!("{}vmin", format_number(*v)),
        CssValue::Vmax(v) => format!("{}vmax", format_number(*v)),
        CssValue::Ch(v) => format!("{}ch", format_number(*v)),
        CssValue::Ex(v) => format!("{}ex", format_number(*v)),
        CssValue::Cm(v) => format!("{}cm", format_number(*v)),
        CssValue::Mm(v) => format!("{}mm", format_number(*v)),
        CssValue::In(v) => format!("{}in", format_number(*v)),
        CssValue::Pt(v) => format!("{}pt", format_number(*v)),
    }
}

fn format_number(v: f32) -> String {
    if (v - v.round()).abs() < 0.0001 {
        format!("{}", v as i32)
    } else {
        format!("{:.6}", v)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn color_to_css(color: &Color) -> Option<String> {
    match color {
        Color::Rgba(r, g, b, 255) => {
            if *r == 0 && *g == 0 && *b == 0 {
                Some("black".to_string())
            } else if *r == 255 && *g == 255 && *b == 255 {
                Some("white".to_string())
            } else {
                Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
            }
        }
        Color::Rgba(r, g, b, a) => {
            Some(format!("rgba({}, {}, {}, {})", r, g, b, *a as f32 / 255.0))
        }
        Color::Current => Some("currentColor".to_string()),
        Color::Transparent => Some("transparent".to_string()),
    }
}

fn break_value_to_css(brk: BreakValue) -> &'static str {
    match brk {
        BreakValue::Auto => "auto",
        BreakValue::Avoid => "avoid",
        BreakValue::AvoidPage => "avoid-page",
        BreakValue::Page => "page",
        BreakValue::Left => "left",
        BreakValue::Right => "right",
        BreakValue::Column => "column",
        BreakValue::AvoidColumn => "avoid-column",
    }
}

// Helper to normalize CssValue - treat zero values as None (default)
fn normalize_spacing(val: &Option<CssValue>) -> Option<&CssValue> {
    match val {
        Some(CssValue::Px(v)) if v.abs() < 0.001 => None,
        Some(CssValue::Em(v)) if v.abs() < 0.001 => None,
        Some(CssValue::Percent(v)) if v.abs() < 0.001 => None,
        Some(v) => Some(v),
        None => None,
    }
}

fn normalize_display(val: &Option<Display>) -> Option<Display> {
    match val {
        Some(Display::Block) => None,
        other => *other,
    }
}

fn normalize_font_style(val: &Option<FontStyle>) -> Option<FontStyle> {
    match val {
        Some(FontStyle::Normal) => None,
        other => *other,
    }
}

impl PartialEq for ParsedStyle {
    fn eq(&self, other: &Self) -> bool {
        self.font_family == other.font_family
            && self.font_size == other.font_size
            && self.font_weight == other.font_weight
            && normalize_font_style(&self.font_style) == normalize_font_style(&other.font_style)
            && self.font_variant == other.font_variant
            && self.text_transform == other.text_transform
            && self.text_align == other.text_align
            && normalize_spacing(&self.text_indent) == normalize_spacing(&other.text_indent)
            && self.line_height == other.line_height
            && normalize_spacing(&self.margin_top) == normalize_spacing(&other.margin_top)
            && normalize_spacing(&self.margin_bottom) == normalize_spacing(&other.margin_bottom)
            && normalize_spacing(&self.margin_left) == normalize_spacing(&other.margin_left)
            && normalize_spacing(&self.margin_right) == normalize_spacing(&other.margin_right)
            && normalize_spacing(&self.padding_top) == normalize_spacing(&other.padding_top)
            && normalize_spacing(&self.padding_bottom) == normalize_spacing(&other.padding_bottom)
            && normalize_spacing(&self.padding_left) == normalize_spacing(&other.padding_left)
            && normalize_spacing(&self.padding_right) == normalize_spacing(&other.padding_right)
            && self.color == other.color
            && self.background_color == other.background_color
            && self.border_top == other.border_top
            && self.border_bottom == other.border_bottom
            && self.border_left == other.border_left
            && self.border_right == other.border_right
            && normalize_display(&self.display) == normalize_display(&other.display)
            && self.position == other.position
            && self.left == other.left
            && self.width == other.width
            && self.height == other.height
            && self.min_width == other.min_width
            && self.min_height == other.min_height
            && self.max_width == other.max_width
            && self.max_height == other.max_height
            && self.vertical_align == other.vertical_align
            && self.clear == other.clear
            && self.word_break == other.word_break
            && self.overflow == other.overflow
            && self.visibility == other.visibility
            && self.break_before == other.break_before
            && self.break_after == other.break_after
            && self.break_inside == other.break_inside
            && self.border_radius_tl == other.border_radius_tl
            && self.border_radius_tr == other.border_radius_tr
            && self.border_radius_br == other.border_radius_br
            && self.border_radius_bl == other.border_radius_bl
            && self.letter_spacing == other.letter_spacing
            && self.word_spacing == other.word_spacing
            && self.white_space_nowrap == other.white_space_nowrap
            && self.text_decoration_underline == other.text_decoration_underline
            && self.text_decoration_overline == other.text_decoration_overline
            && self.text_decoration_line_through == other.text_decoration_line_through
            && self.text_decoration_line_style == other.text_decoration_line_style
            && self.opacity == other.opacity
            && self.is_image == other.is_image
            && self.is_inline == other.is_inline
            && self.is_heading == other.is_heading
            && self.lang == other.lang
            && self.list_style_type == other.list_style_type
            && self.list_style_position == other.list_style_position
            && self.writing_mode == other.writing_mode
            && self.text_combine_upright == other.text_combine_upright
            && self.box_shadow == other.box_shadow
            && self.text_shadow == other.text_shadow
            && self.ruby_position == other.ruby_position
            && self.ruby_align == other.ruby_align
            && self.ruby_merge == other.ruby_merge
            && self.text_emphasis_style == other.text_emphasis_style
            && self.text_emphasis_color == other.text_emphasis_color
            && self.border_collapse == other.border_collapse
            && self.border_spacing_horizontal == other.border_spacing_horizontal
            && self.border_spacing_vertical == other.border_spacing_vertical
            && self.drop_cap == other.drop_cap
            && self.transform == other.transform
            && self.transform_origin == other.transform_origin
            && self.baseline_shift == other.baseline_shift
            && self.column_count == other.column_count
            && self.float == other.float
            && self.hyphens == other.hyphens
            && self.box_sizing == other.box_sizing
            && self.unicode_bidi == other.unicode_bidi
            && self.line_break == other.line_break
            && self.text_orientation == other.text_orientation
    }
}

impl Eq for ParsedStyle {}

impl std::hash::Hash for ParsedStyle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.font_family.hash(state);
        self.font_size.hash(state);
        self.font_weight.hash(state);
        normalize_font_style(&self.font_style).hash(state);
        self.font_variant.hash(state);
        self.text_transform.hash(state);
        self.text_align.hash(state);
        normalize_spacing(&self.text_indent).hash(state);
        self.line_height.hash(state);
        normalize_spacing(&self.margin_top).hash(state);
        normalize_spacing(&self.margin_bottom).hash(state);
        normalize_spacing(&self.margin_left).hash(state);
        normalize_spacing(&self.margin_right).hash(state);
        normalize_spacing(&self.padding_top).hash(state);
        normalize_spacing(&self.padding_bottom).hash(state);
        normalize_spacing(&self.padding_left).hash(state);
        normalize_spacing(&self.padding_right).hash(state);
        self.color.hash(state);
        self.background_color.hash(state);
        self.border_top.hash(state);
        self.border_bottom.hash(state);
        self.border_left.hash(state);
        self.border_right.hash(state);
        normalize_display(&self.display).hash(state);
        self.position.hash(state);
        self.left.hash(state);
        self.width.hash(state);
        self.height.hash(state);
        self.min_width.hash(state);
        self.min_height.hash(state);
        self.max_width.hash(state);
        self.max_height.hash(state);
        self.vertical_align.hash(state);
        self.clear.hash(state);
        self.word_break.hash(state);
        self.overflow.hash(state);
        self.visibility.hash(state);
        self.break_before.hash(state);
        self.break_after.hash(state);
        self.break_inside.hash(state);
        self.border_radius_tl.hash(state);
        self.border_radius_tr.hash(state);
        self.border_radius_br.hash(state);
        self.border_radius_bl.hash(state);
        self.letter_spacing.hash(state);
        self.word_spacing.hash(state);
        self.white_space_nowrap.hash(state);
        self.text_decoration_underline.hash(state);
        self.text_decoration_overline.hash(state);
        self.text_decoration_line_through.hash(state);
        self.text_decoration_line_style.hash(state);
        self.opacity.hash(state);
        self.is_image.hash(state);
        self.is_inline.hash(state);
        self.is_heading.hash(state);
        self.lang.hash(state);
        self.list_style_type.hash(state);
        self.list_style_position.hash(state);
        self.writing_mode.hash(state);
        self.text_combine_upright.hash(state);
        self.box_shadow.hash(state);
        self.text_shadow.hash(state);
        self.ruby_position.hash(state);
        self.ruby_align.hash(state);
        self.ruby_merge.hash(state);
        self.text_emphasis_style.hash(state);
        self.text_emphasis_color.hash(state);
        self.border_collapse.hash(state);
        self.drop_cap.hash(state);
        self.transform.hash(state);
        self.transform_origin.hash(state);
        self.baseline_shift.hash(state);
        self.column_count.hash(state);
        self.float.hash(state);
    }
}
