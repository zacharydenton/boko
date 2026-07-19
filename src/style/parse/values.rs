//! CSS value parsing functions.
//!
//! This module contains parsers for CSS values like colors, lengths, and integers.

use cssparser::{ParseError, Parser, Token};

use crate::style::properties::{Color, Length};

/// Text decoration value (can combine underline and line-through).
#[derive(Debug, Clone, Copy, Default)]
pub struct TextDecorationValue {
    /// Whether the `underline` line is present.
    pub underline: bool,
    /// Whether the `line-through` (strikethrough) line is present.
    pub line_through: bool,
}

pub(crate) fn parse_color(input: &mut Parser<'_, '_>) -> Option<Color> {
    // Try named colors first
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        if let Some(color) = named_color(&token.to_ascii_lowercase()) {
            return Some(color);
        }
        // Not a recognized color keyword (e.g. inherit/initial/unset): fall
        // through so the caller can decide, rather than treating it as a color.
        return None;
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

/// Look up a CSS named color. Names are matched lowercase.
fn named_color(name: &str) -> Option<Color> {
    let (r, g, b) = match name {
        "transparent" => return Some(Color::TRANSPARENT),
        "black" => (0, 0, 0),
        "silver" => (192, 192, 192),
        "gray" | "grey" => (128, 128, 128),
        "white" => (255, 255, 255),
        "maroon" => (128, 0, 0),
        "red" => (255, 0, 0),
        "purple" => (128, 0, 128),
        "fuchsia" | "magenta" => (255, 0, 255),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "olive" => (128, 128, 0),
        "yellow" => (255, 255, 0),
        "navy" => (0, 0, 128),
        "blue" => (0, 0, 255),
        "teal" => (0, 128, 128),
        "aqua" | "cyan" => (0, 255, 255),
        "orange" => (255, 165, 0),
        "pink" => (255, 192, 203),
        "brown" => (165, 42, 42),
        "gold" => (255, 215, 0),
        "indigo" => (75, 0, 130),
        "violet" => (238, 130, 238),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkred" => (139, 0, 0),
        "darkgreen" => (0, 100, 0),
        "darkblue" => (0, 0, 139),
        "lightblue" => (173, 216, 230),
        "darkorange" => (255, 140, 0),
        "beige" => (245, 245, 220),
        "ivory" => (255, 255, 240),
        "khaki" => (240, 230, 140),
        "salmon" => (250, 128, 114),
        "crimson" => (220, 20, 60),
        "coral" => (255, 127, 80),
        "tan" => (210, 180, 140),
        "turquoise" => (64, 224, 208),
        "lavender" => (230, 230, 250),
        "whitesmoke" => (245, 245, 245),
        _ => return None,
    };
    Some(Color::rgb(r, g, b))
}

/// Parse an `rgb()` / `rgba()` color, accepting both comma-separated
/// (`rgb(1, 2, 3)`) and modern space-separated (`rgb(1 2 3 / 0.5)`) syntaxes,
/// with an optional alpha component.
fn parse_rgb_function<'i, 't>(input: &mut Parser<'i, 't>) -> Result<Color, ParseError<'i, ()>> {
    let name = input.expect_function()?.clone();
    if !name.eq_ignore_ascii_case("rgb") && !name.eq_ignore_ascii_case("rgba") {
        return Err(input.new_custom_error(()));
    }
    input.parse_nested_block(|input| {
        let r = parse_color_component(input)?;
        let comma = input.try_parse(|i| i.expect_comma()).is_ok();
        let g = parse_color_component(input)?;
        if comma {
            input.expect_comma()?;
        }
        let b = parse_color_component(input)?;

        // Optional alpha, after a comma (legacy) or a slash (modern).
        let a = if comma {
            if input.try_parse(|i| i.expect_comma()).is_ok() {
                parse_alpha_component(input)?
            } else {
                255
            }
        } else if input.try_parse(|i| i.expect_delim('/')).is_ok() {
            parse_alpha_component(input)?
        } else {
            255
        };
        Ok(Color::rgba(r, g, b, a))
    })
}

/// Parse an alpha value: a number in `0.0..=1.0` or a percentage, mapped to
/// `0..=255`.
fn parse_alpha_component<'i, 't>(input: &mut Parser<'i, 't>) -> Result<u8, ParseError<'i, ()>> {
    let location = input.current_source_location();
    match input.next()? {
        Token::Number { value, .. } => Ok((value * 255.0).round().clamp(0.0, 255.0) as u8),
        Token::Percentage { unit_value, .. } => {
            Ok((unit_value * 255.0).round().clamp(0.0, 255.0) as u8)
        }
        _ => Err(location.new_custom_error(())),
    }
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

/// Parse letter-/word-spacing: a length, or the `normal` reset keyword
/// (mapped to `Length::Auto`, the unset value — both mean no extra spacing).
pub(crate) fn parse_spacing(input: &mut Parser<'_, '_>) -> Option<Length> {
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Some(Length::Auto);
    }
    parse_length(input)
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

#[cfg(test)]
mod tests {
    use super::*;
    use cssparser::{Parser, ParserInput};

    fn color(css: &str) -> Option<Color> {
        let mut input = ParserInput::new(css);
        parse_color(&mut Parser::new(&mut input))
    }

    #[test]
    fn parses_rgba_and_extended_named_colors() {
        assert_eq!(color("orange"), Some(Color::rgb(255, 165, 0)));
        assert_eq!(color("PURPLE"), Some(Color::rgb(128, 0, 128)));
        assert_eq!(
            color("rgba(10, 20, 30, 0.5)"),
            Some(Color::rgba(10, 20, 30, 128))
        );
        // Modern space + slash-alpha syntax.
        assert_eq!(color("rgb(1 2 3 / 50%)"), Some(Color::rgba(1, 2, 3, 128)));
        // Non-color keywords are not treated as colors.
        assert_eq!(color("inherit"), None);
    }
}
