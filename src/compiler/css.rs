//! CSS parsing and cascade implementation.
//!
//! This module uses a "fat enum" approach where each `Property` variant contains
//! both the property identity and its parsed value. This trades memory efficiency
//! for simpler code - adding a new property requires changes in only two places:
//! the `Property` enum and `Property::parse()`.

use std::cmp::Ordering;

use cssparser::{
    AtRuleParser, DeclarationParser, ParseError, Parser, ParserInput, QualifiedRuleParser,
    RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
use selectors::context::{MatchingContext, SelectorCaches};
use selectors::parser::Selector;

use super::element_ref::{BokoSelectors, ElementRef};
use crate::ir::{
    BorderCollapse, BorderStyle, BoxSizing, BreakValue, Clear, Color, ComputedStyle,
    DecorationStyle, Display, Float, FontStyle, FontWeight, Hyphens, Length, ListStylePosition,
    ListStyleType, OverflowWrap, StylePool, TextAlign, TextTransform, VerticalAlign, Visibility,
    WordBreak,
};

// ============================================================================
// Declaration - A CSS property-value pair
// ============================================================================

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
    FontVariant(crate::ir::FontVariant),

    // Text properties
    TextAlign(TextAlign),
    TextIndent(Length),
    LineHeight(Length),
    LetterSpacing(Length),
    WordSpacing(Length),
    TextTransform(TextTransform),
    Hyphens(Hyphens),
    WhiteSpace(bool), // true = nowrap
    VerticalAlign(VerticalAlignValue),

    // Text decoration
    TextDecoration(TextDecorationValue),
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

/// Vertical alignment value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlignValue {
    Baseline,
    Top,
    Middle,
    Bottom,
    TextTop,
    TextBottom,
    Super,
    Sub,
}

/// Text decoration value (can combine underline and line-through).
#[derive(Debug, Clone, Copy, Default)]
pub struct TextDecorationValue {
    pub underline: bool,
    pub line_through: bool,
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
                [Self::MarginTop(t), Self::MarginRight(r), Self::MarginBottom(b), Self::MarginLeft(l)]
            }),
            "padding" => Self::parse_length_rect(input, |t, r, b, l| {
                [Self::PaddingTop(t), Self::PaddingRight(r), Self::PaddingBottom(b), Self::PaddingLeft(l)]
            }),
            "border-width" => Self::parse_length_rect(input, |t, r, b, l| {
                [Self::BorderTopWidth(t), Self::BorderRightWidth(r), Self::BorderBottomWidth(b), Self::BorderLeftWidth(l)]
            }),
            "border-style" => parse_border_style_shorthand_values(input)
                .map(|(t, r, b, l)| {
                    vec![Self::BorderTopStyle(t), Self::BorderRightStyle(r), Self::BorderBottomStyle(b), Self::BorderLeftStyle(l)]
                })
                .unwrap_or_default(),
            "border-color" => parse_color_shorthand_values(input)
                .map(|(t, r, b, l)| {
                    vec![Self::BorderTopColor(t), Self::BorderRightColor(r), Self::BorderBottomColor(b), Self::BorderLeftColor(l)]
                })
                .unwrap_or_default(),
            "border" => parse_border_shorthand(input),
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

            // Font properties
            "font-family" => parse_font_family(input).map(Self::FontFamily),
            "font-size" => parse_length(input).map(Self::FontSize),
            "font-weight" => parse_font_weight(input).map(Self::FontWeight),
            "font-style" => parse_font_style(input).map(Self::FontStyle),
            "font-variant" | "font-variant-caps" => parse_font_variant(input).map(Self::FontVariant),

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
            "break-inside" | "page-break-inside" => parse_break_inside(input).map(Self::BreakInside),

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

// ============================================================================
// Stylesheet and Rule Structures
// ============================================================================

/// A parsed CSS stylesheet.
#[derive(Debug, Default, Clone)]
pub struct Stylesheet {
    pub rules: Vec<CssRule>,
    /// @font-face rules defining font family to file mappings.
    pub font_faces: Vec<crate::ir::FontFace>,
}

/// A CSS rule with selectors and declarations.
///
/// Declarations are separated into normal and important vectors,
/// following the lightningcss pattern for memory efficiency.
#[derive(Debug, Clone)]
pub struct CssRule {
    pub selectors: Vec<Selector<BokoSelectors>>,
    /// Normal (non-important) declarations.
    pub declarations: Vec<Declaration>,
    /// Important declarations (those with !important).
    pub important_declarations: Vec<Declaration>,
    pub specificity: Specificity,
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
struct MatchedRule<'a> {
    declaration: &'a Declaration,
    origin: Origin,
    specificity: Specificity,
    order: usize,
    important: bool,
}

impl Stylesheet {
    /// Parse a CSS stylesheet from a string.
    pub fn parse(css: &str) -> Self {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let mut rules = Vec::new();
        let mut font_faces = Vec::new();

        let mut rule_parser = TopLevelRuleParser {
            rules: &mut rules,
            font_faces: &mut font_faces,
        };
        let stylesheet_parser = StyleSheetParser::new(&mut parser, &mut rule_parser);

        for result in stylesheet_parser {
            // Ignore errors - lenient parsing
            let _ = result;
        }

        Self { rules, font_faces }
    }

    /// Check if the stylesheet is empty.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Parser for top-level stylesheet rules.
struct TopLevelRuleParser<'a> {
    rules: &'a mut Vec<CssRule>,
    font_faces: &'a mut Vec<crate::ir::FontFace>,
}

/// Prelude for @font-face rules (empty, just a marker).
struct FontFacePrelude;

impl<'i> AtRuleParser<'i> for TopLevelRuleParser<'_> {
    type Prelude = FontFacePrelude;
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: cssparser::CowRcStr<'i>,
        _input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("font-face") {
            // @font-face has no prelude, just a block
            Ok(FontFacePrelude)
        } else {
            // Skip other at-rules
            Err(_input.new_custom_error(()))
        }
    }

    fn parse_block<'t>(
        &mut self,
        _prelude: Self::Prelude,
        _start: &cssparser::ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
        // Parse @font-face declarations
        if let Some(font_face) = parse_font_face_block(input) {
            self.font_faces.push(font_face);
        }
        Ok(())
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
        let mut important_declarations = Vec::new();
        let mut decl_parser = DeclarationListParser {
            declarations: &mut declarations,
            important_declarations: &mut important_declarations,
        };

        for result in RuleBodyParser::new(input, &mut decl_parser) {
            // Ignore errors - lenient parsing
            let _ = result;
        }

        self.rules.push(CssRule {
            selectors: prelude,
            declarations,
            important_declarations,
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
    important_declarations: &'a mut Vec<Declaration>,
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
        let decls = Declaration::parse(&name, input);
        if !decls.is_empty() {
            let important = input.try_parse(cssparser::parse_important).is_ok();
            let target = if important {
                &mut *self.important_declarations
            } else {
                &mut *self.declarations
            };
            target.extend(decls);
        }
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

// ============================================================================
// Value Parsing Functions
// ============================================================================

fn parse_color(input: &mut Parser<'_, '_>) -> Option<Color> {
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

fn parse_length(input: &mut Parser<'_, '_>) -> Option<Length> {
    match input.next().ok()? {
        Token::Dimension { value, unit, .. } => {
            let length = match unit.as_ref() {
                "px" => Length::Px(*value),
                "em" => Length::Em(*value),
                "rem" => Length::Rem(*value),
                "%" => Length::Percent(*value),
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

/// Parse line-height value (handles unitless numbers and "normal" keyword).
fn parse_line_height(input: &mut Parser<'_, '_>) -> Option<Length> {
    match input.next().ok()? {
        Token::Dimension { value, unit, .. } => {
            let length = match unit.as_ref() {
                "px" => Length::Px(*value),
                "em" => Length::Em(*value),
                "rem" => Length::Rem(*value),
                "%" => Length::Percent(*value),
                _ => return None,
            };
            Some(length)
        }
        Token::Percentage { unit_value, .. } => Some(Length::Percent(*unit_value * 100.0)),
        // Unitless number becomes em multiplier
        Token::Number { value, .. } => Some(Length::Em(*value)),
        Token::Ident(ident) => match ident.as_ref() {
            "normal" => Some(Length::Auto),
            _ => None,
        },
        _ => None,
    }
}

/// Parse margin/padding shorthand with 1-4 values.
/// Returns (top, right, bottom, left) following CSS box model rules.
fn parse_box_shorthand_values(
    input: &mut Parser<'_, '_>,
) -> Option<(Length, Length, Length, Length)> {
    let mut values = Vec::with_capacity(4);

    // Parse up to 4 length values
    while values.len() < 4 {
        if let Some(len) = parse_length(input) {
            values.push(len);
        } else {
            break;
        }
    }

    // Expand according to CSS shorthand rules:
    // 1 value: all sides
    // 2 values: top/bottom, left/right
    // 3 values: top, left/right, bottom
    // 4 values: top, right, bottom, left
    expand_shorthand_4(values)
}

/// Parse border-style shorthand with 1-4 values.
/// Returns (top, right, bottom, left) following CSS box model rules.
fn parse_border_style_shorthand_values(
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
fn parse_color_shorthand_values(
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

/// Expand 1-4 values to (top, right, bottom, left) following CSS shorthand rules.
fn expand_shorthand_4<T: Copy>(values: Vec<T>) -> Option<(T, T, T, T)> {
    match values.len() {
        1 => {
            let v = values[0];
            Some((v, v, v, v))
        }
        2 => {
            let (tb, lr) = (values[0], values[1]);
            Some((tb, lr, tb, lr))
        }
        3 => {
            let (t, lr, b) = (values[0], values[1], values[2]);
            Some((t, lr, b, lr))
        }
        4 => Some((values[0], values[1], values[2], values[3])),
        _ => None,
    }
}

/// Parse a single border-style value (used by shorthand parser).
fn parse_border_style_value(input: &mut Parser<'_, '_>) -> Option<BorderStyle> {
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

/// Parse a single border-width value (length or keyword).
fn parse_border_width_value(input: &mut Parser<'_, '_>) -> Option<Length> {
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

/// Parse combined border shorthand (e.g., `border: 1px solid red`).
/// Order-insensitive parsing of width, style, and color.
fn parse_border_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let mut width: Option<Length> = None;
    let mut style: Option<BorderStyle> = None;
    let mut color: Option<Color> = None;

    // Parse up to 3 values in any order
    for _ in 0..3 {
        // Try border-style first (keywords)
        if style.is_none() {
            if let Ok(s) = input.try_parse(|i| {
                parse_border_style_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            }) {
                style = Some(s);
                continue;
            }
        }

        // Try color (keywords or hex/rgb)
        if color.is_none() {
            if let Ok(c) = input
                .try_parse(|i| parse_color(i).ok_or_else(|| i.new_custom_error::<_, ()>(())))
            {
                color = Some(c);
                continue;
            }
        }

        // Try width (length values)
        if width.is_none() {
            if let Ok(w) = input.try_parse(|i| {
                parse_border_width_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            }) {
                width = Some(w);
                continue;
            }
        }

        // No more values
        break;
    }

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

fn parse_font_weight(input: &mut Parser<'_, '_>) -> Option<FontWeight> {
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        let weight = match token.as_ref() {
            "normal" => FontWeight::NORMAL,
            "bold" => FontWeight::BOLD,
            "lighter" => FontWeight(300),
            "bolder" => FontWeight(700),
            _ => return None,
        };
        return Some(weight);
    }

    if let Ok(Token::Number {
        int_value: Some(v), ..
    }) = input.next()
    {
        let v = *v;
        if (100..=900).contains(&v) && v % 100 == 0 {
            return Some(FontWeight(v as u16));
        }
    }

    None
}

fn parse_font_style(input: &mut Parser<'_, '_>) -> Option<FontStyle> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "normal" => Some(FontStyle::Normal),
        "italic" => Some(FontStyle::Italic),
        "oblique" => Some(FontStyle::Oblique),
        _ => None,
    }
}

fn parse_text_align(input: &mut Parser<'_, '_>) -> Option<TextAlign> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "left" => Some(TextAlign::Left),
        "right" => Some(TextAlign::Right),
        "center" => Some(TextAlign::Center),
        "justify" => Some(TextAlign::Justify),
        "start" => Some(TextAlign::Start),
        "end" => Some(TextAlign::End),
        _ => None,
    }
}

fn parse_display(input: &mut Parser<'_, '_>) -> Option<Display> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "block" => Some(Display::Block),
        "inline" => Some(Display::Inline),
        "inline-block" => Some(Display::InlineBlock),
        "none" => Some(Display::None),
        "list-item" => Some(Display::ListItem),
        "table-cell" => Some(Display::TableCell),
        "table-row" => Some(Display::TableRow),
        _ => None,
    }
}

fn parse_font_family(input: &mut Parser<'_, '_>) -> Option<String> {
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
        Some(families.join(", "))
    }
}

fn parse_text_decoration(input: &mut Parser<'_, '_>) -> Option<TextDecorationValue> {
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

fn parse_vertical_align(input: &mut Parser<'_, '_>) -> Option<VerticalAlignValue> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "baseline" => Some(VerticalAlignValue::Baseline),
        "top" => Some(VerticalAlignValue::Top),
        "middle" => Some(VerticalAlignValue::Middle),
        "bottom" => Some(VerticalAlignValue::Bottom),
        "text-top" => Some(VerticalAlignValue::TextTop),
        "text-bottom" => Some(VerticalAlignValue::TextBottom),
        "super" => Some(VerticalAlignValue::Super),
        "sub" => Some(VerticalAlignValue::Sub),
        _ => None,
    }
}

fn parse_list_style_type(input: &mut Parser<'_, '_>) -> Option<ListStyleType> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
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
    }
}

fn parse_font_variant(input: &mut Parser<'_, '_>) -> Option<crate::ir::FontVariant> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "small-caps" => Some(crate::ir::FontVariant::SmallCaps),
        "normal" | "none" => Some(crate::ir::FontVariant::Normal),
        _ => None,
    }
}

fn parse_text_transform(input: &mut Parser<'_, '_>) -> Option<TextTransform> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "none" => Some(TextTransform::None),
        "uppercase" => Some(TextTransform::Uppercase),
        "lowercase" => Some(TextTransform::Lowercase),
        "capitalize" => Some(TextTransform::Capitalize),
        _ => None,
    }
}

fn parse_hyphens(input: &mut Parser<'_, '_>) -> Option<Hyphens> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "auto" => Some(Hyphens::Auto),
        "manual" => Some(Hyphens::Manual),
        "none" => Some(Hyphens::None),
        _ => None,
    }
}

fn parse_white_space(input: &mut Parser<'_, '_>) -> Option<bool> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "nowrap" | "pre" => Some(true),
        "normal" | "pre-wrap" | "pre-line" => Some(false),
        _ => None,
    }
}

fn parse_decoration_style(input: &mut Parser<'_, '_>) -> Option<DecorationStyle> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "solid" => Some(DecorationStyle::Solid),
        "dotted" => Some(DecorationStyle::Dotted),
        "dashed" => Some(DecorationStyle::Dashed),
        "double" => Some(DecorationStyle::Double),
        "none" => Some(DecorationStyle::None),
        _ => None,
    }
}

fn parse_float(input: &mut Parser<'_, '_>) -> Option<Float> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "left" => Some(Float::Left),
        "right" => Some(Float::Right),
        "none" => Some(Float::None),
        _ => None,
    }
}

fn parse_clear(input: &mut Parser<'_, '_>) -> Option<Clear> {
    let token = input.expect_ident_cloned().ok()?;
    Clear::from_css(&token)
}

fn parse_integer(input: &mut Parser<'_, '_>) -> Option<u32> {
    if let Ok(Token::Number {
        int_value: Some(v), ..
    }) = input.next().cloned()
        && v >= 0
    {
        return Some(v as u32);
    }
    None
}

fn parse_word_break(input: &mut Parser<'_, '_>) -> Option<WordBreak> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "normal" => Some(WordBreak::Normal),
        "break-all" => Some(WordBreak::BreakAll),
        "keep-all" => Some(WordBreak::KeepAll),
        "break-word" => Some(WordBreak::BreakWord),
        _ => None,
    }
}

fn parse_overflow_wrap(input: &mut Parser<'_, '_>) -> Option<OverflowWrap> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "normal" => Some(OverflowWrap::Normal),
        "break-word" => Some(OverflowWrap::BreakWord),
        "anywhere" => Some(OverflowWrap::Anywhere),
        _ => None,
    }
}

fn parse_break_value(input: &mut Parser<'_, '_>) -> Option<BreakValue> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "auto" => Some(BreakValue::Auto),
        "always" | "page" | "left" | "right" | "recto" | "verso" => Some(BreakValue::Always),
        "avoid" | "avoid-page" => Some(BreakValue::Avoid),
        "column" | "avoid-column" => Some(BreakValue::Column),
        _ => None,
    }
}

fn parse_break_inside(input: &mut Parser<'_, '_>) -> Option<BreakValue> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "auto" => Some(BreakValue::Auto),
        "avoid" | "avoid-page" | "avoid-column" => Some(BreakValue::Avoid),
        _ => None,
    }
}

fn parse_list_style_position(input: &mut Parser<'_, '_>) -> Option<ListStylePosition> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "inside" => Some(ListStylePosition::Inside),
        "outside" => Some(ListStylePosition::Outside),
        _ => None,
    }
}

fn parse_border_collapse(input: &mut Parser<'_, '_>) -> Option<BorderCollapse> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "collapse" => Some(BorderCollapse::Collapse),
        "separate" => Some(BorderCollapse::Separate),
        _ => None,
    }
}

fn parse_visibility(input: &mut Parser<'_, '_>) -> Option<Visibility> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "visible" => Some(Visibility::Visible),
        "hidden" => Some(Visibility::Hidden),
        "collapse" => Some(Visibility::Collapse),
        _ => None,
    }
}

fn parse_box_sizing(input: &mut Parser<'_, '_>) -> Option<BoxSizing> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "content-box" => Some(BoxSizing::ContentBox),
        "border-box" => Some(BoxSizing::BorderBox),
        _ => None,
    }
}

// ============================================================================
// @font-face Parsing
// ============================================================================

/// Parse a @font-face block and return a FontFace if successful.
///
/// @font-face rules have the form:
/// ```css
/// @font-face {
///     font-family: "Ubuntu";
///     font-weight: bold;
///     font-style: normal;
///     src: url(../fonts/Ubuntu-B.ttf);
/// }
/// ```
fn parse_font_face_block(input: &mut Parser<'_, '_>) -> Option<crate::ir::FontFace> {
    let mut font_family: Option<String> = None;
    let mut font_weight = FontWeight::NORMAL;
    let mut font_style = FontStyle::Normal;
    let mut src: Option<String> = None;

    // Parse declarations within the @font-face block
    loop {
        // Try to parse a declaration
        if let Ok(name) = input.expect_ident_cloned() {
            let name_str = name.as_ref();
            if input.expect_colon().is_ok() {
                match name_str {
                    "font-family" => {
                        font_family = parse_font_face_family(input);
                    }
                    "font-weight" => {
                        if let Some(w) = parse_font_weight(input) {
                            font_weight = w;
                        }
                    }
                    "font-style" => {
                        if let Some(s) = parse_font_style(input) {
                            font_style = s;
                        }
                    }
                    "src" => {
                        src = parse_font_face_src(input);
                    }
                    _ => {
                        // Skip unknown properties
                        while input.next().is_ok() {
                            // Consume until we hit a semicolon or end of block
                            if matches!(input.current_source_location().line, _) {
                                break;
                            }
                        }
                    }
                }
                // Consume semicolon if present
                let _ = input.try_parse(|i| i.expect_semicolon());
            }
        } else {
            // No more declarations
            break;
        }
    }

    // Require both font-family and src
    match (font_family, src) {
        (Some(family), Some(source)) => {
            Some(crate::ir::FontFace::new(family, font_weight, font_style, source))
        }
        _ => None,
    }
}

/// Parse font-family value in @font-face (quoted or unquoted name).
fn parse_font_face_family(input: &mut Parser<'_, '_>) -> Option<String> {
    // Try quoted string first
    if let Ok(s) = input.try_parse(|i| i.expect_string_cloned()) {
        return Some(s.to_string());
    }
    // Try unquoted identifier
    if let Ok(s) = input.try_parse(|i| i.expect_ident_cloned()) {
        return Some(s.to_string());
    }
    None
}

/// Parse src value in @font-face: url(...) or local(...).
fn parse_font_face_src(input: &mut Parser<'_, '_>) -> Option<String> {
    // We support url() format only
    if let Ok(url) = input.try_parse(|i| i.expect_url_or_string()) {
        return Some(url.as_ref().to_string());
    }
    // Try parsing url() function with string argument
    if let Ok(url) = input.try_parse(|i| -> Result<String, ParseError<'_, ()>> {
        i.expect_function_matching("url")?;
        let url_str = i.parse_nested_block(|nested| {
            nested
                .expect_string_cloned()
                .map(|s| s.to_string())
                .map_err(|e| e.into())
        })?;
        Ok(url_str)
    }) {
        return Some(url);
    }
    None
}

/// Create a new style with only CSS-inherited properties from parent.
///
/// CSS inherited properties include:
/// - color, font-*, line-height, text-align, text-indent
/// - letter-spacing, word-spacing, hyphens, text-transform
/// - list-style-*, visibility
///
/// Non-inherited properties (width, height, margin, padding, display, etc.)
/// are NOT copied from the parent.
fn inherit_from_parent(parent: &ComputedStyle) -> ComputedStyle {
    ComputedStyle {
        // Font properties (inherited)
        font_size: parent.font_size,
        font_weight: parent.font_weight,
        font_style: parent.font_style,
        font_variant: parent.font_variant,
        font_family: parent.font_family.clone(),
        // Text properties (inherited)
        color: parent.color,
        text_align: parent.text_align,
        text_indent: parent.text_indent,
        line_height: parent.line_height,
        letter_spacing: parent.letter_spacing,
        word_spacing: parent.word_spacing,
        text_transform: parent.text_transform,
        hyphens: parent.hyphens,
        // Text decoration (inherited in some contexts)
        text_decoration_underline: parent.text_decoration_underline,
        text_decoration_line_through: parent.text_decoration_line_through,
        underline_style: parent.underline_style,
        underline_color: parent.underline_color,
        overline: parent.overline,
        // List properties (inherited, but only apply to display:list-item)
        list_style_type: parent.list_style_type,
        list_style_position: parent.list_style_position,
        // Other inherited properties
        visibility: parent.visibility,
        language: parent.language.clone(),
        // Non-inherited properties use defaults
        ..ComputedStyle::default()
    }
}

/// Compute styles for an element by applying the cascade.
pub fn compute_styles(
    elem: ElementRef<'_>,
    stylesheets: &[(Stylesheet, Origin)],
    parent_style: Option<&ComputedStyle>,
    _style_pool: &mut StylePool,
) -> ComputedStyle {
    // Pre-allocate with typical capacity (most elements match 5-20 declarations)
    let mut matched: Vec<MatchedRule> = Vec::with_capacity(16);
    let mut order = 0;

    // Reuse selector caches across all rule matching for this element
    let mut caches = SelectorCaches::default();

    for (stylesheet, origin) in stylesheets {
        for rule in &stylesheet.rules {
            if rule_matches_with_caches(elem, rule, &mut caches) {
                // Collect normal declarations
                for decl in &rule.declarations {
                    matched.push(MatchedRule {
                        declaration: decl,
                        origin: *origin,
                        specificity: rule.specificity,
                        order,
                        important: false,
                    });
                    order += 1;
                }
                // Collect important declarations
                for decl in &rule.important_declarations {
                    matched.push(MatchedRule {
                        declaration: decl,
                        origin: *origin,
                        specificity: rule.specificity,
                        order,
                        important: true,
                    });
                    order += 1;
                }
            }
        }
    }

    // Sort by cascade order (skip if 0-1 matches)
    if matched.len() > 1 {
        // Use unstable sort - faster and order of equal elements doesn't matter
        matched.sort_unstable_by(|a, b| {
            // Important declarations win
            if a.important != b.important {
                return b.important.cmp(&a.important);
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
    }

    // Start with inherited values from parent (only CSS-inherited properties)
    let mut style = if let Some(parent) = parent_style {
        inherit_from_parent(parent)
    } else {
        ComputedStyle::default()
    };

    // Apply matched declarations in cascade order
    for matched_rule in &matched {
        apply_declaration(&mut style, matched_rule.declaration);
    }

    style
}

/// Check if a rule matches an element (with shared caches for better performance).
fn rule_matches_with_caches(
    elem: ElementRef<'_>,
    rule: &CssRule,
    caches: &mut SelectorCaches,
) -> bool {
    let mut context = MatchingContext::new(
        selectors::matching::MatchingMode::Normal,
        None,
        caches,
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
    match decl {
        // Colors
        Declaration::Color(c) => style.color = Some(*c),
        Declaration::BackgroundColor(c) => style.background_color = Some(*c),

        // Font properties
        Declaration::FontFamily(s) => style.font_family = Some(s.clone()),
        Declaration::FontSize(l) => style.font_size = *l,
        Declaration::FontWeight(w) => style.font_weight = *w,
        Declaration::FontStyle(s) => style.font_style = *s,
        Declaration::FontVariant(v) => style.font_variant = *v,

        // Text properties
        Declaration::TextAlign(a) => style.text_align = *a,
        Declaration::TextIndent(l) => style.text_indent = *l,
        Declaration::LineHeight(l) => style.line_height = *l,
        Declaration::LetterSpacing(l) => style.letter_spacing = *l,
        Declaration::WordSpacing(l) => style.word_spacing = *l,
        Declaration::TextTransform(t) => style.text_transform = *t,
        Declaration::Hyphens(h) => style.hyphens = *h,
        Declaration::WhiteSpace(nowrap) => style.no_break = *nowrap,
        Declaration::VerticalAlign(v) => {
            style.vertical_align = match v {
                VerticalAlignValue::Baseline => VerticalAlign::Baseline,
                VerticalAlignValue::Top => VerticalAlign::Top,
                VerticalAlignValue::Middle => VerticalAlign::Middle,
                VerticalAlignValue::Bottom => VerticalAlign::Bottom,
                VerticalAlignValue::TextTop => VerticalAlign::TextTop,
                VerticalAlignValue::TextBottom => VerticalAlign::TextBottom,
                VerticalAlignValue::Super => VerticalAlign::Super,
                VerticalAlignValue::Sub => VerticalAlign::Sub,
            };
        }

        // Text decoration
        Declaration::TextDecoration(d) => {
            style.text_decoration_underline = d.underline;
            style.text_decoration_line_through = d.line_through;
        }
        Declaration::TextDecorationStyle(s) => style.underline_style = *s,
        Declaration::TextDecorationColor(c) => style.underline_color = Some(*c),

        // Margins
        Declaration::Margin(l) => {
            style.margin_top = *l;
            style.margin_right = *l;
            style.margin_bottom = *l;
            style.margin_left = *l;
        }
        Declaration::MarginTop(l) => style.margin_top = *l,
        Declaration::MarginRight(l) => style.margin_right = *l,
        Declaration::MarginBottom(l) => style.margin_bottom = *l,
        Declaration::MarginLeft(l) => style.margin_left = *l,

        // Padding
        Declaration::Padding(l) => {
            style.padding_top = *l;
            style.padding_right = *l;
            style.padding_bottom = *l;
            style.padding_left = *l;
        }
        Declaration::PaddingTop(l) => style.padding_top = *l,
        Declaration::PaddingRight(l) => style.padding_right = *l,
        Declaration::PaddingBottom(l) => style.padding_bottom = *l,
        Declaration::PaddingLeft(l) => style.padding_left = *l,

        // Dimensions
        Declaration::Width(l) => style.width = *l,
        Declaration::Height(l) => style.height = *l,
        Declaration::MaxWidth(l) => style.max_width = *l,
        Declaration::MaxHeight(l) => style.max_height = *l,
        Declaration::MinWidth(l) => style.min_width = *l,
        Declaration::MinHeight(l) => style.min_height = *l,

        // Display & positioning
        Declaration::Display(d) => style.display = *d,
        Declaration::Float(f) => style.float = *f,
        Declaration::Clear(c) => style.clear = *c,
        Declaration::Visibility(v) => style.visibility = *v,
        Declaration::BoxSizing(bs) => style.box_sizing = *bs,

        // Pagination control
        Declaration::Orphans(n) => style.orphans = *n,
        Declaration::Widows(n) => style.widows = *n,

        // Text wrapping
        Declaration::WordBreak(wb) => style.word_break = *wb,
        Declaration::OverflowWrap(ow) => style.overflow_wrap = *ow,

        // Page breaks
        Declaration::BreakBefore(b) => style.break_before = *b,
        Declaration::BreakAfter(b) => style.break_after = *b,
        Declaration::BreakInside(b) => style.break_inside = *b,

        // Border style
        Declaration::BorderStyle(s) => {
            style.border_style_top = *s;
            style.border_style_right = *s;
            style.border_style_bottom = *s;
            style.border_style_left = *s;
        }
        Declaration::BorderTopStyle(s) => style.border_style_top = *s,
        Declaration::BorderRightStyle(s) => style.border_style_right = *s,
        Declaration::BorderBottomStyle(s) => style.border_style_bottom = *s,
        Declaration::BorderLeftStyle(s) => style.border_style_left = *s,

        // Border width
        Declaration::BorderWidth(l) => {
            style.border_width_top = *l;
            style.border_width_right = *l;
            style.border_width_bottom = *l;
            style.border_width_left = *l;
        }
        Declaration::BorderTopWidth(l) => style.border_width_top = *l,
        Declaration::BorderRightWidth(l) => style.border_width_right = *l,
        Declaration::BorderBottomWidth(l) => style.border_width_bottom = *l,
        Declaration::BorderLeftWidth(l) => style.border_width_left = *l,

        // Border color
        Declaration::BorderColor(c) => {
            style.border_color_top = Some(*c);
            style.border_color_right = Some(*c);
            style.border_color_bottom = Some(*c);
            style.border_color_left = Some(*c);
        }
        Declaration::BorderTopColor(c) => style.border_color_top = Some(*c),
        Declaration::BorderRightColor(c) => style.border_color_right = Some(*c),
        Declaration::BorderBottomColor(c) => style.border_color_bottom = Some(*c),
        Declaration::BorderLeftColor(c) => style.border_color_left = Some(*c),

        // Border radius
        Declaration::BorderRadius(l) => {
            style.border_radius_top_left = *l;
            style.border_radius_top_right = *l;
            style.border_radius_bottom_left = *l;
            style.border_radius_bottom_right = *l;
        }
        Declaration::BorderTopLeftRadius(l) => style.border_radius_top_left = *l,
        Declaration::BorderTopRightRadius(l) => style.border_radius_top_right = *l,
        Declaration::BorderBottomLeftRadius(l) => style.border_radius_bottom_left = *l,
        Declaration::BorderBottomRightRadius(l) => style.border_radius_bottom_right = *l,

        // List properties
        Declaration::ListStyleType(lst) => style.list_style_type = *lst,
        Declaration::ListStylePosition(p) => style.list_style_position = *p,

        // Table properties
        Declaration::BorderCollapse(bc) => style.border_collapse = *bc,
        Declaration::BorderSpacing(l) => style.border_spacing = *l,
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
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
        assert!(matches!(rule.declarations[0], Declaration::Color(_)));
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
        if let Declaration::Color(c) = decl {
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
        if let Declaration::FontSize(Length::Px(v)) = decl {
            assert!((*v - 16.0).abs() < 0.001);
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

        // The important declaration should be in important_declarations
        assert_eq!(stylesheet.rules[0].important_declarations.len(), 1);
        assert!(matches!(
            stylesheet.rules[0].important_declarations[0],
            Declaration::Color(_)
        ));
        // The normal declaration should be in declarations
        assert_eq!(stylesheet.rules[1].declarations.len(), 1);
    }

    #[test]
    fn test_inherit_from_parent_only_inherited_properties() {
        use crate::ir::Length;

        // Create a parent style with both inherited and non-inherited properties
        let mut parent = ComputedStyle::default();
        parent.color = Some(Color::rgb(255, 0, 0)); // inherited
        parent.font_size = Length::Px(20.0); // inherited
        parent.text_align = TextAlign::Center; // inherited
        parent.width = Length::Percent(75.0); // NOT inherited
        parent.margin_top = Length::Em(2.0); // NOT inherited
        parent.display = Display::Block; // NOT inherited

        // Inherit from parent
        let child = inherit_from_parent(&parent);

        // Inherited properties should be copied
        assert_eq!(child.color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(child.font_size, Length::Px(20.0));
        assert_eq!(child.text_align, TextAlign::Center);

        // Non-inherited properties should be at default values
        let default = ComputedStyle::default();
        assert_eq!(child.width, default.width, "width should not be inherited");
        assert_eq!(
            child.margin_top, default.margin_top,
            "margin-top should not be inherited"
        );
        assert_eq!(
            child.display, default.display,
            "display should not be inherited"
        );
    }

    #[test]
    fn test_margin_shorthand_expansion() {
        // Test margin: 0 auto (common centering pattern)
        let css = "p { margin: 0 auto; }";
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.rules.len(), 1);
        let decls = &stylesheet.rules[0].declarations;
        // Should expand to 4 declarations
        assert_eq!(decls.len(), 4);

        // Find margin-left and margin-right
        let margin_left = decls
            .iter()
            .find(|d| matches!(d, Declaration::MarginLeft(_)));
        let margin_right = decls
            .iter()
            .find(|d| matches!(d, Declaration::MarginRight(_)));
        let margin_top = decls
            .iter()
            .find(|d| matches!(d, Declaration::MarginTop(_)));

        assert!(margin_left.is_some(), "margin-left should exist");
        assert!(margin_right.is_some(), "margin-right should exist");
        assert!(margin_top.is_some(), "margin-top should exist");

        // Verify auto values for left/right
        if let Declaration::MarginLeft(len) = margin_left.unwrap() {
            assert_eq!(*len, Length::Auto, "margin-left should be auto");
        } else {
            panic!("margin-left should be a length");
        }

        if let Declaration::MarginRight(len) = margin_right.unwrap() {
            assert_eq!(*len, Length::Auto, "margin-right should be auto");
        } else {
            panic!("margin-right should be a length");
        }

        // Verify 0 for top/bottom
        if let Declaration::MarginTop(len) = margin_top.unwrap() {
            assert_eq!(*len, Length::Px(0.0), "margin-top should be 0");
        } else {
            panic!("margin-top should be a length");
        }
    }

    #[test]
    fn test_line_height_unitless_number() {
        // CSS line-height can be a unitless number (multiplier)
        let css = "p { line-height: 1.5; }";
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.rules.len(), 1);
        let decl = &stylesheet.rules[0].declarations[0];

        // Unitless 1.5 should be converted to 1.5em
        if let Declaration::LineHeight(len) = decl {
            assert_eq!(
                *len,
                Length::Em(1.5),
                "unitless line-height should become em"
            );
        } else {
            panic!("line-height should be a length");
        }
    }

    #[test]
    fn test_line_height_with_unit() {
        // line-height with explicit unit
        let css = "p { line-height: 24px; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let Declaration::LineHeight(len) = decl {
            assert_eq!(*len, Length::Px(24.0));
        } else {
            panic!("line-height should be a length");
        }
    }

    #[test]
    fn test_line_height_normal() {
        // line-height: normal should become Auto
        let css = "p { line-height: normal; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let Declaration::LineHeight(len) = decl {
            assert_eq!(*len, Length::Auto, "line-height: normal should be Auto");
        } else {
            panic!("line-height should be a length");
        }
    }

    #[test]
    fn test_box_sizing_parsing() {
        let css = "div { box-sizing: border-box; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let Declaration::BoxSizing(bs) = decl {
            assert_eq!(*bs, BoxSizing::BorderBox);
        } else {
            panic!("box-sizing should be a BoxSizing value");
        }
    }

    #[test]
    fn test_box_sizing_content_box() {
        let css = "div { box-sizing: content-box; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let Declaration::BoxSizing(bs) = decl {
            assert_eq!(*bs, BoxSizing::ContentBox);
        } else {
            panic!("box-sizing should be a BoxSizing value");
        }
    }

    // ========================================================================
    // Tests for new CSS properties
    // ========================================================================

    #[test]
    fn test_max_height() {
        let css = "img { max-height: 100%; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let Declaration::MaxHeight(len) = decl {
            assert_eq!(*len, Length::Percent(100.0));
        } else {
            panic!("max-height should be a Length");
        }
    }

    #[test]
    fn test_min_width() {
        let css = "div { min-width: 200px; }";
        let stylesheet = Stylesheet::parse(css);

        let decl = &stylesheet.rules[0].declarations[0];
        if let Declaration::MinWidth(len) = decl {
            assert_eq!(*len, Length::Px(200.0));
        } else {
            panic!("min-width should be a Length");
        }
    }

    #[test]
    fn test_clear() {
        use crate::ir::Clear;

        for (css_value, expected) in [
            ("none", Clear::None),
            ("left", Clear::Left),
            ("right", Clear::Right),
            ("both", Clear::Both),
        ] {
            let css = format!("div {{ clear: {}; }}", css_value);
            let stylesheet = Stylesheet::parse(&css);

            let decl = &stylesheet.rules[0].declarations[0];
            if let Declaration::Clear(clear) = decl {
                assert_eq!(
                    *clear, expected,
                    "clear: {} should parse correctly",
                    css_value
                );
            } else {
                panic!("clear should be a Clear value");
            }
        }
    }

    #[test]
    fn test_orphans_widows() {
        let css = "p { orphans: 3; widows: 2; }";
        let stylesheet = Stylesheet::parse(css);

        let orphans_decl = stylesheet.rules[0]
            .declarations
            .iter()
            .find(|d| matches!(d, Declaration::Orphans(_)))
            .expect("orphans should exist");
        let widows_decl = stylesheet.rules[0]
            .declarations
            .iter()
            .find(|d| matches!(d, Declaration::Widows(_)))
            .expect("widows should exist");

        if let Declaration::Orphans(n) = orphans_decl {
            assert_eq!(*n, 3);
        } else {
            panic!("orphans should be an Integer");
        }

        if let Declaration::Widows(n) = widows_decl {
            assert_eq!(*n, 2);
        } else {
            panic!("widows should be an Integer");
        }
    }

    #[test]
    fn test_word_break() {
        use crate::ir::WordBreak;

        for (css_value, expected) in [
            ("normal", WordBreak::Normal),
            ("break-all", WordBreak::BreakAll),
            ("keep-all", WordBreak::KeepAll),
            ("break-word", WordBreak::BreakWord),
        ] {
            let css = format!("p {{ word-break: {}; }}", css_value);
            let stylesheet = Stylesheet::parse(&css);

            let decl = &stylesheet.rules[0].declarations[0];
            if let Declaration::WordBreak(wb) = decl {
                assert_eq!(
                    *wb, expected,
                    "word-break: {} should parse correctly",
                    css_value
                );
            } else {
                panic!("word-break should be a WordBreak value");
            }
        }
    }

    #[test]
    fn test_overflow_wrap() {
        use crate::ir::OverflowWrap;

        for (css_value, expected) in [
            ("normal", OverflowWrap::Normal),
            ("break-word", OverflowWrap::BreakWord),
            ("anywhere", OverflowWrap::Anywhere),
        ] {
            let css = format!("p {{ overflow-wrap: {}; }}", css_value);
            let stylesheet = Stylesheet::parse(&css);

            let decl = &stylesheet.rules[0].declarations[0];
            if let Declaration::OverflowWrap(ow) = decl {
                assert_eq!(
                    *ow, expected,
                    "overflow-wrap: {} should parse correctly",
                    css_value
                );
            } else {
                panic!("overflow-wrap should be an OverflowWrap value");
            }
        }
    }

    // ========================================================================
    // @font-face Tests
    // ========================================================================

    #[test]
    fn test_font_face_basic() {
        let css = r#"
            @font-face {
                font-family: "Ubuntu";
                font-weight: normal;
                font-style: normal;
                src: url(../fonts/Ubuntu-M.ttf);
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.font_faces.len(), 1);
        let ff = &stylesheet.font_faces[0];
        assert_eq!(ff.font_family, "Ubuntu");
        assert_eq!(ff.font_weight, FontWeight::NORMAL);
        assert_eq!(ff.font_style, FontStyle::Normal);
        assert_eq!(ff.src, "../fonts/Ubuntu-M.ttf");
    }

    #[test]
    fn test_font_face_bold_italic() {
        let css = r#"
            @font-face {
                font-family: Ubuntu;
                font-weight: bold;
                font-style: italic;
                src: url(../fonts/Ubuntu-BI.ttf);
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.font_faces.len(), 1);
        let ff = &stylesheet.font_faces[0];
        assert_eq!(ff.font_family, "Ubuntu");
        assert_eq!(ff.font_weight, FontWeight::BOLD);
        assert_eq!(ff.font_style, FontStyle::Italic);
        assert_eq!(ff.src, "../fonts/Ubuntu-BI.ttf");
    }

    #[test]
    fn test_font_face_multiple() {
        let css = r#"
            @font-face {
                font-family: "Ubuntu";
                src: url(fonts/Ubuntu-M.ttf);
            }
            @font-face {
                font-family: "UbuntuMono";
                src: url(fonts/UbuntuMono-R.ttf);
            }
            p { color: red; }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // Should have 2 font-faces and 1 rule
        assert_eq!(stylesheet.font_faces.len(), 2);
        assert_eq!(stylesheet.rules.len(), 1);

        assert_eq!(stylesheet.font_faces[0].font_family, "Ubuntu");
        assert_eq!(stylesheet.font_faces[1].font_family, "UbuntuMono");
    }

    #[test]
    fn test_font_face_defaults() {
        // Font-face with only font-family and src (weight/style default to normal)
        let css = r#"
            @font-face {
                font-family: "MyFont";
                src: url(myfont.ttf);
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.font_faces.len(), 1);
        let ff = &stylesheet.font_faces[0];
        assert_eq!(ff.font_family, "MyFont");
        assert_eq!(ff.font_weight, FontWeight::NORMAL);
        assert_eq!(ff.font_style, FontStyle::Normal);
    }

    // ========================================================================
    // vertical-align Tests
    // ========================================================================

    #[test]
    fn test_vertical_align_values() {
        for (css_value, expected) in [
            ("baseline", VerticalAlign::Baseline),
            ("top", VerticalAlign::Top),
            ("middle", VerticalAlign::Middle),
            ("bottom", VerticalAlign::Bottom),
            ("text-top", VerticalAlign::TextTop),
            ("text-bottom", VerticalAlign::TextBottom),
            ("super", VerticalAlign::Super),
            ("sub", VerticalAlign::Sub),
        ] {
            let css = format!("td {{ vertical-align: {}; }}", css_value);
            let stylesheet = Stylesheet::parse(&css);

            assert_eq!(stylesheet.rules.len(), 1);
            let decl = &stylesheet.rules[0].declarations[0];
            if let Declaration::VerticalAlign(va) = decl {
                let ir_va = match va {
                    VerticalAlignValue::Baseline => VerticalAlign::Baseline,
                    VerticalAlignValue::Top => VerticalAlign::Top,
                    VerticalAlignValue::Middle => VerticalAlign::Middle,
                    VerticalAlignValue::Bottom => VerticalAlign::Bottom,
                    VerticalAlignValue::TextTop => VerticalAlign::TextTop,
                    VerticalAlignValue::TextBottom => VerticalAlign::TextBottom,
                    VerticalAlignValue::Super => VerticalAlign::Super,
                    VerticalAlignValue::Sub => VerticalAlign::Sub,
                };
                assert_eq!(
                    ir_va, expected,
                    "vertical-align: {} should parse correctly",
                    css_value
                );
            } else {
                panic!(
                    "vertical-align: {} should parse as VerticalAlign declaration",
                    css_value
                );
            }
        }
    }

    // ========================================================================
    // Border Shorthand Tests
    // ========================================================================

    #[test]
    fn test_border_style_shorthand_two_values() {
        // border-style: solid none means top/bottom=solid, left/right=none
        let css = "div { border-style: solid none; }";
        let stylesheet = Stylesheet::parse(css);

        assert_eq!(stylesheet.rules.len(), 1);
        let decls = &stylesheet.rules[0].declarations;
        assert_eq!(decls.len(), 4, "Should expand to 4 declarations");

        // Check each side
        let top = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderTopStyle(s) => Some(*s),
                _ => None,
            })
            .expect("border-top-style should exist");
        let bottom = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderBottomStyle(s) => Some(*s),
                _ => None,
            })
            .expect("border-bottom-style should exist");
        let left = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderLeftStyle(s) => Some(*s),
                _ => None,
            })
            .expect("border-left-style should exist");
        let right = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderRightStyle(s) => Some(*s),
                _ => None,
            })
            .expect("border-right-style should exist");

        assert_eq!(top, BorderStyle::Solid, "top should be solid");
        assert_eq!(bottom, BorderStyle::Solid, "bottom should be solid");
        assert_eq!(left, BorderStyle::None, "left should be none");
        assert_eq!(right, BorderStyle::None, "right should be none");
    }

    #[test]
    fn test_border_style_shorthand_one_value() {
        // border-style: dashed means all sides=dashed
        let css = "div { border-style: dashed; }";
        let stylesheet = Stylesheet::parse(css);

        let decls = &stylesheet.rules[0].declarations;
        assert_eq!(decls.len(), 4);

        for decl in decls {
            match decl {
                Declaration::BorderTopStyle(s)
                | Declaration::BorderRightStyle(s)
                | Declaration::BorderBottomStyle(s)
                | Declaration::BorderLeftStyle(s) => {
                    assert_eq!(*s, BorderStyle::Dashed);
                }
                _ => panic!("Unexpected declaration type"),
            }
        }
    }

    #[test]
    fn test_border_style_shorthand_four_values() {
        // border-style: solid dotted dashed double (top, right, bottom, left)
        let css = "div { border-style: solid dotted dashed double; }";
        let stylesheet = Stylesheet::parse(css);

        let decls = &stylesheet.rules[0].declarations;
        assert_eq!(decls.len(), 4);

        let top = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderTopStyle(s) => Some(*s),
                _ => None,
            })
            .unwrap();
        let right = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderRightStyle(s) => Some(*s),
                _ => None,
            })
            .unwrap();
        let bottom = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderBottomStyle(s) => Some(*s),
                _ => None,
            })
            .unwrap();
        let left = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderLeftStyle(s) => Some(*s),
                _ => None,
            })
            .unwrap();

        assert_eq!(top, BorderStyle::Solid);
        assert_eq!(right, BorderStyle::Dotted);
        assert_eq!(bottom, BorderStyle::Dashed);
        assert_eq!(left, BorderStyle::Double);
    }

    #[test]
    fn test_border_width_shorthand_two_values() {
        // border-width: 3px 0 means top/bottom=3px, left/right=0
        let css = "div { border-width: 3px 0; }";
        let stylesheet = Stylesheet::parse(css);

        let decls = &stylesheet.rules[0].declarations;
        assert_eq!(decls.len(), 4);

        let top = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderTopWidth(l) => Some(*l),
                _ => None,
            })
            .unwrap();
        let left = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderLeftWidth(l) => Some(*l),
                _ => None,
            })
            .unwrap();

        assert_eq!(top, Length::Px(3.0));
        assert_eq!(left, Length::Px(0.0));
    }

    #[test]
    fn test_border_color_shorthand_two_values() {
        // border-color: red blue means top/bottom=red, left/right=blue
        let css = "div { border-color: #ff0000 #0000ff; }";
        let stylesheet = Stylesheet::parse(css);

        let decls = &stylesheet.rules[0].declarations;
        assert_eq!(decls.len(), 4);

        let top = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderTopColor(c) => Some(*c),
                _ => None,
            })
            .unwrap();
        let left = decls
            .iter()
            .find_map(|d| match d {
                Declaration::BorderLeftColor(c) => Some(*c),
                _ => None,
            })
            .unwrap();

        assert_eq!(top, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(left, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    // ========================================================================
    // Combined border shorthand tests
    // ========================================================================

    #[test]
    fn test_border_combined_shorthand() {
        // border: 1px solid red
        let css = "div { border: 1px solid red; }";
        let stylesheet = Stylesheet::parse(css);

        let decls = &stylesheet.rules[0].declarations;
        // Should expand to 12 declarations (4 width + 4 style + 4 color)
        assert_eq!(decls.len(), 12);

        // Check we have all expected types
        let width_count = decls
            .iter()
            .filter(|d| {
                matches!(
                    d,
                    Declaration::BorderTopWidth(_)
                        | Declaration::BorderRightWidth(_)
                        | Declaration::BorderBottomWidth(_)
                        | Declaration::BorderLeftWidth(_)
                )
            })
            .count();
        let style_count = decls
            .iter()
            .filter(|d| {
                matches!(
                    d,
                    Declaration::BorderTopStyle(_)
                        | Declaration::BorderRightStyle(_)
                        | Declaration::BorderBottomStyle(_)
                        | Declaration::BorderLeftStyle(_)
                )
            })
            .count();
        let color_count = decls
            .iter()
            .filter(|d| {
                matches!(
                    d,
                    Declaration::BorderTopColor(_)
                        | Declaration::BorderRightColor(_)
                        | Declaration::BorderBottomColor(_)
                        | Declaration::BorderLeftColor(_)
                )
            })
            .count();

        assert_eq!(width_count, 4, "should have 4 width declarations");
        assert_eq!(style_count, 4, "should have 4 style declarations");
        assert_eq!(color_count, 4, "should have 4 color declarations");
    }

    #[test]
    fn test_border_shorthand_order_insensitive() {
        // CSS allows border values in any order
        let css1 = "div { border: solid 2px #888; }";
        let css2 = "div { border: #888 solid 2px; }";
        let css3 = "div { border: 2px #888 solid; }";

        for css in [css1, css2, css3] {
            let stylesheet = Stylesheet::parse(css);
            let decls = &stylesheet.rules[0].declarations;

            // All should produce 12 declarations
            assert_eq!(decls.len(), 12, "should have 12 declarations for: {}", css);

            // Check that we got the right values
            let width = decls
                .iter()
                .find_map(|d| match d {
                    Declaration::BorderTopWidth(w) => Some(*w),
                    _ => None,
                })
                .expect("should have width");
            let style = decls
                .iter()
                .find_map(|d| match d {
                    Declaration::BorderTopStyle(s) => Some(*s),
                    _ => None,
                })
                .expect("should have style");
            let color = decls
                .iter()
                .find_map(|d| match d {
                    Declaration::BorderTopColor(c) => Some(*c),
                    _ => None,
                })
                .expect("should have color");

            assert_eq!(width, Length::Px(2.0), "width should be 2px for: {}", css);
            assert_eq!(style, BorderStyle::Solid, "style should be solid for: {}", css);
            assert_eq!(
                color,
                Color { r: 136, g: 136, b: 136, a: 255 },
                "color should be #888 for: {}",
                css
            );
        }
    }

    #[test]
    fn test_border_shorthand_partial() {
        // border: solid (style only)
        let css = "div { border: solid; }";
        let stylesheet = Stylesheet::parse(css);

        let decls = &stylesheet.rules[0].declarations;
        // Should only expand style (4 declarations)
        assert_eq!(decls.len(), 4);

        for decl in decls {
            match decl {
                Declaration::BorderTopStyle(s)
                | Declaration::BorderRightStyle(s)
                | Declaration::BorderBottomStyle(s)
                | Declaration::BorderLeftStyle(s) => {
                    assert_eq!(*s, BorderStyle::Solid);
                }
                _ => panic!("Unexpected declaration type: {:?}", decl),
            }
        }
    }
}

