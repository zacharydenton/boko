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
    BorderStyle, BoxSizing, BreakValue, Color, ComputedStyle, DecorationStyle, Display, Float,
    FontStyle, FontWeight, Hyphens, Length, ListStylePosition, ListStyleType, StylePool, TextAlign,
    TextTransform, Visibility,
};

// ============================================================================
// PropertyId - Interned CSS property names for zero-allocation parsing
// ============================================================================

/// Macro to define CSS properties with bidirectional string mapping.
/// Each entry maps a CSS property name to an enum variant.
macro_rules! define_properties {
    ($($css_name:literal => $variant:ident),* $(,)?) => {
        /// CSS property identifier.
        ///
        /// Using an enum instead of String eliminates heap allocations during parsing
        /// and enables faster matching via enum dispatch instead of string comparison.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum PropertyId {
            $($variant,)*
            /// Unknown property (rare - only for truly unrecognized properties)
            Unknown,
        }

        impl PropertyId {
            /// Parse a property name string into a PropertyId.
            #[inline]
            pub fn from_str(name: &str) -> Self {
                match name {
                    $($css_name => PropertyId::$variant,)*
                    _ => PropertyId::Unknown,
                }
            }

            /// Get the CSS property name as a string.
            #[inline]
            pub fn name(&self) -> &'static str {
                match self {
                    $(PropertyId::$variant => $css_name,)*
                    PropertyId::Unknown => "<unknown>",
                }
            }
        }
    };
}

define_properties! {
    // Colors
    "color" => Color,
    "background-color" => BackgroundColor,

    // Font properties
    "font-family" => FontFamily,
    "font-size" => FontSize,
    "font-weight" => FontWeight,
    "font-style" => FontStyle,
    "font-variant" => FontVariant,
    "font-variant-caps" => FontVariantCaps,

    // Text properties
    "text-align" => TextAlign,
    "text-indent" => TextIndent,
    "line-height" => LineHeight,
    "letter-spacing" => LetterSpacing,
    "word-spacing" => WordSpacing,
    "text-transform" => TextTransform,
    "hyphens" => Hyphens,
    "white-space" => WhiteSpace,
    "vertical-align" => VerticalAlign,

    // Text decoration
    "text-decoration" => TextDecoration,
    "text-decoration-line" => TextDecorationLine,
    "text-decoration-style" => TextDecorationStyle,
    "text-decoration-color" => TextDecorationColor,

    // Box model - margins
    "margin" => Margin,
    "margin-top" => MarginTop,
    "margin-right" => MarginRight,
    "margin-bottom" => MarginBottom,
    "margin-left" => MarginLeft,

    // Box model - padding
    "padding" => Padding,
    "padding-top" => PaddingTop,
    "padding-right" => PaddingRight,
    "padding-bottom" => PaddingBottom,
    "padding-left" => PaddingLeft,

    // Dimensions
    "width" => Width,
    "height" => Height,
    "max-width" => MaxWidth,
    "min-height" => MinHeight,

    // Display & positioning
    "display" => Display,
    "float" => Float,
    "visibility" => Visibility,
    "box-sizing" => BoxSizing,

    // Page breaks
    "break-before" => BreakBefore,
    "break-after" => BreakAfter,
    "break-inside" => BreakInside,
    "page-break-before" => PageBreakBefore,
    "page-break-after" => PageBreakAfter,
    "page-break-inside" => PageBreakInside,

    // Border style
    "border-style" => BorderStyle,
    "border-top-style" => BorderTopStyle,
    "border-right-style" => BorderRightStyle,
    "border-bottom-style" => BorderBottomStyle,
    "border-left-style" => BorderLeftStyle,

    // Border width
    "border-width" => BorderWidth,
    "border-top-width" => BorderTopWidth,
    "border-right-width" => BorderRightWidth,
    "border-bottom-width" => BorderBottomWidth,
    "border-left-width" => BorderLeftWidth,

    // Border color
    "border-color" => BorderColor,
    "border-top-color" => BorderTopColor,
    "border-right-color" => BorderRightColor,
    "border-bottom-color" => BorderBottomColor,
    "border-left-color" => BorderLeftColor,

    // Border radius
    "border-radius" => BorderRadius,
    "border-top-left-radius" => BorderTopLeftRadius,
    "border-top-right-radius" => BorderTopRightRadius,
    "border-bottom-left-radius" => BorderBottomLeftRadius,
    "border-bottom-right-radius" => BorderBottomRightRadius,

    // List properties
    "list-style-type" => ListStyleType,
    "list-style-position" => ListStylePosition,
}

// ============================================================================
// Stylesheet and Rule Structures
// ============================================================================

/// A parsed CSS stylesheet.
#[derive(Debug, Default, Clone)]
pub struct Stylesheet {
    pub rules: Vec<CssRule>,
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

/// A CSS declaration (property: value).
///
/// Note: The `important` flag is no longer stored here - declarations are
/// separated into normal and important vectors in CssRule.
#[derive(Debug, Clone)]
pub struct Declaration {
    pub property: PropertyId,
    pub value: PropertyValue,
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
    // Phase 1-7 additions
    TextTransform(TextTransform),
    Hyphens(Hyphens),
    BreakValue(BreakValue),
    Float(Float),
    BorderStyle(BorderStyle),
    ListStylePosition(ListStylePosition),
    Visibility(Visibility),
    DecorationStyle(DecorationStyle),
    Bool(bool),
    BoxSizing(BoxSizing),
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
        let property_id = PropertyId::from_str(&name);

        // Handle margin/padding shorthand expansion
        if (property_id == PropertyId::Margin || property_id == PropertyId::Padding)
            && let Some((top, right, bottom, left)) = parse_box_shorthand(input)
        {
            let important = input.try_parse(cssparser::parse_important).is_ok();
            let target = if important {
                &mut *self.important_declarations
            } else {
                &mut *self.declarations
            };

            let (top_id, right_id, bottom_id, left_id) = if property_id == PropertyId::Margin {
                (
                    PropertyId::MarginTop,
                    PropertyId::MarginRight,
                    PropertyId::MarginBottom,
                    PropertyId::MarginLeft,
                )
            } else {
                (
                    PropertyId::PaddingTop,
                    PropertyId::PaddingRight,
                    PropertyId::PaddingBottom,
                    PropertyId::PaddingLeft,
                )
            };

            target.push(Declaration {
                property: top_id,
                value: PropertyValue::Length(top),
            });
            target.push(Declaration {
                property: right_id,
                value: PropertyValue::Length(right),
            });
            target.push(Declaration {
                property: bottom_id,
                value: PropertyValue::Length(bottom),
            });
            target.push(Declaration {
                property: left_id,
                value: PropertyValue::Length(left),
            });
            return Ok(());
        }

        let value = parse_property_value(property_id, input);
        let important = input.try_parse(cssparser::parse_important).is_ok();

        let target = if important {
            &mut *self.important_declarations
        } else {
            &mut *self.declarations
        };

        target.push(Declaration {
            property: property_id,
            value,
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

/// Parse a property value based on the property ID.
fn parse_property_value(property: PropertyId, input: &mut Parser<'_, '_>) -> PropertyValue {
    match property {
        // Colors
        PropertyId::Color | PropertyId::BackgroundColor => {
            parse_color(input).unwrap_or(PropertyValue::None)
        }

        // Font properties
        PropertyId::FontSize => parse_font_size(input).unwrap_or(PropertyValue::None),
        PropertyId::FontWeight => parse_font_weight(input).unwrap_or(PropertyValue::None),
        PropertyId::FontStyle => parse_font_style(input).unwrap_or(PropertyValue::None),
        PropertyId::FontFamily => parse_font_family(input).unwrap_or(PropertyValue::None),
        PropertyId::FontVariant | PropertyId::FontVariantCaps => {
            parse_font_variant(input).unwrap_or(PropertyValue::None)
        }

        // Text properties
        PropertyId::TextAlign => parse_text_align(input).unwrap_or(PropertyValue::None),
        PropertyId::LineHeight => parse_line_height(input).unwrap_or(PropertyValue::None),
        PropertyId::TextTransform => parse_text_transform(input).unwrap_or(PropertyValue::None),
        PropertyId::Hyphens => parse_hyphens(input).unwrap_or(PropertyValue::None),
        PropertyId::WhiteSpace => parse_white_space(input).unwrap_or(PropertyValue::None),
        PropertyId::VerticalAlign => parse_vertical_align(input).unwrap_or(PropertyValue::None),

        // Text decoration
        PropertyId::TextDecoration | PropertyId::TextDecorationLine => {
            parse_text_decoration(input).unwrap_or(PropertyValue::None)
        }
        PropertyId::TextDecorationStyle => {
            parse_decoration_style(input).unwrap_or(PropertyValue::None)
        }
        PropertyId::TextDecorationColor => parse_color(input).unwrap_or(PropertyValue::None),

        // Length-based properties
        PropertyId::TextIndent
        | PropertyId::LetterSpacing
        | PropertyId::WordSpacing
        | PropertyId::Margin
        | PropertyId::MarginTop
        | PropertyId::MarginRight
        | PropertyId::MarginBottom
        | PropertyId::MarginLeft
        | PropertyId::Padding
        | PropertyId::PaddingTop
        | PropertyId::PaddingRight
        | PropertyId::PaddingBottom
        | PropertyId::PaddingLeft
        | PropertyId::Width
        | PropertyId::Height
        | PropertyId::MaxWidth
        | PropertyId::MinHeight
        | PropertyId::BorderWidth
        | PropertyId::BorderTopWidth
        | PropertyId::BorderRightWidth
        | PropertyId::BorderBottomWidth
        | PropertyId::BorderLeftWidth
        | PropertyId::BorderRadius
        | PropertyId::BorderTopLeftRadius
        | PropertyId::BorderTopRightRadius
        | PropertyId::BorderBottomLeftRadius
        | PropertyId::BorderBottomRightRadius => {
            parse_length(input).unwrap_or(PropertyValue::None)
        }

        // Display & positioning
        PropertyId::Display => parse_display(input).unwrap_or(PropertyValue::None),
        PropertyId::Float => parse_float(input).unwrap_or(PropertyValue::None),
        PropertyId::Visibility => parse_visibility(input).unwrap_or(PropertyValue::None),
        PropertyId::BoxSizing => parse_box_sizing(input).unwrap_or(PropertyValue::None),

        // Page break properties
        PropertyId::BreakBefore
        | PropertyId::BreakAfter
        | PropertyId::PageBreakBefore
        | PropertyId::PageBreakAfter => parse_break_value(input).unwrap_or(PropertyValue::None),
        PropertyId::BreakInside | PropertyId::PageBreakInside => {
            parse_break_inside(input).unwrap_or(PropertyValue::None)
        }

        // Border style
        PropertyId::BorderStyle
        | PropertyId::BorderTopStyle
        | PropertyId::BorderRightStyle
        | PropertyId::BorderBottomStyle
        | PropertyId::BorderLeftStyle => parse_border_style(input).unwrap_or(PropertyValue::None),

        // Border color
        PropertyId::BorderColor
        | PropertyId::BorderTopColor
        | PropertyId::BorderRightColor
        | PropertyId::BorderBottomColor
        | PropertyId::BorderLeftColor => parse_color(input).unwrap_or(PropertyValue::None),

        // List properties
        PropertyId::ListStyleType => parse_list_style_type(input).unwrap_or(PropertyValue::None),
        PropertyId::ListStylePosition => {
            parse_list_style_position(input).unwrap_or(PropertyValue::None)
        }

        // Unknown properties
        PropertyId::Unknown => {
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
                return Some(PropertyValue::Keyword(token.to_string()));
            }
            _ => return None,
        };
        return Some(PropertyValue::Color(color));
    }

    // Try ID token (which is how cssparser parses hex colors like #ff0000)
    if let Ok(Token::IDHash(hash)) = input.try_parse(|i| i.next().cloned())
        && let Some(color) = parse_hex_color(hash.as_ref())
    {
        return Some(PropertyValue::Color(color));
    }

    // Try hash token
    if let Ok(Token::Hash(hash)) = input.try_parse(|i| i.next().cloned())
        && let Some(color) = parse_hex_color(hash.as_ref())
    {
        return Some(PropertyValue::Color(color));
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
        Token::Number { value, .. } if *value == 0.0 => {
            Some(PropertyValue::Length(Length::Px(0.0)))
        }
        Token::Ident(ident) => match ident.as_ref() {
            "auto" => Some(PropertyValue::Length(Length::Auto)),
            "inherit" | "initial" | "unset" => Some(PropertyValue::Keyword(ident.to_string())),
            _ => None,
        },
        _ => None,
    }
}

/// Parse a single length value, returning Length directly.
fn parse_length_value(input: &mut Parser<'_, '_>) -> Option<Length> {
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

/// Parse margin/padding shorthand with 1-4 values.
/// Returns (top, right, bottom, left) following CSS box model rules.
fn parse_box_shorthand(input: &mut Parser<'_, '_>) -> Option<(Length, Length, Length, Length)> {
    let mut values = Vec::with_capacity(4);

    // Parse up to 4 length values
    while values.len() < 4 {
        if let Some(len) = parse_length_value(input) {
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

/// Parse font-size, including keywords like 'smaller' and 'larger'.
fn parse_font_size(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    // First try to parse as a keyword
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        let length = match token.as_ref() {
            // Relative sizes: smaller = 0.833em, larger = 1.2em
            "smaller" => Length::Em(0.833333),
            "larger" => Length::Em(1.2),
            // Absolute sizes (approximate em values)
            "xx-small" => Length::Em(0.5625),
            "x-small" => Length::Em(0.625),
            "small" => Length::Em(0.833333),
            "medium" => Length::Em(1.0),
            "large" => Length::Em(1.125),
            "x-large" => Length::Em(1.5),
            "xx-large" => Length::Em(2.0),
            "xxx-large" => Length::Em(3.0),
            "inherit" | "initial" | "unset" => {
                return Some(PropertyValue::Keyword(token.to_string()));
            }
            _ => return None,
        };
        return Some(PropertyValue::Length(length));
    }

    // Fall back to parsing as a length
    parse_length(input)
}

fn parse_font_weight(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    if let Ok(token) = input.try_parse(|i| i.expect_ident_cloned()) {
        let weight = match token.as_ref() {
            "normal" => FontWeight::NORMAL,
            "bold" => FontWeight::BOLD,
            "lighter" => FontWeight(300),
            "bolder" => FontWeight(700),
            "inherit" | "initial" | "unset" => {
                return Some(PropertyValue::Keyword(token.to_string()));
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

/// Parse line-height, which can be a length OR a unitless number (multiplier).
/// Unitless numbers like `1.5` are converted to em values for KFX compatibility.
fn parse_line_height(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
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
        // Unitless number (like 1.5) - treat as em multiplier
        Token::Number { value, .. } => Some(PropertyValue::Length(Length::Em(*value))),
        Token::Ident(ident) => match ident.as_ref() {
            "normal" => Some(PropertyValue::Length(Length::Auto)),
            "inherit" | "initial" | "unset" => Some(PropertyValue::Keyword(ident.to_string())),
            _ => None,
        },
        _ => None,
    }
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

// ============================================================================
// Phase 1-7: New property parsing functions
// ============================================================================

fn parse_text_transform(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let transform = match token.as_ref() {
        "none" => TextTransform::None,
        "uppercase" => TextTransform::Uppercase,
        "lowercase" => TextTransform::Lowercase,
        "capitalize" => TextTransform::Capitalize,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::TextTransform(transform))
}

fn parse_hyphens(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let hyphens = match token.as_ref() {
        "auto" => Hyphens::Auto,
        "manual" => Hyphens::Manual,
        "none" => Hyphens::None,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::Hyphens(hyphens))
}

fn parse_white_space(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "nowrap" | "pre" => Some(PropertyValue::Bool(true)),
        "normal" | "pre-wrap" | "pre-line" => Some(PropertyValue::Bool(false)),
        "inherit" | "initial" | "unset" => Some(PropertyValue::Keyword(token.to_string())),
        _ => None,
    }
}

fn parse_decoration_style(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let style = match token.as_ref() {
        "solid" => DecorationStyle::Solid,
        "dotted" => DecorationStyle::Dotted,
        "dashed" => DecorationStyle::Dashed,
        "double" => DecorationStyle::Double,
        "none" => DecorationStyle::None,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::DecorationStyle(style))
}

fn parse_float(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let float = match token.as_ref() {
        "left" => Float::Left,
        "right" => Float::Right,
        "none" => Float::None,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::Float(float))
}

fn parse_break_value(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let value = match token.as_ref() {
        "auto" => BreakValue::Auto,
        "always" | "page" | "left" | "right" | "recto" | "verso" => BreakValue::Always,
        "avoid" | "avoid-page" => BreakValue::Avoid,
        "column" | "avoid-column" => BreakValue::Column,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::BreakValue(value))
}

fn parse_break_inside(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let value = match token.as_ref() {
        "auto" => BreakValue::Auto,
        "avoid" | "avoid-page" | "avoid-column" => BreakValue::Avoid,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::BreakValue(value))
}

fn parse_border_style(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let style = match token.as_ref() {
        "none" => BorderStyle::None,
        "solid" => BorderStyle::Solid,
        "dotted" => BorderStyle::Dotted,
        "dashed" => BorderStyle::Dashed,
        "double" => BorderStyle::Double,
        "groove" => BorderStyle::Groove,
        "ridge" => BorderStyle::Ridge,
        "inset" => BorderStyle::Inset,
        "outset" => BorderStyle::Outset,
        "hidden" => BorderStyle::None,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::BorderStyle(style))
}

fn parse_list_style_position(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let position = match token.as_ref() {
        "inside" => ListStylePosition::Inside,
        "outside" => ListStylePosition::Outside,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::ListStylePosition(position))
}

fn parse_visibility(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let visibility = match token.as_ref() {
        "visible" => Visibility::Visible,
        "hidden" => Visibility::Hidden,
        "collapse" => Visibility::Collapse,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::Visibility(visibility))
}

fn parse_box_sizing(input: &mut Parser<'_, '_>) -> Option<PropertyValue> {
    let token = input.expect_ident_cloned().ok()?;
    let box_sizing = match token.as_ref() {
        "content-box" => BoxSizing::ContentBox,
        "border-box" => BoxSizing::BorderBox,
        "inherit" | "initial" | "unset" => return Some(PropertyValue::Keyword(token.to_string())),
        _ => return None,
    };
    Some(PropertyValue::BoxSizing(box_sizing))
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
        // List properties (inherited)
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
    match decl.property {
        // Colors
        PropertyId::Color => {
            if let PropertyValue::Color(c) = &decl.value {
                style.color = Some(*c);
            }
        }
        PropertyId::BackgroundColor => {
            if let PropertyValue::Color(c) = &decl.value {
                style.background_color = Some(*c);
            }
        }

        // Font properties
        PropertyId::FontFamily => {
            if let PropertyValue::String(s) = &decl.value {
                style.font_family = Some(s.clone());
            }
        }
        PropertyId::FontSize => {
            if let PropertyValue::Length(l) = &decl.value {
                style.font_size = *l;
            }
        }
        PropertyId::FontWeight => {
            if let PropertyValue::FontWeight(w) = &decl.value {
                style.font_weight = *w;
            }
        }
        PropertyId::FontStyle => {
            if let PropertyValue::FontStyle(s) = &decl.value {
                style.font_style = *s;
            }
        }
        PropertyId::FontVariant | PropertyId::FontVariantCaps => {
            if let PropertyValue::Keyword(k) = &decl.value {
                style.font_variant = match k.as_str() {
                    "small-caps" => crate::ir::FontVariant::SmallCaps,
                    _ => crate::ir::FontVariant::Normal,
                };
            }
        }

        // Text properties
        PropertyId::TextAlign => {
            if let PropertyValue::TextAlign(a) = &decl.value {
                style.text_align = *a;
            }
        }
        PropertyId::TextIndent => {
            if let PropertyValue::Length(l) = &decl.value {
                style.text_indent = *l;
            }
        }
        PropertyId::LineHeight => {
            if let PropertyValue::Length(l) = &decl.value {
                style.line_height = *l;
            }
        }
        PropertyId::LetterSpacing => {
            if let PropertyValue::Length(l) = &decl.value {
                style.letter_spacing = *l;
            }
        }
        PropertyId::WordSpacing => {
            if let PropertyValue::Length(l) = &decl.value {
                style.word_spacing = *l;
            }
        }
        PropertyId::TextTransform => {
            if let PropertyValue::TextTransform(t) = &decl.value {
                style.text_transform = *t;
            }
        }
        PropertyId::Hyphens => {
            if let PropertyValue::Hyphens(h) = &decl.value {
                style.hyphens = *h;
            }
        }
        PropertyId::WhiteSpace => {
            if let PropertyValue::Bool(nowrap) = &decl.value {
                style.no_break = *nowrap;
            }
        }
        PropertyId::VerticalAlign => {
            if let PropertyValue::Keyword(k) = &decl.value {
                style.vertical_align_super = k == "super";
                style.vertical_align_sub = k == "sub";
            }
        }

        // Text decoration
        PropertyId::TextDecoration | PropertyId::TextDecorationLine => {
            if let PropertyValue::Keyword(k) = &decl.value {
                style.text_decoration_underline = k.contains("underline");
                style.text_decoration_line_through = k.contains("line-through");
            }
        }
        PropertyId::TextDecorationStyle => {
            if let PropertyValue::DecorationStyle(s) = &decl.value {
                style.underline_style = *s;
            }
        }
        PropertyId::TextDecorationColor => {
            if let PropertyValue::Color(c) = &decl.value {
                style.underline_color = Some(*c);
            }
        }

        // Margins
        PropertyId::Margin => {
            // Shorthand should have been expanded, but handle just in case
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_top = *l;
                style.margin_right = *l;
                style.margin_bottom = *l;
                style.margin_left = *l;
            }
        }
        PropertyId::MarginTop => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_top = *l;
            }
        }
        PropertyId::MarginRight => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_right = *l;
            }
        }
        PropertyId::MarginBottom => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_bottom = *l;
            }
        }
        PropertyId::MarginLeft => {
            if let PropertyValue::Length(l) = &decl.value {
                style.margin_left = *l;
            }
        }

        // Padding
        PropertyId::Padding => {
            // Shorthand should have been expanded, but handle just in case
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_top = *l;
                style.padding_right = *l;
                style.padding_bottom = *l;
                style.padding_left = *l;
            }
        }
        PropertyId::PaddingTop => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_top = *l;
            }
        }
        PropertyId::PaddingRight => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_right = *l;
            }
        }
        PropertyId::PaddingBottom => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_bottom = *l;
            }
        }
        PropertyId::PaddingLeft => {
            if let PropertyValue::Length(l) = &decl.value {
                style.padding_left = *l;
            }
        }

        // Dimensions
        PropertyId::Width => {
            if let PropertyValue::Length(l) = &decl.value {
                style.width = *l;
            }
        }
        PropertyId::Height => {
            if let PropertyValue::Length(l) = &decl.value {
                style.height = *l;
            }
        }
        PropertyId::MaxWidth => {
            if let PropertyValue::Length(l) = &decl.value {
                style.max_width = *l;
            }
        }
        PropertyId::MinHeight => {
            if let PropertyValue::Length(l) = &decl.value {
                style.min_height = *l;
            }
        }

        // Display & positioning
        PropertyId::Display => {
            if let PropertyValue::Display(d) = &decl.value {
                style.display = *d;
            }
        }
        PropertyId::Float => {
            if let PropertyValue::Float(f) = &decl.value {
                style.float = *f;
            }
        }
        PropertyId::Visibility => {
            if let PropertyValue::Visibility(v) = &decl.value {
                style.visibility = *v;
            }
        }
        PropertyId::BoxSizing => {
            if let PropertyValue::BoxSizing(bs) = &decl.value {
                style.box_sizing = *bs;
            }
        }

        // Page breaks
        PropertyId::BreakBefore | PropertyId::PageBreakBefore => {
            if let PropertyValue::BreakValue(b) = &decl.value {
                style.break_before = *b;
            }
        }
        PropertyId::BreakAfter | PropertyId::PageBreakAfter => {
            if let PropertyValue::BreakValue(b) = &decl.value {
                style.break_after = *b;
            }
        }
        PropertyId::BreakInside | PropertyId::PageBreakInside => {
            if let PropertyValue::BreakValue(b) = &decl.value {
                style.break_inside = *b;
            }
        }

        // Border style
        PropertyId::BorderStyle => {
            if let PropertyValue::BorderStyle(s) = &decl.value {
                style.border_style_top = *s;
                style.border_style_right = *s;
                style.border_style_bottom = *s;
                style.border_style_left = *s;
            }
        }
        PropertyId::BorderTopStyle => {
            if let PropertyValue::BorderStyle(s) = &decl.value {
                style.border_style_top = *s;
            }
        }
        PropertyId::BorderRightStyle => {
            if let PropertyValue::BorderStyle(s) = &decl.value {
                style.border_style_right = *s;
            }
        }
        PropertyId::BorderBottomStyle => {
            if let PropertyValue::BorderStyle(s) = &decl.value {
                style.border_style_bottom = *s;
            }
        }
        PropertyId::BorderLeftStyle => {
            if let PropertyValue::BorderStyle(s) = &decl.value {
                style.border_style_left = *s;
            }
        }

        // Border width
        PropertyId::BorderWidth => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_width_top = *l;
                style.border_width_right = *l;
                style.border_width_bottom = *l;
                style.border_width_left = *l;
            }
        }
        PropertyId::BorderTopWidth => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_width_top = *l;
            }
        }
        PropertyId::BorderRightWidth => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_width_right = *l;
            }
        }
        PropertyId::BorderBottomWidth => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_width_bottom = *l;
            }
        }
        PropertyId::BorderLeftWidth => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_width_left = *l;
            }
        }

        // Border color
        PropertyId::BorderColor => {
            if let PropertyValue::Color(c) = &decl.value {
                style.border_color_top = Some(*c);
                style.border_color_right = Some(*c);
                style.border_color_bottom = Some(*c);
                style.border_color_left = Some(*c);
            }
        }
        PropertyId::BorderTopColor => {
            if let PropertyValue::Color(c) = &decl.value {
                style.border_color_top = Some(*c);
            }
        }
        PropertyId::BorderRightColor => {
            if let PropertyValue::Color(c) = &decl.value {
                style.border_color_right = Some(*c);
            }
        }
        PropertyId::BorderBottomColor => {
            if let PropertyValue::Color(c) = &decl.value {
                style.border_color_bottom = Some(*c);
            }
        }
        PropertyId::BorderLeftColor => {
            if let PropertyValue::Color(c) = &decl.value {
                style.border_color_left = Some(*c);
            }
        }

        // Border radius
        PropertyId::BorderRadius => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_radius_top_left = *l;
                style.border_radius_top_right = *l;
                style.border_radius_bottom_left = *l;
                style.border_radius_bottom_right = *l;
            }
        }
        PropertyId::BorderTopLeftRadius => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_radius_top_left = *l;
            }
        }
        PropertyId::BorderTopRightRadius => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_radius_top_right = *l;
            }
        }
        PropertyId::BorderBottomLeftRadius => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_radius_bottom_left = *l;
            }
        }
        PropertyId::BorderBottomRightRadius => {
            if let PropertyValue::Length(l) = &decl.value {
                style.border_radius_bottom_right = *l;
            }
        }

        // List properties
        PropertyId::ListStyleType => {
            if let PropertyValue::ListStyleType(lst) = &decl.value {
                style.list_style_type = *lst;
            }
        }
        PropertyId::ListStylePosition => {
            if let PropertyValue::ListStylePosition(p) = &decl.value {
                style.list_style_position = *p;
            }
        }

        // Unknown - nothing to do
        PropertyId::Unknown => {}
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
        assert_eq!(rule.declarations[0].property, PropertyId::Color);
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

        // The important declaration should be in important_declarations
        assert_eq!(stylesheet.rules[0].important_declarations.len(), 1);
        assert_eq!(
            stylesheet.rules[0].important_declarations[0].property,
            PropertyId::Color
        );
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
            .find(|d| d.property == PropertyId::MarginLeft);
        let margin_right = decls
            .iter()
            .find(|d| d.property == PropertyId::MarginRight);
        let margin_top = decls.iter().find(|d| d.property == PropertyId::MarginTop);

        assert!(margin_left.is_some(), "margin-left should exist");
        assert!(margin_right.is_some(), "margin-right should exist");
        assert!(margin_top.is_some(), "margin-top should exist");

        // Verify auto values for left/right
        if let PropertyValue::Length(len) = &margin_left.unwrap().value {
            assert_eq!(*len, Length::Auto, "margin-left should be auto");
        } else {
            panic!("margin-left should be a length");
        }

        if let PropertyValue::Length(len) = &margin_right.unwrap().value {
            assert_eq!(*len, Length::Auto, "margin-right should be auto");
        } else {
            panic!("margin-right should be a length");
        }

        // Verify 0 for top/bottom
        if let PropertyValue::Length(len) = &margin_top.unwrap().value {
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
        assert_eq!(decl.property, PropertyId::LineHeight);

        // Unitless 1.5 should be converted to 1.5em
        if let PropertyValue::Length(len) = &decl.value {
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
        if let PropertyValue::Length(len) = &decl.value {
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
        if let PropertyValue::Length(len) = &decl.value {
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
        assert_eq!(decl.property, PropertyId::BoxSizing);
        if let PropertyValue::BoxSizing(bs) = &decl.value {
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
        if let PropertyValue::BoxSizing(bs) = &decl.value {
            assert_eq!(*bs, BoxSizing::ContentBox);
        } else {
            panic!("box-sizing should be a BoxSizing value");
        }
    }

    #[test]
    fn test_property_id_roundtrip() {
        // Test that PropertyId::from_str and name() are consistent
        let properties = [
            "color",
            "background-color",
            "font-size",
            "margin-top",
            "border-radius",
        ];
        for prop in properties {
            let id = PropertyId::from_str(prop);
            assert_ne!(id, PropertyId::Unknown, "{} should be recognized", prop);
            assert_eq!(id.name(), prop, "name() should match input");
        }
    }
}
