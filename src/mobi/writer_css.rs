//! Token-aware edits to the original CSS, without reserializing unrelated rules.

use cssparser::{Delimiter, ParseError, Parser, Token};
use std::collections::HashMap;

type Edit = (usize, usize, String);
const MAX_DEPTH: usize = 128;

pub(super) fn rewrite(css: &[u8], href: &str, resources: &HashMap<String, usize>) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(css) else {
        return css.to_vec();
    };
    let mut edits = Vec::new();
    visit(&mut Parser::new(text), href, resources, &mut edits, 0, true);
    apply(text, 0, text.len(), &edits).into_bytes()
}

fn apply(text: &str, start: usize, end: usize, edits: &[Edit]) -> String {
    let mut out = String::new();
    let mut pos = start;
    for (a, b, replacement) in edits {
        out.push_str(&text[pos..*a]);
        out.push_str(replacement);
        pos = *b;
    }
    out.push_str(&text[pos..end]);
    out
}

/// Token positions refer to the original sheet even inside nested blocks.
fn visit(
    parser: &mut Parser<'_>,
    href: &str,
    resources: &HashMap<String, usize>,
    edits: &mut Vec<Edit>,
    depth: usize,
    rules: bool,
) {
    let mut group = false;
    while !parser.is_exhausted() {
        let start = parser.position().byte_index();
        let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else {
            break;
        };
        match token {
            Token::AtKeyword(name) if rules && name.eq_ignore_ascii_case("font-face") => {
                // A font-face has an empty prelude. Don't scan ahead into an
                // unrelated block when the input is malformed.
                if parser.try_parse(|p| p.expect_curly_bracket_block()).is_ok() {
                    let mut replacement = None;
                    let _: Result<(), ParseError<()>> = parser.parse_nested_block(|p| {
                        replacement = font_face(p, href, resources);
                        Ok(())
                    });
                    edits.push((
                        start,
                        parser.position().byte_index(),
                        replacement.unwrap_or_default(),
                    ));
                }
            }
            Token::AtKeyword(name)
                if rules
                    && (name.eq_ignore_ascii_case("charset")
                        || name.eq_ignore_ascii_case("namespace")) =>
            {
                let _: Result<(), ParseError<()>> =
                    parser.parse_until_before(Delimiter::Semicolon, |p| {
                        while p.next_including_whitespace_and_comments().is_ok() {}
                        Ok(())
                    });
                let _ = parser.try_parse(|p| p.expect_semicolon());
                edits.push((start, parser.position().byte_index(), String::new()));
            }
            Token::AtKeyword(name) if rules => {
                group = [
                    "media",
                    "supports",
                    "layer",
                    "container",
                    "document",
                    "scope",
                    "starting-style",
                ]
                .iter()
                .any(|s| name.eq_ignore_ascii_case(s));
            }
            Token::Semicolon => group = false,
            Token::UnquotedUrl(url) => {
                if let Some(url) = embedded_url(&url, href, resources) {
                    edits.push((start, parser.position().byte_index(), format!("url({url})")));
                }
            }
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                let url: Result<_, ParseError<()>> =
                    parser.parse_nested_block(|p| Ok(p.expect_string_cloned()?));
                if let Ok(url) = url
                    && let Some(url) = embedded_url(&url, href, resources)
                {
                    edits.push((start, parser.position().byte_index(), format!("url({url})")));
                }
            }
            Token::CurlyBracketBlock if depth < MAX_DEPTH => {
                let _: Result<(), ParseError<()>> = parser.parse_nested_block(|p| {
                    visit(p, href, resources, edits, depth + 1, group);
                    Ok(())
                });
                group = false;
            }
            Token::SquareBracketBlock | Token::ParenthesisBlock | Token::Function(_)
                if depth < MAX_DEPTH =>
            {
                let _: Result<(), ParseError<()>> = parser.parse_nested_block(|p| {
                    visit(p, href, resources, edits, depth + 1, false);
                    Ok(())
                });
            }
            _ => {}
        }
    }
}

fn embedded_url(url: &str, href: &str, resources: &HashMap<String, usize>) -> Option<String> {
    let resolved = super::writer_svg::local_reference(href, url)?;
    let index = resources.get(&resolved)?;
    let mut buf = [0; 4];
    super::write_base32_4(*index, &mut buf);
    Some(format!(
        "kindle:embed:{}",
        std::str::from_utf8(&buf).unwrap()
    ))
}

fn font_face(
    parser: &mut Parser<'_>,
    href: &str,
    resources: &HashMap<String, usize>,
) -> Option<String> {
    let beginning = parser.position();
    let mut edits = Vec::new();
    let mut has_source = false;
    while !parser.is_exhausted() {
        let declaration_start = parser.position().byte_index();
        let _: Result<(), ParseError<()>> = parser.parse_until_before(Delimiter::Semicolon, |p| {
            let Ok(name) = p.expect_ident_cloned() else {
                while p.next_including_whitespace_and_comments().is_ok() {}
                return Ok(());
            };
            if !name.eq_ignore_ascii_case("src") || p.expect_colon().is_err() {
                while p.next_including_whitespace_and_comments().is_ok() {}
                return Ok(());
            }
            let value_start = p.position().byte_index();
            let mut sources = Vec::new();
            while !p.is_exhausted() {
                let _: Result<(), ParseError<()>> =
                    p.parse_until_before(Delimiter::Comma, |source| {
                        if let Some(value) = font_source(source, href, resources) {
                            sources.push(value);
                        }
                        while source.next_including_whitespace_and_comments().is_ok() {}
                        Ok(())
                    });
                let _ = p.try_parse(|p| p.expect_comma());
            }
            has_source |= !sources.is_empty();
            edits.push((
                if sources.is_empty() {
                    declaration_start
                } else {
                    value_start
                },
                p.position().byte_index(),
                sources.join(", "),
            ));
            Ok(())
        });
        let _ = parser.try_parse(|p| p.expect_semicolon());
    }
    if !has_source {
        return None;
    }
    let body = parser.slice_from(beginning);
    let local_edits: Vec<_> = edits
        .into_iter()
        .map(|(a, b, s)| (a - beginning.byte_index(), b - beginning.byte_index(), s))
        .collect();
    Some(format!(
        "@font-face {{{}}}",
        apply(body, 0, body.len(), &local_edits)
    ))
}

fn font_source(
    parser: &mut Parser<'_>,
    href: &str,
    resources: &HashMap<String, usize>,
) -> Option<String> {
    let start = parser.position();
    let token = parser.next().ok()?.clone();
    let url = match token {
        Token::UnquotedUrl(url) => url,
        Token::Function(name) if name.eq_ignore_ascii_case("url") => {
            let url: Result<_, ParseError<()>> =
                parser.parse_nested_block(|p| Ok(p.expect_string_cloned()?));
            url.ok()?
        }
        // Kindle cannot resolve installed fonts from the EPUB author's system.
        _ => return None,
    };
    let url_end = parser.position();
    let replacement = embedded_url(&url, href, resources);
    // Data URLs are self-contained; a missing archive path is not. Do not
    // discard legitimate embedded data just because it has no resource record.
    if replacement.is_none()
        && !url
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("data:"))
    {
        return None;
    }
    // Retain format()/tech() hints, including commas and braces in strings.
    while parser.next_including_whitespace_and_comments().is_ok() {}
    Some(match replacement {
        Some(url) => format!("url({url}){}", parser.slice_from(url_end)),
        None => parser.slice_from(start).to_string(),
    })
}
