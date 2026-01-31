//! CSS declaration parsing and stylesheet structures.
//!
//! This module handles parsing CSS from strings into structured declarations
//! that can be applied to computed styles.

use cssparser::{
    AtRuleParser, DeclarationParser, ParseError, Parser, ParserInput, QualifiedRuleParser,
    RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
use selectors::parser::Selector;

use super::properties::*;
use crate::dom::element_ref::BokoSelectors;
use crate::model::FontFace;

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
    FontVariant(FontVariant),

    // Text properties
    TextAlign(TextAlign),
    TextIndent(Length),
    LineHeight(Length),
    LetterSpacing(Length),
    WordSpacing(Length),
    TextTransform(TextTransform),
    Hyphens(Hyphens),
    WhiteSpace(WhiteSpace),
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
                [
                    Self::MarginTop(t),
                    Self::MarginRight(r),
                    Self::MarginBottom(b),
                    Self::MarginLeft(l),
                ]
            }),
            "padding" => Self::parse_length_rect(input, |t, r, b, l| {
                [
                    Self::PaddingTop(t),
                    Self::PaddingRight(r),
                    Self::PaddingBottom(b),
                    Self::PaddingLeft(l),
                ]
            }),
            "border-width" => Self::parse_length_rect(input, |t, r, b, l| {
                [
                    Self::BorderTopWidth(t),
                    Self::BorderRightWidth(r),
                    Self::BorderBottomWidth(b),
                    Self::BorderLeftWidth(l),
                ]
            }),
            "border-style" => parse_border_style_shorthand_values(input)
                .map(|(t, r, b, l)| {
                    vec![
                        Self::BorderTopStyle(t),
                        Self::BorderRightStyle(r),
                        Self::BorderBottomStyle(b),
                        Self::BorderLeftStyle(l),
                    ]
                })
                .unwrap_or_default(),
            "border-color" => parse_color_shorthand_values(input)
                .map(|(t, r, b, l)| {
                    vec![
                        Self::BorderTopColor(t),
                        Self::BorderRightColor(r),
                        Self::BorderBottomColor(b),
                        Self::BorderLeftColor(l),
                    ]
                })
                .unwrap_or_default(),
            "border" => parse_border_shorthand(input),
            "border-top" => parse_border_top_shorthand(input),
            "border-right" => parse_border_right_shorthand(input),
            "border-bottom" => parse_border_bottom_shorthand(input),
            "border-left" => parse_border_left_shorthand(input),
            "list-style" => parse_list_style_shorthand(input),
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
            "background" => parse_background_shorthand(input).map(Self::BackgroundColor),

            // Font properties
            "font-family" => parse_font_family(input).map(Self::FontFamily),
            "font-size" => parse_font_size(input).map(Self::FontSize),
            "font-weight" => parse_font_weight(input).map(Self::FontWeight),
            "font-style" => parse_font_style(input).map(Self::FontStyle),
            "font-variant" | "font-variant-caps" => {
                parse_font_variant(input).map(Self::FontVariant)
            }

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
            "break-inside" | "page-break-inside" => {
                parse_break_inside(input).map(Self::BreakInside)
            }

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
    pub font_faces: Vec<FontFace>,
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
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.ids
            .cmp(&other.ids)
            .then(self.classes.cmp(&other.classes))
            .then(self.elements.cmp(&other.elements))
    }
}

impl PartialOrd for Specificity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Origin of a style (for cascade ordering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    UserAgent = 0,
    Author = 1,
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
    font_faces: &'a mut Vec<FontFace>,
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

/// Parse the CSS `background` shorthand and extract just the color component.
///
/// The background shorthand can contain: color, image, position, repeat, size, attachment,
/// origin, clip - in any order. We parse tokens in a loop and extract any color we find.
/// See https://www.w3.org/TR/css-backgrounds-3/#background
fn parse_background_shorthand(input: &mut Parser<'_, '_>) -> Option<Color> {
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

fn parse_length(input: &mut Parser<'_, '_>) -> Option<Length> {
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

/// Parse font-size value (handles lengths, percentages, and keywords).
///
/// Supports absolute keywords: xx-small, x-small, small, medium, large, x-large, xx-large
/// Supports relative keywords: smaller, larger
fn parse_font_size(input: &mut Parser<'_, '_>) -> Option<Length> {
    match input.next().ok()? {
        Token::Dimension { value, unit, .. } => {
            let length = match unit.as_ref() {
                "px" => Length::Px(*value),
                "em" => Length::Em(*value),
                "rem" => Length::Rem(*value),
                "%" => Length::Percent(*value),
                "pt" => Length::Px(*value * 96.0 / 72.0), // Convert pt to px
                _ => return None,
            };
            Some(length)
        }
        Token::Percentage { unit_value, .. } => Some(Length::Percent(*unit_value * 100.0)),
        Token::Number { value, .. } if *value == 0.0 => Some(Length::Px(0.0)),
        Token::Ident(ident) => match ident.as_ref() {
            // Absolute size keywords (based on 16px default)
            // Values from CSS spec: https://www.w3.org/TR/css-fonts-3/#absolute-size-value
            "xx-small" => Some(Length::Rem(0.5625)), // 9px / 16px
            "x-small" => Some(Length::Rem(0.625)),   // 10px / 16px
            "small" => Some(Length::Rem(0.8125)),    // 13px / 16px
            "medium" => Some(Length::Rem(1.0)),      // 16px / 16px
            "large" => Some(Length::Rem(1.125)),     // 18px / 16px
            "x-large" => Some(Length::Rem(1.5)),     // 24px / 16px
            "xx-large" => Some(Length::Rem(2.0)),    // 32px / 16px
            "xxx-large" => Some(Length::Rem(3.0)),   // 48px / 16px (CSS4)
            // Relative size keywords (relative to parent, use em)
            "smaller" => Some(Length::Em(0.833)), // ~1/1.2
            "larger" => Some(Length::Em(1.2)),
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
        if style.is_none()
            && let Ok(s) = input.try_parse(|i| {
                parse_border_style_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            })
        {
            style = Some(s);
            continue;
        }

        // Try color (keywords or hex/rgb)
        if color.is_none()
            && let Ok(c) =
                input.try_parse(|i| parse_color(i).ok_or_else(|| i.new_custom_error::<_, ()>(())))
        {
            color = Some(c);
            continue;
        }

        // Try width (length values)
        if width.is_none()
            && let Ok(w) = input.try_parse(|i| {
                parse_border_width_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            })
        {
            width = Some(w);
            continue;
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

/// Parse border values (width, style, color) in any order, returning them as a tuple.
fn parse_border_values(
    input: &mut Parser<'_, '_>,
) -> (Option<Length>, Option<BorderStyle>, Option<Color>) {
    let mut width: Option<Length> = None;
    let mut style: Option<BorderStyle> = None;
    let mut color: Option<Color> = None;

    for _ in 0..3 {
        if style.is_none()
            && let Ok(s) = input.try_parse(|i| {
                parse_border_style_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            })
        {
            style = Some(s);
            continue;
        }

        if color.is_none()
            && let Ok(c) =
                input.try_parse(|i| parse_color(i).ok_or_else(|| i.new_custom_error::<_, ()>(())))
        {
            color = Some(c);
            continue;
        }

        if width.is_none()
            && let Ok(w) = input.try_parse(|i| {
                parse_border_width_value(i).ok_or_else(|| i.new_custom_error::<_, ()>(()))
            })
        {
            width = Some(w);
            continue;
        }

        break;
    }

    (width, style, color)
}

/// Parse border-top shorthand (e.g., `border-top: 1px solid red`).
fn parse_border_top_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let (width, style, color) = parse_border_values(input);
    if width.is_none() && style.is_none() && color.is_none() {
        return vec![];
    }

    let mut decls = Vec::with_capacity(3);
    if let Some(w) = width {
        decls.push(Declaration::BorderTopWidth(w));
    }
    if let Some(s) = style {
        decls.push(Declaration::BorderTopStyle(s));
    }
    if let Some(c) = color {
        decls.push(Declaration::BorderTopColor(c));
    }
    decls
}

/// Parse border-right shorthand.
fn parse_border_right_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let (width, style, color) = parse_border_values(input);
    if width.is_none() && style.is_none() && color.is_none() {
        return vec![];
    }

    let mut decls = Vec::with_capacity(3);
    if let Some(w) = width {
        decls.push(Declaration::BorderRightWidth(w));
    }
    if let Some(s) = style {
        decls.push(Declaration::BorderRightStyle(s));
    }
    if let Some(c) = color {
        decls.push(Declaration::BorderRightColor(c));
    }
    decls
}

/// Parse border-bottom shorthand.
fn parse_border_bottom_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let (width, style, color) = parse_border_values(input);
    if width.is_none() && style.is_none() && color.is_none() {
        return vec![];
    }

    let mut decls = Vec::with_capacity(3);
    if let Some(w) = width {
        decls.push(Declaration::BorderBottomWidth(w));
    }
    if let Some(s) = style {
        decls.push(Declaration::BorderBottomStyle(s));
    }
    if let Some(c) = color {
        decls.push(Declaration::BorderBottomColor(c));
    }
    decls
}

/// Parse border-left shorthand.
fn parse_border_left_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
    let (width, style, color) = parse_border_values(input);
    if width.is_none() && style.is_none() && color.is_none() {
        return vec![];
    }

    let mut decls = Vec::with_capacity(3);
    if let Some(w) = width {
        decls.push(Declaration::BorderLeftWidth(w));
    }
    if let Some(s) = style {
        decls.push(Declaration::BorderLeftStyle(s));
    }
    if let Some(c) = color {
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

fn parse_font_variant(input: &mut Parser<'_, '_>) -> Option<FontVariant> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "small-caps" => Some(FontVariant::SmallCaps),
        "normal" | "none" => Some(FontVariant::Normal),
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

fn parse_white_space(input: &mut Parser<'_, '_>) -> Option<WhiteSpace> {
    let token = input.expect_ident_cloned().ok()?;
    match token.as_ref() {
        "normal" => Some(WhiteSpace::Normal),
        "nowrap" => Some(WhiteSpace::Nowrap),
        "pre" => Some(WhiteSpace::Pre),
        "pre-wrap" => Some(WhiteSpace::PreWrap),
        "pre-line" => Some(WhiteSpace::PreLine),
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

/// Parse the list-style shorthand: list-style-type, list-style-position, list-style-image
/// We only care about type and position (image is not supported).
fn parse_list_style_shorthand(input: &mut Parser<'_, '_>) -> Vec<Declaration> {
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
fn parse_font_face_block(input: &mut Parser<'_, '_>) -> Option<FontFace> {
    let mut font_family: Option<String> = None;
    let mut font_weight = FontWeight::NORMAL;
    let mut font_style = FontStyle::Normal;
    let mut src: Option<String> = None;

    // Parse declarations within the @font-face block
    while let Ok(name) = input.expect_ident_cloned() {
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
    }

    // Require both font-family and src
    match (font_family, src) {
        (Some(family), Some(source)) => {
            Some(FontFace::new(family, font_weight, font_style, source))
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
