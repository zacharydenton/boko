//! CSS parsing and manipulation using cssparser
//!
//! Provides utilities for:
//! - Parsing CSS stylesheets
//! - URL rewriting for resources
//! - CSS cleanup and normalization

use std::fmt::Write;

use cssparser::{BasicParseErrorKind, ParseError, Parser, ParserInput, Token};
use regex_lite::Regex;

type CssParseError<'i> = ParseError<'i, ()>;

/// Rewrite URLs in CSS (for resource relocation)
pub fn rewrite_css_urls<F>(css: &str, rewriter: F) -> String
where
    F: Fn(&str) -> String,
{
    // Match url(...) with optional quotes - regex-lite doesn't support backreferences
    let url_pattern = Regex::new(r#"url\s*\(\s*['"]?([^)'"\s]+)['"]?\s*\)"#).unwrap();

    let mut result = css.to_string();
    let replacements: Vec<_> = url_pattern
        .captures_iter(css)
        .filter_map(|cap| {
            let full_match = cap.get(0)?;
            let url = cap.get(1)?.as_str();
            let new_url = rewriter(url);
            if new_url != url {
                Some((full_match.start(), full_match.end(), format!("url(\"{}\")", new_url)))
            } else {
                None
            }
        })
        .collect();

    // Apply replacements in reverse order
    for (start, end, replacement) in replacements.into_iter().rev() {
        result.replace_range(start..end, &replacement);
    }

    result
}

/// Extract all URLs referenced in CSS
pub fn extract_css_urls(css: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);

    extract_urls_recursive(&mut parser, &mut urls);
    urls
}

fn extract_urls_recursive(parser: &mut Parser, urls: &mut Vec<String>) {
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        match token {
            Token::UnquotedUrl(url) => {
                urls.push(url.to_string());
            }
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                let _ = parser.parse_nested_block(|p| {
                    if let Ok(token) = p.next() {
                        if let Token::QuotedString(url) = token {
                            urls.push(url.to_string());
                        }
                    }
                    Ok::<_, CssParseError>(())
                });
            }
            Token::CurlyBracketBlock | Token::ParenthesisBlock | Token::SquareBracketBlock => {
                let _ = parser.parse_nested_block(|p| {
                    extract_urls_recursive(p, urls);
                    Ok::<_, CssParseError>(())
                });
            }
            _ => {}
        }
    }
}

/// Clean CSS by removing browser-specific prefixes and problematic properties
pub fn clean_css(css: &str) -> String {
    let mut output = String::new();
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);

    clean_css_recursive(&mut parser, &mut output);
    output
}

fn clean_css_recursive(parser: &mut Parser, output: &mut String) {
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        match token {
            // Skip vendor-prefixed at-rules
            Token::AtKeyword(name) if name.starts_with('-') => {
                skip_at_rule(parser);
            }
            Token::AtKeyword(name) => {
                output.push('@');
                output.push_str(name);
            }
            // Skip vendor-prefixed properties
            Token::Ident(name) if name.starts_with('-') || name.starts_with("mso-") => {
                // Skip until semicolon or end of block
                skip_declaration(parser);
            }
            Token::CurlyBracketBlock => {
                output.push('{');
                let _ = parser.parse_nested_block(|p| {
                    clean_css_recursive(p, output);
                    Ok::<_, CssParseError>(())
                });
                output.push('}');
            }
            _ => {
                write_token(output, token);
            }
        }
    }
}

fn skip_at_rule(parser: &mut Parser) {
    let mut depth = 0;
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        match token {
            Token::CurlyBracketBlock => {
                depth += 1;
                let _ = parser.parse_nested_block(|p| {
                    while p.next_including_whitespace_and_comments().is_ok() {}
                    Ok::<_, CssParseError>(())
                });
                if depth == 0 {
                    break;
                }
            }
            Token::Semicolon if depth == 0 => break,
            _ => {}
        }
    }
}

fn skip_declaration(parser: &mut Parser) {
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        match token {
            Token::Semicolon => break,
            Token::CurlyBracketBlock => {
                // Unexpected, but handle it
                let _ = parser.parse_nested_block(|p| {
                    while p.next_including_whitespace_and_comments().is_ok() {}
                    Ok::<_, CssParseError>(())
                });
                break;
            }
            _ => {}
        }
    }
}

fn write_token(output: &mut String, token: &Token) {
    match token {
        Token::Ident(s) => output.push_str(s),
        Token::AtKeyword(s) => {
            output.push('@');
            output.push_str(s);
        }
        Token::Hash(s) | Token::IDHash(s) => {
            output.push('#');
            output.push_str(s);
        }
        Token::QuotedString(s) => {
            output.push('"');
            output.push_str(s);
            output.push('"');
        }
        Token::Number { value, .. } => {
            let _ = write!(output, "{}", value);
        }
        Token::Percentage { unit_value, .. } => {
            let _ = write!(output, "{}%", unit_value * 100.0);
        }
        Token::Dimension { value, unit, .. } => {
            let _ = write!(output, "{}{}", value, unit);
        }
        Token::WhiteSpace(s) => output.push_str(s),
        Token::Comment(s) => {
            output.push_str("/*");
            output.push_str(s);
            output.push_str("*/");
        }
        Token::Colon => output.push(':'),
        Token::Semicolon => output.push(';'),
        Token::Comma => output.push(','),
        Token::Delim(c) => output.push(*c),
        Token::Function(name) => {
            output.push_str(name);
            output.push('(');
        }
        Token::ParenthesisBlock => output.push('('),
        Token::SquareBracketBlock => output.push('['),
        Token::CurlyBracketBlock => output.push('{'),
        Token::CloseParenthesis => output.push(')'),
        Token::CloseSquareBracket => output.push(']'),
        Token::CloseCurlyBracket => output.push('}'),
        Token::UnquotedUrl(url) => {
            output.push_str("url(");
            output.push_str(url);
            output.push(')');
        }
        Token::IncludeMatch => output.push_str("~="),
        Token::DashMatch => output.push_str("|="),
        Token::PrefixMatch => output.push_str("^="),
        Token::SuffixMatch => output.push_str("$="),
        Token::SubstringMatch => output.push_str("*="),
        Token::CDO => output.push_str("<!--"),
        Token::CDC => output.push_str("-->"),
        Token::BadUrl(_) | Token::BadString(_) => {}
    }
}

/// Extract @import URLs from CSS
pub fn extract_imports(css: &str) -> Vec<String> {
    let import_pattern = Regex::new(r#"@import\s+(?:url\s*\(\s*)?['"]?([^'");\s]+)['"]?\s*\)?\s*;"#).unwrap();

    import_pattern
        .captures_iter(css)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_urls() {
        let css = r#"
            body { background: url('bg.png'); }
            .icon { background-image: url("icon.svg"); }
        "#;
        let urls = extract_css_urls(css);

        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"bg.png".to_string()));
        assert!(urls.contains(&"icon.svg".to_string()));
    }

    #[test]
    fn test_rewrite_urls() {
        let css = "body { background: url('images/bg.png'); }";
        let rewritten = rewrite_css_urls(css, |url| url.replace("images/", "assets/"));

        assert!(rewritten.contains("assets/bg.png"));
    }

    #[test]
    fn test_clean_css() {
        let css = r#"
p { color: red; -webkit-transform: scale(1); }
div { mso-special: value; font-size: 12px; }
"#;
        let cleaned = clean_css(css);

        assert!(cleaned.contains("color"));
        assert!(cleaned.contains("red"));
        assert!(cleaned.contains("font-size"));
        assert!(!cleaned.contains("-webkit-"));
        assert!(!cleaned.contains("mso-"));
    }

    #[test]
    fn test_extract_imports() {
        let css = r#"
            @import url('styles.css');
            @import "other.css";
            @import url(third.css);
        "#;
        let imports = extract_imports(css);

        assert_eq!(imports.len(), 3);
        assert!(imports.contains(&"styles.css".to_string()));
        assert!(imports.contains(&"other.css".to_string()));
        assert!(imports.contains(&"third.css".to_string()));
    }
}
