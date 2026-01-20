//! CSS parsing for style extraction.
//!
//! This module provides CSS parsing capabilities for extracting styles
//! from EPUB stylesheets to apply to KFX output.

use cssparser::{
    AtRuleParser, AtRuleType, BasicParseErrorKind, CowRcStr, ParseError, Parser, ParserInput,
    QualifiedRuleParser, RuleListParser, SourceLocation, Token,
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

/// Font variant (small-caps, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontVariant {
    #[default]
    Normal,
    SmallCaps,
}

/// Color value
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// R, G, B, A (0-255)
    Rgba(u8, u8, u8, u8),
    /// Current color keyword
    #[default]
    Current,
    /// Transparent keyword
    Transparent,
}

/// Border style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Hidden,
    Solid,
    Dotted,
    Dashed,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

/// Border properties
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Border {
    pub width: Option<CssValue>,
    pub style: BorderStyle,
    pub color: Option<Color>,
}

/// Display type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
    Other,
}

/// Position type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
}

/// Vertical alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Top,
    Middle,
    Bottom,
    Super,
    Sub,
    TextTop,
    TextBottom,
}

/// Clear property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

/// Word break property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
}

/// Break (page/column) value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BreakValue {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    Page,
    Left,
    Right,
    Column,
    AvoidColumn,
}

/// Overflow property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Scroll,
    Auto,
    Clip,
}

/// Visibility property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

/// Parsed CSS style properties
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ParsedStyle {
    pub font_family: Option<String>,
    pub font_size: Option<CssValue>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_variant: Option<FontVariant>,
    pub text_align: Option<TextAlign>,
    pub text_indent: Option<CssValue>,
    pub line_height: Option<CssValue>,
    pub margin_top: Option<CssValue>,
    pub margin_bottom: Option<CssValue>,
    pub margin_left: Option<CssValue>,
    pub margin_right: Option<CssValue>,
    pub color: Option<Color>,
    pub background_color: Option<Color>,
    pub border_top: Option<Border>,
    pub border_bottom: Option<Border>,
    pub border_left: Option<Border>,
    pub border_right: Option<Border>,
    pub display: Option<Display>,
    pub position: Option<Position>,
    pub left: Option<CssValue>,
    pub width: Option<CssValue>,
    pub height: Option<CssValue>,
    pub min_width: Option<CssValue>,
    pub min_height: Option<CssValue>,
    pub max_width: Option<CssValue>,
    pub max_height: Option<CssValue>,
    pub vertical_align: Option<VerticalAlign>,
    pub clear: Option<Clear>,
    pub word_break: Option<WordBreak>,
    pub overflow: Option<Overflow>,
    pub visibility: Option<Visibility>,
    pub break_before: Option<BreakValue>,
    pub break_after: Option<BreakValue>,
    pub break_inside: Option<BreakValue>,
    pub border_radius_tl: Option<CssValue>,
    pub border_radius_tr: Option<CssValue>,
    pub border_radius_br: Option<CssValue>,
    pub border_radius_bl: Option<CssValue>,
    pub letter_spacing: Option<CssValue>,
    pub word_spacing: Option<CssValue>,
    pub white_space_nowrap: Option<bool>,
    pub text_decoration_underline: bool,
    pub text_decoration_overline: bool,
    pub text_decoration_line_through: bool,
    /// Opacity as integer 0-100 (representing 0.0-1.0)
    pub opacity: Option<u8>,
    /// Whether this style is for an image element (set when creating ContentItem::Image)
    pub is_image: bool,
    /// Whether this style is for an inline element like a link (uses $127: $349 instead of $383)
    pub is_inline: bool,
    /// Actual image width in pixels (set for image styles when dimensions are known)
    pub image_width_px: Option<u32>,
    /// Actual image height in pixels (set for image styles when dimensions are known)
    pub image_height_px: Option<u32>,
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
        if other.font_variant.is_some() {
            self.font_variant = other.font_variant;
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
        if other.color.is_some() {
            self.color.clone_from(&other.color);
        }
        if other.background_color.is_some() {
            self.background_color.clone_from(&other.background_color);
        }
        if other.border_top.is_some() {
            self.border_top.clone_from(&other.border_top);
        }
        if other.border_bottom.is_some() {
            self.border_bottom.clone_from(&other.border_bottom);
        }
        if other.border_left.is_some() {
            self.border_left.clone_from(&other.border_left);
        }
        if other.border_right.is_some() {
            self.border_right.clone_from(&other.border_right);
        }
        if other.display.is_some() {
            self.display = other.display;
        }
        if other.position.is_some() {
            self.position = other.position;
        }
        if other.left.is_some() {
            self.left.clone_from(&other.left);
        }
        if other.width.is_some() {
            self.width.clone_from(&other.width);
        }
        if other.height.is_some() {
            self.height.clone_from(&other.height);
        }
        if other.min_width.is_some() {
            self.min_width.clone_from(&other.min_width);
        }
        if other.min_height.is_some() {
            self.min_height.clone_from(&other.min_height);
        }
        if other.max_width.is_some() {
            self.max_width.clone_from(&other.max_width);
        }
        if other.max_height.is_some() {
            self.max_height.clone_from(&other.max_height);
        }
        if other.vertical_align.is_some() {
            self.vertical_align = other.vertical_align;
        }
        if other.clear.is_some() {
            self.clear = other.clear;
        }
        if other.word_break.is_some() {
            self.word_break = other.word_break;
        }
        if other.overflow.is_some() {
            self.overflow = other.overflow;
        }
        if other.visibility.is_some() {
            self.visibility = other.visibility;
        }
        if other.break_before.is_some() {
            self.break_before = other.break_before;
        }
        if other.break_after.is_some() {
            self.break_after = other.break_after;
        }
        if other.break_inside.is_some() {
            self.break_inside = other.break_inside;
        }
        if other.border_radius_tl.is_some() {
            self.border_radius_tl.clone_from(&other.border_radius_tl);
        }
        if other.border_radius_tr.is_some() {
            self.border_radius_tr.clone_from(&other.border_radius_tr);
        }
        if other.border_radius_br.is_some() {
            self.border_radius_br.clone_from(&other.border_radius_br);
        }
        if other.border_radius_bl.is_some() {
            self.border_radius_bl.clone_from(&other.border_radius_bl);
        }
        if other.letter_spacing.is_some() {
            self.letter_spacing.clone_from(&other.letter_spacing);
        }
        if other.word_spacing.is_some() {
            self.word_spacing.clone_from(&other.word_spacing);
        }
        if other.white_space_nowrap.is_some() {
            self.white_space_nowrap = other.white_space_nowrap;
        }
        if other.text_decoration_underline {
            self.text_decoration_underline = true;
        }
        if other.text_decoration_overline {
            self.text_decoration_overline = true;
        }
        if other.text_decoration_line_through {
            self.text_decoration_line_through = true;
        }
        if other.opacity.is_some() {
            self.opacity = other.opacity;
        }
        // is_image is preserved if already set (once marked as image, stays image)
        if other.is_image {
            self.is_image = true;
        }
        // is_inline is preserved if already set (once marked as inline, stays inline)
        if other.is_inline {
            self.is_inline = true;
        }
        // Image dimensions - preserve if set
        if other.image_width_px.is_some() {
            self.image_width_px = other.image_width_px;
        }
        if other.image_height_px.is_some() {
            self.image_height_px = other.image_height_px;
        }
    }

    /// Check if this style indicates the element is hidden/invisible
    /// Elements are considered hidden if:
    /// - display: none
    /// - position: absolute with large negative left offset (e.g., -999em)
    pub fn is_hidden(&self) -> bool {
        // display: none
        if self.display == Some(Display::None) {
            return true;
        }

        // position: absolute with large negative left offset
        if self.position == Some(Position::Absolute)
            && let Some(ref left) = self.left
        {
            match left {
                CssValue::Em(v) if *v < -100.0 => return true,
                CssValue::Px(v) if *v < -1000.0 => return true,
                _ => {}
            }
        }

        false
    }

    /// Check if this style has any properties set
    pub fn is_empty(&self) -> bool {
        self.font_family.is_none()
            && self.font_size.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
            && self.font_variant.is_none()
            && self.text_align.is_none()
            && self.text_indent.is_none()
            && self.line_height.is_none()
            && self.margin_top.is_none()
            && self.margin_bottom.is_none()
            && self.margin_left.is_none()
            && self.margin_right.is_none()
            && self.color.is_none()
            && self.background_color.is_none()
            && self.border_top.is_none()
            && self.border_bottom.is_none()
            && self.border_left.is_none()
            && self.border_right.is_none()
            && self.display.is_none()
            && self.position.is_none()
            && self.left.is_none()
    }
}

pub use kuchiki::{ElementData, NodeDataRef, NodeRef, Selectors};

/// A CSS rule with kuchiki-compatible selectors
#[derive(Debug)]
pub struct CssRule {
    pub selectors: Selectors,
    pub style: ParsedStyle,
}

/// User-agent stylesheet with browser default styles.
/// These are applied at lowest specificity before document styles.
/// Based on standard browser defaults for HTML elements.
const USER_AGENT_CSS: &str = r#"
h1 { font-size: 2em; font-weight: bold; }
h2 { font-size: 1.5em; font-weight: bold; }
h3 { font-size: 1.17em; font-weight: bold; }
h4 { font-size: 1em; font-weight: bold; }
h5 { font-size: 0.83em; font-weight: bold; }
h6 { font-size: 0.67em; font-weight: bold; }
b, strong { font-weight: bold; }
i, em { font-style: italic; }
"#;

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
        let mut raw_rules = Vec::new();

        let rule_parser = CssRuleParser {
            rules: &mut raw_rules,
        };

        for result in RuleListParser::new_for_stylesheet(&mut parser, rule_parser) {
            // Ignore errors, just collect successful rules
            let _ = result;
        }

        // Convert raw rules to CssRules with kuchiki selectors
        let rules = raw_rules
            .into_iter()
            .filter_map(|(selector_str, style)| {
                Selectors::compile(&selector_str)
                    .ok()
                    .map(|selectors| CssRule { selectors, style })
            })
            .collect();

        Stylesheet { rules }
    }

    /// Parse a CSS stylesheet with browser default styles prepended.
    /// User-agent styles are applied at lowest specificity, so document
    /// styles will override them.
    pub fn parse_with_defaults(css: &str) -> Self {
        let combined = format!("{}\n{}", USER_AGENT_CSS, css);
        Self::parse(&combined)
    }

    /// Parse an inline style attribute (style="...")
    /// Returns a ParsedStyle with the declarations from the inline style
    pub fn parse_inline_style(style_attr: &str) -> ParsedStyle {
        let mut input = ParserInput::new(style_attr);
        let mut parser = Parser::new(&mut input);
        parse_declaration_block(&mut parser)
    }

    /// Get only the directly-matched styles for an element, WITHOUT CSS inheritance.
    /// This is useful when the output format (like KFX) has its own inheritance mechanism.
    pub fn get_direct_style_for_element(&self, element: &NodeDataRef<ElementData>) -> ParsedStyle {
        let mut result = ParsedStyle::default();

        // Collect matching rules with their specificity
        let mut matches: Vec<(kuchiki::Specificity, &ParsedStyle)> = Vec::new();

        for rule in &self.rules {
            for selector in &rule.selectors.0 {
                if selector.matches(element) {
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

/// Raw parsed rule: (selector_string, style)
type RawRule = (String, ParsedStyle);

struct CssRuleParser<'a> {
    rules: &'a mut Vec<RawRule>,
}

impl<'i> QualifiedRuleParser<'i> for CssRuleParser<'_> {
    type Prelude = String;
    type QualifiedRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        // Collect the selector string for later compilation with kuchiki
        let start = input.position();
        while input.next().is_ok() {}
        let selector_str = input.slice_from(start).to_string();
        Ok(selector_str)
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _location: SourceLocation,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        let style = parse_declaration_block(input);
        if !style.is_empty() {
            self.rules.push((prelude, style));
        }
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for CssRuleParser<'_> {
    type PreludeNoBlock = ();
    type PreludeBlock = ();
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<AtRuleType<Self::PreludeNoBlock, Self::PreludeBlock>, ParseError<'i, Self::Error>>
    {
        // Skip all @rules (@import, @media, @font-face, etc.)
        let _ = name;
        // Consume tokens to find the end
        while input.next().is_ok() {}
        Err(input.new_error(BasicParseErrorKind::AtRuleInvalid(name)))
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
        "font-variant" => {
            style.font_variant = parse_font_variant(values);
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
        "display" => {
            style.display = parse_display(values);
        }
        "position" => {
            style.position = parse_position(values);
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

#[cfg(test)]
mod tests {
    use super::*;
    use kuchiki::traits::*;

    /// Helper to get the style for an element in an HTML document
    fn get_style_for(stylesheet: &Stylesheet, html: &str, selector: &str) -> ParsedStyle {
        let doc = kuchiki::parse_html().one(html);
        let element = doc.select_first(selector).expect("Element not found");
        stylesheet.get_direct_style_for_element(&element)
    }

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
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert_eq!(p_style.text_align, Some(TextAlign::Justify));
        assert!(matches!(p_style.margin_bottom, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // Check h1 style
        let h1_style = get_style_for(&stylesheet, "<h1>Test</h1>", "h1");
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
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert_eq!(p_style.text_align, Some(TextAlign::Left));

        // Class should override element
        let class_style = get_style_for(&stylesheet, r#"<div class="special">Test</div>"#, "div");
        assert_eq!(class_style.text_align, Some(TextAlign::Right));

        // Element.class should have highest specificity
        let combined_style = get_style_for(&stylesheet, r#"<p class="special">Test</p>"#, "p");
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

        let m1 = get_style_for(&stylesheet, r#"<div class="m1">Test</div>"#, "div");
        assert!(matches!(m1.margin_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(m1.margin_left, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        let m2 = get_style_for(&stylesheet, r#"<div class="m2">Test</div>"#, "div");
        assert!(matches!(m2.margin_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(m2.margin_left, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));
    }

    #[test]
    fn test_text_indent() {
        let css = r#"
            p {
                margin-top: 0;
                margin-bottom: 0;
                text-indent: 1em;
            }
        "#;

        let stylesheet = Stylesheet::parse(css);
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");

        assert!(
            matches!(p_style.text_indent, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01),
            "Expected text-indent: 1em, got {:?}",
            p_style.text_indent
        );
        assert!(
            matches!(p_style.margin_top, Some(CssValue::Px(v)) if v.abs() < 0.01),
            "Expected margin-top: 0, got {:?}",
            p_style.margin_top
        );
    }

    #[test]
    fn test_inline_style_parsing() {
        let inline = Stylesheet::parse_inline_style(
            "font-weight: bold; text-align: center; margin-top: 2em",
        );

        assert!(matches!(inline.font_weight, Some(FontWeight::Bold)));
        assert_eq!(inline.text_align, Some(TextAlign::Center));
        assert!(matches!(inline.margin_top, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));
    }

    #[test]
    fn test_epictetus_css_styles() {
        // Simplified version of epictetus.epub CSS
        let css = r#"
            p {
                margin-top: 0;
                margin-right: 0;
                margin-bottom: 0;
                margin-left: 0;
                text-indent: 1em;
            }

            blockquote {
                margin-top: 1em;
                margin-right: 2.5em;
                margin-bottom: 1em;
                margin-left: 2.5em;
            }

            h1, h2, h3, h4, h5, h6 {
                margin-top: 3em;
                margin-bottom: 3em;
                text-align: center;
            }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Check paragraph styles
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert!(matches!(p_style.text_indent, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // Check blockquote styles
        let bq_style = get_style_for(&stylesheet, "<blockquote>Test</blockquote>", "blockquote");
        assert!(matches!(bq_style.margin_left, Some(CssValue::Em(e)) if (e - 2.5).abs() < 0.01));
        assert!(matches!(bq_style.margin_right, Some(CssValue::Em(e)) if (e - 2.5).abs() < 0.01));

        // Check h1-h6 grouped selector
        let h1_style = get_style_for(&stylesheet, "<h1>Test</h1>", "h1");
        assert_eq!(h1_style.text_align, Some(TextAlign::Center));
        assert!(matches!(h1_style.margin_top, Some(CssValue::Em(e)) if (e - 3.0).abs() < 0.01));

        let h3_style = get_style_for(&stylesheet, "<h3>Test</h3>", "h3");
        assert_eq!(h3_style.text_align, Some(TextAlign::Center));
    }

    #[test]
    fn test_descendant_selector() {
        // Test proper descendant selector matching (only possible with DOM-based selectors)
        let css = r#"
            div p { color: red; }
            p { color: blue; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // p inside div should match "div p" selector
        let nested_style = get_style_for(&stylesheet, "<div><p>Test</p></div>", "p");
        // Both selectors match, but "div p" is more specific (0,0,2 vs 0,0,1)
        // Actually they have same specificity but "div p" comes first
        // Wait, specificity of "div p" is 0,0,2 (two element selectors)
        // and "p" is 0,0,1 (one element selector)
        // So "div p" should win
        assert!(nested_style.color.is_some());

        // Standalone p should only match "p" selector
        let standalone_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert!(standalone_style.color.is_some());
    }

    #[test]
    fn test_font_variant_small_caps() {
        let css = r#"
            h1 { font-variant: small-caps; }
            .normal { font-variant: normal; }
            strong { font-variant: small-caps; font-weight: normal; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // h1 should have small-caps
        let h1_style = get_style_for(&stylesheet, "<h1>Test</h1>", "h1");
        assert_eq!(h1_style.font_variant, Some(FontVariant::SmallCaps));

        // .normal should have normal
        let normal_style = get_style_for(&stylesheet, r#"<div class="normal">Test</div>"#, "div");
        assert_eq!(normal_style.font_variant, Some(FontVariant::Normal));

        // strong should have small-caps
        let strong_style = get_style_for(&stylesheet, "<strong>Test</strong>", "strong");
        assert_eq!(strong_style.font_variant, Some(FontVariant::SmallCaps));
    }

    #[test]
    fn test_font_size_various_values() {
        let css = r#"
            .small { font-size: 0.67em; }
            .medium-small { font-size: 0.83em; }
            .normal { font-size: 1em; }
            .large { font-size: 1.17em; }
            .larger { font-size: 1.5em; }
            .percent-small { font-size: 67%; }
            .percent-large { font-size: 150%; }
            .smaller { font-size: smaller; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // 0.67em
        let small = get_style_for(&stylesheet, r#"<div class="small">Test</div>"#, "div");
        assert!(matches!(small.font_size, Some(CssValue::Em(e)) if (e - 0.67).abs() < 0.01));

        // 0.83em
        let med_small = get_style_for(
            &stylesheet,
            r#"<div class="medium-small">Test</div>"#,
            "div",
        );
        assert!(matches!(med_small.font_size, Some(CssValue::Em(e)) if (e - 0.83).abs() < 0.01));

        // 1em
        let normal = get_style_for(&stylesheet, r#"<div class="normal">Test</div>"#, "div");
        assert!(matches!(normal.font_size, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // 1.17em
        let large = get_style_for(&stylesheet, r#"<div class="large">Test</div>"#, "div");
        assert!(matches!(large.font_size, Some(CssValue::Em(e)) if (e - 1.17).abs() < 0.01));

        // 67%
        let pct_small = get_style_for(
            &stylesheet,
            r#"<div class="percent-small">Test</div>"#,
            "div",
        );
        assert!(
            matches!(pct_small.font_size, Some(CssValue::Percent(p)) if (p - 67.0).abs() < 0.01)
        );

        // smaller keyword
        let smaller = get_style_for(&stylesheet, r#"<div class="smaller">Test</div>"#, "div");
        assert!(matches!(smaller.font_size, Some(CssValue::Keyword(ref k)) if k == "smaller"));
    }

    #[test]
    fn test_bold_strong_font_weight_normal() {
        // Standard Ebooks uses b/strong with font-weight: normal for semantic markup
        // This tests that we correctly parse explicit font-weight: normal
        let css = r#"
            b, strong {
                font-variant: small-caps;
                font-weight: normal;
            }
            .bold { font-weight: bold; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // b element should have font-weight: normal (NOT bold)
        let b_style = get_style_for(&stylesheet, "<b>Test</b>", "b");
        assert_eq!(
            b_style.font_weight,
            Some(FontWeight::Normal),
            "b element should have font-weight: normal"
        );
        assert_eq!(
            b_style.font_variant,
            Some(FontVariant::SmallCaps),
            "b element should have small-caps"
        );

        // strong element should also have font-weight: normal
        let strong_style = get_style_for(&stylesheet, "<strong>Test</strong>", "strong");
        assert_eq!(
            strong_style.font_weight,
            Some(FontWeight::Normal),
            "strong element should have font-weight: normal"
        );

        // .bold class should have font-weight: bold
        let bold_style = get_style_for(&stylesheet, r#"<span class="bold">Test</span>"#, "span");
        assert_eq!(
            bold_style.font_weight,
            Some(FontWeight::Bold),
            ".bold class should have font-weight: bold"
        );
    }

    #[test]
    fn test_hidden_elements_detection() {
        // Elements with position: absolute and left: -999em should be detected as hidden
        let css = r#"
            .hidden {
                position: absolute;
                left: -999em;
            }
            .visible {
                position: relative;
            }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Hidden element should have display: none or be detectable as hidden
        let hidden_style = get_style_for(&stylesheet, r#"<div class="hidden">Test</div>"#, "div");
        assert!(
            hidden_style.is_hidden(),
            "Element with position:absolute; left:-999em should be hidden"
        );

        // Visible element should not be hidden
        let visible_style = get_style_for(&stylesheet, r#"<div class="visible">Test</div>"#, "div");
        assert!(
            !visible_style.is_hidden(),
            "Element with position:relative should not be hidden"
        );
    }

    #[test]
    fn test_display_none_detection() {
        let css = r#"
            .hidden { display: none; }
            .block { display: block; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let hidden_style = get_style_for(&stylesheet, r#"<div class="hidden">Test</div>"#, "div");
        assert!(hidden_style.is_hidden(), "display:none should be hidden");

        let block_style = get_style_for(&stylesheet, r#"<div class="block">Test</div>"#, "div");
        assert!(
            !block_style.is_hidden(),
            "display:block should not be hidden"
        );
    }
}
