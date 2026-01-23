//! CSS property parsing functions.
//!
//! Contains all individual CSS property parsers (parse_color, parse_font_*, etc.)
//! and the declaration block parser.

use cssparser::{Parser, Token};

use super::BreakValue;
use super::style::ParsedStyle;
use super::types::{
    Border, BorderCollapse, BorderStyle, BoxSizing, Clear, Color, CssFloat, CssValue, Direction,
    Display, FontStyle, FontVariant, FontWeight, Hyphens, LineBreak, ListStylePosition,
    ListStyleType, Overflow, Position, TextAlign, TextCombineUpright, TextOrientation,
    TextTransform, UnicodeBidi, VerticalAlign, Visibility, WordBreak, WritingMode,
};

/// Parse a declaration block (property: value; ...)
pub(crate) fn parse_declaration_block<'i, 't>(input: &mut Parser<'i, 't>) -> ParsedStyle {
    let mut style = ParsedStyle::default();

    loop {
        input.skip_whitespace();

        if input.is_exhausted() {
            break;
        }

        // Try to parse a declaration
        let result: Result<(), cssparser::ParseError<'i, ()>> = input.try_parse(|i| {
            let property = match i.next()? {
                Token::Ident(name) => name.to_string().to_lowercase(),
                _ => return Err(i.new_custom_error(())),
            };

            i.skip_whitespace();

            match i.next()? {
                Token::Colon => {}
                _ => return Err(i.new_custom_error(())),
            }

            i.skip_whitespace();

            // Collect value tokens until semicolon
            let mut values: Vec<Token> = Vec::new();
            loop {
                match i.next() {
                    Ok(Token::Semicolon) => break,
                    Ok(t) => values.push(t.clone()),
                    Err(_) => break,
                }
            }

            apply_property(&mut style, &property, &values);
            Ok(())
        });

        if result.is_err() {
            // Skip to next semicolon to recover
            loop {
                match input.next() {
                    Ok(Token::Semicolon) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }
    }

    style
}

/// Apply a CSS property value to the style
fn apply_property(style: &mut ParsedStyle, property: &str, values: &[Token]) {
    match property {
        "font-family" => {
            style.font_family = parse_font_family(values);
        }
        "font-size" => {
            style.font_size = parse_length_value(values);
        }
        "font-weight" => {
            style.font_weight = parse_font_weight(values);
        }
        "font-style" => {
            style.font_style = parse_font_style(values);
        }
        "font-variant" => {
            style.font_variant = parse_font_variant(values);
        }
        "text-align" => {
            style.text_align = parse_text_align(values);
        }
        "text-transform" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.text_transform = match val.to_ascii_lowercase().as_str() {
                    "uppercase" => Some(TextTransform::Uppercase),
                    "lowercase" => Some(TextTransform::Lowercase),
                    "capitalize" => Some(TextTransform::Capitalize),
                    "none" => Some(TextTransform::None),
                    _ => None,
                };
            }
        }
        "text-indent" => {
            style.text_indent = parse_length_value(values);
        }
        "line-height" => {
            style.line_height = parse_length_value(values);
        }
        "margin" => {
            // Shorthand: 1-4 values
            let parsed: Vec<CssValue> = values.iter().filter_map(parse_single_length).collect();
            match parsed.len() {
                1 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_right = Some(parsed[0].clone());
                    style.margin_bottom = Some(parsed[0].clone());
                    style.margin_left = Some(parsed[0].clone());
                }
                2 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_bottom = Some(parsed[0].clone());
                    style.margin_left = Some(parsed[1].clone());
                    style.margin_right = Some(parsed[1].clone());
                }
                3 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_left = Some(parsed[1].clone());
                    style.margin_right = Some(parsed[1].clone());
                    style.margin_bottom = Some(parsed[2].clone());
                }
                4 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_right = Some(parsed[1].clone());
                    style.margin_bottom = Some(parsed[2].clone());
                    style.margin_left = Some(parsed[3].clone());
                }
                _ => {}
            }
        }
        "margin-top" => {
            style.margin_top = parse_length_value(values);
        }
        "margin-bottom" => {
            style.margin_bottom = parse_length_value(values);
        }
        "margin-left" => {
            style.margin_left = parse_length_value(values);
        }
        "margin-right" => {
            style.margin_right = parse_length_value(values);
        }
        "padding" => {
            // Shorthand: 1-4 values (same as margin)
            let parsed: Vec<CssValue> = values.iter().filter_map(parse_single_length).collect();
            match parsed.len() {
                1 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_right = Some(parsed[0].clone());
                    style.padding_bottom = Some(parsed[0].clone());
                    style.padding_left = Some(parsed[0].clone());
                }
                2 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_bottom = Some(parsed[0].clone());
                    style.padding_left = Some(parsed[1].clone());
                    style.padding_right = Some(parsed[1].clone());
                }
                3 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_left = Some(parsed[1].clone());
                    style.padding_right = Some(parsed[1].clone());
                    style.padding_bottom = Some(parsed[2].clone());
                }
                4 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_right = Some(parsed[1].clone());
                    style.padding_bottom = Some(parsed[2].clone());
                    style.padding_left = Some(parsed[3].clone());
                }
                _ => {}
            }
        }
        "padding-top" => {
            style.padding_top = parse_length_value(values);
        }
        "padding-bottom" => {
            style.padding_bottom = parse_length_value(values);
        }
        "padding-left" => {
            style.padding_left = parse_length_value(values);
        }
        "padding-right" => {
            style.padding_right = parse_length_value(values);
        }
        "color" => {
            style.color = parse_color(values);
        }
        "background-color" => {
            style.background_color = parse_color(values);
        }
        "border" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_top = Some(border.clone());
                style.border_bottom = Some(border.clone());
                style.border_left = Some(border.clone());
                style.border_right = Some(border.clone());
            }
        }
        "border-top" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_top = Some(border);
            }
        }
        "border-bottom" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_bottom = Some(border);
            }
        }
        "border-left" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_left = Some(border);
            }
        }
        "border-right" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_right = Some(border);
            }
        }
        // Border shorthand properties for width/style/color
        "border-width" => {
            if let Some(width) = parse_length_value(values) {
                // Apply width to all borders, creating border if needed
                for border in [
                    &mut style.border_top,
                    &mut style.border_right,
                    &mut style.border_bottom,
                    &mut style.border_left,
                ] {
                    if let Some(b) = border {
                        b.width = Some(width.clone());
                    } else {
                        *border = Some(Border {
                            width: Some(width.clone()),
                            style: BorderStyle::Solid, // Default style when width is set
                            color: None,
                        });
                    }
                }
            }
        }
        "border-style" => {
            if let Some(Token::Ident(val)) = values.first() {
                let bs = match val.to_ascii_lowercase().as_str() {
                    "solid" => BorderStyle::Solid,
                    "dashed" => BorderStyle::Dashed,
                    "dotted" => BorderStyle::Dotted,
                    "double" => BorderStyle::Double,
                    "groove" => BorderStyle::Groove,
                    "ridge" => BorderStyle::Ridge,
                    "inset" => BorderStyle::Inset,
                    "outset" => BorderStyle::Outset,
                    "hidden" => BorderStyle::Hidden,
                    _ => BorderStyle::None,
                };
                if bs != BorderStyle::None && bs != BorderStyle::Hidden {
                    // Apply style to all borders, creating border if needed
                    for border in [
                        &mut style.border_top,
                        &mut style.border_right,
                        &mut style.border_bottom,
                        &mut style.border_left,
                    ] {
                        if let Some(b) = border {
                            b.style = bs;
                        } else {
                            *border = Some(Border {
                                width: None,
                                style: bs,
                                color: None,
                            });
                        }
                    }
                }
            }
        }
        "border-color" => {
            if let Some(color) = parse_color(values) {
                // Apply color to all borders, creating border if needed
                for border in [
                    &mut style.border_top,
                    &mut style.border_right,
                    &mut style.border_bottom,
                    &mut style.border_left,
                ] {
                    if let Some(b) = border {
                        b.color = Some(color.clone());
                    } else {
                        *border = Some(Border {
                            width: None,
                            style: BorderStyle::Solid, // Default style when color is set
                            color: Some(color.clone()),
                        });
                    }
                }
            }
        }
        "display" => {
            style.display = parse_display(values);
        }
        "position" => {
            style.position = parse_position(values);
        }
        "top" => {
            style.top = parse_length_value(values);
        }
        "right" => {
            style.right = parse_length_value(values);
        }
        "bottom" => {
            style.bottom = parse_length_value(values);
        }
        "left" => {
            style.left = parse_length_value(values);
        }
        "width" => {
            style.width = parse_length_value(values);
        }
        "height" => {
            style.height = parse_length_value(values);
        }
        "min-width" => {
            style.min_width = parse_length_value(values);
        }
        "min-height" => {
            style.min_height = parse_length_value(values);
        }
        "max-width" => {
            style.max_width = parse_length_value(values);
        }
        "max-height" => {
            style.max_height = parse_length_value(values);
        }
        "vertical-align" => {
            style.vertical_align = parse_vertical_align(values);
        }
        "clear" => {
            style.clear = parse_clear(values);
        }
        "float" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.float = match val.to_ascii_lowercase().as_str() {
                    "none" => Some(CssFloat::None),
                    "left" => Some(CssFloat::Left),
                    "right" => Some(CssFloat::Right),
                    _ => None,
                };
            }
        }
        "word-break" => {
            style.word_break = parse_word_break(values);
        }
        "overflow" | "overflow-x" | "overflow-y" => {
            style.overflow = parse_overflow(values);
        }
        "visibility" => {
            style.visibility = parse_visibility(values);
        }
        "break-before" | "page-break-before" => {
            style.break_before = parse_break_value(values);
        }
        "break-after" | "page-break-after" => {
            style.break_after = parse_break_value(values);
        }
        "break-inside" | "page-break-inside" => {
            style.break_inside = parse_break_value(values);
        }
        "border-radius" => {
            // Shorthand: 1-4 values (for simplicity, apply to all corners)
            if let Some(val) = parse_length_value(values) {
                style.border_radius_tl = Some(val.clone());
                style.border_radius_tr = Some(val.clone());
                style.border_radius_br = Some(val.clone());
                style.border_radius_bl = Some(val);
            }
        }
        "border-top-left-radius" => {
            style.border_radius_tl = parse_length_value(values);
        }
        "border-top-right-radius" => {
            style.border_radius_tr = parse_length_value(values);
        }
        "border-bottom-right-radius" => {
            style.border_radius_br = parse_length_value(values);
        }
        "border-bottom-left-radius" => {
            style.border_radius_bl = parse_length_value(values);
        }
        "letter-spacing" => {
            style.letter_spacing = parse_length_value(values);
        }
        "word-spacing" => {
            style.word_spacing = parse_length_value(values);
        }
        "white-space" => {
            if let Some(Token::Ident(val)) = values.first() {
                match val.to_ascii_lowercase().as_str() {
                    "nowrap" | "pre" => style.white_space_nowrap = Some(true),
                    "normal" | "pre-wrap" | "pre-line" => style.white_space_nowrap = Some(false),
                    _ => {}
                }
            }
        }
        "text-decoration" | "text-decoration-line" => {
            for token in values {
                if let Token::Ident(val) = token {
                    match val.to_ascii_lowercase().as_str() {
                        "underline" => style.text_decoration_underline = true,
                        "overline" => style.text_decoration_overline = true,
                        "line-through" => style.text_decoration_line_through = true,
                        "none" => {
                            style.text_decoration_underline = false;
                            style.text_decoration_overline = false;
                            style.text_decoration_line_through = false;
                        }
                        _ => {}
                    }
                }
            }
        }
        "text-decoration-color" => {
            // Parse the color and apply to all active decorations
            // When text-decoration-color is set, it applies to underline, overline, and line-through
            if let Some(color) = parse_color(values) {
                style.text_decoration_underline_color = Some(color.clone());
                style.text_decoration_overline_color = Some(color.clone());
                style.text_decoration_line_through_color = Some(color);
            }
        }
        "opacity" => {
            if let Some(Token::Number { value, .. }) = values.first() {
                // Clamp to 0-1 and convert to 0-100
                let clamped = value.clamp(0.0, 1.0);
                style.opacity = Some((clamped * 100.0) as u8);
            } else if let Some(Token::Percentage { unit_value, .. }) = values.first() {
                // unit_value is already 0-1 for percentage
                let clamped = unit_value.clamp(0.0, 1.0);
                style.opacity = Some((clamped * 100.0) as u8);
            }
        }
        // P1: List style properties
        "list-style-type" => {
            style.list_style_type = parse_list_style_type(values);
        }
        "list-style-position" => {
            style.list_style_position = parse_list_style_position(values);
        }
        "list-style" => {
            // Shorthand: can contain type, position, and image
            // Parse type and position, ignore image for now
            if style.list_style_type.is_none() {
                style.list_style_type = parse_list_style_type(values);
            }
            if style.list_style_position.is_none() {
                style.list_style_position = parse_list_style_position(values);
            }
        }
        // P2: Writing mode properties
        "writing-mode" => {
            style.writing_mode = parse_writing_mode(values);
        }
        "text-combine-upright" | "-webkit-text-combine" => {
            style.text_combine_upright = parse_text_combine_upright(values);
        }
        // P4: Shadow properties
        "box-shadow" => {
            style.box_shadow = parse_shadow_value(values);
        }
        "text-shadow" => {
            style.text_shadow = parse_shadow_value(values);
        }
        // Background image
        "background-image" => {
            style.background_image = parse_background_image(values);
        }
        "background" => {
            // Shorthand - extract url() if present
            if let Some(url) = parse_background_image(values) {
                style.background_image = Some(url);
            }
        }
        // CSS hyphens property (also handle vendor prefixes)
        "hyphens" | "-webkit-hyphens" | "-moz-hyphens" | "-epub-hyphens" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.hyphens = match val.to_ascii_lowercase().as_str() {
                    "none" => Some(Hyphens::None),
                    "manual" => Some(Hyphens::Manual),
                    "auto" => Some(Hyphens::Auto),
                    _ => None,
                };
            }
        }
        // CSS box-sizing property
        "box-sizing" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.box_sizing = match val.to_ascii_lowercase().as_str() {
                    "content-box" => Some(BoxSizing::ContentBox),
                    "border-box" => Some(BoxSizing::BorderBox),
                    "padding-box" => Some(BoxSizing::PaddingBox),
                    _ => None,
                };
            }
        }
        // CSS unicode-bidi property
        "unicode-bidi" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.unicode_bidi = match val.to_ascii_lowercase().as_str() {
                    "normal" => Some(UnicodeBidi::Normal),
                    "embed" => Some(UnicodeBidi::Embed),
                    "isolate" => Some(UnicodeBidi::Isolate),
                    "bidi-override" => Some(UnicodeBidi::BidiOverride),
                    "isolate-override" => Some(UnicodeBidi::IsolateOverride),
                    "plaintext" => Some(UnicodeBidi::Plaintext),
                    _ => None,
                };
            }
        }
        // CSS direction property (LTR/RTL)
        "direction" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.direction = match val.to_ascii_lowercase().as_str() {
                    "ltr" => Some(Direction::Ltr),
                    "rtl" => Some(Direction::Rtl),
                    _ => None,
                };
            }
        }
        // CSS line-break property (CJK line breaking rules)
        "line-break" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.line_break = match val.to_ascii_lowercase().as_str() {
                    "auto" => Some(LineBreak::Auto),
                    "normal" => Some(LineBreak::Normal),
                    "loose" => Some(LineBreak::Loose),
                    "strict" => Some(LineBreak::Strict),
                    "anywhere" => Some(LineBreak::Anywhere),
                    _ => None,
                };
            }
        }
        // CSS text-orientation property (vertical writing mode)
        "text-orientation" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.text_orientation = match val.to_ascii_lowercase().as_str() {
                    "mixed" => Some(TextOrientation::Mixed),
                    "upright" => Some(TextOrientation::Upright),
                    "sideways" | "sideways-right" => Some(TextOrientation::Sideways),
                    _ => None,
                };
            }
        }
        // Table CSS properties
        "border-collapse" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.border_collapse = match val.to_ascii_lowercase().as_str() {
                    "collapse" => Some(BorderCollapse::Collapse),
                    "separate" => Some(BorderCollapse::Separate),
                    _ => None,
                };
            }
        }
        "border-spacing" => {
            // border-spacing: <horizontal> [<vertical>]
            // If one value, applies to both; if two, first is horizontal, second is vertical
            let lengths: Vec<CssValue> = values.iter().filter_map(parse_single_length).collect();
            match lengths.len() {
                1 => {
                    style.border_spacing_horizontal = Some(lengths[0].clone());
                    style.border_spacing_vertical = Some(lengths[0].clone());
                }
                2 => {
                    style.border_spacing_horizontal = Some(lengths[0].clone());
                    style.border_spacing_vertical = Some(lengths[1].clone());
                }
                _ => {}
            }
        }
        _ => {
            // Ignore unsupported properties
        }
    }
}

fn parse_border(values: &[Token]) -> Border {
    let mut border = Border::default();

    // Naive parsing: check for width, style, color in any order
    for token in values {
        if let Some(width) = parse_single_length(token) {
            border.width = Some(width);
        } else if let Some(color) = parse_single_color(token) {
            border.color = Some(color);
        } else if let Some(style) = parse_border_style_token(token) {
            border.style = style;
        }
    }

    // Default to solid if width/color present but no style
    if border.style == BorderStyle::None && (border.width.is_some() || border.color.is_some()) {
        border.style = BorderStyle::Solid;
    }

    border
}

fn parse_border_style_token(token: &Token) -> Option<BorderStyle> {
    if let Token::Ident(name) = token {
        match name.to_ascii_lowercase().as_str() {
            "none" => Some(BorderStyle::None),
            "hidden" => Some(BorderStyle::Hidden),
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
    } else {
        None
    }
}

fn parse_single_color(token: &Token) -> Option<Color> {
    parse_color(std::slice::from_ref(token))
}

fn parse_color(values: &[Token]) -> Option<Color> {
    for token in values {
        match token {
            Token::Hash(value) | Token::IDHash(value) => {
                // Parse hex color
                let s = value.as_ref();
                match s.len() {
                    3 => {
                        // #RGB
                        let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
                        let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
                        let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
                        return Some(Color::Rgba(r, g, b, 255));
                    }
                    6 => {
                        // #RRGGBB
                        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                        return Some(Color::Rgba(r, g, b, 255));
                    }
                    _ => continue,
                }
            }
            Token::Ident(name) => {
                let name = name.to_ascii_lowercase();
                match name.as_str() {
                    "currentcolor" => return Some(Color::Current),
                    "transparent" => return Some(Color::Transparent),
                    "black" => return Some(Color::Rgba(0, 0, 0, 255)),
                    "white" => return Some(Color::Rgba(255, 255, 255, 255)),
                    "red" => return Some(Color::Rgba(255, 0, 0, 255)),
                    "green" => return Some(Color::Rgba(0, 128, 0, 255)),
                    "blue" => return Some(Color::Rgba(0, 0, 255, 255)),
                    // Add more named colors as needed or use a crate for full support
                    _ => continue,
                }
            }
            // Add rgb() / rgba() function parsing if needed
            _ => continue,
        }
    }
    None
}

fn parse_font_family(values: &[Token]) -> Option<String> {
    let mut fonts = Vec::new();

    for token in values {
        match token {
            Token::Ident(name) => fonts.push(name.to_string()),
            Token::QuotedString(name) => fonts.push(name.to_string()),
            Token::Comma => {} // Skip commas between fonts
            _ => continue,
        }
    }

    if fonts.is_empty() {
        None
    } else {
        Some(fonts.join(","))
    }
}

fn parse_font_weight(values: &[Token]) -> Option<FontWeight> {
    for token in values {
        match token {
            Token::Ident(name) => {
                let name = name.to_ascii_lowercase();
                match name.as_str() {
                    "normal" => return Some(FontWeight::Normal),
                    "bold" => return Some(FontWeight::Bold),
                    _ => continue,
                }
            }
            Token::Number { int_value, .. } => {
                if let Some(weight) = int_value {
                    return Some(FontWeight::Weight(*weight as u16));
                }
            }
            _ => continue,
        }
    }
    None
}

fn parse_font_style(values: &[Token]) -> Option<FontStyle> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "normal" => return Some(FontStyle::Normal),
                "italic" => return Some(FontStyle::Italic),
                "oblique" => return Some(FontStyle::Oblique),
                _ => continue,
            }
        }
    }
    None
}

fn parse_font_variant(values: &[Token]) -> Option<FontVariant> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "normal" => return Some(FontVariant::Normal),
                "small-caps" => return Some(FontVariant::SmallCaps),
                _ => continue,
            }
        }
    }
    None
}

fn parse_text_align(values: &[Token]) -> Option<TextAlign> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "left" | "start" => return Some(TextAlign::Left),
                "right" | "end" => return Some(TextAlign::Right),
                "center" => return Some(TextAlign::Center),
                "justify" => return Some(TextAlign::Justify),
                _ => continue,
            }
        }
    }
    None
}

fn parse_display(values: &[Token]) -> Option<Display> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "none" => return Some(Display::None),
                "block" => return Some(Display::Block),
                "inline" => return Some(Display::Inline),
                "inline-block" | "flex" | "grid" | "table" => return Some(Display::Other),
                _ => continue,
            }
        }
    }
    None
}

fn parse_position(values: &[Token]) -> Option<Position> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "static" => return Some(Position::Static),
                "relative" => return Some(Position::Relative),
                "absolute" => return Some(Position::Absolute),
                "fixed" => return Some(Position::Fixed),
                _ => continue,
            }
        }
    }
    None
}

fn parse_length_value(values: &[Token]) -> Option<CssValue> {
    for token in values {
        if let Some(value) = parse_single_length(token) {
            return Some(value);
        }
    }
    None
}

fn parse_single_length(token: &Token) -> Option<CssValue> {
    match token {
        Token::Dimension { value, unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            match unit.as_str() {
                "px" => Some(CssValue::Px(*value)),
                "em" => Some(CssValue::Em(*value)),
                "rem" => Some(CssValue::Rem(*value)),
                // P1: Additional units
                "vw" => Some(CssValue::Vw(*value)),
                "vh" => Some(CssValue::Vh(*value)),
                "vmin" => Some(CssValue::Vmin(*value)),
                "vmax" => Some(CssValue::Vmax(*value)),
                "ch" => Some(CssValue::Ch(*value)),
                "ex" => Some(CssValue::Ex(*value)),
                "cm" => Some(CssValue::Cm(*value)),
                "mm" => Some(CssValue::Mm(*value)),
                "in" => Some(CssValue::In(*value)),
                "pt" => Some(CssValue::Pt(*value)),
                _ => None,
            }
        }
        Token::Percentage { unit_value, .. } => Some(CssValue::Percent(*unit_value * 100.0)),
        Token::Number { value, .. } => {
            if *value == 0.0 {
                Some(CssValue::Px(0.0))
            } else {
                Some(CssValue::Number(*value))
            }
        }
        Token::Ident(name) => {
            let name = name.to_ascii_lowercase();
            Some(CssValue::Keyword(name))
        }
        _ => None,
    }
}

fn parse_vertical_align(values: &[Token]) -> Option<VerticalAlign> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "baseline" => return Some(VerticalAlign::Baseline),
                "top" => return Some(VerticalAlign::Top),
                "middle" => return Some(VerticalAlign::Middle),
                "bottom" => return Some(VerticalAlign::Bottom),
                "super" => return Some(VerticalAlign::Super),
                "sub" => return Some(VerticalAlign::Sub),
                "text-top" => return Some(VerticalAlign::TextTop),
                "text-bottom" => return Some(VerticalAlign::TextBottom),
                _ => continue,
            }
        }
    }
    None
}

fn parse_clear(values: &[Token]) -> Option<Clear> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "none" => return Some(Clear::None),
                "left" => return Some(Clear::Left),
                "right" => return Some(Clear::Right),
                "both" => return Some(Clear::Both),
                _ => continue,
            }
        }
    }
    None
}

fn parse_word_break(values: &[Token]) -> Option<WordBreak> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "normal" => return Some(WordBreak::Normal),
                "break-all" => return Some(WordBreak::BreakAll),
                "keep-all" => return Some(WordBreak::KeepAll),
                _ => continue,
            }
        }
    }
    None
}

fn parse_overflow(values: &[Token]) -> Option<Overflow> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "visible" => return Some(Overflow::Visible),
                "hidden" => return Some(Overflow::Hidden),
                "scroll" => return Some(Overflow::Scroll),
                "auto" => return Some(Overflow::Auto),
                "clip" => return Some(Overflow::Clip),
                _ => continue,
            }
        }
    }
    None
}

fn parse_visibility(values: &[Token]) -> Option<Visibility> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "visible" => return Some(Visibility::Visible),
                "hidden" => return Some(Visibility::Hidden),
                "collapse" => return Some(Visibility::Collapse),
                _ => continue,
            }
        }
    }
    None
}

fn parse_break_value(values: &[Token]) -> Option<BreakValue> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "auto" => return Some(BreakValue::Auto),
                "avoid" => return Some(BreakValue::Avoid),
                "avoid-page" => return Some(BreakValue::AvoidPage),
                "page" => return Some(BreakValue::Page),
                "left" => return Some(BreakValue::Left),
                "right" => return Some(BreakValue::Right),
                "column" => return Some(BreakValue::Column),
                "avoid-column" => return Some(BreakValue::AvoidColumn),
                // Legacy page-break-* value mapping
                "always" => return Some(BreakValue::Page),
                _ => continue,
            }
        }
    }
    None
}

// P1: List style type parsing
fn parse_list_style_type(values: &[Token]) -> Option<ListStyleType> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "disc" => return Some(ListStyleType::Disc),
                "circle" => return Some(ListStyleType::Circle),
                "square" => return Some(ListStyleType::Square),
                "decimal" => return Some(ListStyleType::Decimal),
                "decimal-leading-zero" => return Some(ListStyleType::DecimalLeadingZero),
                "lower-alpha" | "lower-latin" => return Some(ListStyleType::LowerAlpha),
                "upper-alpha" | "upper-latin" => return Some(ListStyleType::UpperAlpha),
                "lower-roman" => return Some(ListStyleType::LowerRoman),
                "upper-roman" => return Some(ListStyleType::UpperRoman),
                "lower-greek" => return Some(ListStyleType::LowerGreek),
                "none" => return Some(ListStyleType::None),
                _ => continue,
            }
        }
    }
    None
}

// P1: List style position parsing
fn parse_list_style_position(values: &[Token]) -> Option<ListStylePosition> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "inside" => return Some(ListStylePosition::Inside),
                "outside" => return Some(ListStylePosition::Outside),
                _ => continue,
            }
        }
    }
    None
}

// P2: Writing mode parsing
fn parse_writing_mode(values: &[Token]) -> Option<WritingMode> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "horizontal-tb" => return Some(WritingMode::HorizontalTb),
                "vertical-rl" => return Some(WritingMode::VerticalRl),
                "vertical-lr" => return Some(WritingMode::VerticalLr),
                // Legacy values
                "lr" | "lr-tb" | "rl" | "rl-tb" => return Some(WritingMode::HorizontalTb),
                "tb" | "tb-rl" => return Some(WritingMode::VerticalRl),
                "tb-lr" => return Some(WritingMode::VerticalLr),
                _ => continue,
            }
        }
    }
    None
}

// P2: Text combine upright parsing
fn parse_text_combine_upright(values: &[Token]) -> Option<TextCombineUpright> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "none" => return Some(TextCombineUpright::None),
                "all" => return Some(TextCombineUpright::All),
                _ => continue,
            }
        }
    }
    // Check for digits(N) function - simplified parsing
    None
}

// P4: Parse shadow value as raw string (box-shadow, text-shadow)
fn parse_shadow_value(values: &[Token]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    // Check for "none" keyword
    if values.len() == 1
        && let Token::Ident(name) = &values[0]
        && name.to_ascii_lowercase() == "none"
    {
        return None;
    }
    // Collect all tokens as a string (simplified)
    let parts: Vec<String> = values
        .iter()
        .filter_map(|t| match t {
            Token::Dimension { value, unit, .. } => Some(format!("{}{}", value, unit)),
            Token::Number { value, .. } => Some(format!("{}", value)),
            Token::Ident(name) => Some(name.to_string()),
            Token::Hash(h) => Some(format!("#{}", h)),
            Token::Comma => Some(",".to_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Parse background-image: url(...) to extract the URL
fn parse_background_image(values: &[Token]) -> Option<String> {
    for token in values {
        match token {
            Token::UnquotedUrl(url) => {
                return Some(url.to_string());
            }
            Token::QuotedString(url) => {
                // Handle url("...") case where quoted string follows url function
                return Some(url.to_string());
            }
            Token::Ident(name) if name.to_ascii_lowercase() == "none" => {
                return None;
            }
            Token::Function(name) if name.to_ascii_lowercase() == "url" => {
                // The URL content would be in subsequent tokens
                continue;
            }
            _ => continue,
        }
    }
    None
}
