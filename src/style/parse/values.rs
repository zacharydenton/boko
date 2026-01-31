//! CSS value parsing functions.
//!
//! This module contains parsers for CSS values like colors, lengths, and integers.

use cssparser::{ParseError, Parser, Token};

use crate::style::properties::{Color, Length};

/// Text decoration value (can combine underline and line-through).
#[derive(Debug, Clone, Copy, Default)]
pub struct TextDecorationValue {
    pub underline: bool,
    pub line_through: bool,
}

pub(crate) fn parse_color(input: &mut Parser<'_, '_>) -> Option<Color> {
    // Try named colors first
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        let color = match token.as_ref() {
            "black" => Color::BLACK,
            "white" => Color::WHITE,
            "red" => Color::rgb(255, 0, 0),
            "green" => Color::rgb(0, 128, 0),
            "blue" => Color::rgb(0, 0, 255),
            "yellow" => Color::rgb(255, 255, 0),
            "cyan" => Color::rgb(0, 255, 255),
            "magenta" => Color::rgb(255, 0, 255),
            "gray" | "grey" => Color::rgb(128, 128, 128),
            "transparent" => Color::TRANSPARENT,
            // Skip inherit/initial/unset for now
            _ => return None,
        };
        return Some(color);
    }

    // Try ID token (which is how cssparser parses hex colors like #ff0000)
    // Or Hash token (for colors starting with digits like #222299)
    // We must check the token type INSIDE try_parse, otherwise try_parse won't reset
    // the position when we get the wrong token variant.
    if let Ok(hash) = input.try_parse(|i| -> Result<_, ParseError<'_, ()>> {
        match i.next()? {
            Token::IDHash(h) | Token::Hash(h) => Ok(h.clone()),
            _ => Err(i.new_custom_error(())),
        }
    }) && let Some(color) = parse_hex_color(hash.as_ref())
    {
        return Some(color);
    }

    // Try rgb() or rgba()
    if let Ok(color) = input.try_parse(parse_rgb_function) {
        return Some(color);
    }

    None
}

/// Parse the CSS `background` shorthand and extract just the color component.
///
/// The background shorthand can contain: color, image, position, repeat, size, attachment,
/// origin, clip - in any order. We parse tokens in a loop and extract any color we find.
/// See https://www.w3.org/TR/css-backgrounds-3/#background
pub(crate) fn parse_background_shorthand(input: &mut Parser<'_, '_>) -> Option<Color> {
    let mut color: Option<Color> = None;

    // Try to parse each component in any order, like lightningcss does
    loop {
        // Try to parse a color if we haven't found one yet
        if color.is_none()
            && let Ok(c) =
                input.try_parse(|i| parse_color(i).ok_or(i.new_custom_error::<_, ()>(())))
        {
            color = Some(c);
            continue;
        }

        // Skip over url() functions (background-image)
        if input.try_parse(|i| i.expect_url()).is_ok() {
            continue;
        }

        // Skip over functions like linear-gradient() etc.
        if input
            .try_parse(|i: &mut Parser<'_, '_>| {
                let _ = i.expect_function()?;
                i.parse_nested_block(
                    |nested: &mut Parser<'_, '_>| -> Result<(), ParseError<'_, ()>> {
                        while nested.next().is_ok() {}
                        Ok(())
                    },
                )
            })
            .is_ok()
        {
            continue;
        }

        // Skip over known keywords: repeat-x, repeat-y, no-repeat, cover, contain,
        // fixed, scroll, local, padding-box, border-box, content-box, etc.
        if input
            .try_parse(|i| {
                let ident = i.expect_ident()?;
                match ident.as_ref() {
                // repeat keywords
                "repeat" | "repeat-x" | "repeat-y" | "no-repeat" | "space" | "round" |
                // size keywords
                "cover" | "contain" | "auto" |
                // attachment keywords
                "scroll" | "fixed" | "local" |
                // box keywords (origin/clip)
                "padding-box" | "border-box" | "content-box" |
                // position keywords
                "top" | "bottom" | "left" | "right" | "center" |
                // none keyword
                "none" => Ok(()),
                _ => Err(i.new_custom_error::<_, ()>(())),
            }
            })
            .is_ok()
        {
            continue;
        }

        // Skip over lengths and percentages (for position/size)
        if input
            .try_parse(|i| match i.next()? {
                Token::Dimension { .. } | Token::Percentage { .. } | Token::Number { .. } => Ok(()),
                _ => Err(i.new_custom_error::<_, ()>(())),
            })
            .is_ok()
        {
            continue;
        }

        // Skip the "/" delimiter used between position and size
        if input.try_parse(|i| i.expect_delim('/')).is_ok() {
            continue;
        }

        // Nothing matched, exit the loop
        break;
    }

    color
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(Color::rgb(r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::rgb(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color::rgba(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgb_function<'i, 't>(input: &mut Parser<'i, 't>) -> Result<Color, ParseError<'i, ()>> {
    input.expect_function_matching("rgb")?;
    input.parse_nested_block(|input| {
        let r = parse_color_component(input)?;
        input.expect_comma()?;
        let g = parse_color_component(input)?;
        input.expect_comma()?;
        let b = parse_color_component(input)?;
        Ok(Color::rgb(r, g, b))
    })
}

fn parse_color_component<'i, 't>(input: &mut Parser<'i, 't>) -> Result<u8, ParseError<'i, ()>> {
    let location = input.current_source_location();
    match input.next()? {
        Token::Number {
            int_value: Some(v), ..
        } => Ok((*v).clamp(0, 255) as u8),
        Token::Percentage { unit_value, .. } => {
            Ok((unit_value * 255.0).round().clamp(0.0, 255.0) as u8)
        }
        _ => Err(location.new_custom_error(())),
    }
}

pub(crate) fn parse_length(input: &mut Parser<'_, '_>) -> Option<Length> {
    match input.next().ok()? {
        Token::Dimension { value, unit, .. } => {
            let length = match unit.as_ref() {
                "px" => Length::Px(*value),
                "em" => Length::Em(*value),
                "rem" => Length::Rem(*value),
                "%" => Length::Percent(*value),
                // ex = x-height, approximately 0.5em
                "ex" => Length::Em(*value * 0.5),
                // pt = points, 1pt = 96/72 px
                "pt" => Length::Px(*value * 96.0 / 72.0),
                _ => return None,
            };
            Some(length)
        }
        Token::Percentage { unit_value, .. } => Some(Length::Percent(*unit_value * 100.0)),
        Token::Number { value, .. } if *value == 0.0 => Some(Length::Px(0.0)),
        Token::Ident(ident) => match ident.as_ref() {
            "auto" => Some(Length::Auto),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn parse_integer(input: &mut Parser<'_, '_>) -> Option<u32> {
    if let Ok(Token::Number {
        int_value: Some(v), ..
    }) = input.next().cloned()
        && v >= 0
    {
        return Some(v as u32);
    }
    None
}

pub(crate) fn parse_text_decoration(input: &mut Parser<'_, '_>) -> Option<TextDecorationValue> {
    let mut result = TextDecorationValue::default();
    let mut found = false;
    while let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        match token.as_ref() {
            "underline" => result.underline = true,
            "line-through" => result.line_through = true,
            "none" => {}
            _ => continue,
        }
        found = true;
    }
    if found { Some(result) } else { None }
}

