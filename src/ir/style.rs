//! Style pool with SoA (Struct of Arrays) layout for efficient storage.

use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{Hash, Hasher};

/// Trait for converting IR style values back to CSS strings.
pub trait ToCss {
    /// Write this value as CSS to the buffer.
    fn to_css(&self, buf: &mut String);

    /// Convert to a CSS string (convenience method).
    fn to_css_string(&self) -> String {
        let mut buf = String::new();
        self.to_css(&mut buf);
        buf
    }
}

/// Unique identifier for a style in the StylePool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StyleId(pub u32);

impl StyleId {
    /// The default style (always 0).
    pub const DEFAULT: StyleId = StyleId(0);
}

/// Font weight (100-900, with named constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const NORMAL: FontWeight = FontWeight(400);
    pub const BOLD: FontWeight = FontWeight(700);
}

impl ToCss for FontWeight {
    fn to_css(&self, buf: &mut String) {
        match self.0 {
            400 => buf.push_str("normal"),
            700 => buf.push_str("bold"),
            w => write!(buf, "{}", w).unwrap(),
        }
    }
}

/// Font style (normal, italic, oblique).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

impl ToCss for FontStyle {
    fn to_css(&self, buf: &mut String) {
        buf.push_str(match self {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        });
    }
}

/// Text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

impl ToCss for TextAlign {
    fn to_css(&self, buf: &mut String) {
        buf.push_str(match self {
            TextAlign::Start => "start",
            TextAlign::End => "end",
            TextAlign::Left => "left",
            TextAlign::Right => "right",
            TextAlign::Center => "center",
            TextAlign::Justify => "justify",
        });
    }
}

/// Display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
    ListItem,
    TableCell,
    TableRow,
}

/// CSS list-style-type values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ListStyleType {
    /// No marker
    #[default]
    None,
    /// • (default for ul)
    Disc,
    /// ○
    Circle,
    /// ▪
    Square,
    /// 1, 2, 3 (default for ol)
    Decimal,
    /// a, b, c
    LowerAlpha,
    /// A, B, C
    UpperAlpha,
    /// i, ii, iii
    LowerRoman,
    /// I, II, III
    UpperRoman,
}

impl ToCss for ListStyleType {
    fn to_css(&self, buf: &mut String) {
        buf.push_str(match self {
            ListStyleType::None => "none",
            ListStyleType::Disc => "disc",
            ListStyleType::Circle => "circle",
            ListStyleType::Square => "square",
            ListStyleType::Decimal => "decimal",
            ListStyleType::LowerAlpha => "lower-alpha",
            ListStyleType::UpperAlpha => "upper-alpha",
            ListStyleType::LowerRoman => "lower-roman",
            ListStyleType::UpperRoman => "upper-roman",
        });
    }
}

impl ToCss for Display {
    fn to_css(&self, buf: &mut String) {
        buf.push_str(match self {
            Display::Block => "block",
            Display::Inline => "inline",
            Display::None => "none",
            Display::ListItem => "list-item",
            Display::TableCell => "table-cell",
            Display::TableRow => "table-row",
        });
    }
}

/// RGBA color (8 bits per channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };
    pub const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };

    /// Create a new opaque color.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Create a new color with alpha.
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl ToCss for Color {
    fn to_css(&self, buf: &mut String) {
        if self.a == 255 {
            // Opaque: use #RRGGBB
            write!(buf, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b).unwrap();
        } else if self.a == 0 {
            buf.push_str("transparent");
        } else {
            // With alpha: use rgba()
            let alpha = self.a as f32 / 255.0;
            write!(buf, "rgba({},{},{},{:.2})", self.r, self.g, self.b, alpha).unwrap();
        }
    }
}

/// Length value with unit.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    #[default]
    Auto,
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
}

impl Eq for Length {}

impl Hash for Length {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Length::Auto => 0u8.hash(state),
            Length::Px(v) => {
                1u8.hash(state);
                v.to_bits().hash(state);
            }
            Length::Em(v) => {
                2u8.hash(state);
                v.to_bits().hash(state);
            }
            Length::Rem(v) => {
                3u8.hash(state);
                v.to_bits().hash(state);
            }
            Length::Percent(v) => {
                4u8.hash(state);
                v.to_bits().hash(state);
            }
        }
    }
}

impl ToCss for Length {
    fn to_css(&self, buf: &mut String) {
        match self {
            Length::Auto => buf.push_str("auto"),
            Length::Px(v) => {
                if *v == 0.0 {
                    buf.push('0');
                } else {
                    write!(buf, "{}px", v).unwrap();
                }
            }
            Length::Em(v) => write!(buf, "{}em", v).unwrap(),
            Length::Rem(v) => write!(buf, "{}rem", v).unwrap(),
            Length::Percent(v) => write!(buf, "{}%", v).unwrap(),
        }
    }
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

    // Vertical alignment for inline elements
    pub vertical_align_super: bool,
    pub vertical_align_sub: bool,

    // List properties
    pub list_style_type: ListStyleType,

    // Font variant
    pub font_variant_small_caps: bool,
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
        self.vertical_align_super
    }

    /// Check if the style is subscript.
    #[inline]
    pub fn is_subscript(&self) -> bool {
        self.vertical_align_sub
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
        self.font_variant_small_caps
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
        if self.vertical_align_super {
            buf.push_str("vertical-align: super; ");
        } else if self.vertical_align_sub {
            buf.push_str("vertical-align: sub; ");
        }

        // List style
        if self.list_style_type != default.list_style_type {
            buf.push_str("list-style-type: ");
            self.list_style_type.to_css(buf);
            buf.push_str("; ");
        }

        // Font variant
        if self.font_variant_small_caps {
            buf.push_str("font-variant: small-caps; ");
        }
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
