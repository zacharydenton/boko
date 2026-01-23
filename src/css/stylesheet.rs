//! Stylesheet parsing and rule matching.
//!
//! Contains the Stylesheet struct for parsing CSS and matching selectors,
//! along with the CSS rule parser implementation.

use cssparser::{
    AtRuleParser, AtRuleType, BasicParseErrorKind, CowRcStr, ParseError, Parser, ParserInput,
    QualifiedRuleParser, RuleListParser, SourceLocation,
};
use kuchiki::{ElementData, NodeDataRef, Selectors};

use super::parsing::parse_declaration_block;
use super::style::ParsedStyle;

/// Re-export kuchiki types for external use
pub use kuchiki::NodeRef;

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
                // Pre-process selector to remove pseudo-elements that kuchiki doesn't support
                // E.g., "*,::after,::before" becomes just "*"
                let cleaned = clean_selector(&selector_str);
                if cleaned.is_empty() {
                    return None;
                }
                Selectors::compile(&cleaned)
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

/// Clean a CSS selector by removing pseudo-elements that kuchiki doesn't support.
/// This allows rules like `*,::after,::before { ... }` to work.
fn clean_selector(selector: &str) -> String {
    // Split by comma to handle selector lists
    let parts: Vec<&str> = selector.split(',').collect();

    // Filter out pseudo-element selectors and clean the remaining ones
    let cleaned: Vec<String> = parts
        .iter()
        .map(|s| s.trim())
        // Remove parts that are just pseudo-elements
        .filter(|s| !s.starts_with("::") && !s.starts_with(':'))
        // For parts that contain pseudo-elements, strip them
        .map(|s| {
            // Remove ::before, ::after, etc. from the end
            if let Some(idx) = s.find("::") {
                s[..idx].trim().to_string()
            } else if let Some(idx) = s.find(':') {
                // Also handle single-colon pseudo-classes like :hover
                // But preserve structural pseudo-classes if they're the whole selector
                let before = &s[..idx];
                if before.is_empty() {
                    // Pure pseudo-class like :root or :host
                    s.to_string()
                } else {
                    before.trim().to_string()
                }
            } else {
                s.to_string()
            }
        })
        // Filter out empty strings
        .filter(|s| !s.is_empty())
        .collect();

    cleaned.join(", ")
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
