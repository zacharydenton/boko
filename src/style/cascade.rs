//! CSS cascade implementation.
//!
//! This module implements the CSS cascade algorithm that resolves
//! which style declarations apply to an element based on specificity,
//! importance, and source order.

use std::cmp::Ordering;

use selectors::context::{MatchingContext, SelectorCaches};

use super::declaration::Declaration;
use super::parse::{CssRule, Origin, Specificity, Stylesheet};
use super::types::{ComputedStyle, StylePool};
use crate::dom::element_ref::ElementRef;

/// A matched rule with ordering information for the cascade.
#[derive(Debug)]
struct MatchedRule<'a> {
    declaration: &'a Declaration,
    origin: Origin,
    specificity: Specificity,
    order: usize,
    important: bool,
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
        Declaration::WhiteSpace(ws) => style.white_space = *ws,
        Declaration::VerticalAlign(v) => style.vertical_align = *v,

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
