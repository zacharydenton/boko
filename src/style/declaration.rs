//! CSS declaration parsing.
//!
//! This module contains the Declaration enum and its parse dispatch logic.
//! The actual parsing functions are in the parse/ submodules.

use cssparser::Parser;

use super::parse::border::{
    BorderSide, parse_border_shorthand, parse_border_side_shorthand,
    parse_border_style_shorthand_values, parse_color_shorthand_values,
};
use super::parse::box_model::parse_box_shorthand_values;
use super::parse::font::{
    parse_font_family, parse_font_shorthand, parse_font_size, parse_font_weight, parse_line_height,
};
use super::parse::keywords::{
    parse_border_collapse, parse_border_style_value, parse_box_sizing, parse_break_inside,
    parse_break_value, parse_clear, parse_decoration_style, parse_display, parse_float,
    parse_font_style, parse_font_variant, parse_hyphens, parse_list_style_position,
    parse_list_style_shorthand, parse_list_style_type, parse_overflow_wrap, parse_text_align,
    parse_text_transform, parse_vertical_align, parse_visibility, parse_white_space,
    parse_word_break,
};
use super::parse::values::{
    parse_background_shorthand, parse_color, parse_integer, parse_length, parse_spacing,
    parse_text_decoration,
};
use super::properties::*;

/// A parsed CSS declaration (property: value).
///
/// This "fat enum" combines property identity and value in one type.
/// Each variant corresponds to a CSS property and contains its parsed value.
#[derive(Debug, Clone)]
pub enum Declaration {
    // Colors
    /// `color`: foreground text color.
    Color(Color),
    /// `background-color`: element background color (also produced by the
    /// `background` shorthand, from which only the color component is kept).
    BackgroundColor(Color),

    // Font properties
    /// `font-family`: the first font family from the list, unquoted.
    FontFamily(String),
    /// `font-size`: font size as a [`Length`] (keywords like `small` are
    /// resolved to em values during parsing).
    FontSize(Length),
    /// `font-weight`: numeric weight 100-900 (`normal` = 400, `bold` = 700).
    FontWeight(FontWeight),
    /// `font-style`: normal, italic, or oblique.
    FontStyle(FontStyle),
    /// `font-variant` / `font-variant-caps`: normal or small-caps.
    FontVariant(FontVariant),

    // Text properties
    /// `text-align`: horizontal alignment of inline content.
    TextAlign(TextAlign),
    /// `text-indent`: first-line indentation.
    TextIndent(Length),
    /// `line-height`: line box height; unitless numbers are stored as em.
    LineHeight(Length),
    /// `letter-spacing`: extra spacing between characters.
    LetterSpacing(Length),
    /// `word-spacing`: extra spacing between words.
    WordSpacing(Length),
    /// `text-transform`: case transformation (uppercase, lowercase, capitalize).
    TextTransform(TextTransform),
    /// `hyphens`: automatic hyphenation mode.
    Hyphens(Hyphens),
    /// `white-space`: whitespace collapsing and line-wrapping behavior.
    WhiteSpace(WhiteSpace),
    /// `vertical-align`: inline/table-cell vertical alignment (includes
    /// `super`/`sub` used for superscript/subscript detection).
    VerticalAlign(VerticalAlign),

    // Text decoration
    /// `text-decoration` / `text-decoration-line`: which decoration lines
    /// (underline, overline, line-through) are enabled or explicitly `none`.
    TextDecoration(super::parse::TextDecorationValue),
    /// `text-decoration-style`: line rendering style (solid, dotted, ...).
    TextDecorationStyle(DecorationStyle),
    /// `text-decoration-color`: color of the decoration line.
    TextDecorationColor(Color),

    // Box model - margins
    /// `margin` applied uniformly to all four sides (the `margin` shorthand
    /// itself is parsed into the four per-side variants; this variant exists
    /// for programmatic use and applies to all sides during cascade).
    Margin(Length),
    /// `margin-top`.
    MarginTop(Length),
    /// `margin-right`.
    MarginRight(Length),
    /// `margin-bottom`.
    MarginBottom(Length),
    /// `margin-left`.
    MarginLeft(Length),

    // Box model - padding
    /// `padding` applied uniformly to all four sides (the `padding` shorthand
    /// itself expands to the four per-side variants).
    Padding(Length),
    /// `padding-top`.
    PaddingTop(Length),
    /// `padding-right`.
    PaddingRight(Length),
    /// `padding-bottom`.
    PaddingBottom(Length),
    /// `padding-left`.
    PaddingLeft(Length),

    // Dimensions
    /// `width`: content box width.
    Width(Length),
    /// `height`: content box height.
    Height(Length),
    /// `max-width`: maximum content box width.
    MaxWidth(Length),
    /// `max-height`: maximum content box height.
    MaxHeight(Length),
    /// `min-width`: minimum content box width.
    MinWidth(Length),
    /// `min-height`: minimum content box height.
    MinHeight(Length),

    // Display & positioning
    /// `display`: box display mode (block, inline, none, list-item, ...).
    Display(Display),
    /// `float`: left/right float positioning.
    Float(Float),
    /// `clear`: which floated sides subsequent content must clear.
    Clear(Clear),
    /// `visibility`: visible, hidden, or collapse.
    Visibility(Visibility),
    /// `box-sizing`: whether width/height include padding and border.
    BoxSizing(BoxSizing),

    // Pagination control
    /// `orphans`: minimum lines left at the bottom of a page before a break.
    Orphans(u32),
    /// `widows`: minimum lines carried to the top of a page after a break.
    Widows(u32),

    // Text wrapping
    /// `word-break`: where lines may break within words.
    WordBreak(WordBreak),
    /// `overflow-wrap`: emergency breaking of otherwise-unbreakable words.
    OverflowWrap(OverflowWrap),

    // Page breaks
    /// `break-before` / `page-break-before`: page break before the element.
    BreakBefore(BreakValue),
    /// `break-after` / `page-break-after`: page break after the element.
    BreakAfter(BreakValue),
    /// `break-inside` / `page-break-inside`: page breaks within the element.
    BreakInside(BreakValue),

    // Border style
    /// `border-style` applied uniformly to all four sides (the shorthand with
    /// multiple values expands to the per-side variants instead).
    BorderStyle(BorderStyle),
    /// `border-top-style`.
    BorderTopStyle(BorderStyle),
    /// `border-right-style`.
    BorderRightStyle(BorderStyle),
    /// `border-bottom-style`.
    BorderBottomStyle(BorderStyle),
    /// `border-left-style`.
    BorderLeftStyle(BorderStyle),

    // Border width
    /// `border-width` applied uniformly to all four sides (the shorthand with
    /// multiple values expands to the per-side variants instead).
    BorderWidth(Length),
    /// `border-top-width`.
    BorderTopWidth(Length),
    /// `border-right-width`.
    BorderRightWidth(Length),
    /// `border-bottom-width`.
    BorderBottomWidth(Length),
    /// `border-left-width`.
    BorderLeftWidth(Length),

    // Border color
    /// `border-color` applied uniformly to all four sides (the shorthand with
    /// multiple values expands to the per-side variants instead).
    BorderColor(Color),
    /// `border-top-color`.
    BorderTopColor(Color),
    /// `border-right-color`.
    BorderRightColor(Color),
    /// `border-bottom-color`.
    BorderBottomColor(Color),
    /// `border-left-color`.
    BorderLeftColor(Color),

    // Border radius
    /// `border-radius` applied uniformly to all four corners.
    BorderRadius(Length),
    /// `border-top-left-radius`.
    BorderTopLeftRadius(Length),
    /// `border-top-right-radius`.
    BorderTopRightRadius(Length),
    /// `border-bottom-left-radius`.
    BorderBottomLeftRadius(Length),
    /// `border-bottom-right-radius`.
    BorderBottomRightRadius(Length),

    // List properties
    /// `list-style-type`: list marker kind (disc, decimal, roman, ...).
    ListStyleType(ListStyleType),
    /// `list-style-position`: marker inside or outside the item's box.
    ListStylePosition(ListStylePosition),

    // Table properties
    /// `border-collapse`: separate vs. collapsed table borders.
    BorderCollapse(BorderCollapse),
    /// `border-spacing`: gap between table cell borders (single value; used
    /// for both axes).
    BorderSpacing(Length),
}

impl Declaration {
    /// Parse a CSS declaration from a property name and value parser.
    ///
    /// Returns a Vec of declarations. For most properties this is a single declaration,
    /// but shorthands like `margin`, `border`, etc. expand to multiple declarations.
    /// Returns an empty Vec if the property is unknown or the value fails to parse.
    pub fn parse(name: &str, input: &mut Parser<'_, '_>) -> Vec<Self> {
        // Try shorthand properties first (they expand to multiple declarations)
        if let Some(decls) = Self::parse_shorthand(name, input) {
            return decls;
        }

        // Single-value properties
        Self::parse_single(name, input).into_iter().collect()
    }

    /// Parse shorthand properties that expand to multiple declarations.
    fn parse_shorthand(name: &str, input: &mut Parser<'_, '_>) -> Option<Vec<Self>> {
        Some(match name {
            "margin" => Self::parse_length_rect(input, |t, r, b, l| {
                [
                    Self::MarginTop(t),
                    Self::MarginRight(r),
                    Self::MarginBottom(b),
                    Self::MarginLeft(l),
                ]
            }),
            "padding" => Self::parse_length_rect(input, |t, r, b, l| {
                [
                    Self::PaddingTop(t),
                    Self::PaddingRight(r),
                    Self::PaddingBottom(b),
                    Self::PaddingLeft(l),
                ]
            }),
            "border-width" => Self::parse_length_rect(input, |t, r, b, l| {
                [
                    Self::BorderTopWidth(t),
                    Self::BorderRightWidth(r),
                    Self::BorderBottomWidth(b),
                    Self::BorderLeftWidth(l),
                ]
            }),
            "border-style" => parse_border_style_shorthand_values(input)
                .map(|(t, r, b, l)| {
                    vec![
                        Self::BorderTopStyle(t),
                        Self::BorderRightStyle(r),
                        Self::BorderBottomStyle(b),
                        Self::BorderLeftStyle(l),
                    ]
                })
                .unwrap_or_default(),
            "border-color" => parse_color_shorthand_values(input)
                .map(|(t, r, b, l)| {
                    vec![
                        Self::BorderTopColor(t),
                        Self::BorderRightColor(r),
                        Self::BorderBottomColor(b),
                        Self::BorderLeftColor(l),
                    ]
                })
                .unwrap_or_default(),
            "border" => parse_border_shorthand(input),
            "border-top" => parse_border_side_shorthand(input, BorderSide::Top),
            "border-right" => parse_border_side_shorthand(input, BorderSide::Right),
            "border-bottom" => parse_border_side_shorthand(input, BorderSide::Bottom),
            "border-left" => parse_border_side_shorthand(input, BorderSide::Left),
            "list-style" => parse_list_style_shorthand(input),
            "font" => parse_font_shorthand(input).unwrap_or_default(),
            _ => return None,
        })
    }

    /// Parse a 1-4 value Length rect shorthand (margin, padding, border-width).
    fn parse_length_rect<F>(input: &mut Parser<'_, '_>, make_decls: F) -> Vec<Self>
    where
        F: FnOnce(Length, Length, Length, Length) -> [Self; 4],
    {
        parse_box_shorthand_values(input)
            .map(|(t, r, b, l)| make_decls(t, r, b, l).into())
            .unwrap_or_default()
    }

    /// Parse single-value properties.
    fn parse_single(name: &str, input: &mut Parser<'_, '_>) -> Option<Self> {
        match name {
            // Colors
            "color" => parse_color(input).map(Self::Color),
            "background-color" => parse_color(input).map(Self::BackgroundColor),
            "background" => parse_background_shorthand(input).map(Self::BackgroundColor),

            // Font properties
            "font-family" => parse_font_family(input).map(Self::FontFamily),
            "font-size" => parse_font_size(input).map(Self::FontSize),
            "font-weight" => parse_font_weight(input).map(Self::FontWeight),
            "font-style" => parse_font_style(input).map(Self::FontStyle),
            "font-variant" | "font-variant-caps" => {
                parse_font_variant(input).map(Self::FontVariant)
            }

            // Text properties
            "text-align" => parse_text_align(input).map(Self::TextAlign),
            "text-indent" => parse_length(input).map(Self::TextIndent),
            "line-height" => parse_line_height(input).map(Self::LineHeight),
            // `normal` is the spacing reset keyword (parse_length only knows
            // `auto`); both mean "no extra spacing" (`Length::Auto`).
            "letter-spacing" => parse_spacing(input).map(Self::LetterSpacing),
            "word-spacing" => parse_spacing(input).map(Self::WordSpacing),
            "text-transform" => parse_text_transform(input).map(Self::TextTransform),
            // Vendor-prefixed hyphenation aliases are common in EPUB CSS.
            "hyphens" | "-epub-hyphens" | "-webkit-hyphens" | "-moz-hyphens"
            | "adobe-hyphenate" => parse_hyphens(input).map(Self::Hyphens),
            "white-space" => parse_white_space(input).map(Self::WhiteSpace),
            "vertical-align" => parse_vertical_align(input).map(Self::VerticalAlign),

            // Text decoration
            "text-decoration" | "text-decoration-line" => {
                parse_text_decoration(input).map(Self::TextDecoration)
            }
            "text-decoration-style" => parse_decoration_style(input).map(Self::TextDecorationStyle),
            "text-decoration-color" => parse_color(input).map(Self::TextDecorationColor),

            // Box model - margins (individual)
            "margin-top" => parse_length(input).map(Self::MarginTop),
            "margin-right" => parse_length(input).map(Self::MarginRight),
            "margin-bottom" => parse_length(input).map(Self::MarginBottom),
            "margin-left" => parse_length(input).map(Self::MarginLeft),

            // Box model - padding (individual)
            "padding-top" => parse_length(input).map(Self::PaddingTop),
            "padding-right" => parse_length(input).map(Self::PaddingRight),
            "padding-bottom" => parse_length(input).map(Self::PaddingBottom),
            "padding-left" => parse_length(input).map(Self::PaddingLeft),

            // Dimensions
            "width" => parse_length(input).map(Self::Width),
            "height" => parse_length(input).map(Self::Height),
            "max-width" => parse_length(input).map(Self::MaxWidth),
            "max-height" => parse_length(input).map(Self::MaxHeight),
            "min-width" => parse_length(input).map(Self::MinWidth),
            "min-height" => parse_length(input).map(Self::MinHeight),

            // Display & positioning
            "display" => parse_display(input).map(Self::Display),
            "float" => parse_float(input).map(Self::Float),
            "clear" => parse_clear(input).map(Self::Clear),
            "visibility" => parse_visibility(input).map(Self::Visibility),
            "box-sizing" => parse_box_sizing(input).map(Self::BoxSizing),

            // Pagination control
            "orphans" => parse_integer(input).map(Self::Orphans),
            "widows" => parse_integer(input).map(Self::Widows),

            // Text wrapping
            "word-break" => parse_word_break(input).map(Self::WordBreak),
            "overflow-wrap" => parse_overflow_wrap(input).map(Self::OverflowWrap),

            // Page breaks
            "break-before" | "page-break-before" => parse_break_value(input).map(Self::BreakBefore),
            "break-after" | "page-break-after" => parse_break_value(input).map(Self::BreakAfter),
            "break-inside" | "page-break-inside" => {
                parse_break_inside(input).map(Self::BreakInside)
            }

            // Border style (individual sides)
            "border-top-style" => parse_border_style_value(input).map(Self::BorderTopStyle),
            "border-right-style" => parse_border_style_value(input).map(Self::BorderRightStyle),
            "border-bottom-style" => parse_border_style_value(input).map(Self::BorderBottomStyle),
            "border-left-style" => parse_border_style_value(input).map(Self::BorderLeftStyle),

            // Border width (individual sides)
            "border-top-width" => parse_length(input).map(Self::BorderTopWidth),
            "border-right-width" => parse_length(input).map(Self::BorderRightWidth),
            "border-bottom-width" => parse_length(input).map(Self::BorderBottomWidth),
            "border-left-width" => parse_length(input).map(Self::BorderLeftWidth),

            // Border color (individual sides)
            "border-top-color" => parse_color(input).map(Self::BorderTopColor),
            "border-right-color" => parse_color(input).map(Self::BorderRightColor),
            "border-bottom-color" => parse_color(input).map(Self::BorderBottomColor),
            "border-left-color" => parse_color(input).map(Self::BorderLeftColor),

            // Border radius
            "border-radius" => parse_length(input).map(Self::BorderRadius),
            "border-top-left-radius" => parse_length(input).map(Self::BorderTopLeftRadius),
            "border-top-right-radius" => parse_length(input).map(Self::BorderTopRightRadius),
            "border-bottom-left-radius" => parse_length(input).map(Self::BorderBottomLeftRadius),
            "border-bottom-right-radius" => parse_length(input).map(Self::BorderBottomRightRadius),

            // List properties
            "list-style-type" => parse_list_style_type(input).map(Self::ListStyleType),
            "list-style-position" => parse_list_style_position(input).map(Self::ListStylePosition),

            // Table properties
            "border-collapse" => parse_border_collapse(input).map(Self::BorderCollapse),
            "border-spacing" => parse_length(input).map(Self::BorderSpacing),

            // Unknown properties
            _ => {
                while input.next().is_ok() {}
                None
            }
        }
    }
}
