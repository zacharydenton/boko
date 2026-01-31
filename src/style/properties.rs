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

enum_property! {
    /// Font style (normal, italic, oblique).
    pub enum FontStyle {
        #[default]
        Normal => "normal",
        Italic => "italic",
        Oblique => "oblique",
    }
}

enum_property! {
    /// Font variant (normal, small-caps).
    pub enum FontVariant {
        #[default]
        Normal => "normal",
        SmallCaps => "small-caps",
    }
}

enum_property! {
    /// Text transform values.
    pub enum TextTransform {
        #[default]
        None => "none",
        Uppercase => "uppercase",
        Lowercase => "lowercase",
        Capitalize => "capitalize",
    }
}

enum_property! {
    /// Hyphenation mode.
    /// Default is `Manual` so that explicit `hyphens: auto` is emitted in KFX output.
    pub enum Hyphens {
        Auto => "auto",
        #[default]
        Manual => "manual",
        None => "none",
    }
}

enum_property! {
    /// Text decoration line style.
    pub enum DecorationStyle {
        #[default]
        None => "none",
        Solid => "solid",
        Dotted => "dotted",
        Dashed => "dashed",
        Double => "double",
    }
}

enum_property! {
    /// Float positioning.
    pub enum Float {
        #[default]
        None => "none",
        Left => "left",
        Right => "right",
    }
}

enum_property! {
    /// Page break behavior.
    pub enum BreakValue {
        #[default]
        Auto => "auto",
        Always => "always",
        Avoid => "avoid",
        Column => "column",
    }
}

enum_property! {
    /// Border style values.
    pub enum BorderStyle {
        #[default]
        None => "none",
        Solid => "solid",
        Dotted => "dotted",
        Dashed => "dashed",
        Double => "double",
        Groove => "groove",
        Ridge => "ridge",
        Inset => "inset",
        Outset => "outset",
    }
}

enum_property! {
    /// List style position.
    pub enum ListStylePosition {
        #[default]
        Outside => "outside",
        Inside => "inside",
    }
}

enum_property! {
    /// CSS visibility values.
    pub enum Visibility {
        #[default]
        Visible => "visible",
        Hidden => "hidden",
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
    /// CSS clear values for float clearing.
    pub enum Clear {
        #[default]
        None => "none",
        Left => "left",
        Right => "right",
        Both => "both",
    }
}

enum_property! {
    /// CSS word-break values.
    pub enum WordBreak {
        #[default]
        Normal => "normal",
        BreakAll => "break-all",
        KeepAll => "keep-all",
        BreakWord => "break-word",
    }
}

enum_property! {
    /// CSS overflow-wrap values.
    pub enum OverflowWrap {
        #[default]
        Normal => "normal",
        BreakWord => "break-word",
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
    /// CSS vertical-align values for inline and table-cell elements.
    pub enum VerticalAlign {
        #[default]
        Baseline => "baseline",
        Top => "top",
        Middle => "middle",
        Bottom => "bottom",
        TextTop => "text-top",
        TextBottom => "text-bottom",
        Super => "super",
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
    /// Text alignment.
    pub enum TextAlign {
        #[default]
        Start => "start",
        End => "end",
        Left => "left",
        Right => "right",
        Center => "center",
        Justify => "justify",
    }
}

enum_property! {
    /// Display mode.
    pub enum Display {
        #[default]
        Block => "block",
        Inline => "inline",
        InlineBlock => "inline-block",
        None => "none",
        ListItem => "list-item",
        TableCell => "table-cell",
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
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
