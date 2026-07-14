//! CSS property types and the enum_property! macro.
//!
//! This module contains all the CSS property value types that are used
//! in the style system.

use std::fmt::Write;
use std::hash::{Hash, Hasher};

use super::ToCss;

/// Macro for defining CSS keyword enums with automatic ToCss implementation.
///
/// Inspired by lightningcss's `enum_property!` macro, this reduces boilerplate
/// for enums that map directly to CSS keywords.
///
/// # Example
///
/// ```ignore
/// enum_property! {
///     /// Font style (normal, italic, oblique).
///     pub enum FontStyle {
///         #[default]
///         Normal => "normal",
///         Italic => "italic",
///         Oblique => "oblique",
///     }
/// }
/// ```
macro_rules! enum_property {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $css:literal
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            /// Returns the CSS keyword for this value.
            #[inline]
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $css,)*
                }
            }

            /// Parse a CSS keyword into this enum.
            #[inline]
            pub fn from_css(s: &str) -> Option<Self> {
                match s {
                    $($css => Some($name::$variant),)*
                    _ => None,
                }
            }
        }

        impl ToCss for $name {
            fn to_css(&self, buf: &mut String) {
                buf.push_str(self.as_str());
            }
        }
    };
}

// Export the macro for use within the crate
pub(crate) use enum_property;

/// CSS `font-weight` as a numeric weight (100-900).
///
/// `normal` parses to 400 and `bold` to 700; the derived default of 0 means
/// "unset". Serializes back to the `normal`/`bold` keywords where possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontWeight(
    /// Numeric weight (100-900); 0 means unset.
    pub u16,
);

impl FontWeight {
    /// `font-weight: normal` (400).
    pub const NORMAL: FontWeight = FontWeight(400);
    /// `font-weight: bold` (700).
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

enum_property! {
    /// CSS `font-style` values (normal, italic, oblique).
    pub enum FontStyle {
        /// Upright glyphs (CSS initial value).
        #[default]
        Normal => "normal",
        /// Cursive italic face.
        Italic => "italic",
        /// Slanted (oblique) face; treated as italic by exporters.
        Oblique => "oblique",
    }
}

enum_property! {
    /// CSS `font-variant` / `font-variant-caps` values.
    pub enum FontVariant {
        /// Regular glyphs (CSS initial value).
        #[default]
        Normal => "normal",
        /// Lowercase letters rendered as small capitals.
        SmallCaps => "small-caps",
    }
}

enum_property! {
    /// CSS `text-transform` values.
    pub enum TextTransform {
        /// No case transformation (CSS initial value).
        #[default]
        None => "none",
        /// Render all text in uppercase.
        Uppercase => "uppercase",
        /// Render all text in lowercase.
        Lowercase => "lowercase",
        /// Capitalize the first letter of each word.
        Capitalize => "capitalize",
    }
}

enum_property! {
    /// CSS `hyphens` values (automatic hyphenation mode).
    /// Default is `Manual` so that explicit `hyphens: auto` is emitted in KFX output.
    pub enum Hyphens {
        /// Break words at language-appropriate hyphenation points.
        Auto => "auto",
        /// Break only at explicit hyphenation characters (CSS initial value).
        #[default]
        Manual => "manual",
        /// Never hyphenate, even at explicit hyphenation characters.
        None => "none",
    }
}

enum_property! {
    /// CSS `text-decoration-style` values (how the decoration line is drawn).
    ///
    /// `None` is a boko extension meaning "unset" (CSS has no `none` keyword
    /// here; the CSS initial value is `solid`).
    pub enum DecorationStyle {
        /// Unset — no explicit decoration style (renders as solid).
        #[default]
        None => "none",
        /// A single solid line.
        Solid => "solid",
        /// A dotted line.
        Dotted => "dotted",
        /// A dashed line.
        Dashed => "dashed",
        /// A double line.
        Double => "double",
    }
}

enum_property! {
    /// CSS `float` values.
    pub enum Float {
        /// Not floated (CSS initial value).
        #[default]
        None => "none",
        /// Float to the left; content flows along the right side.
        Left => "left",
        /// Float to the right; content flows along the left side.
        Right => "right",
    }
}

enum_property! {
    /// CSS `break-before`/`break-after`/`break-inside` (and legacy
    /// `page-break-*`) values controlling pagination.
    pub enum BreakValue {
        /// No forced or avoided break (CSS initial value).
        #[default]
        Auto => "auto",
        /// Force a page break.
        Always => "always",
        /// Avoid a break if possible.
        Avoid => "avoid",
        /// Force a column break.
        Column => "column",
    }
}

enum_property! {
    /// CSS `border-style` values (per-side line style).
    pub enum BorderStyle {
        /// No border (CSS initial value).
        #[default]
        None => "none",
        /// A single solid line.
        Solid => "solid",
        /// A dotted line.
        Dotted => "dotted",
        /// A dashed line.
        Dashed => "dashed",
        /// Two parallel solid lines.
        Double => "double",
        /// Carved (3D grooved) appearance.
        Groove => "groove",
        /// Extruded (3D ridged) appearance.
        Ridge => "ridge",
        /// Embedded (3D inset) appearance.
        Inset => "inset",
        /// Embossed (3D outset) appearance.
        Outset => "outset",
    }
}

enum_property! {
    /// CSS `list-style-position` values (marker placement).
    pub enum ListStylePosition {
        /// Marker outside the list item's principal box (CSS initial value).
        #[default]
        Outside => "outside",
        /// Marker inside the list item's box, as the first inline content.
        Inside => "inside",
    }
}

enum_property! {
    /// CSS `visibility` values.
    pub enum Visibility {
        /// Element is rendered normally (CSS initial value).
        #[default]
        Visible => "visible",
        /// Element is invisible but still occupies layout space.
        Hidden => "hidden",
        /// Like `hidden`, but table rows/columns release their space.
        Collapse => "collapse",
    }
}

enum_property! {
    /// CSS box-sizing values.
    pub enum BoxSizing {
        /// Width/height include only content (CSS default)
        #[default]
        ContentBox => "content-box",
        /// Width/height include padding and border
        BorderBox => "border-box",
    }
}

enum_property! {
    /// CSS `clear` values for float clearing.
    pub enum Clear {
        /// Do not clear floats (CSS initial value).
        #[default]
        None => "none",
        /// Move below any left-floated boxes.
        Left => "left",
        /// Move below any right-floated boxes.
        Right => "right",
        /// Move below floated boxes on both sides.
        Both => "both",
    }
}

enum_property! {
    /// CSS `word-break` values (where lines may break within words).
    pub enum WordBreak {
        /// Default line-breaking rules (CSS initial value).
        #[default]
        Normal => "normal",
        /// Allow breaks between any two characters.
        BreakAll => "break-all",
        /// Disallow breaks within CJK words.
        KeepAll => "keep-all",
        /// Deprecated alias behaving like `overflow-wrap: break-word`.
        BreakWord => "break-word",
    }
}

enum_property! {
    /// CSS `overflow-wrap` values (emergency breaking of long words).
    pub enum OverflowWrap {
        /// Break only at normal word break points (CSS initial value).
        #[default]
        Normal => "normal",
        /// Break otherwise-unbreakable words if a line would overflow.
        BreakWord => "break-word",
        /// Like `break-word`, but soft-wrap opportunities affect
        /// min-content sizing.
        Anywhere => "anywhere",
    }
}

enum_property! {
    /// CSS white-space values.
    pub enum WhiteSpace {
        /// Normal whitespace handling: collapse whitespace, wrap lines.
        #[default]
        Normal => "normal",
        /// Collapse whitespace but don't wrap lines.
        Nowrap => "nowrap",
        /// Preserve whitespace and newlines, don't wrap lines.
        Pre => "pre",
        /// Preserve whitespace and newlines, wrap lines.
        PreWrap => "pre-wrap",
        /// Collapse whitespace except newlines, wrap lines.
        PreLine => "pre-line",
    }
}

enum_property! {
    /// CSS `vertical-align` values for inline and table-cell elements.
    ///
    /// `Super` and `Sub` are how boko represents superscript/subscript text
    /// (see `ComputedStyle::is_superscript`/`is_subscript`).
    pub enum VerticalAlign {
        /// Align with the parent's baseline (CSS initial value).
        #[default]
        Baseline => "baseline",
        /// Align with the top of the line box (or table cell).
        Top => "top",
        /// Align with the middle of the line box (or table cell).
        Middle => "middle",
        /// Align with the bottom of the line box (or table cell).
        Bottom => "bottom",
        /// Align with the top of the parent's font.
        TextTop => "text-top",
        /// Align with the bottom of the parent's font.
        TextBottom => "text-bottom",
        /// Superscript baseline shift.
        Super => "super",
        /// Subscript baseline shift.
        Sub => "sub",
    }
}

enum_property! {
    /// CSS border-collapse values for tables.
    pub enum BorderCollapse {
        /// Borders are separated (CSS default for tables).
        #[default]
        Separate => "separate",
        /// Adjacent borders are collapsed into a single border.
        Collapse => "collapse",
    }
}

enum_property! {
    /// CSS `text-align` values.
    pub enum TextAlign {
        /// Align toward the start of the writing direction (CSS initial value).
        #[default]
        Start => "start",
        /// Align toward the end of the writing direction.
        End => "end",
        /// Left-align inline content.
        Left => "left",
        /// Right-align inline content.
        Right => "right",
        /// Center inline content.
        Center => "center",
        /// Justify lines to both margins.
        Justify => "justify",
    }
}

enum_property! {
    /// CSS `display` values (the subset boko models).
    ///
    /// Note: the default is `Block`, not CSS's `inline` — boko assigns
    /// display per element role, so the struct default is only a fallback.
    pub enum Display {
        /// Block-level box.
        #[default]
        Block => "block",
        /// Inline box.
        Inline => "inline",
        /// Inline-level block container.
        InlineBlock => "inline-block",
        /// Element generates no boxes (removed from layout).
        None => "none",
        /// Block box with a list marker (`li`).
        ListItem => "list-item",
        /// Table cell box (`td`/`th`).
        TableCell => "table-cell",
        /// Table row box (`tr`).
        TableRow => "table-row",
    }
}

enum_property! {
    /// CSS list-style-type values.
    pub enum ListStyleType {
        /// No marker
        None => "none",
        /// Disc bullet (CSS default)
        #[default]
        Disc => "disc",
        /// Circle bullet
        Circle => "circle",
        /// Square bullet
        Square => "square",
        /// Decimal numbers (default for ol)
        Decimal => "decimal",
        /// Lowercase letters
        LowerAlpha => "lower-alpha",
        /// Uppercase letters
        UpperAlpha => "upper-alpha",
        /// Lowercase roman numerals
        LowerRoman => "lower-roman",
        /// Uppercase roman numerals
        UpperRoman => "upper-roman",
    }
}

/// RGBA color (8 bits per channel).
///
/// Parsed from any CSS color syntax (hex, `rgb()`/`rgba()`, named colors).
/// Serializes as `#rrggbb` when opaque, `transparent` when fully
/// transparent, and `rgba()` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    /// Red channel (0-255).
    pub r: u8,
    /// Green channel (0-255).
    pub g: u8,
    /// Blue channel (0-255).
    pub b: u8,
    /// Alpha channel (0 = fully transparent, 255 = opaque).
    pub a: u8,
}

impl Color {
    /// Opaque black (`#000000`).
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Opaque white (`#ffffff`).
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    /// Fully transparent (all channels zero).
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

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

/// CSS length value with unit.
///
/// Supports absolute pixels, font-relative `em`/`rem`, and percentages.
/// At parse time `pt` is converted to `Px` (1pt = 96/72 px) and `ex` to
/// `Em` (~0.5em). `Auto` doubles as both CSS `auto` and "property unset" —
/// it is the `Default`, so a default-initialized field means the property
/// was never specified.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// The `auto` keyword; also the default, meaning "unset".
    #[default]
    Auto,
    /// Absolute length in CSS pixels (other absolute units are converted).
    Px(f32),
    /// Length relative to the element's font size.
    Em(f32),
    /// Length relative to the root font size.
    Rem(f32),
    /// Percentage of the containing block's corresponding dimension.
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
