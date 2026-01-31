//! CSS keyword parsing.
//!
//! This module provides the keyword_parser! macro and keyword parsers
//! that leverage the existing enum_property! macro's from_css method.

use cssparser::Parser;

use crate::style::properties::{
    BorderCollapse, BorderStyle, BoxSizing, BreakValue, Clear, DecorationStyle, Display, Float,
    FontStyle, FontVariant, Hyphens, ListStylePosition, ListStyleType, OverflowWrap, TextAlign,
    TextTransform, VerticalAlign, Visibility, WhiteSpace, WordBreak,
};

use crate::style::Declaration;

/// Macro for generating keyword parser functions.
///
/// For types defined with enum_property!, this generates a simple parser
/// that reads an identifier and calls the type's from_css method.
macro_rules! keyword_parser {
    ($fn_name:ident, $type:ty) => {
        pub(crate) fn $fn_name(input: &mut Parser<'_, '_>) -> Option<$type> {
            let token = input.expect_ident_cloned().ok()?;
            <$type>::from_css(token.as_ref())
        }
    };
}

// Generate keyword parsers for all enum_property! types
keyword_parser!(parse_font_style, FontStyle);
keyword_parser!(parse_font_variant, FontVariant);
keyword_parser!(parse_text_align, TextAlign);
keyword_parser!(parse_text_transform, TextTransform);
keyword_parser!(parse_hyphens, Hyphens);
keyword_parser!(parse_white_space, WhiteSpace);
keyword_parser!(parse_decoration_style, DecorationStyle);
keyword_parser!(parse_display, Display);
keyword_parser!(parse_float, Float);
keyword_parser!(parse_clear, Clear);
keyword_parser!(parse_visibility, Visibility);
keyword_parser!(parse_box_sizing, BoxSizing);
keyword_parser!(parse_word_break, WordBreak);
keyword_parser!(parse_overflow_wrap, OverflowWrap);
keyword_parser!(parse_list_style_type, ListStyleType);
keyword_parser!(parse_list_style_position, ListStylePosition);
keyword_parser!(parse_border_collapse, BorderCollapse);
keyword_parser!(parse_vertical_align, VerticalAlign);

/// Parse break-before/break-after values with CSS aliases.
pub(crate) fn parse_break_value(input: &mut Parser<'_, '_>) -> Option<BreakValue> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "auto" => Some(BreakValue::Auto),
        "always" | "page" | "left" | "right" | "recto" | "verso" => Some(BreakValue::Always),
        "avoid" | "avoid-page" => Some(BreakValue::Avoid),
        "column" | "avoid-column" => Some(BreakValue::Column),
        _ => None,
    }
}

/// Parse break-inside values with CSS aliases.
pub(crate) fn parse_break_inside(input: &mut Parser<'_, '_>) -> Option<BreakValue> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "auto" => Some(BreakValue::Auto),
        "avoid" | "avoid-page" | "avoid-column" => Some(BreakValue::Avoid),
        _ => None,
    }
}

/// Parse a single border-style value.
pub(crate) fn parse_border_style_value(input: &mut Parser<'_, '_>) -> Option<BorderStyle> {
    let token = input.try_parse(|i| i.expect_ident_cloned()).ok()?;
    match token.as_ref() {
        "none" | "hidden" => Some(BorderStyle::None),
        "solid" => Some(BorderStyle::Solid),
        "dotted" => Some(BorderStyle::Dotted),
        "dashed" => Some(BorderStyle::Dashed),
        "double" => Some(BorderStyle::Double),
        "groove" => Some(BorderStyle::Groove),
        "ridge" => Some(BorderStyle::Ridge),
        "inset" => Some(BorderStyle::Inset),
        "outset" => Some(BorderStyle::Outset),
        _ => None,
    }
}

/// Parse the list-style shorthand: list-style-type, list-style-position, list-style-image
/// We only care about type and position (image is not supported).
pub(crate) fn parse_list_style_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let mut list_style_type = None;
    let mut list_style_position = None;

    // Parse up to 3 values in any order
    for _ in 0..3 {
        if input.is_exhausted() {
            break;
        }

        let state = input.state();
        if let Ok(ident) = input.expect_ident_cloned() {
            let ident_lower = ident.to_ascii_lowercase();
            // Check for list-style-type values
            let maybe_type = match ident_lower.as_ref() {
                "none" => Some(ListStyleType::None),
                "disc" => Some(ListStyleType::Disc),
                "circle" => Some(ListStyleType::Circle),
                "square" => Some(ListStyleType::Square),
                "decimal" => Some(ListStyleType::Decimal),
                "lower-alpha" | "lower-latin" => Some(ListStyleType::LowerAlpha),
                "upper-alpha" | "upper-latin" => Some(ListStyleType::UpperAlpha),
                "lower-roman" => Some(ListStyleType::LowerRoman),
                "upper-roman" => Some(ListStyleType::UpperRoman),
                _ => None,
            };
            if maybe_type.is_some() && list_style_type.is_none() {
                list_style_type = maybe_type;
                continue;
            }

            // Check for list-style-position values
            let maybe_position = match ident_lower.as_ref() {
                "inside" => Some(ListStylePosition::Inside),
                "outside" => Some(ListStylePosition::Outside),
                _ => None,
            };
            if maybe_position.is_some() && list_style_position.is_none() {
                list_style_position = maybe_position;
                continue;
            }

            // Unknown identifier, restore and break
            input.reset(&state);
            break;
        } else {
            // Not an identifier (could be url() for list-style-image), skip it
            input.reset(&state);
            // Try to skip one value
            if input.expect_url().is_ok() || input.expect_function().is_ok() {
                continue;
            }
            break;
        }
    }

    let mut decls = Vec::new();
    if let Some(t) = list_style_type {
        decls.push(Declaration::ListStyleType(t));
    }
    if let Some(p) = list_style_position {
        decls.push(Declaration::ListStylePosition(p));
    }
    decls
}
