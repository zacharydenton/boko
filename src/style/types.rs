//! Core style types: ComputedStyle and StylePool.

use std::collections::HashMap;
use std::fmt::Write;

use super::properties::*;
use super::ToCss;

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

impl ToCss for ComputedStyle {
    fn to_css(&self, buf: &mut String) {
        let default = ComputedStyle::default();

        // Font properties
        if let Some(ref family) = self.font_family {
            write!(buf, "font-family: {}; ", family).unwrap();
        }
        if self.font_size != default.font_size {
            buf.push_str("font-size: ");
            self.font_size.to_css(buf);
            buf.push_str("; ");
        }
        if self.font_weight != default.font_weight {
            buf.push_str("font-weight: ");
            self.font_weight.to_css(buf);
            buf.push_str("; ");
        }
        if self.font_style != default.font_style {
            buf.push_str("font-style: ");
            self.font_style.to_css(buf);
            buf.push_str("; ");
        }

        // Colors
        if let Some(color) = self.color {
            buf.push_str("color: ");
            color.to_css(buf);
            buf.push_str("; ");
        }
        if let Some(bg) = self.background_color {
            buf.push_str("background-color: ");
            bg.to_css(buf);
            buf.push_str("; ");
        }

        // Text properties
        if self.text_align != default.text_align {
            buf.push_str("text-align: ");
            self.text_align.to_css(buf);
            buf.push_str("; ");
        }
        if self.text_indent != default.text_indent {
            buf.push_str("text-indent: ");
            self.text_indent.to_css(buf);
            buf.push_str("; ");
        }
        if self.line_height != default.line_height {
            buf.push_str("line-height: ");
            self.line_height.to_css(buf);
            buf.push_str("; ");
        }

        // Text decorations
        let mut decorations = Vec::new();
        if self.text_decoration_underline {
            decorations.push("underline");
        }
        if self.text_decoration_line_through {
            decorations.push("line-through");
        }
        if !decorations.is_empty() {
            write!(buf, "text-decoration: {}; ", decorations.join(" ")).unwrap();
        }

        // Display (only if not block, which is the semantic default for most elements)
        if self.display != default.display {
            buf.push_str("display: ");
            self.display.to_css(buf);
            buf.push_str("; ");
        }

        // Margins
        if self.margin_top != default.margin_top {
            buf.push_str("margin-top: ");
            self.margin_top.to_css(buf);
            buf.push_str("; ");
        }
        if self.margin_bottom != default.margin_bottom {
            buf.push_str("margin-bottom: ");
            self.margin_bottom.to_css(buf);
            buf.push_str("; ");
        }
        if self.margin_left != default.margin_left {
            buf.push_str("margin-left: ");
            self.margin_left.to_css(buf);
            buf.push_str("; ");
        }
        if self.margin_right != default.margin_right {
            buf.push_str("margin-right: ");
            self.margin_right.to_css(buf);
            buf.push_str("; ");
        }

        // Padding
        if self.padding_top != default.padding_top {
            buf.push_str("padding-top: ");
            self.padding_top.to_css(buf);
            buf.push_str("; ");
        }
        if self.padding_bottom != default.padding_bottom {
            buf.push_str("padding-bottom: ");
            self.padding_bottom.to_css(buf);
            buf.push_str("; ");
        }
        if self.padding_left != default.padding_left {
            buf.push_str("padding-left: ");
            self.padding_left.to_css(buf);
            buf.push_str("; ");
        }
        if self.padding_right != default.padding_right {
            buf.push_str("padding-right: ");
            self.padding_right.to_css(buf);
            buf.push_str("; ");
        }

        // Vertical alignment
        if self.vertical_align != default.vertical_align {
            buf.push_str("vertical-align: ");
            self.vertical_align.to_css(buf);
            buf.push_str("; ");
        }

        // List style
        if self.list_style_type != default.list_style_type {
            buf.push_str("list-style-type: ");
            self.list_style_type.to_css(buf);
            buf.push_str("; ");
        }

        // Font variant
        if self.font_variant != FontVariant::Normal {
            buf.push_str("font-variant: ");
            self.font_variant.to_css(buf);
            buf.push_str("; ");
        }

        // Letter spacing
        if self.letter_spacing != default.letter_spacing {
            buf.push_str("letter-spacing: ");
            self.letter_spacing.to_css(buf);
            buf.push_str("; ");
        }

        // Word spacing
        if self.word_spacing != default.word_spacing {
            buf.push_str("word-spacing: ");
            self.word_spacing.to_css(buf);
            buf.push_str("; ");
        }

        // Text transform
        if self.text_transform != default.text_transform {
            buf.push_str("text-transform: ");
            self.text_transform.to_css(buf);
            buf.push_str("; ");
        }

        // Hyphens
        if self.hyphens != default.hyphens {
            buf.push_str("hyphens: ");
            self.hyphens.to_css(buf);
            buf.push_str("; ");
        }

        // White-space
        if self.white_space != default.white_space {
            buf.push_str("white-space: ");
            self.white_space.to_css(buf);
            buf.push_str("; ");
        }

        // Underline style (if different from boolean)
        if self.underline_style != default.underline_style {
            buf.push_str("text-decoration-style: ");
            self.underline_style.to_css(buf);
            buf.push_str("; ");
        }

        // Overline
        if self.overline {
            buf.push_str("text-decoration-line: overline; ");
        }

        // Underline color
        if let Some(color) = self.underline_color {
            buf.push_str("text-decoration-color: ");
            color.to_css(buf);
            buf.push_str("; ");
        }

        // Width
        if self.width != default.width {
            buf.push_str("width: ");
            self.width.to_css(buf);
            buf.push_str("; ");
        }

        // Height
        if self.height != default.height {
            buf.push_str("height: ");
            self.height.to_css(buf);
            buf.push_str("; ");
        }

        // Max-width
        if self.max_width != default.max_width {
            buf.push_str("max-width: ");
            self.max_width.to_css(buf);
            buf.push_str("; ");
        }

        // Min-height
        if self.min_height != default.min_height {
            buf.push_str("min-height: ");
            self.min_height.to_css(buf);
            buf.push_str("; ");
        }

        // Float
        if self.float != default.float {
            buf.push_str("float: ");
            self.float.to_css(buf);
            buf.push_str("; ");
        }

        // Break before
        if self.break_before != default.break_before {
            buf.push_str("break-before: ");
            self.break_before.to_css(buf);
            buf.push_str("; ");
        }

        // Break after
        if self.break_after != default.break_after {
            buf.push_str("break-after: ");
            self.break_after.to_css(buf);
            buf.push_str("; ");
        }

        // Break inside
        if self.break_inside != default.break_inside {
            buf.push_str("break-inside: ");
            self.break_inside.to_css(buf);
            buf.push_str("; ");
        }

        // Border styles
        if self.border_style_top != default.border_style_top {
            buf.push_str("border-top-style: ");
            self.border_style_top.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_style_right != default.border_style_right {
            buf.push_str("border-right-style: ");
            self.border_style_right.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_style_bottom != default.border_style_bottom {
            buf.push_str("border-bottom-style: ");
            self.border_style_bottom.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_style_left != default.border_style_left {
            buf.push_str("border-left-style: ");
            self.border_style_left.to_css(buf);
            buf.push_str("; ");
        }

        // Border widths
        if self.border_width_top != default.border_width_top {
            buf.push_str("border-top-width: ");
            self.border_width_top.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_width_right != default.border_width_right {
            buf.push_str("border-right-width: ");
            self.border_width_right.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_width_bottom != default.border_width_bottom {
            buf.push_str("border-bottom-width: ");
            self.border_width_bottom.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_width_left != default.border_width_left {
            buf.push_str("border-left-width: ");
            self.border_width_left.to_css(buf);
            buf.push_str("; ");
        }

        // Border colors
        if let Some(color) = self.border_color_top {
            buf.push_str("border-top-color: ");
            color.to_css(buf);
            buf.push_str("; ");
        }
        if let Some(color) = self.border_color_right {
            buf.push_str("border-right-color: ");
            color.to_css(buf);
            buf.push_str("; ");
        }
        if let Some(color) = self.border_color_bottom {
            buf.push_str("border-bottom-color: ");
            color.to_css(buf);
            buf.push_str("; ");
        }
        if let Some(color) = self.border_color_left {
            buf.push_str("border-left-color: ");
            color.to_css(buf);
            buf.push_str("; ");
        }

        // Border radius
        if self.border_radius_top_left != default.border_radius_top_left {
            buf.push_str("border-top-left-radius: ");
            self.border_radius_top_left.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_radius_top_right != default.border_radius_top_right {
            buf.push_str("border-top-right-radius: ");
            self.border_radius_top_right.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_radius_bottom_left != default.border_radius_bottom_left {
            buf.push_str("border-bottom-left-radius: ");
            self.border_radius_bottom_left.to_css(buf);
            buf.push_str("; ");
        }
        if self.border_radius_bottom_right != default.border_radius_bottom_right {
            buf.push_str("border-bottom-right-radius: ");
            self.border_radius_bottom_right.to_css(buf);
            buf.push_str("; ");
        }

        // List style position
        if self.list_style_position != default.list_style_position {
            buf.push_str("list-style-position: ");
            self.list_style_position.to_css(buf);
            buf.push_str("; ");
        }

        // Visibility
        if self.visibility != default.visibility {
            buf.push_str("visibility: ");
            self.visibility.to_css(buf);
            buf.push_str("; ");
        }

        // Language (output as data attribute comment - not standard CSS)
        // Note: language is stored but typically output via HTML lang attribute
    }
}

/// SoA style pool for efficient storage and deduplication.
///
/// Styles are interned: identical styles share the same StyleId.
/// This is memory-efficient when many elements share the same style.
#[derive(Clone)]
pub struct StylePool {
    /// All unique styles.
    styles: Vec<ComputedStyle>,
    /// Hash-based deduplication map.
    intern_map: HashMap<ComputedStyle, StyleId>,
}

impl Default for StylePool {
    fn default() -> Self {
        Self::new()
    }
}

impl StylePool {
    /// Create a new style pool with the default style at index 0.
    pub fn new() -> Self {
        let default_style = ComputedStyle::default();
        let mut intern_map = HashMap::new();
        intern_map.insert(default_style.clone(), StyleId::DEFAULT);

        Self {
            styles: vec![default_style],
            intern_map,
        }
    }

    /// Intern a style, returning its StyleId.
    ///
    /// If an identical style already exists, returns the existing ID.
    /// Otherwise, allocates a new style and returns its ID.
    pub fn intern(&mut self, style: ComputedStyle) -> StyleId {
        if let Some(&id) = self.intern_map.get(&style) {
            return id;
        }

        let id = StyleId(self.styles.len() as u32);
        self.intern_map.insert(style.clone(), id);
        self.styles.push(style);
        id
    }

    /// Get a style by ID.
    pub fn get(&self, id: StyleId) -> Option<&ComputedStyle> {
        self.styles.get(id.0 as usize)
    }

    /// Get the number of unique styles.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Check if the pool is empty (should never be, as default style is always present).
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Iterate over all (StyleId, ComputedStyle) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (StyleId, &ComputedStyle)> {
        self.styles
            .iter()
            .enumerate()
            .map(|(i, s)| (StyleId(i as u32), s))
    }
}

impl std::fmt::Debug for StylePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StylePool")
            .field("count", &self.styles.len())
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

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

    #[test]
    fn test_style_pool_interning() {
        let mut pool = StylePool::new();

        let mut style1 = ComputedStyle::default();
        style1.font_weight = FontWeight::BOLD;

        let id1 = pool.intern(style1.clone());
        let id2 = pool.intern(style1);

        // Same style should get same ID
        assert_eq!(id1, id2);
        assert_eq!(pool.len(), 2); // default + bold
    }

    #[test]
    fn test_style_pool_iter() {
        let mut pool = StylePool::new();

        let mut style = ComputedStyle::default();
        style.font_weight = FontWeight::BOLD;
        pool.intern(style);

        let ids: Vec<StyleId> = pool.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![StyleId(0), StyleId(1)]);
    }
}
