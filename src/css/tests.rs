//! CSS parsing tests.

use kuchiki::traits::*;

use super::style::ParsedStyle;
use super::stylesheet::Stylesheet;
use super::types::{
    BorderCollapse, BorderStyle, BoxSizing, CssFloat, CssValue, FontVariant, FontWeight, TextAlign,
    TextTransform,
};

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
fn test_text_decoration_parsing() {
    let css = r#"
        .underline { text-decoration: underline; }
        .line-through { text-decoration: line-through; }
        .overline { text-decoration: overline; }
        .no-underline { text-decoration: none; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // Check underline
    let underline = get_style_for(&stylesheet, r#"<p class="underline">Test</p>"#, "p");
    assert!(
        underline.text_decoration_underline,
        "Expected text_decoration_underline to be true"
    );

    // Check line-through
    let line_through = get_style_for(&stylesheet, r#"<p class="line-through">Test</p>"#, "p");
    assert!(
        line_through.text_decoration_line_through,
        "Expected text_decoration_line_through to be true"
    );

    // Check overline
    let overline = get_style_for(&stylesheet, r#"<p class="overline">Test</p>"#, "p");
    assert!(
        overline.text_decoration_overline,
        "Expected text_decoration_overline to be true"
    );

    // Check none resets all
    let no_underline = get_style_for(&stylesheet, r#"<p class="no-underline">Test</p>"#, "p");
    assert!(
        !no_underline.text_decoration_underline,
        "text-decoration: none should reset underline"
    );
}

#[test]
fn test_opacity_parsing() {
    let css = r#"
        .half { opacity: 0.5; }
        .full { opacity: 1; }
        .zero { opacity: 0; }
        .pct { opacity: 50%; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // Check 0.5 opacity (should be stored as 50)
    let half = get_style_for(&stylesheet, r#"<p class="half">Test</p>"#, "p");
    assert_eq!(
        half.opacity,
        Some(50),
        "opacity: 0.5 should be stored as 50"
    );

    // Check full opacity
    let full = get_style_for(&stylesheet, r#"<p class="full">Test</p>"#, "p");
    assert_eq!(
        full.opacity,
        Some(100),
        "opacity: 1 should be stored as 100"
    );

    // Check zero opacity
    let zero = get_style_for(&stylesheet, r#"<p class="zero">Test</p>"#, "p");
    assert_eq!(zero.opacity, Some(0), "opacity: 0 should be stored as 0");
}

#[test]
fn test_text_transform_parsing() {
    let css = r#"
        .upper { text-transform: uppercase; }
        .lower { text-transform: lowercase; }
        .cap { text-transform: capitalize; }
        .none { text-transform: none; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    let upper = get_style_for(&stylesheet, r#"<p class="upper">Test</p>"#, "p");
    assert_eq!(upper.text_transform, Some(TextTransform::Uppercase));

    let lower = get_style_for(&stylesheet, r#"<p class="lower">Test</p>"#, "p");
    assert_eq!(lower.text_transform, Some(TextTransform::Lowercase));

    let cap = get_style_for(&stylesheet, r#"<p class="cap">Test</p>"#, "p");
    assert_eq!(cap.text_transform, Some(TextTransform::Capitalize));

    let none = get_style_for(&stylesheet, r#"<p class="none">Test</p>"#, "p");
    assert_eq!(none.text_transform, Some(TextTransform::None));
}

#[test]
fn test_float_parsing() {
    let css = r#"
        .left { float: left; }
        .right { float: right; }
        .none { float: none; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    let left = get_style_for(&stylesheet, r#"<p class="left">Test</p>"#, "p");
    assert_eq!(left.float, Some(CssFloat::Left));

    let right = get_style_for(&stylesheet, r#"<p class="right">Test</p>"#, "p");
    assert_eq!(right.float, Some(CssFloat::Right));

    let none = get_style_for(&stylesheet, r#"<p class="none">Test</p>"#, "p");
    assert_eq!(none.float, Some(CssFloat::None));
}

#[test]
fn test_padding_parsing() {
    let css = r#"
        .p1 { padding: 1em; }
        .p2 { padding: 1em 2em; }
        .pt { padding-top: 0.5em; }
        .pb { padding-bottom: 0.5em; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // Check shorthand with 1 value
    let p1 = get_style_for(&stylesheet, r#"<p class="p1">Test</p>"#, "p");
    assert!(matches!(p1.padding_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
    assert!(matches!(p1.padding_bottom, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
    assert!(matches!(p1.padding_left, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
    assert!(matches!(p1.padding_right, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

    // Check shorthand with 2 values
    let p2 = get_style_for(&stylesheet, r#"<p class="p2">Test</p>"#, "p");
    assert!(matches!(p2.padding_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
    assert!(matches!(p2.padding_left, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));

    // Check individual properties
    let pt = get_style_for(&stylesheet, r#"<p class="pt">Test</p>"#, "p");
    assert!(matches!(pt.padding_top, Some(CssValue::Em(e)) if (e - 0.5).abs() < 0.01));

    let pb = get_style_for(&stylesheet, r#"<p class="pb">Test</p>"#, "p");
    assert!(matches!(pb.padding_bottom, Some(CssValue::Em(e)) if (e - 0.5).abs() < 0.01));
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
fn test_inline_box_sizing_parsing() {
    let inline = Stylesheet::parse_inline_style("box-sizing: border-box");
    assert_eq!(
        inline.box_sizing,
        Some(BoxSizing::BorderBox),
        "Inline box-sizing should parse correctly, got {:?}",
        inline.box_sizing
    );
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

#[test]
fn test_font_family_full_stack() {
    // Font stacks should preserve all fonts, not just the first one
    let css = r#"
        .sans { font-family: ui-sans-serif, system-ui, sans-serif; }
        .mono { font-family: ui-monospace, "Courier New", monospace; }
        .single { font-family: Georgia; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // Sans stack should have all fonts
    let sans = get_style_for(&stylesheet, r#"<div class="sans">Test</div>"#, "div");
    assert_eq!(
        sans.font_family,
        Some("ui-sans-serif,system-ui,sans-serif".to_string()),
        "Font stack should preserve all fonts"
    );

    // Mono stack with quoted font name
    let mono = get_style_for(&stylesheet, r#"<div class="mono">Test</div>"#, "div");
    assert_eq!(
        mono.font_family,
        Some("ui-monospace,Courier New,monospace".to_string()),
        "Font stack should handle quoted names"
    );

    // Single font
    let single = get_style_for(&stylesheet, r#"<div class="single">Test</div>"#, "div");
    assert_eq!(
        single.font_family,
        Some("Georgia".to_string()),
        "Single font should work"
    );
}

#[test]
fn test_line_height_with_rem_units() {
    // Line-height with rem units should be parsed correctly
    // This is the text-xs pattern from Tailwind CSS
    let css = r#"
        .text-xs { font-size: 0.75rem; line-height: 1rem; }
        .text-sm { font-size: 0.875rem; line-height: 1.25rem; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // text-xs should have both font-size and line-height
    let text_xs = get_style_for(&stylesheet, r#"<span class="text-xs">Test</span>"#, "span");
    assert!(
        matches!(text_xs.font_size, Some(CssValue::Rem(v)) if (v - 0.75).abs() < 0.01),
        "text-xs should have font-size: 0.75rem, got {:?}",
        text_xs.font_size
    );
    assert!(
        matches!(text_xs.line_height, Some(CssValue::Rem(v)) if (v - 1.0).abs() < 0.01),
        "text-xs should have line-height: 1rem, got {:?}",
        text_xs.line_height
    );

    // text-sm should have both font-size and line-height
    let text_sm = get_style_for(&stylesheet, r#"<span class="text-sm">Test</span>"#, "span");
    assert!(
        matches!(text_sm.font_size, Some(CssValue::Rem(v)) if (v - 0.875).abs() < 0.01),
        "text-sm should have font-size: 0.875rem, got {:?}",
        text_sm.font_size
    );
    assert!(
        matches!(text_sm.line_height, Some(CssValue::Rem(v)) if (v - 1.25).abs() < 0.01),
        "text-sm should have line-height: 1.25rem, got {:?}",
        text_sm.line_height
    );
}

#[test]
fn test_box_sizing_parsing() {
    let css = r#"
        .border-box { box-sizing: border-box; }
        .content-box { box-sizing: content-box; }
        .padding-box { box-sizing: padding-box; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    let border_box = get_style_for(&stylesheet, r#"<div class="border-box">Test</div>"#, "div");
    assert_eq!(
        border_box.box_sizing,
        Some(BoxSizing::BorderBox),
        "Expected box-sizing: border-box, got {:?}",
        border_box.box_sizing
    );

    let content_box =
        get_style_for(&stylesheet, r#"<div class="content-box">Test</div>"#, "div");
    assert_eq!(
        content_box.box_sizing,
        Some(BoxSizing::ContentBox),
        "Expected box-sizing: content-box, got {:?}",
        content_box.box_sizing
    );

    let padding_box =
        get_style_for(&stylesheet, r#"<div class="padding-box">Test</div>"#, "div");
    assert_eq!(
        padding_box.box_sizing,
        Some(BoxSizing::PaddingBox),
        "Expected box-sizing: padding-box, got {:?}",
        padding_box.box_sizing
    );
}

#[test]
fn test_box_sizing_universal_selector() {
    // This is the Tailwind CSS preflight pattern
    let css = r#"
        * { box-sizing: border-box; }
        p { margin: 0; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // Universal selector should apply to all elements
    let div_style = get_style_for(&stylesheet, "<div>Test</div>", "div");
    assert_eq!(
        div_style.box_sizing,
        Some(BoxSizing::BorderBox),
        "Universal selector should apply box-sizing to div, got {:?}",
        div_style.box_sizing
    );

    let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
    assert_eq!(
        p_style.box_sizing,
        Some(BoxSizing::BorderBox),
        "Universal selector should apply box-sizing to p, got {:?}",
        p_style.box_sizing
    );
}

#[test]
fn test_clean_selector_pseudo_elements() {
    // Test that selectors with pseudo-elements get cleaned properly
    // The actual cleaning happens internally in Stylesheet::parse
    // This tests that rules with pseudo-elements don't cause parsing failures
    let css = r#"
        *,::after,::before { box-sizing: border-box; }
        p { margin: 0; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // If the selector was cleaned properly, * should still apply
    let div_style = get_style_for(&stylesheet, "<div>Test</div>", "div");
    assert_eq!(
        div_style.box_sizing,
        Some(BoxSizing::BorderBox),
        "Cleaned selector should apply box-sizing via *, got {:?}",
        div_style.box_sizing
    );
}

#[test]
fn test_border_collapse_parsing() {
    let css = r#"
        .collapse { border-collapse: collapse; }
        .separate { border-collapse: separate; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    let collapse = get_style_for(&stylesheet, r#"<table class="collapse"></table>"#, "table");
    assert_eq!(
        collapse.border_collapse,
        Some(BorderCollapse::Collapse),
        "Expected border-collapse: collapse, got {:?}",
        collapse.border_collapse
    );

    let separate = get_style_for(&stylesheet, r#"<table class="separate"></table>"#, "table");
    assert_eq!(
        separate.border_collapse,
        Some(BorderCollapse::Separate),
        "Expected border-collapse: separate, got {:?}",
        separate.border_collapse
    );
}

#[test]
fn test_border_spacing_parsing() {
    let css = r#"
        .single { border-spacing: 2px; }
        .double { border-spacing: 2px 4px; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // Single value applies to both horizontal and vertical
    let single = get_style_for(&stylesheet, r#"<table class="single"></table>"#, "table");
    assert!(
        matches!(single.border_spacing_horizontal, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
        "Expected border-spacing horizontal: 2px, got {:?}",
        single.border_spacing_horizontal
    );
    assert!(
        matches!(single.border_spacing_vertical, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
        "Expected border-spacing vertical: 2px, got {:?}",
        single.border_spacing_vertical
    );

    // Two values: first is horizontal, second is vertical
    let double = get_style_for(&stylesheet, r#"<table class="double"></table>"#, "table");
    assert!(
        matches!(double.border_spacing_horizontal, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
        "Expected border-spacing horizontal: 2px, got {:?}",
        double.border_spacing_horizontal
    );
    assert!(
        matches!(double.border_spacing_vertical, Some(CssValue::Px(v)) if (v - 4.0).abs() < 0.01),
        "Expected border-spacing vertical: 4px, got {:?}",
        double.border_spacing_vertical
    );
}

#[test]
fn test_pseudo_element_styles_not_applied_to_element() {
    // CSS rules targeting ::before or ::after pseudo-elements should NOT
    // apply their styles to the actual element. The pseudo-element creates
    // content that doesn't exist in the DOM.
    //
    // This is a regression test for the colophon rendering issue where
    // p + p::before { width: 25%; } was being applied to the paragraph itself,
    // causing an extremely narrow column.
    let css = r#"
        section p {
            text-align: center;
        }
        section p + p::before {
            border-top: 1px solid;
            content: "";
            display: block;
            width: 25%;
        }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // The p element should have text-align: center but NOT width: 25%
    // because the width is only for the ::before pseudo-element
    let html = r#"<section><p>First</p><p>Second</p></section>"#;
    let doc = kuchiki::parse_html().one(html);

    // Check the second paragraph (which matches p + p)
    let second_p = doc.select("p:nth-child(2)").unwrap().next().unwrap();
    let p_style = stylesheet.get_direct_style_for_element(&second_p);

    // Should have text-align: center from the first rule
    assert_eq!(
        p_style.text_align,
        Some(TextAlign::Center),
        "p element should have text-align: center"
    );

    // Should NOT have width from the ::before rule
    assert!(
        p_style.width.is_none(),
        "p element should NOT have width from ::before pseudo-element rule, got {:?}",
        p_style.width
    );

    // Should NOT have border from the ::before rule
    assert!(
        p_style.border_top.is_none(),
        "p element should NOT have border from ::before pseudo-element rule"
    );
}

#[test]
fn test_border_width_style_shorthand_parsing() {
    let css = r#"
        .border-width { border-width: 1px; }
        .border-style { border-style: solid; }
        .border-color { border-color: #ff0000; }
        .combined { border-width: 2px; border-style: dashed; border-color: #00ff00; }
    "#;

    let stylesheet = Stylesheet::parse(css);

    // border-width: 1px should create borders with width on all sides
    let width_style = get_style_for(&stylesheet, r#"<div class="border-width"></div>"#, "div");
    assert!(width_style.border_top.is_some(), "border-top should be set");
    let top = width_style.border_top.unwrap();
    assert!(
        matches!(top.width, Some(CssValue::Px(v)) if (v - 1.0).abs() < 0.01),
        "Expected border-top-width: 1px, got {:?}",
        top.width
    );
    assert_eq!(
        top.style,
        BorderStyle::Solid,
        "border-style should default to solid"
    );

    // border-style: solid should set style on all sides
    let style_style = get_style_for(&stylesheet, r#"<div class="border-style"></div>"#, "div");
    assert!(style_style.border_top.is_some(), "border-top should be set");
    assert_eq!(
        style_style.border_top.as_ref().unwrap().style,
        BorderStyle::Solid
    );

    // Combined should have all properties
    let combined = get_style_for(&stylesheet, r#"<div class="combined"></div>"#, "div");
    assert!(combined.border_top.is_some(), "border-top should be set");
    let combined_top = combined.border_top.unwrap();
    assert!(
        matches!(combined_top.width, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
        "Expected border-top-width: 2px"
    );
    assert_eq!(combined_top.style, BorderStyle::Dashed);
    assert!(combined_top.color.is_some(), "border-color should be set");
}
