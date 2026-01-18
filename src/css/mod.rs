//! CSS parsing for style extraction.
//!
//! This module provides CSS parsing capabilities for extracting styles
//! from EPUB stylesheets to apply to KFX output.

use cssparser::{
    AtRuleParser, BasicParseErrorKind, CowRcStr, ParseError, Parser, ParserInput, ParserState,
    QualifiedRuleParser, StyleSheetParser, Token,
};

/// A parsed CSS value with unit
#[derive(Debug, Clone, PartialEq)]
pub enum CssValue {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    /// Keyword like "auto", "inherit", "normal"
    Keyword(String),
    /// Unitless number (for line-height)
    Number(f32),
}

impl Eq for CssValue {}

impl std::hash::Hash for CssValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            CssValue::Px(v) => {
                state.write_u8(0);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Em(v) => {
                state.write_u8(1);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Rem(v) => {
                state.write_u8(2);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Percent(v) => {
                state.write_u8(3);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Keyword(s) => {
                state.write_u8(4);
                s.hash(state);
            }
            CssValue::Number(v) => {
                state.write_u8(5);
                ((v * 100.0) as i32).hash(state);
            }
        }
    }
}

impl CssValue {
}

/// Text alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    #[default]
    Justify,
}

/// Font weight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontWeight {
    Normal,
    Bold,
    /// Numeric weight 100-900
    Weight(u16),
}

impl FontWeight {
    pub fn is_bold(&self) -> bool {
        match self {
            FontWeight::Bold => true,
            FontWeight::Weight(w) => *w >= 700,
            FontWeight::Normal => false,
        }
    }
}

/// Font style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// Parsed CSS style properties
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ParsedStyle {
    pub font_family: Option<String>,
    pub font_size: Option<CssValue>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub text_align: Option<TextAlign>,
    pub text_indent: Option<CssValue>,
    pub line_height: Option<CssValue>,
    pub margin_top: Option<CssValue>,
    pub margin_bottom: Option<CssValue>,
    pub margin_left: Option<CssValue>,
    pub margin_right: Option<CssValue>,
}

impl ParsedStyle {
    /// Merge another style into this one (other takes precedence)
    pub fn merge(&mut self, other: &ParsedStyle) {
        if other.font_family.is_some() {
            self.font_family.clone_from(&other.font_family);
        }
        if other.font_size.is_some() {
            self.font_size.clone_from(&other.font_size);
        }
        if other.font_weight.is_some() {
            self.font_weight = other.font_weight;
        }
        if other.font_style.is_some() {
            self.font_style = other.font_style;
        }
        if other.text_align.is_some() {
            self.text_align = other.text_align;
        }
        if other.text_indent.is_some() {
            self.text_indent.clone_from(&other.text_indent);
        }
        if other.line_height.is_some() {
            self.line_height.clone_from(&other.line_height);
        }
        if other.margin_top.is_some() {
            self.margin_top.clone_from(&other.margin_top);
        }
        if other.margin_bottom.is_some() {
            self.margin_bottom.clone_from(&other.margin_bottom);
        }
        if other.margin_left.is_some() {
            self.margin_left.clone_from(&other.margin_left);
        }
        if other.margin_right.is_some() {
            self.margin_right.clone_from(&other.margin_right);
        }
    }

    /// Check if this style has any properties set
    pub fn is_empty(&self) -> bool {
        self.font_family.is_none()
            && self.font_size.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
            && self.text_align.is_none()
            && self.text_indent.is_none()
            && self.line_height.is_none()
            && self.margin_top.is_none()
            && self.margin_bottom.is_none()
            && self.margin_left.is_none()
            && self.margin_right.is_none()
    }
}

/// A CSS selector (simplified)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Selector {
    /// Element selector: p, h1, div, etc.
    Element(String),
    /// Class selector: .classname
    Class(String),
    /// ID selector: #idname
    Id(String),
    /// Element with class: p.classname
    ElementClass(String, String),
    /// Universal selector: *
    Universal,
}

impl Selector {
    /// Check if this selector matches the given element
    pub fn matches(&self, element: &str, classes: &[&str], id: Option<&str>) -> bool {
        match self {
            Selector::Element(e) => e.eq_ignore_ascii_case(element),
            Selector::Class(c) => classes.iter().any(|cls| cls.eq_ignore_ascii_case(c)),
            Selector::Id(i) => id.map(|id| id.eq_ignore_ascii_case(i)).unwrap_or(false),
            Selector::ElementClass(e, c) => {
                e.eq_ignore_ascii_case(element)
                    && classes.iter().any(|cls| cls.eq_ignore_ascii_case(c))
            }
            Selector::Universal => true,
        }
    }

    /// Get specificity score (higher = more specific)
    pub fn specificity(&self) -> u32 {
        match self {
            Selector::Universal => 0,
            Selector::Element(_) => 1,
            Selector::Class(_) => 10,
            Selector::ElementClass(_, _) => 11,
            Selector::Id(_) => 100,
        }
    }
}

/// A CSS rule: selector(s) + declarations
#[derive(Debug, Clone)]
pub struct CssRule {
    pub selectors: Vec<Selector>,
    pub style: ParsedStyle,
}

/// Parsed stylesheet containing all rules
#[derive(Debug, Default)]
pub struct Stylesheet {
    rules: Vec<CssRule>,
}

impl Stylesheet {
    /// Parse a CSS stylesheet from a string
    pub fn parse(css: &str) -> Self {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let mut rules = Vec::new();

        let mut rule_parser = RuleListParser { rules: &mut rules };

        for result in StyleSheetParser::new(&mut parser, &mut rule_parser) {
            // Ignore errors, just collect successful rules
            let _ = result;
        }

        Stylesheet { rules }
    }

    /// Get the computed style for an element
    pub fn compute_style(&self, element: &str, classes: &[&str], id: Option<&str>) -> ParsedStyle {
        let mut result = ParsedStyle::default();

        // Collect matching rules with their specificity
        let mut matches: Vec<(u32, &ParsedStyle)> = Vec::new();

        for rule in &self.rules {
            for selector in &rule.selectors {
                if selector.matches(element, classes, id) {
                    matches.push((selector.specificity(), &rule.style));
                }
            }
        }

        // Sort by specificity (stable sort preserves source order for equal specificity)
        matches.sort_by_key(|(spec, _)| *spec);

        // Apply rules in order (lowest specificity first)
        for (_, style) in matches {
            result.merge(style);
        }

        result
    }
}

// =============================================================================
// CSS Parser Implementation
// =============================================================================

struct RuleListParser<'a> {
    rules: &'a mut Vec<CssRule>,
}

impl<'i> QualifiedRuleParser<'i> for RuleListParser<'_> {
    type Prelude = Vec<Selector>;
    type QualifiedRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        parse_selector_list(input)
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        let style = parse_declaration_block(input);
        if !style.is_empty() {
            self.rules.push(CssRule {
                selectors: prelude,
                style,
            });
        }
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for RuleListParser<'_> {
    type Prelude = ();
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        // Skip all @rules (@import, @media, @font-face, etc.)
        let _ = name;
        // Consume tokens to find the end
        while input.next().is_ok() {}
        Err(input.new_custom_error(()))
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        Err(cssparser::ParseError {
            kind: cssparser::ParseErrorKind::Basic(BasicParseErrorKind::AtRuleInvalid(
                CowRcStr::from(""),
            )),
            location: cssparser::SourceLocation { line: 0, column: 0 },
        })
    }

    fn rule_without_block(
        &mut self,
        _prelude: Self::Prelude,
        _start: &ParserState,
    ) -> Result<Self::AtRule, ()> {
        Err(())
    }
}

/// Parse a list of selectors separated by commas
fn parse_selector_list<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<Vec<Selector>, ParseError<'i, ()>> {
    let mut selectors = Vec::new();

    loop {
        input.skip_whitespace();

        if input.is_exhausted() {
            break;
        }

        match parse_single_selector(input) {
            Ok(sel) => selectors.push(sel),
            Err(_) => {
                // Skip to next comma or end
                while let Ok(token) = input.next() {
                    if matches!(token, Token::Comma) {
                        break;
                    }
                }
                continue;
            }
        }

        input.skip_whitespace();

        match input.next() {
            Ok(Token::Comma) => continue,
            _ => break,
        }
    }

    if selectors.is_empty() {
        Err(input.new_custom_error(()))
    } else {
        Ok(selectors)
    }
}

/// Parse a single selector
fn parse_single_selector<'i, 't>(
    input: &mut Parser<'i, 't>,
) -> Result<Selector, ParseError<'i, ()>> {
    input.skip_whitespace();

    let token = input.next()?.clone();

    match token {
        Token::Ident(name) => {
            let element = name.to_string().to_lowercase();
            // Check for class following element (e.g., p.class)
            let result = input.try_parse(|i| match i.next_including_whitespace()? {
                Token::Delim('.') => match i.next()? {
                    Token::Ident(class) => Ok(class.to_string()),
                    _ => Err(i.new_custom_error::<(), ()>(())),
                },
                _ => Err(i.new_custom_error(())),
            });

            if let Ok(class) = result {
                Ok(Selector::ElementClass(element, class.to_lowercase()))
            } else {
                Ok(Selector::Element(element))
            }
        }
        Token::Delim('.') => match input.next()? {
            Token::Ident(class) => Ok(Selector::Class(class.to_string().to_lowercase())),
            _ => Err(input.new_custom_error(())),
        },
        Token::IDHash(id) => Ok(Selector::Id(id.to_string().to_lowercase())),
        Token::Delim('*') => Ok(Selector::Universal),
        _ => Err(input.new_custom_error(())),
    }
}

/// Parse a declaration block (property: value; ...)
fn parse_declaration_block<'i, 't>(input: &mut Parser<'i, 't>) -> ParsedStyle {
    let mut style = ParsedStyle::default();

    loop {
        input.skip_whitespace();

        if input.is_exhausted() {
            break;
        }

        // Try to parse a declaration
        let result: Result<(), ParseError<'i, ()>> = input.try_parse(|i| {
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
        "text-align" => {
            style.text_align = parse_text_align(values);
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
        _ => {
            // Ignore unsupported properties
        }
    }
}

fn parse_font_family(values: &[Token]) -> Option<String> {
    for token in values {
        match token {
            Token::Ident(name) => return Some(name.to_string()),
            Token::QuotedString(name) => return Some(name.to_string()),
            _ => continue,
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_stylesheet() {
        let css = r#"
            p { text-align: justify; margin-bottom: 1em; }
            h1 { font-size: 2em; text-align: center; font-weight: bold; }
            .italic { font-style: italic; }
        "#;

        let stylesheet = Stylesheet::parse(css);
        assert_eq!(stylesheet.rules.len(), 3);

        // Check p style
        let p_style = stylesheet.compute_style("p", &[], None);
        assert_eq!(p_style.text_align, Some(TextAlign::Justify));
        assert!(matches!(p_style.margin_bottom, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // Check h1 style
        let h1_style = stylesheet.compute_style("h1", &[], None);
        assert_eq!(h1_style.text_align, Some(TextAlign::Center));
        assert!(matches!(h1_style.font_size, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));
        assert!(matches!(h1_style.font_weight, Some(FontWeight::Bold)));
    }

    #[test]
    fn test_selector_specificity() {
        let css = r#"
            p { text-align: left; }
            .special { text-align: right; }
            p.special { text-align: center; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Element only
        let p_style = stylesheet.compute_style("p", &[], None);
        assert_eq!(p_style.text_align, Some(TextAlign::Left));

        // Class should override element
        let class_style = stylesheet.compute_style("div", &["special"], None);
        assert_eq!(class_style.text_align, Some(TextAlign::Right));

        // Element.class should have highest specificity
        let combined_style = stylesheet.compute_style("p", &["special"], None);
        assert_eq!(combined_style.text_align, Some(TextAlign::Center));
    }

    #[test]
    fn test_margin_shorthand() {
        let css = r#"
            .m1 { margin: 1em; }
            .m2 { margin: 1em 2em; }
            .m4 { margin: 1em 2em 3em 4em; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let m1 = stylesheet.compute_style("div", &["m1"], None);
        assert!(matches!(m1.margin_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(m1.margin_left, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        let m2 = stylesheet.compute_style("div", &["m2"], None);
        assert!(matches!(m2.margin_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(m2.margin_left, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));
    }
}
