//! CSS cascade implementation.
//!
//! This module implements the CSS cascade algorithm that resolves
//! which style declarations apply to an element based on specificity,
//! importance, and source order.

use std::cmp::Ordering;
use std::collections::HashMap;

use selectors::context::{MatchingContext, SelectorCaches};
use selectors::parser::{Component, Selector};

use super::declaration::Declaration;
use super::parse::{CssRule, Origin, Specificity, Stylesheet};
use super::style_pool::StylePool;
use super::types::ComputedStyle;
use crate::dom::element_ref::{BokoSelectors, ElementRef};

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

/// A reference to a rule as `(stylesheet index, rule index)`. Ordering by this
/// tuple reproduces CSS source order, which the cascade uses as its final
/// tiebreak — so candidate rules must be visited in this order.
type RuleRef = (u32, u32);

/// The bucket a selector is filed under: the single most-selective *positive*
/// requirement (id > class > local name) in its rightmost compound selector.
/// Selectors with no such requirement (`*`, attribute-only, `:is()`/`:not()`
/// with no bare tag/class/id, ...) fall in the universal bucket and are always
/// checked, so a rule that could match is never skipped.
enum BucketKey {
    Id(String),
    Class(String),
    Local(String),
    Universal,
}

/// Determine the bucket key for one selector from its rightmost compound.
///
/// `Selector::iter()` yields exactly the rightmost compound's components (it
/// stops at the first combinator). We only ever read a *positive* tag/class/id
/// requirement; combinators, attribute selectors, negations and functional
/// pseudo-classes are ignored, which keeps the key conservative — the full
/// `matches_selector` still runs on every candidate.
fn selector_bucket_key(selector: &Selector<BokoSelectors>) -> BucketKey {
    let mut id: Option<String> = None;
    let mut class: Option<String> = None;
    let mut local: Option<String> = None;
    for component in selector.iter() {
        match component {
            Component::ID(v) if id.is_none() => id = Some(v.0.clone()),
            Component::Class(v) if class.is_none() => class = Some(v.0.clone()),
            Component::LocalName(name) if local.is_none() => {
                local = Some(name.lower_name.as_ref().to_ascii_lowercase());
            }
            _ => {}
        }
    }
    if let Some(id) = id {
        BucketKey::Id(id)
    } else if let Some(class) = class {
        BucketKey::Class(class)
    } else if let Some(local) = local {
        BucketKey::Local(local)
    } else {
        BucketKey::Universal
    }
}

/// Reusable per-element scratch state for [`compute_styles_indexed`].
///
/// Owning this across a whole chapter avoids re-allocating the candidate
/// list and rebuilding the selectors crate's matching caches for every
/// element (the caches are designed to be shared across a traversal).
#[derive(Default)]
pub struct CascadeScratch {
    caches: SelectorCaches,
    candidates: Vec<RuleRef>,
}

/// A selector-bucketed view of a set of stylesheets, so that computing styles
/// for an element only tests rules whose rightmost compound could match it,
/// instead of every rule of every stylesheet (O(elements × rules)).
pub struct CascadeIndex<'a> {
    stylesheets: &'a [(&'a Stylesheet, Origin)],
    by_id: HashMap<String, Vec<RuleRef>>,
    by_class: HashMap<String, Vec<RuleRef>>,
    by_local: HashMap<String, Vec<RuleRef>>,
    universal: Vec<RuleRef>,
}

impl<'a> CascadeIndex<'a> {
    /// Build the index by bucketing every selector of every rule.
    ///
    /// Takes borrowed stylesheets so callers can share parsed sheets (e.g.
    /// `Arc<Stylesheet>` caches) across chapters without deep-cloning rules.
    pub fn build(stylesheets: &'a [(&'a Stylesheet, Origin)]) -> Self {
        let mut index = CascadeIndex {
            stylesheets,
            by_id: HashMap::new(),
            by_class: HashMap::new(),
            by_local: HashMap::new(),
            universal: Vec::new(),
        };
        for (sheet_idx, (sheet, _origin)) in stylesheets.iter().enumerate() {
            for (rule_idx, rule) in sheet.rules.iter().enumerate() {
                let rref = (sheet_idx as u32, rule_idx as u32);
                // A rule matches if any of its selectors match, so file it under
                // each selector's bucket.
                for selector in &rule.selectors {
                    match selector_bucket_key(selector) {
                        BucketKey::Id(k) => index.by_id.entry(k).or_default().push(rref),
                        BucketKey::Class(k) => index.by_class.entry(k).or_default().push(rref),
                        BucketKey::Local(k) => index.by_local.entry(k).or_default().push(rref),
                        BucketKey::Universal => index.universal.push(rref),
                    }
                }
            }
        }
        index
    }

    /// Fill `out` with the candidate rules for an element, in source order and
    /// de-duplicated. Any rule not returned provably cannot match: matching a
    /// bucketed selector requires the element to carry that id/class/tag.
    fn candidate_rules(&self, elem: ElementRef<'_>, out: &mut Vec<RuleRef>) {
        out.clear();
        out.extend_from_slice(&self.universal);
        // Look up by lowercased tag; the full matcher decides case. Lowercasing
        // only widens the candidate set, so it can never drop a real match.
        // html5ever already lowercases HTML local names, so allocating a
        // lowercase copy is only needed in the rare uppercase case.
        if let Some(name) = elem.dom.element_name(elem.id) {
            let name = name.as_ref();
            let bucket = if name.bytes().any(|b| b.is_ascii_uppercase()) {
                self.by_local.get(name.to_ascii_lowercase().as_str())
            } else {
                self.by_local.get(name)
            };
            if let Some(v) = bucket {
                out.extend_from_slice(v);
            }
        }
        if let Some(id) = elem.dom.element_id(elem.id)
            && let Some(v) = self.by_id.get(id)
        {
            out.extend_from_slice(v);
        }
        for class in elem.dom.element_classes(elem.id) {
            if let Some(v) = self.by_class.get(class.as_str()) {
                out.extend_from_slice(v);
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// Compute styles for an element by applying the cascade.
///
/// Builds a one-shot [`CascadeIndex`]. Callers that compute styles for many
/// elements against the same stylesheets should build the index once and call
/// [`compute_styles_indexed`] instead.
pub fn compute_styles(
    elem: ElementRef<'_>,
    stylesheets: &[(Stylesheet, Origin)],
    parent_style: Option<&ComputedStyle>,
    style_pool: &mut StylePool,
) -> ComputedStyle {
    let refs: Vec<(&Stylesheet, Origin)> = stylesheets.iter().map(|(s, o)| (s, *o)).collect();
    let index = CascadeIndex::build(&refs);
    compute_styles_indexed(
        elem,
        &index,
        parent_style,
        style_pool,
        &mut CascadeScratch::default(),
    )
}

/// Compute styles for an element using a prebuilt [`CascadeIndex`].
pub fn compute_styles_indexed(
    elem: ElementRef<'_>,
    index: &CascadeIndex<'_>,
    parent_style: Option<&ComputedStyle>,
    _style_pool: &mut StylePool,
    scratch: &mut CascadeScratch,
) -> ComputedStyle {
    // Pre-allocate with typical capacity (most elements match 5-20 declarations)
    let mut matched: Vec<MatchedRule> = Vec::with_capacity(16);
    let mut order = 0;

    let CascadeScratch { caches, candidates } = scratch;
    index.candidate_rules(elem, candidates);

    // Candidate rules are already in source order, so `order` reproduces the
    // exhaustive-scan cascade exactly.
    for &(sheet_idx, rule_idx) in candidates.iter() {
        let (stylesheet, origin) = index.stylesheets[sheet_idx as usize];
        let rule = &stylesheet.rules[rule_idx as usize];
        if rule_matches_with_caches(elem, rule, caches) {
            // Collect normal declarations
            for decl in &rule.declarations {
                matched.push(MatchedRule {
                    declaration: decl,
                    origin,
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
                    origin,
                    specificity: rule.specificity,
                    order,
                    important: true,
                });
                order += 1;
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
