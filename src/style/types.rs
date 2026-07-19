//! Core style types: ComputedStyle and StyleId.

use super::properties::*;

/// Unique identifier for a style in the StylePool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StyleId(
    /// Index into the owning `StylePool`; 0 is the default style.
    pub u32,
);

impl StyleId {
    /// The default style (always 0).
    pub const DEFAULT: StyleId = StyleId(0);
}

/// Computed style for a node (all properties resolved).
///
/// One flat struct with every property boko tracks after the cascade.
/// Enum-typed and `Length` fields use their `Default` (usually the CSS
/// initial value; `Length::Auto` for lengths); `Option` fields are `None`
/// when the property was never set, letting exporters skip emitting them.
///
/// Margins are the exception to the `Length::Auto`-as-unset convention:
/// their CSS initial value is `0`, so [`Default`] gives them `Px(0)` and
/// `Length::Auto` means the author explicitly wrote `margin: auto`
/// (horizontal centering). Conflating the two centered every block whose
/// margins were simply never set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComputedStyle {
    // Font properties
    /// `font-family`; `None` means inherit the reader default.
    pub font_family: Option<String>,
    /// `font-size`; `Length::Auto` means unset (reader default size).
    pub font_size: Length,
    /// `font-weight` as a numeric weight (default 0 means unset; 400 normal, 700 bold).
    pub font_weight: FontWeight,
    /// `font-style` (normal, italic, oblique).
    pub font_style: FontStyle,

    // Colors
    /// `color` (text foreground); `None` means unset.
    pub color: Option<Color>,
    /// `background-color`; `None` means unset (transparent).
    pub background_color: Option<Color>,

    // Text
    /// `text-align` for block content.
    pub text_align: TextAlign,
    /// `text-indent` for the first line; `Length::Auto` means unset.
    pub text_indent: Length,
    /// `line-height`; unitless values are stored as em, `Auto` means unset.
    pub line_height: Length,
    /// Whether `text-decoration` includes an underline line.
    pub text_decoration_underline: bool,
    /// Whether `text-decoration` includes a line-through (strikethrough) line.
    pub text_decoration_line_through: bool,

    // Box model
    /// `display` box mode (block, inline, none, list-item, ...).
    pub display: Display,
    /// `margin-top`; initial value `Px(0)`, `Length::Auto` means explicit `auto`.
    pub margin_top: Length,
    /// `margin-bottom`; initial value `Px(0)`, `Length::Auto` means explicit `auto`.
    pub margin_bottom: Length,
    /// `margin-left`; initial value `Px(0)`, `Length::Auto` means explicit `auto`.
    pub margin_left: Length,
    /// `margin-right`; initial value `Px(0)`, `Length::Auto` means explicit `auto`.
    pub margin_right: Length,
    /// `padding-top`; `Length::Auto` means unset.
    pub padding_top: Length,
    /// `padding-bottom`; `Length::Auto` means unset.
    pub padding_bottom: Length,
    /// `padding-left`; `Length::Auto` means unset.
    pub padding_left: Length,
    /// `padding-right`; `Length::Auto` means unset.
    pub padding_right: Length,

    /// `vertical-align` for inline and table-cell elements; `Super`/`Sub`
    /// drive superscript/subscript detection in exporters.
    pub vertical_align: VerticalAlign,

    /// `list-style-type` (marker kind for list items).
    pub list_style_type: ListStyleType,

    /// `font-variant` (normal or small-caps).
    pub font_variant: FontVariant,

    // Text spacing
    /// `letter-spacing`; `Length::Auto` means unset.
    pub letter_spacing: Length,
    /// `word-spacing`; `Length::Auto` means unset.
    pub word_spacing: Length,

    /// `text-transform` case transformation.
    pub text_transform: TextTransform,

    /// `hyphens` automatic hyphenation mode (defaults to `Manual`).
    pub hyphens: Hyphens,

    /// `white-space` collapsing/wrapping behavior.
    pub white_space: WhiteSpace,

    // Text decoration extensions
    /// `text-decoration-style` for the underline line (solid, dotted, ...).
    pub underline_style: DecorationStyle,
    /// Whether `text-decoration` includes an overline line.
    pub overline: bool,
    /// `text-decoration-color`; `None` means the current text color.
    pub underline_color: Option<Color>,

    // Layout properties
    /// `width`; `Length::Auto` means unset.
    pub width: Length,
    /// `height`; `Length::Auto` means unset.
    pub height: Length,
    /// `max-width`; `Length::Auto` means unset.
    pub max_width: Length,
    /// `min-height`; `Length::Auto` means unset.
    pub min_height: Length,
    /// `float` positioning (none, left, right).
    pub float: Float,

    // Page break properties
    /// `break-before` / `page-break-before`.
    pub break_before: BreakValue,
    /// `break-after` / `page-break-after`.
    pub break_after: BreakValue,
    /// `break-inside` / `page-break-inside`.
    pub break_inside: BreakValue,

    // Border properties (4 sides)
    /// `border-top-style`.
    pub border_style_top: BorderStyle,
    /// `border-right-style`.
    pub border_style_right: BorderStyle,
    /// `border-bottom-style`.
    pub border_style_bottom: BorderStyle,
    /// `border-left-style`.
    pub border_style_left: BorderStyle,
    /// `border-top-width`; `Length::Auto` means unset.
    pub border_width_top: Length,
    /// `border-right-width`; `Length::Auto` means unset.
    pub border_width_right: Length,
    /// `border-bottom-width`; `Length::Auto` means unset.
    pub border_width_bottom: Length,
    /// `border-left-width`; `Length::Auto` means unset.
    pub border_width_left: Length,
    /// `border-top-color`; `None` means the current text color.
    pub border_color_top: Option<Color>,
    /// `border-right-color`; `None` means the current text color.
    pub border_color_right: Option<Color>,
    /// `border-bottom-color`; `None` means the current text color.
    pub border_color_bottom: Option<Color>,
    /// `border-left-color`; `None` means the current text color.
    pub border_color_left: Option<Color>,
    // Border radius (corners)
    /// `border-top-left-radius`; `Length::Auto` means unset.
    pub border_radius_top_left: Length,
    /// `border-top-right-radius`; `Length::Auto` means unset.
    pub border_radius_top_right: Length,
    /// `border-bottom-left-radius`; `Length::Auto` means unset.
    pub border_radius_bottom_left: Length,
    /// `border-bottom-right-radius`; `Length::Auto` means unset.
    pub border_radius_bottom_right: Length,

    /// `list-style-position` (marker inside or outside the item box).
    pub list_style_position: ListStylePosition,

    // Language & rendering
    /// Content language (from `xml:lang`/`lang` attributes, not CSS); used by
    /// KFX export for hyphenation dictionaries. `None` means unset.
    pub language: Option<String>,
    /// `visibility` (visible, hidden, collapse).
    pub visibility: Visibility,
    /// `box-sizing` (content-box or border-box).
    pub box_sizing: BoxSizing,

    // Additional layout properties
    /// `max-height`; `Length::Auto` means unset.
    pub max_height: Length,
    /// `min-width`; `Length::Auto` means unset.
    pub min_width: Length,
    /// `clear` (which floated sides following content must clear).
    pub clear: Clear,

    // Pagination control
    /// `orphans`: minimum lines kept at the bottom of a page (0 = unset).
    pub orphans: u32,
    /// `widows`: minimum lines carried to the next page (0 = unset).
    pub widows: u32,

    // Text wrapping
    /// `word-break` (where lines may break within words).
    pub word_break: WordBreak,
    /// `overflow-wrap` (emergency breaking of long words).
    pub overflow_wrap: OverflowWrap,

    // Table properties
    /// `border-collapse` for tables (separate or collapse).
    pub border_collapse: BorderCollapse,
    /// `border-spacing` between table cells; `Length::Auto` means unset.
    pub border_spacing: Length,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            // CSS initial margin is 0; `Length::Auto` is reserved for an
            // explicit `margin: auto` (see the struct docs).
            margin_top: Length::Px(0.0),
            margin_bottom: Length::Px(0.0),
            margin_left: Length::Px(0.0),
            margin_right: Length::Px(0.0),
            font_family: Default::default(),
            font_size: Default::default(),
            font_weight: Default::default(),
            font_style: Default::default(),
            color: Default::default(),
            background_color: Default::default(),
            text_align: Default::default(),
            text_indent: Default::default(),
            line_height: Default::default(),
            text_decoration_underline: Default::default(),
            text_decoration_line_through: Default::default(),
            display: Default::default(),
            padding_top: Default::default(),
            padding_bottom: Default::default(),
            padding_left: Default::default(),
            padding_right: Default::default(),
            vertical_align: Default::default(),
            list_style_type: Default::default(),
            font_variant: Default::default(),
            letter_spacing: Default::default(),
            word_spacing: Default::default(),
            text_transform: Default::default(),
            hyphens: Default::default(),
            white_space: Default::default(),
            underline_style: Default::default(),
            overline: Default::default(),
            underline_color: Default::default(),
            width: Default::default(),
            height: Default::default(),
            max_width: Default::default(),
            min_height: Default::default(),
            float: Default::default(),
            break_before: Default::default(),
            break_after: Default::default(),
            break_inside: Default::default(),
            border_style_top: Default::default(),
            border_style_right: Default::default(),
            border_style_bottom: Default::default(),
            border_style_left: Default::default(),
            border_width_top: Default::default(),
            border_width_right: Default::default(),
            border_width_bottom: Default::default(),
            border_width_left: Default::default(),
            border_color_top: Default::default(),
            border_color_right: Default::default(),
            border_color_bottom: Default::default(),
            border_color_left: Default::default(),
            border_radius_top_left: Default::default(),
            border_radius_top_right: Default::default(),
            border_radius_bottom_left: Default::default(),
            border_radius_bottom_right: Default::default(),
            list_style_position: Default::default(),
            language: Default::default(),
            visibility: Default::default(),
            box_sizing: Default::default(),
            max_height: Default::default(),
            min_width: Default::default(),
            clear: Default::default(),
            orphans: Default::default(),
            widows: Default::default(),
            word_break: Default::default(),
            overflow_wrap: Default::default(),
            border_collapse: Default::default(),
            border_spacing: Default::default(),
        }
    }
}

impl ComputedStyle {
    /// Check if this style differs from the default (has any non-default properties).
    pub fn is_default(&self) -> bool {
        *self == ComputedStyle::default()
    }

    // --- Modifier checks for exporters ---

    /// Check if the style is bold (font-weight >= 700).
    #[inline]
    pub fn is_bold(&self) -> bool {
        self.font_weight.0 >= 700
    }

    /// Check if the style is italic.
    #[inline]
    pub fn is_italic(&self) -> bool {
        matches!(self.font_style, FontStyle::Italic | FontStyle::Oblique)
    }

    /// Check if the style has underline decoration.
    #[inline]
    pub fn is_underline(&self) -> bool {
        self.text_decoration_underline
    }

    /// Check if the style has strikethrough decoration.
    #[inline]
    pub fn is_strikethrough(&self) -> bool {
        self.text_decoration_line_through
    }

    /// Check if the style is superscript.
    #[inline]
    pub fn is_superscript(&self) -> bool {
        self.vertical_align == VerticalAlign::Super
    }

    /// Check if the style is subscript.
    #[inline]
    pub fn is_subscript(&self) -> bool {
        self.vertical_align == VerticalAlign::Sub
    }

    /// Check if the style uses a monospace font.
    pub fn is_monospace(&self) -> bool {
        self.font_family
            .as_ref()
            .map(|f| {
                let lower = f.to_lowercase();
                lower.contains("mono")
                    || lower.contains("courier")
                    || lower.contains("consolas")
                    || lower.contains("menlo")
            })
            .unwrap_or(false)
    }

    /// Check if the list style is ordered (numbered).
    pub fn is_ordered_list(&self) -> bool {
        matches!(
            self.list_style_type,
            ListStyleType::Decimal
                | ListStyleType::LowerAlpha
                | ListStyleType::UpperAlpha
                | ListStyleType::LowerRoman
                | ListStyleType::UpperRoman
        )
    }

    /// Check if the style uses small-caps font variant.
    #[inline]
    pub fn is_small_caps(&self) -> bool {
        matches!(self.font_variant, FontVariant::SmallCaps)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::style::ToCss;

    #[test]
    fn test_color_to_css_opaque() {
        assert_eq!(Color::BLACK.to_css_string(), "#000000");
        assert_eq!(Color::WHITE.to_css_string(), "#ffffff");
        assert_eq!(Color::rgb(255, 0, 0).to_css_string(), "#ff0000");
        assert_eq!(Color::rgb(0, 128, 255).to_css_string(), "#0080ff");
    }

    #[test]
    fn test_color_to_css_transparent() {
        assert_eq!(Color::TRANSPARENT.to_css_string(), "transparent");
    }

    #[test]
    fn test_color_to_css_alpha() {
        let color = Color::rgba(255, 0, 0, 128);
        let css = color.to_css_string();
        assert!(css.starts_with("rgba(255,0,0,"));
        assert!(css.contains("0.50")); // ~128/255
    }

    #[test]
    fn test_length_to_css() {
        assert_eq!(Length::Auto.to_css_string(), "auto");
        assert_eq!(Length::Px(0.0).to_css_string(), "0");
        assert_eq!(Length::Px(16.0).to_css_string(), "16px");
        assert_eq!(Length::Em(1.5).to_css_string(), "1.5em");
        assert_eq!(Length::Rem(2.0).to_css_string(), "2rem");
        assert_eq!(Length::Percent(50.0).to_css_string(), "50%");
    }

    #[test]
    fn test_font_weight_to_css() {
        assert_eq!(FontWeight::NORMAL.to_css_string(), "normal");
        assert_eq!(FontWeight::BOLD.to_css_string(), "bold");
        assert_eq!(FontWeight(300).to_css_string(), "300");
        assert_eq!(FontWeight(600).to_css_string(), "600");
    }

    #[test]
    fn test_font_style_to_css() {
        assert_eq!(FontStyle::Normal.to_css_string(), "normal");
        assert_eq!(FontStyle::Italic.to_css_string(), "italic");
        assert_eq!(FontStyle::Oblique.to_css_string(), "oblique");
    }

    #[test]
    fn test_text_align_to_css() {
        assert_eq!(TextAlign::Left.to_css_string(), "left");
        assert_eq!(TextAlign::Center.to_css_string(), "center");
        assert_eq!(TextAlign::Justify.to_css_string(), "justify");
    }

    #[test]
    fn test_display_to_css() {
        assert_eq!(Display::Block.to_css_string(), "block");
        assert_eq!(Display::Inline.to_css_string(), "inline");
        assert_eq!(Display::None.to_css_string(), "none");
    }

    #[test]
    fn test_computed_style_to_css_default() {
        let style = ComputedStyle::default();
        // Default style should produce empty CSS (no non-default properties)
        assert_eq!(style.to_css_string(), "");
    }

    #[test]
    fn test_computed_style_to_css_bold() {
        let mut style = ComputedStyle::default();
        style.font_weight = FontWeight::BOLD;
        let css = style.to_css_string();
        assert!(css.contains("font-weight: bold;"));
    }

    #[test]
    fn test_computed_style_to_css_multiple() {
        let mut style = ComputedStyle::default();
        style.font_weight = FontWeight::BOLD;
        style.font_style = FontStyle::Italic;
        style.color = Some(Color::rgb(255, 0, 0));
        style.text_align = TextAlign::Center;

        let css = style.to_css_string();
        assert!(css.contains("font-weight: bold;"));
        assert!(css.contains("font-style: italic;"));
        assert!(css.contains("color: #ff0000;"));
        assert!(css.contains("text-align: center;"));
    }

    #[test]
    fn test_computed_style_to_css_decorations() {
        let mut style = ComputedStyle::default();
        style.text_decoration_underline = true;
        style.text_decoration_line_through = true;

        let css = style.to_css_string();
        assert!(css.contains("text-decoration: underline line-through;"));
    }
}
