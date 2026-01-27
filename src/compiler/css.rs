//! CSS parsing and cascade implementation.

use std::cmp::Ordering;

use cssparser::{
    AtRuleParser, DeclarationParser, ParseError, Parser, ParserInput, QualifiedRuleParser,
    RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
use selectors::context::{MatchingContext, SelectorCaches};
use selectors::parser::Selector;

use super::element_ref::{BokoSelectors, ElementRef};
use crate::ir::{
    Color, ComputedStyle, Display, FontStyle, FontWeight, Length, ListStyleType, StylePool,
    TextAlign,
};

/// A parsed CSS stylesheet.
#[derive(Debug, Default, Clone)]
pub struct Stylesheet {
    pub rules: Vec<CssRule>,
}

/// A CSS rule with selectors and declarations.
#[derive(Debug, Clone)]
pub struct CssRule {
    pub selectors: Vec<Selector<BokoSelectors>>,
    pub declarations: Vec<Declaration>,
    pub specificity: Specificity,
}

/// A CSS declaration (property: value).
#[derive(Debug, Clone)]
pub struct Declaration {
    pub property: String,
    pub value: PropertyValue,
    pub important: bool,
}

/// Parsed CSS property value.
#[derive(Debug, Clone)]
pub enum PropertyValue {
    Color(Color),
    Length(Length),
    FontWeight(FontWeight),
    FontStyle(FontStyle),
    TextAlign(TextAlign),
    Display(Display),
    ListStyleType(ListStyleType),
    String(String),
    Keyword(String),
    None,
}

/// CSS specificity for cascade ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Specificity {
    pub ids: u16,
    pub classes: u16,
    pub elements: u16,
}

impl Specificity {
    pub fn from_selector(selector: &Selector<BokoSelectors>) -> Self {
        let spec = selector.specificity();
        // selectors crate packs specificity as (id << 20) | (class << 10) | elements
        Self {
            ids: ((spec >> 20) & 0x3FF) as u16,
            classes: ((spec >> 10) & 0x3FF) as u16,
            elements: (spec & 0x3FF) as u16,
        }
    }
}

impl Ord for Specificity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ids
            .cmp(&other.ids)
            .then(self.classes.cmp(&other.classes))
            .then(self.elements.cmp(&other.elements))
    }
}

impl PartialOrd for Specificity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Origin of a style (for cascade ordering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    UserAgent = 0,
    Author = 1,
}

/// A matched rule with ordering information for the cascade.
#[derive(Debug)]
pub struct MatchedRule<'a> {
    pub declaration: &'a Declaration,
    pub origin: Origin,
    pub specificity: Specificity,
    pub order: usize,
}

impl Stylesheet {
    /// Parse a CSS stylesheet from a string.
    pub fn parse(css: &str) -> Self {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let mut rules = Vec::new();

        let mut rule_parser = TopLevelRuleParser { rules: &mut rules };
        let stylesheet_parser = StyleSheetParser::new(&mut parser, &mut rule_parser);

        for result in stylesheet_parser {
            // Ignore errors - lenient parsing
            let _ = result;
        }

        Self { rules }
    }

    /// Check if the stylesheet is empty.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Parser for top-level stylesheet rules.
struct TopLevelRuleParser<'a> {
    rules: &'a mut Vec<CssRule>,
}

impl<'i> AtRuleParser<'i> for TopLevelRuleParser<'_> {
    type Prelude = ();
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        _name: cssparser::CowRcStr<'i>,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        // Skip at-rules for now
        Err(_input.new_custom_error(()))
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &cssparser::ParserState,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        Err(_input.new_custom_error(()))
    }
}

impl<'i> QualifiedRuleParser<'i> for TopLevelRuleParser<'_> {
    type Prelude = Vec<Selector<BokoSelectors>>;
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
        _start: &cssparser::ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        let specificity = prelude
            .first()
            .map(Specificity::from_selector)
            .unwrap_or_default();

        let mut declarations = Vec::new();
        let mut decl_parser = DeclarationListParser {
            declarations: &mut declarations,
        };

        for result in RuleBodyParser::new(input, &mut decl_parser) {
            // Ignore errors - lenient parsing
            let _ = result;
        }

        self.rules.push(CssRule {
            selectors: prelude,
            declarations,
            specificity,
        });

        Ok(())
    }
}

/// Parse a comma-separated list of selectors.
fn parse_selector_list<'i>(
    parser: &mut Parser<'i, '_>,
) -> Result<Vec<Selector<BokoSelectors>>, ParseError<'i, ()>> {
    let location = parser.current_source_location();
    let selectors = selectors::parser::SelectorList::parse(
        &BokoSelectors,
        parser,
        selectors::parser::ParseRelative::No,
    )
    .map_err(|_| location.new_custom_error(()))?;

    Ok(selectors.slice().to_vec())
}

struct DeclarationListParser<'a> {
    declarations: &'a mut Vec<Declaration>,
}

impl<'i> cssparser::AtRuleParser<'i> for DeclarationListParser<'_> {
    type Prelude = ();
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        _name: cssparser::CowRcStr<'i>,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        Err(_input.new_custom_error(()))
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &cssparser::ParserState,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        Err(_input.new_custom_error(()))
    }
}

impl<'i> cssparser::QualifiedRuleParser<'i> for DeclarationListParser<'_> {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        Err(_input.new_custom_error(()))
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &cssparser::ParserState,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        Err(_input.new_custom_error(()))
    }
}

impl<'i> DeclarationParser<'i> for DeclarationListParser<'_> {
    type Declaration = ();
    type Error = ();

    fn parse_value<'t>(
        &mut self,
        name: cssparser::CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _start: &cssparser::ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, Self::Error>> {
        let property = name.to_string();
        let value = parse_property_value(&property, input);
        let important = input.try_parse(cssparser::parse_important).is_ok();

        self.declarations.push(Declaration {
            property,
            value,
            important,
        });

        Ok(())
    }
}

impl<'i> RuleBodyItemParser<'i, (), ()> for DeclarationListParser<'_> {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}

/// Parse a property value based on the property name.
fn parse_property_value(property: &str, input: &mut Parser<'_, '_>) -> PropertyValue {
    match property {
        "color" | "background-color" => parse_color(input).unwrap_or(PropertyValue::None),

        "font-size" | "margin" | "margin-top" | "margin-bottom" | "margin-left"
        | "margin-right" | "padding" | "padding-top" | "padding-bottom" | "padding-left"
        | "padding-right" | "text-indent" | "line-height" => {
            parse_length(input).unwrap_or(PropertyValue::None)
        }

        "font-weight" => parse_font_weight(input).unwrap_or(PropertyValue::None),

        "font-style" => parse_font_style(input).unwrap_or(PropertyValue::None),

        "text-align" => parse_text_align(input).unwrap_or(PropertyValue::None),

        "display" => parse_display(input).unwrap_or(PropertyValue::None),

        "font-family" => parse_font_family(input).unwrap_or(PropertyValue::None),

        "text-decoration" | "text-decoration-line" => {
            parse_text_decoration(input).unwrap_or(PropertyValue::None)
        }

        "vertical-align" => parse_vertical_align(input).unwrap_or(PropertyValue::None),

        "list-style-type" => parse_list_style_type(input).unwrap_or(PropertyValue::None),

        "font-variant" | "font-variant-caps" => {
            parse_font_variant(input).unwrap_or(PropertyValue::None)
        }

        _ => {
            // Consume remaining tokens for unknown properties
            while input.next().is_ok() {}
            PropertyValue::None
        }
    }
}

fn parse_color(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
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
            "inherit" | "initial" | "unset" => {
                return Some(PropertyValue::Keyword(token.to_string()))
            }
            _ => return None,
        };
        return Some(PropertyValue::Color(color));
    }

    // Try ID token (which is how cssparser parses hex colors like #ff0000)
    if let Ok(Token::IDHash(hash)) = input.try_parse(|i| i.next().cloned()) {
        if let Some(color) = parse_hex_color(hash.as_ref()) {
            return Some(PropertyValue::Color(color));
        }
    }

    // Try hash token
    if let Ok(Token::Hash(hash)) = input.try_parse(|i| i.next().cloned()) {
        if let Some(color) = parse_hex_color(hash.as_ref()) {
            return Some(PropertyValue::Color(color));
        }
    }

    // Try rgb() or rgba()
    if let Ok(color) = input.try_parse(parse_rgb_function) {
        return Some(PropertyValue::Color(color));
    }

    None
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

fn parse_length(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    match input.next().ok()? {
        Token::Dimension { value, unit, .. } => {
            let length = match unit.as_ref() {
                "px" => Length::Px(*value),
                "em" => Length::Em(*value),
                "rem" => Length::Rem(*value),
                "%" => Length::Percent(*value),
                _ => return None,
            };
            Some(PropertyValue::Length(length))
        }
        Token::Percentage { unit_value, .. } => {
            Some(PropertyValue::Length(Length::Percent(*unit_value * 100.0)))
        }
        Token::Number { value, .. } if *value == 0.0 => Some(PropertyValue::Length(Length::Px(0.0))),
        Token::Ident(ident) => match ident.as_ref() {
            "auto" => Some(PropertyValue::Length(Length::Auto)),
            "inherit" | "initial" | "unset" => Some(PropertyValue::Keyword(ident.to_string())),
            _ => None,
        },
        _ => None,
    }
}

fn parse_font_weight(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        let weight = match token.as_ref() {
            "normal" => FontWeight::NORMAL,
            "bold" => FontWeight::BOLD,
            "lighter" => FontWeight(300),
            "bolder" => FontWeight(700),
            "inherit" | "initial" | "unset" => {
                return Some(PropertyValue::Keyword(token.to_string()))
            }
            _ => return None,
        };
        return Some(PropertyValue::FontWeight(weight));
    }

    if let Ok(Token::Number {
        int_value: Some(v), ..
    }) = input.next()
    {
        let v = *v;
        if (100..=900).contains(&v) && v % 100 == 0 {
            return Some(PropertyValue::FontWeight(FontWeight(v as u16)));
        }
    }

    None
}

fn parse_font_style(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let style = match token.as_ref() {
        "normal" => FontStyle::Normal,
        "italic" => FontStyle::Italic,
        "oblique" => FontStyle::Oblique,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::FontStyle(style))
}

fn parse_text_align(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let align = match token.as_ref() {
        "left" => TextAlign::Left,
        "right" => TextAlign::Right,
        "center" => TextAlign::Center,
        "justify" => TextAlign::Justify,
        "start" => TextAlign::Start,
        "end" => TextAlign::End,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::TextAlign(align))
}

fn parse_display(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let display = match token.as_ref() {
        "block" => Display::Block,
        "inline" => Display::Inline,
        "none" => Display::None,
        "list-item" => Display::ListItem,
        "table-cell" => Display::TableCell,
        "table-row" => Display::TableRow,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::Display(display))
}

fn parse_font_family(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let mut families = Vec::new();

    loop {
        if let Ok(token) = input.try_parse(|i| i.expect_string_cloned()) {
            families.push(token.to_string());
        } else if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
            families.push(token.to_string());
        } else {
            break;
        }

        if input.try_parse(|i| i.expect_comma()).is_err() {
            break;
        }
    }

    if families.is_empty() {
        None
    } else {
        Some(PropertyValue::String(families.join(", ")))
    }
}

fn parse_text_decoration(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let mut keywords = Vec::new();
    while let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        keywords.push(token.to_string());
    }
    if keywords.is_empty() {
        None
    } else {
        Some(PropertyValue::Keyword(keywords.join(" ")))
    }
}

fn parse_vertical_align(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        return Some(PropertyValue::Keyword(token.to_string()));
    }
    None
}

fn parse_list_style_type(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let style = match token.as_ref() {
        "none" => ListStyleType::None,
        "disc" => ListStyleType::Disc,
        "circle" => ListStyleType::Circle,
        "square" => ListStyleType::Square,
        "decimal" => ListStyleType::Decimal,
        "lower-alpha" | "lower-latin" => ListStyleType::LowerAlpha,
        "upper-alpha" | "upper-latin" => ListStyleType::UpperAlpha,
        "lower-roman" => ListStyleType::LowerRoman,
        "upper-roman" => ListStyleType::UpperRoman,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::ListStyleType(style))
}

fn parse_font_variant(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "small-caps" => Some(PropertyValue::Keyword("small-caps".to_string())),
        "normal" | "none" => Some(PropertyValue::Keyword("normal".to_string())),
        "inherit" | "initial" | "unset" => Some(PropertyValue::Keyword(token.to_string())),
        _ => None,
    }
}

/// Compute styles for an element by applying the cascade.
pub fn compute_styles(
    elem: ElementRef<'_>,
    stylesheets: &[(Stylesheet, Origin)],
    parent_style: Option<&ComputedStyle>,
    _style_pool: &mut StylePool,
) -> ComputedStyle {
    // Collect matching rules
    let mut matched: Vec<MatchedRule> = Vec::new();
    let mut order = 0;

    for (stylesheet, origin) in stylesheets {
        for rule in &stylesheet.rules {
            if rule_matches(elem, rule) {
                for decl in &rule.declarations {
                    matched.push(MatchedRule {
                        declaration: decl,
                        origin: *origin,
                        specificity: rule.specificity,
                        order,
                    });
                    order += 1;
                }
            }
        }
    }

    // Sort by cascade order: origin, important, specificity, order
    matched.sort_by(|a, b| {
        // Important declarations win
        let a_important = a.declaration.important;
        let b_important = b.declaration.important;
        if a_important != b_important {
            return b_important.cmp(&a_important);
        }

        // Then by origin (author > user-agent)
        let origin_cmp = a.origin.cmp(&b.origin);
        if origin_cmp != Ordering::Equal {
            return origin_cmp;
        }

        // Then by specificity
        let spec_cmp = a.specificity.cmp(&b.specificity);
        if spec_cmp != Ordering::Equal {
            return spec_cmp;
        }

        // Finally by source order
        a.order.cmp(&b.order)
    });

    // Start with inherited values from parent
    let mut style = parent_style.cloned().unwrap_or_default();

    // Apply matched declarations in cascade order
    for matched_rule in &matched {
        apply_declaration(&mut style, matched_rule.declaration);
    }

    style
}

/// Check if a rule matches an element.
fn rule_matches(elem: ElementRef<'_>, rule: &CssRule) -> bool {
    let mut caches = SelectorCaches::default();
    let mut context = MatchingContext::new(
        selectors::matching::MatchingMode::Normal,
        None,
        &mut caches,
        selectors::context::QuirksMode::NoQuirks,
        selectors::matching::NeedsSelectorFlags::No,
        selectors::matching::MatchingForInvalidation::No,
    );

    rule.selectors.iter().any(|selector| {
        selectors::matching::matches_selector(selector, 0, None, &elem, &mut context)
    })
}

/// Apply a declaration to a computed style.
fn apply_declaration(style: &mut ComputedStyle, decl: &Declaration) {
    match decl.property.as_str() {
        "color" => {
            if let PropertyValue::Color(c) = &decl.value {
                style.color = Some(*c);
            }
        }
        "background-color" => {
            if let PropertyValue::Color(c) = &decl.value {
                style.background_color = Some(*c);
            }
        }
        "font-family" => {
            if let PropertyValue::String(s) = &decl.value {
                style.font_family = Some(s.clone());
            }
        }
        "font-size" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.font_size = *l;
            }
        }
        "font-weight" => {
            if let PropertyValue::FontWeight(w) = &decl.value {
                style.font_weight = *w;
            }
        }
        "font-style" => {
            if let PropertyValue::FontStyle(s) = &decl.value {
                style.font_style = *s;
            }
        }
        "text-align" => {
            if let PropertyValue::TextAlign(a) = &decl.value {
                style.text_align = *a;
            }
        }
        "text-indent" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.text_indent = *l;
            }
        }
        "line-height" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.line_height = *l;
            }
        }
        "display" => {
            if let PropertyValue::Display(d) = &decl.value {
                style.display = *d;
            }
        }
        "margin-top" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_top = *l;
            }
        }
        "margin-bottom" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_bottom = *l;
            }
        }
        "margin-left" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_left = *l;
            }
        }
        "margin-right" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_right = *l;
            }
        }
        "padding-top" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_top = *l;
            }
        }
        "padding-bottom" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_bottom = *l;
            }
        }
        "padding-left" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_left = *l;
            }
        }
        "padding-right" => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_right = *l;
            }
        }
        "text-decoration" | "text-decoration-line" => {
            if let PropertyValue::Keyword(k) = &decl.value {
                style.text_decoration_underline = k.contains("underline");
                style.text_decoration_line_through = k.contains("line-through");
            }
        }
        "vertical-align" => {
            if let PropertyValue::Keyword(k) = &decl.value {
                style.vertical_align_super = k == "super";
                style.vertical_align_sub = k == "sub";
            }
        }
        "list-style-type" => {
            if let PropertyValue::ListStyleType(lst) = &decl.value {
                style.list_style_type = *lst;
            }
        }
        "font-variant" | "font-variant-caps" => {
            if let PropertyValue::Keyword(k) = &decl.value {
                style.font_variant = match k.as_str() {
                    "small-caps" => crate::ir::FontVariant::SmallCaps,
                    _ => crate::ir::FontVariant::Normal,
                };
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_rule() {
        let css = "p { color: red; }";
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.rules.len(), 1);
        let rule = &stylesheet.rules[0];
        assert_eq!(rule.selectors.len(), 1);
        assert_eq!(rule.declarations.len(), 1);
        assert_eq!(rule.declarations[0].property, "color");
    }

    #[test]
    fn test_parse_multiple_declarations() {
        let css = "p { color: blue; font-weight: bold; text-align: center; }";
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.rules.len(), 1);
        assert_eq!(stylesheet.rules[0].declarations.len(), 3);
    }

    #[test]
    fn test_parse_hex_colors() {
        let css = "p { color: #ff0000; background-color: #0f0; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let PropertyValue::Color(c) = &decl.value {
            assert_eq!(*c, Color::rgb(255, 0, 0));
        } else {
            panic!("Expected color");
        }
    }

    #[test]
    fn test_parse_lengths() {
        let css = "p { font-size: 16px; margin: 1em; text-indent: 2rem; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let PropertyValue::Length(Length::Px(v)) = decl.value {
            assert!((v - 16.0).abs() < 0.001);
        } else {
            panic!("Expected px length");
        }
    }

    #[test]
    fn test_specificity_ordering() {
        let spec1 = Specificity {
            ids: 1,
            classes: 0,
            elements: 0,
        };
        let spec2 = Specificity {
            ids: 0,
            classes: 10,
            elements: 0,
        };
        let spec3 = Specificity {
            ids: 0,
            classes: 0,
            elements: 100,
        };

        assert!(spec1 > spec2);
        assert!(spec2 > spec3);
    }

    #[test]
    fn test_important_wins() {
        let css = "p { color: red !important; } p { color: blue; }";
        let stylesheet = Stylesheet::parse(css);

        // The important declaration should have important=true
        let first_decl = &stylesheet.rules[0].declarations[0];
        assert!(first_decl.important);
    }
}
