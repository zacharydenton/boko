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
    parse_font_family, parse_font_size, parse_font_weight, parse_line_height,
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
    parse_background_shorthand, parse_color, parse_integer, parse_length, parse_text_decoration,
};
use super::properties::*;

/// A parsed CSS declaration (property: value).
///
/// This "fat enum" combines property identity and value in one type.
/// Each variant corresponds to a CSS property and contains its parsed value.
#[derive(Debug, Clone)]
pub enum Declaration {
    // Colors
    Color(Color),
    BackgroundColor(Color),

    // Font properties
    FontFamily(String),
    FontSize(Length),
    FontWeight(FontWeight),
    FontStyle(FontStyle),
    FontVariant(FontVariant),

    // Text properties
    TextAlign(TextAlign),
    TextIndent(Length),
    LineHeight(Length),
    LetterSpacing(Length),
    WordSpacing(Length),
    TextTransform(TextTransform),
    Hyphens(Hyphens),
    WhiteSpace(WhiteSpace),
    VerticalAlign(VerticalAlign),

    // Text decoration
    TextDecoration(super::parse::TextDecorationValue),
    TextDecorationStyle(DecorationStyle),
    TextDecorationColor(Color),

    // Box model - margins
    Margin(Length),
    MarginTop(Length),
    MarginRight(Length),
    MarginBottom(Length),
    MarginLeft(Length),

    // Box model - padding
    Padding(Length),
    PaddingTop(Length),
    PaddingRight(Length),
    PaddingBottom(Length),
    PaddingLeft(Length),

    // Dimensions
    Width(Length),
    Height(Length),
    MaxWidth(Length),
    MaxHeight(Length),
    MinWidth(Length),
    MinHeight(Length),

    // Display & positioning
    Display(Display),
    Float(Float),
    Clear(Clear),
    Visibility(Visibility),
    BoxSizing(BoxSizing),

    // Pagination control
    Orphans(u32),
    Widows(u32),

    // Text wrapping
    WordBreak(WordBreak),
    OverflowWrap(OverflowWrap),

    // Page breaks
    BreakBefore(BreakValue),
    BreakAfter(BreakValue),
    BreakInside(BreakValue),

    // Border style
    BorderStyle(BorderStyle),
    BorderTopStyle(BorderStyle),
    BorderRightStyle(BorderStyle),
    BorderBottomStyle(BorderStyle),
    BorderLeftStyle(BorderStyle),

    // Border width
    BorderWidth(Length),
    BorderTopWidth(Length),
    BorderRightWidth(Length),
    BorderBottomWidth(Length),
    BorderLeftWidth(Length),

    // Border color
    BorderColor(Color),
    BorderTopColor(Color),
    BorderRightColor(Color),
    BorderBottomColor(Color),
    BorderLeftColor(Color),

    // Border radius
    BorderRadius(Length),
    BorderTopLeftRadius(Length),
    BorderTopRightRadius(Length),
    BorderBottomLeftRadius(Length),
    BorderBottomRightRadius(Length),

    // List properties
    ListStyleType(ListStyleType),
    ListStylePosition(ListStylePosition),

    // Table properties
    BorderCollapse(BorderCollapse),
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
            "letter-spacing" => parse_length(input).map(Self::LetterSpacing),
            "word-spacing" => parse_length(input).map(Self::WordSpacing),
            "text-transform" => parse_text_transform(input).map(Self::TextTransform),
            "hyphens" => parse_hyphens(input).map(Self::Hyphens),
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
