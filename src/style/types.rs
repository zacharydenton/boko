//! Core style types: ComputedStyle and StyleId.

use super::properties::*;

/// Unique identifier for a style in the StylePool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StyleId(pub u32);

impl StyleId {
    /// The default style (always 0).
    pub const DEFAULT: StyleId = StyleId(0);
}

/// Computed style for a node (all properties resolved).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ComputedStyle {
    // Font properties
    pub font_family: Option<String>,
    pub font_size: Length,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,

    // Colors
    pub color: Option<Color>,
    pub background_color: Option<Color>,

    // Text
    pub text_align: TextAlign,
    pub text_indent: Length,
    pub line_height: Length,
    pub text_decoration_underline: bool,
    pub text_decoration_line_through: bool,

    // Box model
    pub display: Display,
    pub margin_top: Length,
    pub margin_bottom: Length,
    pub margin_left: Length,
    pub margin_right: Length,
    pub padding_top: Length,
    pub padding_bottom: Length,
    pub padding_left: Length,
    pub padding_right: Length,

    // Vertical alignment for inline and table-cell elements
    pub vertical_align: VerticalAlign,

    // List properties
    pub list_style_type: ListStyleType,

    // Font variant
    pub font_variant: FontVariant,

    // Text spacing
    pub letter_spacing: Length,
    pub word_spacing: Length,

    // Text transform
    pub text_transform: TextTransform,

    // Hyphenation
    pub hyphens: Hyphens,

    // White-space handling
    pub white_space: WhiteSpace,

    // Phase 2: Text decoration extensions
    pub underline_style: DecorationStyle,
    pub overline: bool,
    pub underline_color: Option<Color>,

    // Phase 3: Layout properties
    pub width: Length,
    pub height: Length,
    pub max_width: Length,
    pub min_height: Length,
    pub float: Float,

    // Phase 4: Page break properties
    pub break_before: BreakValue,
    pub break_after: BreakValue,
    pub break_inside: BreakValue,

    // Phase 5: Border properties (4 sides)
    pub border_style_top: BorderStyle,
    pub border_style_right: BorderStyle,
    pub border_style_bottom: BorderStyle,
    pub border_style_left: BorderStyle,
    pub border_width_top: Length,
    pub border_width_right: Length,
    pub border_width_bottom: Length,
    pub border_width_left: Length,
    pub border_color_top: Option<Color>,
    pub border_color_right: Option<Color>,
    pub border_color_bottom: Option<Color>,
    pub border_color_left: Option<Color>,
    // Border radius (corners)
    pub border_radius_top_left: Length,
    pub border_radius_top_right: Length,
    pub border_radius_bottom_left: Length,
    pub border_radius_bottom_right: Length,

    // Phase 6: List properties
    pub list_style_position: ListStylePosition,

    // Phase 7: Amazon properties
    pub language: Option<String>,
    pub visibility: Visibility,
    pub box_sizing: BoxSizing,

    // Phase 8: Additional layout properties
    pub max_height: Length,
    pub min_width: Length,
    pub clear: Clear,

    // Phase 9: Pagination control
    pub orphans: u32,
    pub widows: u32,

    // Phase 10: Text wrapping
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,

    // Phase 11: Table properties
    pub border_collapse: BorderCollapse,
    pub border_spacing: Length,
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
