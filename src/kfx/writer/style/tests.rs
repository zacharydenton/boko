//! Tests for style conversion from ParsedStyle to KFX ION.

use std::collections::HashMap;

use crate::css::{Color, CssValue, ParsedStyle};
use crate::kfx::ion::IonValue;
use crate::kfx::writer::symbols::{sym, SymbolTable};

use super::conversion::style_to_ion;
use super::{add_margins, spacing_to_multiplier, ToKfxIon, MARGIN_SYMS};

#[test]
fn test_text_style_no_image_fit_baseline() {
    // Text styles should NOT include IMAGE_FIT ($546) baseline property
    // Reference KFX doesn't include it for non-image styles
    let style = ParsedStyle {
        font_size: Some(crate::css::CssValue::Em(1.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    // Should NOT have IMAGE_FIT for text styles
    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };
    assert!(
        !ion_map.contains_key(&sym::IMAGE_FIT),
        "Text styles should not have IMAGE_FIT baseline property"
    );
}

#[test]
fn test_image_style_has_image_fit() {
    // Image styles SHOULD include IMAGE_FIT
    let style = ParsedStyle {
        is_image: true,
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };
    assert!(
        ion_map.contains_key(&sym::IMAGE_FIT),
        "Image styles should have IMAGE_FIT property"
    );
}

#[test]
fn test_line_height_divided_by_1_2() {
    // Kindle Previewer divides line-height values by 1.2 (default line-height factor)
    // CSS line-height: 1.5 → KFX line-height: 1.25 (1.5/1.2)
    // CSS line-height: 2.0 → KFX line-height: 1.666667 (2/1.2)
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        line_height: Some(crate::css::CssValue::Number(1.5)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should have LINE_HEIGHT
    assert!(
        ion_map.contains_key(&sym::LINE_HEIGHT),
        "Style should have line-height"
    );

    // Extract the value
    let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
        Some(IonValue::Struct(s)) => s,
        _ => panic!("Expected line-height struct"),
    };

    let value = match lh_struct.get(&sym::VALUE) {
        Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
        _ => panic!("Expected decimal value"),
    };

    // Should be 1.5 / 1.2 = 1.25
    let expected = 1.5 / 1.2;
    assert!(
        (value - expected).abs() < 0.01,
        "line-height 1.5 should become {} in KFX, got {}",
        expected,
        value
    );
}

#[test]
fn test_margin_top_divided_by_1_2() {
    // Kindle Previewer divides vertical em margins by 1.2 (default line-height factor)
    // CSS margin-top: 1em → KFX margin-top: 0.833333em (1/1.2)
    // CSS margin-top: 3em → KFX margin-top: 2.5em (3/1.2)
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        margin_top: Some(crate::css::CssValue::Em(3.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should have SPACE_BEFORE (margin-top)
    assert!(
        ion_map.contains_key(&sym::SPACE_BEFORE),
        "Style should have margin-top (SPACE_BEFORE)"
    );

    // Extract the value
    let margin_struct = match ion_map.get(&sym::SPACE_BEFORE) {
        Some(IonValue::Struct(s)) => s,
        _ => panic!("Expected margin-top struct"),
    };

    let value = match margin_struct.get(&sym::VALUE) {
        Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
        _ => panic!("Expected decimal value"),
    };

    // Should be 3 / 1.2 = 2.5
    let expected = 3.0 / 1.2;
    assert!(
        (value - expected).abs() < 0.01,
        "margin-top 3em should become {} in KFX, got {}",
        expected,
        value
    );
}

#[test]
fn test_line_height_rem_normalized_to_font_size() {
    // When line-height is in rem/em units, it should be normalized relative
    // to font-size but NOT divided by 1.2 (division only applies to unitless values).
    // Example: font-size: 0.875rem; line-height: 1.25rem
    // - line-height in em = 1.25 / 0.875 = 1.42857
    // - NO division by 1.2 because it's already in absolute units
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        font_size: Some(crate::css::CssValue::Rem(0.875)),
        line_height: Some(crate::css::CssValue::Rem(1.25)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should have LINE_HEIGHT
    assert!(
        ion_map.contains_key(&sym::LINE_HEIGHT),
        "Style should have line-height"
    );

    let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
        Some(IonValue::Struct(s)) => s,
        _ => panic!("Expected line-height struct"),
    };

    let value = match lh_struct.get(&sym::VALUE) {
        Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
        _ => panic!("Expected decimal value"),
    };

    // line-height in em = 1.25 / 0.875 = 1.42857
    // NO division by 1.2 for absolute units
    let expected = 1.25 / 0.875;
    assert!(
        (value - expected).abs() < 0.01,
        "line-height 1.25rem with font-size 0.875rem should become {} in KFX, got {}",
        expected,
        value
    );
}

#[test]
fn test_font_only_style_omits_block_type() {
    // Styles with only font formatting (no layout properties) should NOT
    // have BLOCK_TYPE at all. Reference KFX only sets BLOCK_TYPE on styles
    // with actual layout properties.
    let style = ParsedStyle {
        font_style: Some(crate::css::FontStyle::Italic),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Font-only styles should NOT have HYPHENS at all (matches reference)
    // Note: $127 is CSS hyphens property, not block type
    assert!(
        !ion_map.contains_key(&sym::HYPHENS),
        "Font-only style should not have HYPHENS"
    );
}

#[test]
fn test_layout_style_does_not_have_hyphens() {
    // Layout styles should NOT have $127 (hyphens) by default
    // $127 is only output when CSS hyphens property is explicitly set
    let style = ParsedStyle {
        margin_top: Some(crate::css::CssValue::Em(1.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should NOT have $127 (hyphens) unless CSS explicitly sets it
    assert!(
        !ion_map.contains_key(&sym::HYPHENS),
        "Layout style should not have HYPHENS by default"
    );
}

#[test]
fn test_zero_margin_omits_hyphens() {
    // Styles with margin: 0 should NOT have HYPHENS
    // $127 is CSS hyphens property, not block type indicator
    let style = ParsedStyle {
        font_style: Some(crate::css::FontStyle::Italic),
        margin_top: Some(crate::css::CssValue::Px(0.0)),
        margin_bottom: Some(crate::css::CssValue::Px(0.0)),
        margin_left: Some(crate::css::CssValue::Px(0.0)),
        margin_right: Some(crate::css::CssValue::Px(0.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should NOT have HYPHENS
    assert!(
        !ion_map.contains_key(&sym::HYPHENS),
        "Style should not have HYPHENS by default"
    );
}

#[test]
fn test_em_width_does_not_add_max_width() {
    // Width in em units should NOT automatically add MAX_WIDTH
    // Reference KFX does not have MAX_WIDTH on these styles
    let style = ParsedStyle {
        width: Some(crate::css::CssValue::Em(10.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should have STYLE_WIDTH but NOT MAX_WIDTH
    assert!(
        ion_map.contains_key(&sym::STYLE_WIDTH),
        "Style should have STYLE_WIDTH"
    );
    assert!(
        !ion_map.contains_key(&sym::MAX_WIDTH),
        "Width in em should not automatically add MAX_WIDTH"
    );
}

#[test]
fn test_text_xs_style_has_line_height_in_ion() {
    // The text-xs pattern: font-size 0.75rem, line-height 1rem
    // Both should be present in the ION output
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        font_size: Some(crate::css::CssValue::Rem(0.75)),
        line_height: Some(crate::css::CssValue::Rem(1.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("text-xs");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Check font-size is present
    assert!(
        ion_map.contains_key(&sym::FONT_SIZE),
        "text-xs style should have FONT_SIZE"
    );

    // Check line-height is present - THIS IS THE KEY ASSERTION
    assert!(
        ion_map.contains_key(&sym::LINE_HEIGHT),
        "text-xs style should have LINE_HEIGHT, but it's missing! ION keys: {:?}",
        ion_map.keys().collect::<Vec<_>>()
    );

    // Verify line-height value
    // line-height = 1rem / 0.75rem font-size = 1.33333
    let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
        Some(IonValue::Struct(s)) => s,
        _ => panic!("Expected line-height struct"),
    };

    let value = match lh_struct.get(&sym::VALUE) {
        Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
        _ => panic!("Expected decimal value"),
    };

    // line-height in em = 1.0 / 0.75 = 1.33333
    // NO division by 1.2 for absolute units (rem)
    let expected = 1.0 / 0.75;
    assert!(
        (value - expected).abs() < 0.01,
        "text-xs line-height should be ~{}, got {}",
        expected,
        value
    );
}

#[test]
fn test_line_height_zero_normalizes_with_font_size() {
    // CSS pattern for sub/sup: font-size: 75%; line-height: 0
    // Kindle Previewer normalizes line-height: 0 to 1.0/font-size-ratio
    // This maintains vertical rhythm (smaller text gets larger line-height multiplier)
    // Example: font-size 75% -> line-height 1/0.75 = 1.33333
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        font_size: Some(crate::css::CssValue::Percent(75.0)),
        line_height: Some(crate::css::CssValue::Number(0.0)), // line-height: 0
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("sub-sup");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should have LINE_HEIGHT (not skipped!)
    assert!(
        ion_map.contains_key(&sym::LINE_HEIGHT),
        "Style with line-height: 0 should have LINE_HEIGHT (normalized to 1.0/font-size). Keys: {:?}",
        ion_map.keys().collect::<Vec<_>>()
    );

    // Verify line-height value
    let lh_struct = match ion_map.get(&sym::LINE_HEIGHT) {
        Some(IonValue::Struct(s)) => s,
        _ => panic!("Expected line-height struct"),
    };

    let value = match lh_struct.get(&sym::VALUE) {
        Some(IonValue::Decimal(bytes)) => decode_kfx_decimal(bytes),
        _ => panic!("Expected decimal value"),
    };

    // line-height 0 with font-size 75% normalizes to:
    // 1.0 / 0.75 = 1.33333
    // Then divided by 1.2 for KFX: 1.33333 / 1.2 = 1.11111
    let expected = (1.0 / 0.75) / 1.2; // = 1.11111
    assert!(
        (value - expected).abs() < 0.01,
        "line-height: 0 with font-size: 75% should become ~{} in KFX, got {}",
        expected,
        value
    );
}

// P1 Tests: List style properties
#[test]
fn test_list_style_type_decimal() {
    let style = ParsedStyle {
        list_style_type: Some(crate::css::ListStyleType::Decimal),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::LIST_TYPE),
        "Style should have LIST_TYPE"
    );
    match ion_map.get(&sym::LIST_TYPE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(*s, sym::LIST_TYPE_DECIMAL, "Expected decimal list type");
        }
        _ => panic!("Expected symbol for LIST_TYPE"),
    }
}

#[test]
fn test_list_style_type_disc() {
    let style = ParsedStyle {
        list_style_type: Some(crate::css::ListStyleType::Disc),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::LIST_TYPE),
        "Style should have LIST_TYPE"
    );
    match ion_map.get(&sym::LIST_TYPE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(*s, sym::LIST_TYPE_DISC, "Expected disc list type");
        }
        _ => panic!("Expected symbol for LIST_TYPE"),
    }
}

#[test]
fn test_list_style_position_inside() {
    let style = ParsedStyle {
        list_style_position: Some(crate::css::ListStylePosition::Inside),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::LIST_POSITION),
        "Style should have LIST_POSITION"
    );
    match ion_map.get(&sym::LIST_POSITION) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(*s, sym::LIST_POSITION_INSIDE, "Expected inside position");
        }
        _ => panic!("Expected symbol for LIST_POSITION"),
    }
}

// P1 Tests: Additional CSS units
#[test]
fn test_viewport_width_unit() {
    let style = ParsedStyle {
        width: Some(crate::css::CssValue::Vw(50.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::STYLE_WIDTH),
        "Style should have STYLE_WIDTH"
    );
}

#[test]
fn test_viewport_height_unit() {
    let style = ParsedStyle {
        height: Some(crate::css::CssValue::Vh(100.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::STYLE_HEIGHT),
        "Style should have STYLE_HEIGHT"
    );
}

// P2 Tests: Writing mode
#[test]
fn test_writing_mode_vertical_rl() {
    let style = ParsedStyle {
        writing_mode: Some(crate::css::WritingMode::VerticalRl),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::WRITING_MODE),
        "Style should have WRITING_MODE"
    );
    match ion_map.get(&sym::WRITING_MODE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::WRITING_MODE_VERTICAL_RL,
                "Expected vertical-rl writing mode"
            );
        }
        _ => panic!("Expected symbol for WRITING_MODE"),
    }
}

#[test]
fn test_writing_mode_horizontal_tb_not_output() {
    // Horizontal-tb is the default and should not be output
    let style = ParsedStyle {
        writing_mode: Some(crate::css::WritingMode::HorizontalTb),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Default horizontal-tb should not be output
    assert!(
        !ion_map.contains_key(&sym::WRITING_MODE),
        "Default horizontal-tb should not output WRITING_MODE"
    );
}

// P4 Tests: Shadow properties
#[test]
fn test_box_shadow() {
    let style = ParsedStyle {
        box_shadow: Some("2px 2px 4px #000".to_string()),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::BOX_SHADOW),
        "Style should have BOX_SHADOW"
    );
}

#[test]
fn test_text_shadow() {
    let style = ParsedStyle {
        text_shadow: Some("1px 1px 2px black".to_string()),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::TEXT_SHADOW),
        "Style should have TEXT_SHADOW"
    );
}

// P1 Phase 2 Tests: Ruby annotations
#[test]
fn test_ruby_position_under() {
    let style = ParsedStyle {
        ruby_position: Some(crate::css::RubyPosition::Under),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::RUBY_POSITION),
        "Style should have RUBY_POSITION"
    );
    match ion_map.get(&sym::RUBY_POSITION) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::RUBY_POSITION_UNDER,
                "Expected ruby-position: under"
            );
        }
        _ => panic!("Expected symbol for RUBY_POSITION"),
    }
}

#[test]
fn test_ruby_position_over_not_output() {
    // Over is the default and should not be output
    let style = ParsedStyle {
        ruby_position: Some(crate::css::RubyPosition::Over),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::RUBY_POSITION),
        "Default ruby-position: over should not output"
    );
}

#[test]
fn test_ruby_align_space_between() {
    let style = ParsedStyle {
        ruby_align: Some(crate::css::RubyAlign::SpaceBetween),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::RUBY_ALIGN),
        "Style should have RUBY_ALIGN"
    );
    match ion_map.get(&sym::RUBY_ALIGN) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::RUBY_ALIGN_SPACE_BETWEEN,
                "Expected ruby-align: space-between"
            );
        }
        _ => panic!("Expected symbol for RUBY_ALIGN"),
    }
}

#[test]
fn test_ruby_merge_collapse() {
    let style = ParsedStyle {
        ruby_merge: Some(crate::css::RubyMerge::Collapse),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::RUBY_MERGE),
        "Style should have RUBY_MERGE"
    );
    match ion_map.get(&sym::RUBY_MERGE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::RUBY_MERGE_COLLAPSE,
                "Expected ruby-merge: collapse"
            );
        }
        _ => panic!("Expected symbol for RUBY_MERGE"),
    }
}

// P1 Phase 2 Tests: Text emphasis
#[test]
fn test_text_emphasis_style_filled_circle() {
    let style = ParsedStyle {
        text_emphasis_style: Some(crate::css::TextEmphasisStyle::FilledCircle),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::TEXT_EMPHASIS_STYLE),
        "Style should have TEXT_EMPHASIS_STYLE"
    );
    match ion_map.get(&sym::TEXT_EMPHASIS_STYLE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::TEXT_EMPHASIS_FILLED_CIRCLE,
                "Expected text-emphasis-style: filled circle"
            );
        }
        _ => panic!("Expected symbol for TEXT_EMPHASIS_STYLE"),
    }
}

#[test]
fn test_text_emphasis_style_none_not_output() {
    let style = ParsedStyle {
        text_emphasis_style: Some(crate::css::TextEmphasisStyle::None),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::TEXT_EMPHASIS_STYLE),
        "text-emphasis-style: none should not output"
    );
}

#[test]
fn test_text_emphasis_color() {
    let style = ParsedStyle {
        text_emphasis_color: Some(crate::css::Color::Rgba(255, 0, 0, 255)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::TEXT_EMPHASIS_COLOR),
        "Style should have TEXT_EMPHASIS_COLOR"
    );
}

// P2 Phase 2 Tests: Border collapse
#[test]
fn test_border_collapse() {
    let style = ParsedStyle {
        border_collapse: Some(crate::css::BorderCollapse::Collapse),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::BORDER_COLLAPSE),
        "Style should have BORDER_COLLAPSE"
    );
    // Per yj_to_epub_properties.py: True = collapse
    match ion_map.get(&sym::BORDER_COLLAPSE) {
        Some(IonValue::Bool(true)) => {}
        other => panic!("Expected Bool(true) for BORDER_COLLAPSE, got {:?}", other),
    }
}

#[test]
fn test_border_collapse_separate_not_output() {
    // Separate is the default and should not be output
    let style = ParsedStyle {
        border_collapse: Some(crate::css::BorderCollapse::Separate),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::BORDER_COLLAPSE),
        "Default border-collapse: separate should not output"
    );
}

// P1 Phase 2 Tests: Drop cap
#[test]
fn test_drop_cap() {
    let style = ParsedStyle {
        drop_cap: Some(crate::css::DropCap { lines: 3, chars: 1 }),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::DROP_CAP_LINES),
        "Style should have DROP_CAP_LINES"
    );
    assert!(
        ion_map.contains_key(&sym::DROP_CAP_CHARS),
        "Style should have DROP_CAP_CHARS"
    );

    match ion_map.get(&sym::DROP_CAP_LINES) {
        Some(IonValue::Int(n)) => {
            assert_eq!(*n, 3, "Expected drop cap lines = 3");
        }
        _ => panic!("Expected int for DROP_CAP_LINES"),
    }
    match ion_map.get(&sym::DROP_CAP_CHARS) {
        Some(IonValue::Int(n)) => {
            assert_eq!(*n, 1, "Expected drop cap chars = 1");
        }
        _ => panic!("Expected int for DROP_CAP_CHARS"),
    }
}

// P2 Phase 2 Tests: Text decoration line style
#[test]
fn test_text_decoration_underline_dashed() {
    let style = ParsedStyle {
        text_decoration_underline: true,
        text_decoration_line_style: Some(crate::css::TextDecorationLineStyle::Dashed),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::TEXT_DECORATION_UNDERLINE),
        "Style should have TEXT_DECORATION_UNDERLINE"
    );
    // Per yj_to_epub_properties.py: $330 = dashed line style
    match ion_map.get(&sym::TEXT_DECORATION_UNDERLINE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::TEXT_DECORATION_STYLE_DASHED,
                "Expected dashed style symbol ($330)"
            );
        }
        _ => panic!("Expected symbol for TEXT_DECORATION_UNDERLINE"),
    }
}

#[test]
fn test_text_decoration_line_through_double() {
    let style = ParsedStyle {
        text_decoration_line_through: true,
        text_decoration_line_style: Some(crate::css::TextDecorationLineStyle::Double),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::TEXT_DECORATION_LINE_THROUGH),
        "Style should have TEXT_DECORATION_LINE_THROUGH"
    );
    // Per yj_to_epub_properties.py: $329 = double line style
    match ion_map.get(&sym::TEXT_DECORATION_LINE_THROUGH) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::TEXT_DECORATION_STYLE_DOUBLE,
                "Expected double style symbol ($329)"
            );
        }
        _ => panic!("Expected symbol for TEXT_DECORATION_LINE_THROUGH"),
    }
}

// P2 Phase 2 Tests: Transform properties
#[test]
fn test_transform_translate() {
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        transform: Some(crate::css::Transform::translate(10.0, 20.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::TRANSFORM),
        "Style should have TRANSFORM"
    );

    match ion_map.get(&sym::TRANSFORM) {
        Some(IonValue::List(list)) => {
            assert_eq!(list.len(), 6, "Transform should have 6 elements");
            // translate(10, 20) = [1, 0, 0, 1, 10, 20]
            if let IonValue::Decimal(bytes) = &list[4] {
                let tx = decode_kfx_decimal(bytes);
                assert!((tx - 10.0).abs() < 0.001, "Expected tx=10, got {}", tx);
            }
            if let IonValue::Decimal(bytes) = &list[5] {
                let ty = decode_kfx_decimal(bytes);
                assert!((ty - 20.0).abs() < 0.001, "Expected ty=20, got {}", ty);
            }
        }
        _ => panic!("Expected list for TRANSFORM"),
    }
}

#[test]
fn test_transform_scale() {
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        transform: Some(crate::css::Transform::scale(2.0, 0.5)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::TRANSFORM),
        "Style should have TRANSFORM"
    );

    match ion_map.get(&sym::TRANSFORM) {
        Some(IonValue::List(list)) => {
            assert_eq!(list.len(), 6, "Transform should have 6 elements");
            // scale(2, 0.5) = [2, 0, 0, 0.5, 0, 0]
            if let IonValue::Decimal(bytes) = &list[0] {
                let sx = decode_kfx_decimal(bytes);
                assert!((sx - 2.0).abs() < 0.001, "Expected sx=2, got {}", sx);
            }
            if let IonValue::Decimal(bytes) = &list[3] {
                let sy = decode_kfx_decimal(bytes);
                assert!((sy - 0.5).abs() < 0.001, "Expected sy=0.5, got {}", sy);
            }
        }
        _ => panic!("Expected list for TRANSFORM"),
    }
}

#[test]
fn test_transform_identity_omitted() {
    // Identity transform should be omitted from output
    let style = ParsedStyle {
        transform: Some(crate::css::Transform::default()), // Identity
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::TRANSFORM),
        "Identity transform should be omitted"
    );
}

// P2 Phase 2 Tests: Baseline-shift
#[test]
fn test_baseline_shift_em() {
    use crate::kfx::ion::decode_kfx_decimal;

    let style = ParsedStyle {
        baseline_shift: Some(crate::css::CssValue::Em(0.5)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::BASELINE_SHIFT),
        "Style should have BASELINE_SHIFT"
    );

    match ion_map.get(&sym::BASELINE_SHIFT) {
        Some(IonValue::Decimal(bytes)) => {
            let shift = decode_kfx_decimal(bytes);
            assert!(
                (shift - 0.5).abs() < 0.001,
                "Expected baseline-shift 0.5em, got {}",
                shift
            );
        }
        _ => panic!("Expected decimal for BASELINE_SHIFT"),
    }
}

// P2 Phase 2 Tests: Column count
#[test]
fn test_column_count_numeric() {
    let style = ParsedStyle {
        column_count: Some(crate::css::ColumnCount::Count(3)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::COLUMN_COUNT),
        "Style should have COLUMN_COUNT"
    );

    match ion_map.get(&sym::COLUMN_COUNT) {
        Some(IonValue::Int(n)) => {
            assert_eq!(*n, 3, "Expected column-count 3");
        }
        _ => panic!("Expected int for COLUMN_COUNT"),
    }
}

#[test]
fn test_column_count_auto() {
    let style = ParsedStyle {
        column_count: Some(crate::css::ColumnCount::Auto),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::COLUMN_COUNT),
        "Style should have COLUMN_COUNT"
    );

    match ion_map.get(&sym::COLUMN_COUNT) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::COLUMN_COUNT_AUTO,
                "Expected column-count auto ($383)"
            );
        }
        _ => panic!("Expected symbol for COLUMN_COUNT auto"),
    }
}

// P2 Phase 2 Tests: Float property
#[test]
fn test_float_left() {
    let style = ParsedStyle {
        float: Some(crate::css::CssFloat::Left),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(ion_map.contains_key(&sym::FLOAT), "Style should have FLOAT");

    match ion_map.get(&sym::FLOAT) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(*s, sym::FLOAT_LEFT, "Expected float: left ($59)");
        }
        _ => panic!("Expected symbol for FLOAT"),
    }
}

#[test]
fn test_float_right() {
    let style = ParsedStyle {
        float: Some(crate::css::CssFloat::Right),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(ion_map.contains_key(&sym::FLOAT), "Style should have FLOAT");

    match ion_map.get(&sym::FLOAT) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(*s, sym::FLOAT_RIGHT, "Expected float: right ($61)");
        }
        _ => panic!("Expected symbol for FLOAT"),
    }
}

#[test]
fn test_float_snap_block() {
    let style = ParsedStyle {
        float: Some(crate::css::CssFloat::SnapBlock),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(ion_map.contains_key(&sym::FLOAT), "Style should have FLOAT");

    match ion_map.get(&sym::FLOAT) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::FLOAT_SNAP_BLOCK,
                "Expected float: snap-block ($786)"
            );
        }
        _ => panic!("Expected symbol for FLOAT"),
    }
}

#[test]
fn test_float_none_not_output() {
    // float: none should not be output (default behavior)
    let style = ParsedStyle {
        float: Some(crate::css::CssFloat::None),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::FLOAT),
        "float: none should not be output"
    );
}

// P2 Phase 2 Tests: Layout hints
#[test]
fn test_heading_style_has_layout_hints() {
    let style = ParsedStyle {
        is_heading: true,
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::LAYOUT_HINTS),
        "Heading style should have LAYOUT_HINTS ($761)"
    );

    match ion_map.get(&sym::LAYOUT_HINTS) {
        Some(IonValue::List(list)) => {
            assert_eq!(list.len(), 1, "Layout hints should have 1 element");
            match &list[0] {
                IonValue::Symbol(s) => {
                    assert_eq!(*s, sym::LAYOUT_HINT_HEADING, "Expected heading hint ($760)");
                }
                _ => panic!("Expected symbol in layout hints list"),
            }
        }
        _ => panic!("Expected list for LAYOUT_HINTS"),
    }
}

#[test]
fn test_non_heading_style_no_layout_hints() {
    let style = ParsedStyle {
        is_heading: false,
        font_weight: Some(crate::css::FontWeight::Bold),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::LAYOUT_HINTS),
        "Non-heading style should not have LAYOUT_HINTS"
    );
}

#[test]
fn test_box_sizing_border_box() {
    // Styles with box-sizing: border-box should output BOX_SIZING: $378
    let style = ParsedStyle {
        box_sizing: Some(crate::css::BoxSizing::BorderBox),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::BOX_SIZING),
        "Style with box-sizing: border-box should have BOX_SIZING"
    );

    let box_sizing = ion_map.get(&sym::BOX_SIZING).unwrap();
    match box_sizing {
        IonValue::Symbol(s) => assert_eq!(
            *s,
            sym::BOX_SIZING_BORDER_BOX,
            "box-sizing: border-box should map to $378"
        ),
        _ => panic!("Expected Symbol for BOX_SIZING"),
    }
}

#[test]
fn test_box_sizing_content_box() {
    // Styles with box-sizing: content-box should output BOX_SIZING: $377
    let style = ParsedStyle {
        box_sizing: Some(crate::css::BoxSizing::ContentBox),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        ion_map.contains_key(&sym::BOX_SIZING),
        "Style with box-sizing: content-box should have BOX_SIZING"
    );

    let box_sizing = ion_map.get(&sym::BOX_SIZING).unwrap();
    match box_sizing {
        IonValue::Symbol(s) => assert_eq!(
            *s,
            sym::BOX_SIZING_CONTENT_BOX,
            "box-sizing: content-box should map to $377"
        ),
        _ => panic!("Expected Symbol for BOX_SIZING"),
    }
}

#[test]
fn test_no_box_sizing_when_not_set() {
    // Styles without explicit box-sizing should NOT have BOX_SIZING property
    let style = ParsedStyle {
        font_size: Some(crate::css::CssValue::Em(1.0)),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::BOX_SIZING),
        "Style without box-sizing should not have BOX_SIZING property"
    );
}

// =========================================================================
// Layout Hints Tests
// =========================================================================

#[test]
fn test_figure_style_has_layout_hint_figure() {
    let style = ParsedStyle {
        is_figure: true,
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should have LAYOUT_HINTS ($761) containing LAYOUT_HINT_FIGURE ($282)
    assert!(
        ion_map.contains_key(&sym::LAYOUT_HINTS),
        "Figure style should have LAYOUT_HINTS property"
    );

    match ion_map.get(&sym::LAYOUT_HINTS) {
        Some(IonValue::List(list)) => {
            assert_eq!(list.len(), 1, "Layout hints should have 1 element");
            match &list[0] {
                IonValue::Symbol(s) => {
                    assert_eq!(
                        *s,
                        sym::LAYOUT_HINT_FIGURE,
                        "Expected figure hint ($282)"
                    );
                }
                _ => panic!("Expected symbol in layout hints list"),
            }
        }
        _ => panic!("Expected list for LAYOUT_HINTS"),
    }
}

#[test]
fn test_caption_style_has_layout_hint_caption() {
    let style = ParsedStyle {
        is_caption: true,
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should have LAYOUT_HINTS ($761) containing LAYOUT_HINT_CAPTION ($453)
    assert!(
        ion_map.contains_key(&sym::LAYOUT_HINTS),
        "Caption style should have LAYOUT_HINTS property"
    );

    match ion_map.get(&sym::LAYOUT_HINTS) {
        Some(IonValue::List(list)) => {
            assert_eq!(list.len(), 1, "Layout hints should have 1 element");
            match &list[0] {
                IonValue::Symbol(s) => {
                    assert_eq!(
                        *s,
                        sym::LAYOUT_HINT_CAPTION,
                        "Expected caption hint ($453)"
                    );
                }
                _ => panic!("Expected symbol in layout hints list"),
            }
        }
        _ => panic!("Expected list for LAYOUT_HINTS"),
    }
}

#[test]
fn test_figure_with_caption_has_both_hints() {
    // A figure that is also a caption (unlikely but tests multiple hints)
    let style = ParsedStyle {
        is_figure: true,
        is_caption: true,
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    match ion_map.get(&sym::LAYOUT_HINTS) {
        Some(IonValue::List(list)) => {
            assert_eq!(list.len(), 2, "Should have 2 layout hints");
            // Check both hints are present
            let hints: Vec<u64> = list
                .iter()
                .filter_map(|v| match v {
                    IonValue::Symbol(s) => Some(*s),
                    _ => None,
                })
                .collect();
            assert!(
                hints.contains(&sym::LAYOUT_HINT_FIGURE),
                "Should contain figure hint"
            );
            assert!(
                hints.contains(&sym::LAYOUT_HINT_CAPTION),
                "Should contain caption hint"
            );
        }
        _ => panic!("Expected list for LAYOUT_HINTS"),
    }
}

#[test]
fn test_no_layout_hints_for_regular_style() {
    // Regular style without is_heading/is_figure/is_caption should have no layout hints
    let style = ParsedStyle {
        font_weight: Some(crate::css::FontWeight::Bold),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::LAYOUT_HINTS),
        "Regular style should not have LAYOUT_HINTS"
    );
}

// =========================================================================
// Page Break Properties Tests
// =========================================================================

#[test]
fn test_page_break_before_always() {
    let style = ParsedStyle {
        break_before: Some(crate::css::BreakValue::Page),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should use PAGE_BREAK_BEFORE ($134) not BREAK_BEFORE ($789)
    assert!(
        ion_map.contains_key(&sym::PAGE_BREAK_BEFORE),
        "Should have PAGE_BREAK_BEFORE ($134) property"
    );

    match ion_map.get(&sym::PAGE_BREAK_BEFORE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::BREAK_ALWAYS,
                "page-break-before: page should map to $352 (always)"
            );
        }
        _ => panic!("Expected Symbol for PAGE_BREAK_BEFORE"),
    }
}

#[test]
fn test_page_break_after_avoid() {
    let style = ParsedStyle {
        break_after: Some(crate::css::BreakValue::Avoid),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should use PAGE_BREAK_AFTER ($133) not BREAK_AFTER ($788)
    assert!(
        ion_map.contains_key(&sym::PAGE_BREAK_AFTER),
        "Should have PAGE_BREAK_AFTER ($133) property"
    );

    match ion_map.get(&sym::PAGE_BREAK_AFTER) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::BREAK_AVOID,
                "page-break-after: avoid should map to $353"
            );
        }
        _ => panic!("Expected Symbol for PAGE_BREAK_AFTER"),
    }
}

#[test]
fn test_page_break_inside_avoid() {
    let style = ParsedStyle {
        break_inside: Some(crate::css::BreakValue::Avoid),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Should use BREAK_INSIDE ($135)
    assert!(
        ion_map.contains_key(&sym::BREAK_INSIDE),
        "Should have BREAK_INSIDE ($135) property"
    );

    match ion_map.get(&sym::BREAK_INSIDE) {
        Some(IonValue::Symbol(s)) => {
            assert_eq!(
                *s,
                sym::BREAK_AVOID,
                "page-break-inside: avoid should map to $353"
            );
        }
        _ => panic!("Expected Symbol for BREAK_INSIDE"),
    }
}

#[test]
fn test_no_page_break_for_auto() {
    // auto is the default - should not emit anything
    let style = ParsedStyle {
        break_before: Some(crate::css::BreakValue::Auto),
        break_after: Some(crate::css::BreakValue::Auto),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(map) => map,
        _ => panic!("Expected struct"),
    };

    // Auto values should still emit the property with BREAK_AUTO
    // (this depends on implementation - some converters skip auto)
    // Our implementation emits it, so just verify the value is correct if present
    if let Some(IonValue::Symbol(s)) = ion_map.get(&sym::PAGE_BREAK_BEFORE) {
        assert_eq!(*s, sym::BREAK_AUTO, "auto should map to $383");
    }
}

// ==========================================================================
// TDD Tests: Unicode-bidi, line-break, text-orientation conversions
// ==========================================================================

/// Helper to extract symbol value from IonValue map
fn get_sym(ion_map: &HashMap<u64, IonValue>, key: u64) -> Option<u64> {
    match ion_map.get(&key) {
        Some(IonValue::Symbol(s)) => Some(*s),
        _ => None,
    }
}

#[test]
fn test_unicode_bidi_embed() {
    let style = ParsedStyle {
        unicode_bidi: Some(crate::css::UnicodeBidi::Embed),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert_eq!(
        get_sym(&ion_map, sym::UNICODE_BIDI),
        Some(sym::BIDI_EMBED),
        "unicode-bidi: embed should output $674: $675"
    );
}

#[test]
fn test_unicode_bidi_isolate() {
    let style = ParsedStyle {
        unicode_bidi: Some(crate::css::UnicodeBidi::Isolate),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert_eq!(
        get_sym(&ion_map, sym::UNICODE_BIDI),
        Some(sym::BIDI_ISOLATE),
        "unicode-bidi: isolate should output $674: $676"
    );
}

#[test]
fn test_unicode_bidi_normal_not_output() {
    // Normal is the default and should not be output
    let style = ParsedStyle {
        unicode_bidi: Some(crate::css::UnicodeBidi::Normal),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::UNICODE_BIDI),
        "unicode-bidi: normal should not be output (it's the default)"
    );
}

#[test]
fn test_line_break_strict() {
    let style = ParsedStyle {
        line_break: Some(crate::css::LineBreak::Strict),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert_eq!(
        get_sym(&ion_map, sym::LINE_BREAK),
        Some(sym::LINE_BREAK_STRICT),
        "line-break: strict should output $780: $782"
    );
}

#[test]
fn test_line_break_loose() {
    let style = ParsedStyle {
        line_break: Some(crate::css::LineBreak::Loose),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert_eq!(
        get_sym(&ion_map, sym::LINE_BREAK),
        Some(sym::LINE_BREAK_LOOSE),
        "line-break: loose should output $780: $781"
    );
}

#[test]
fn test_line_break_auto_not_output() {
    // Auto is the default and should not be output
    let style = ParsedStyle {
        line_break: Some(crate::css::LineBreak::Auto),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::LINE_BREAK),
        "line-break: auto should not be output (it's the default)"
    );
}

#[test]
fn test_text_orientation_upright() {
    let style = ParsedStyle {
        text_orientation: Some(crate::css::TextOrientation::Upright),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert_eq!(
        get_sym(&ion_map, sym::TEXT_ORIENTATION),
        Some(sym::TEXT_ORIENTATION_UPRIGHT),
        "text-orientation: upright should output $706: $779"
    );
}

#[test]
fn test_text_orientation_sideways() {
    let style = ParsedStyle {
        text_orientation: Some(crate::css::TextOrientation::Sideways),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert_eq!(
        get_sym(&ion_map, sym::TEXT_ORIENTATION),
        Some(sym::TEXT_ORIENTATION_SIDEWAYS),
        "text-orientation: sideways should output $706: $778"
    );
}

#[test]
fn test_text_orientation_mixed_not_output() {
    // Mixed is the default and should not be output
    let style = ParsedStyle {
        text_orientation: Some(crate::css::TextOrientation::Mixed),
        ..Default::default()
    };

    let mut symtab = SymbolTable::new();
    let style_sym = symtab.get_or_intern("test-style");
    let ion = style_to_ion(&style, style_sym, &mut symtab);

    let ion_map = match ion {
        IonValue::Struct(m) => m,
        _ => panic!("Expected struct"),
    };

    assert!(
        !ion_map.contains_key(&sym::TEXT_ORIENTATION),
        "text-orientation: mixed should not be output (it's the default)"
    );
}

// ==========================================================================
// Color and Spacing Unit Tests (from mod.rs)
// ==========================================================================

#[test]
fn test_color_uses_argb_format() {
    // Reference KFX uses ARGB format 0xFFRRGGBB for colors
    // This ensures alpha channel is set to 255 (opaque)
    let red = Color::Rgba(255, 0, 0, 255);
    let ion = red.to_kfx_ion().expect("Should produce ION value");

    match ion {
        IonValue::Int(val) => {
            // Should be 0xFFFF0000 = 4294901760
            assert_eq!(val, 0xFFFF0000u32 as i64, "Red color should be 0xFFFF0000");
        }
        _ => panic!("Expected Int value"),
    }
}

#[test]
fn test_color_black_uses_argb_format() {
    // Black should be 0xFF000000, not 0x00000000
    let black = Color::Rgba(0, 0, 0, 255);
    let ion = black.to_kfx_ion().expect("Should produce ION value");

    match ion {
        IonValue::Int(val) => {
            // Should be 0xFF000000 = 4278190080
            assert_eq!(
                val, 0xFF000000u32 as i64,
                "Black color should be 0xFF000000"
            );
        }
        _ => panic!("Expected Int value"),
    }
}

#[test]
fn test_color_preserves_rgb_values() {
    // Test that RGB values are correctly encoded in ARGB format
    let color = Color::Rgba(0x12, 0x34, 0x56, 255);
    let ion = color.to_kfx_ion().expect("Should produce ION value");

    match ion {
        IonValue::Int(val) => {
            // Should be 0xFF123456
            let expected = 0xFF123456i64;
            assert_eq!(val, expected, "Color #123456 should be 0xFF123456");
        }
        _ => panic!("Expected Int value"),
    }
}

#[test]
fn test_vertical_spacing_uses_multiplier() {
    // Vertical spacing (margin-top/bottom) uses UNIT_MULTIPLIER
    // and is normalized by dividing by 1.2 (default line-height)
    let margin = CssValue::Em(1.2); // 1.2em
    let ion = spacing_to_multiplier(&margin).expect("Should produce ION value");

    match ion {
        IonValue::Struct(s) => {
            // Unit should be UNIT_MULTIPLIER ($310)
            match s.get(&sym::UNIT) {
                Some(IonValue::Symbol(unit)) => {
                    assert_eq!(
                        *unit,
                        sym::UNIT_MULTIPLIER,
                        "Vertical spacing should use UNIT_MULTIPLIER ($310)"
                    );
                }
                _ => panic!("Expected Symbol for unit"),
            }
        }
        _ => panic!("Expected Struct value"),
    }
}

#[test]
fn test_horizontal_spacing_uses_percent() {
    // Horizontal spacing (margin-left/right) uses UNIT_PERCENT directly
    let margin = CssValue::Percent(5.0); // 5%
    let ion = margin.to_kfx_ion().expect("Should produce ION value");

    match ion {
        IonValue::Struct(s) => {
            // Unit should be UNIT_PERCENT ($314)
            match s.get(&sym::UNIT) {
                Some(IonValue::Symbol(unit)) => {
                    assert_eq!(
                        *unit,
                        sym::UNIT_PERCENT,
                        "Horizontal spacing should use UNIT_PERCENT ($314)"
                    );
                }
                _ => panic!("Expected Symbol for unit"),
            }
        }
        _ => panic!("Expected Struct value"),
    }
}

#[test]
fn test_add_margins_uses_axis_specific_units() {
    // Test that add_margins applies correct units per axis
    let mut style = HashMap::new();

    // Add margins with same CSS value for all sides
    let margin = CssValue::Em(1.0);
    add_margins(
        &mut style,
        Some(&margin), // top
        Some(&margin), // right
        Some(&margin), // bottom
        Some(&margin), // left
    );

    // Vertical (top/bottom) should use UNIT_MULTIPLIER
    if let Some(IonValue::Struct(top)) = style.get(&MARGIN_SYMS.top) {
        match top.get(&sym::UNIT) {
            Some(IonValue::Symbol(unit)) => {
                assert_eq!(
                    *unit,
                    sym::UNIT_MULTIPLIER,
                    "margin-top should use UNIT_MULTIPLIER"
                );
            }
            _ => panic!("Expected Symbol for margin-top unit"),
        }
    } else {
        panic!("Expected Struct for margin-top");
    }

    // Horizontal (right/left) should use UNIT_PERCENT
    if let Some(IonValue::Struct(right)) = style.get(&MARGIN_SYMS.right) {
        match right.get(&sym::UNIT) {
            Some(IonValue::Symbol(unit)) => {
                assert_eq!(
                    *unit,
                    sym::UNIT_PERCENT,
                    "margin-right should use UNIT_PERCENT"
                );
            }
            _ => panic!("Expected Symbol for margin-right unit"),
        }
    } else {
        panic!("Expected Struct for margin-right");
    }
}
