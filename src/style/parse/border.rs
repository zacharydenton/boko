//! Border parsing with BorderSide abstraction.
//!
//! This module consolidates the four nearly-identical border side parsers
//! into a single generic implementation.

use cssparser::Parser;

use crate::style::Declaration;
use crate::style::properties::{BorderStyle, Color, Length};

use super::box_model::expand_shorthand_4;
use super::keywords::parse_border_style_value;
use super::values::{parse_color, parse_length};

/// Represents one of the four border sides.
#[derive(Clone, Copy)]
pub(crate) enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl BorderSide {
    /// Create declarations for this side's width, style, and color.
    pub(crate) fn make_declarations(
        self,
        width: Option<Length>,
        style: Option<BorderStyle>,
        color: Option<Color>,
    ) -> Vec<Declaration> {
        let mut decls = Vec::with_capacity(3);

        if let Some(w) = width {
            decls.push(match self {
                BorderSide::Top => Declaration::BorderTopWidth(w),
                BorderSide::Right => Declaration::BorderRightWidth(w),
                BorderSide::Bottom => Declaration::BorderBottomWidth(w),
                BorderSide::Left => Declaration::BorderLeftWidth(w),
            });
        }

        if let Some(s) = style {
            decls.push(match self {
                BorderSide::Top => Declaration::BorderTopStyle(s),
                BorderSide::Right => Declaration::BorderRightStyle(s),
                BorderSide::Bottom => Declaration::BorderBottomStyle(s),
                BorderSide::Left => Declaration::BorderLeftStyle(s),
            });
        }

        if let Some(c) = color {
            decls.push(match self {
                BorderSide::Top => Declaration::BorderTopColor(c),
                BorderSide::Right => Declaration::BorderRightColor(c),
                BorderSide::Bottom => Declaration::BorderBottomColor(c),
                BorderSide::Left => Declaration::BorderLeftColor(c),
            });
        }

        decls
    }
}

/// Parse a single border-width value (length or keyword).
pub(crate) fn parse_border_width_value(input: &mut Parser<'_, '_>) -> Option<Length> {
    // Try keyword first
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        let width = match token.as_ref() {
            "thin" => Length::Px(1.0),
            "medium" => Length::Px(3.0),
            "thick" => Length::Px(5.0),
            _ => return None,
        };
        return Some(width);
    }

    // Try length
    parse_length(input)
}

/// Parse border values (width, style, color) in any order, returning them as a tuple.
pub(crate) fn parse_border_values(
    input: &mut Parser<'_, '_>,
) -> (Option<Length>, Option<BorderStyle>, Option<Color>) {
    let mut width: Option<Length> = None;
    let mut style: Option<BorderStyle> = None;
    let mut color: Option<Color> = None;

    for _ in 0..3 {
        if style.is_none()
            && let Ok(s) = input.try_parse(|i| {
                parse_border_style_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            })
        {
            style = Some(s);
            continue;
        }

        if color.is_none()
            && let Ok(c) =
                input.try_parse(|i| parse_color(i).ok_or_else(|| i.new_custom_error::<_, ()>(())))
        {
            color = Some(c);
            continue;
        }

        if width.is_none()
            && let Ok(w) = input.try_parse(|i| {
                parse_border_width_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            })
        {
            width = Some(w);
            continue;
        }

        break;
    }

    (width, style, color)
}

/// Parse border-{side} shorthand (e.g., `border-top: 1px solid red`).
pub(crate) fn parse_border_side_shorthand(
    input: &mut Parser<'_, '_>,
    side: BorderSide,
) -> Vec<Declaration> {
    let (width, style, color) = parse_border_values(input);
    if width.is_none() && style.is_none() && color.is_none() {
        return vec![];
    }
    side.make_declarations(width, style, color)
}

/// Parse combined border shorthand (e.g., `border: 1px solid red`).
/// Order-insensitive parsing of width, style, and color.
pub(crate) fn parse_border_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let (width, style, color) = parse_border_values(input);

    // Must have at least one value
    if width.is_none() && style.is_none() && color.is_none() {
        return vec![];
    }

    let mut decls = Vec::with_capacity(12);

    // Expand to all four sides
    if let Some(w) = width {
        decls.push(Declaration::BorderTopWidth(w));
        decls.push(Declaration::BorderRightWidth(w));
        decls.push(Declaration::BorderBottomWidth(w));
        decls.push(Declaration::BorderLeftWidth(w));
    }
    if let Some(s) = style {
        decls.push(Declaration::BorderTopStyle(s));
        decls.push(Declaration::BorderRightStyle(s));
        decls.push(Declaration::BorderBottomStyle(s));
        decls.push(Declaration::BorderLeftStyle(s));
    }
    if let Some(c) = color {
        decls.push(Declaration::BorderTopColor(c));
        decls.push(Declaration::BorderRightColor(c));
        decls.push(Declaration::BorderBottomColor(c));
        decls.push(Declaration::BorderLeftColor(c));
    }

    decls
}

/// Parse border-style shorthand with 1-4 values.
/// Returns (top, right, bottom, left) following CSS box model rules.
pub(crate) fn parse_border_style_shorthand_values(
    input: &mut Parser<'_, '_>,
) -> Option<(BorderStyle, BorderStyle, BorderStyle, BorderStyle)> {
    let mut values = Vec::with_capacity(4);

    // Parse up to 4 border-style values
    while values.len() < 4 {
        if let Some(style) = parse_border_style_value(input) {
            values.push(style);
        } else {
            break;
        }
    }

    expand_shorthand_4(values)
}

/// Parse border-color shorthand with 1-4 values.
/// Returns (top, right, bottom, left) following CSS box model rules.
pub(crate) fn parse_color_shorthand_values(
    input: &mut Parser<'_, '_>,
) -> Option<(Color, Color, Color, Color)> {
    let mut values = Vec::with_capacity(4);

    // Parse up to 4 color values
    while values.len() < 4 {
        if let Some(color) = parse_color(input) {
            values.push(color);
        } else {
            break;
        }
    }

    expand_shorthand_4(values)
}
