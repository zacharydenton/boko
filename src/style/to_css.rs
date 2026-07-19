//! ToCss implementation for ComputedStyle.
//!
//! A single canonical table ([`PROPERTIES`]) is the source of truth for how
//! each `ComputedStyle` property is compared against its default and
//! serialized to CSS. Both `ComputedStyle::to_css` (the CSS declaration blob)
//! and the KFX exporter's per-field extraction
//! (`crate::kfx::style_schema::extract_ir_field`) are driven by this table,
//! so a new `ComputedStyle` field normally needs exactly one entry here.

use std::fmt::Write;

use super::ToCss;
use super::types::ComputedStyle;

/// Serializer for one property: writes the CSS value into `out` and returns
/// `true` when the property differs from its default (`false` writes nothing).
type EmitFn = fn(&ComputedStyle, &ComputedStyle, &mut String) -> bool;

/// One CSS property of `ComputedStyle` in the canonical table.
struct CssProperty {
    /// Canonical CSS property name.
    name: &'static str,
    /// Whether `ComputedStyle::to_css` emits this property in the CSS blob.
    ///
    /// Historically `to_css` never emitted some properties that the KFX
    /// exporter extracts (min-width, max-height, clear, orphans, widows,
    /// word-break, border-collapse, border-spacing). That asymmetry is
    /// preserved: those entries are lookup-only via
    /// [`changed_property_value`].
    in_blob: bool,
    /// How to serialize the value when it differs from the default.
    emit: EmitFn,
}

/// Table entry: emit the field's CSS value when it differs from the default.
macro_rules! prop {
    ($name:expr, $field:ident) => {
        prop!($name, $field, in_blob: true)
    };
    ($name:expr, $field:ident, in_blob: $in_blob:expr) => {
        CssProperty {
            name: $name,
            in_blob: $in_blob,
            emit: |s, d, out| {
                if s.$field == d.$field {
                    false
                } else {
                    s.$field.to_css(out);
                    true
                }
            },
        }
    };
}

/// Table entry: emit an optional color field when it is `Some`.
macro_rules! color_prop {
    ($name:expr, $field:ident) => {
        CssProperty {
            name: $name,
            in_blob: true,
            emit: |s, _d, out| match s.$field {
                Some(color) => {
                    color.to_css(out);
                    true
                }
                None => false,
            },
        }
    };
}

/// Table entry (lookup-only): emit an integer count field when it differs
/// from the default.
macro_rules! count_prop {
    ($name:expr, $field:ident) => {
        CssProperty {
            name: $name,
            in_blob: false,
            emit: |s, d, out| {
                if s.$field == d.$field {
                    false
                } else {
                    write!(out, "{}", s.$field).unwrap();
                    true
                }
            },
        }
    };
}

/// Canonical property table, in `to_css` emission order.
///
/// Blob entries (`in_blob: true`) are emitted by `ComputedStyle::to_css` in
/// this exact order; lookup-only entries at the end exist solely for
/// [`changed_property_value`] (used by the KFX exporter).
const PROPERTIES: &[CssProperty] = &[
    // Font properties.
    CssProperty {
        name: "font-family",
        in_blob: true,
        // The blob quotes family names that need it; the KFX exporter
        // intentionally uses the raw family string instead and does NOT
        // consult this entry (see extract_ir_field).
        emit: |s, _d, out| match &s.font_family {
            Some(family) => {
                quote_font_family(out, family);
                true
            }
            None => false,
        },
    },
    prop!("font-size", font_size),
    prop!("font-weight", font_weight),
    prop!("font-style", font_style),
    // Colors.
    color_prop!("color", color),
    color_prop!("background-color", background_color),
    // Text properties.
    prop!("text-align", text_align),
    prop!("text-indent", text_indent),
    prop!("line-height", line_height),
    // Combined underline/line-through value. The KFX exporter needs the two
    // flags separately and handles them itself (see extract_ir_field).
    CssProperty {
        name: "text-decoration",
        in_blob: true,
        emit: |s, _d, out| {
            match (s.text_decoration_underline, s.text_decoration_line_through) {
                (true, true) => out.push_str("underline line-through"),
                (true, false) => out.push_str("underline"),
                (false, true) => out.push_str("line-through"),
                (false, false) => return false,
            }
            true
        },
    },
    // Display.
    prop!("display", display),
    // Margins (4-sided).
    prop!("margin-top", margin_top),
    prop!("margin-right", margin_right),
    prop!("margin-bottom", margin_bottom),
    prop!("margin-left", margin_left),
    // Padding (4-sided).
    prop!("padding-top", padding_top),
    prop!("padding-right", padding_right),
    prop!("padding-bottom", padding_bottom),
    prop!("padding-left", padding_left),
    // Vertical alignment.
    prop!("vertical-align", vertical_align),
    // List style. The blob emits it whenever non-default; the KFX exporter
    // additionally gates it on display: list-item (see extract_ir_field).
    prop!("list-style-type", list_style_type),
    // Font variant.
    prop!("font-variant", font_variant),
    // Text spacing.
    prop!("letter-spacing", letter_spacing),
    prop!("word-spacing", word_spacing),
    // Text transform.
    prop!("text-transform", text_transform),
    // Hyphenation.
    prop!("hyphens", hyphens),
    // White-space.
    prop!("white-space", white_space),
    // Underline style.
    prop!("text-decoration-style", underline_style),
    // Overline flag. The KFX exporter maps this flag to a decoration style
    // ("solid") rather than a line value, so it does not consult this entry
    // (see extract_ir_field).
    CssProperty {
        name: "text-decoration-line",
        in_blob: true,
        emit: |s, _d, out| {
            if s.overline {
                out.push_str("overline");
                true
            } else {
                false
            }
        },
    },
    // Underline color.
    color_prop!("text-decoration-color", underline_color),
    // Layout dimensions.
    prop!("width", width),
    prop!("height", height),
    prop!("max-width", max_width),
    prop!("min-height", min_height),
    // Float.
    prop!("float", float),
    // Page breaks.
    prop!("break-before", break_before),
    prop!("break-after", break_after),
    prop!("break-inside", break_inside),
    // Border styles (4-sided).
    prop!("border-style-top", border_style_top),
    prop!("border-style-right", border_style_right),
    prop!("border-style-bottom", border_style_bottom),
    prop!("border-style-left", border_style_left),
    // Border widths (4-sided).
    prop!("border-width-top", border_width_top),
    prop!("border-width-right", border_width_right),
    prop!("border-width-bottom", border_width_bottom),
    prop!("border-width-left", border_width_left),
    // Border colors (4-sided optional).
    color_prop!("border-top-color", border_color_top),
    color_prop!("border-right-color", border_color_right),
    color_prop!("border-bottom-color", border_color_bottom),
    color_prop!("border-left-color", border_color_left),
    // Border radius (4-corner).
    prop!("border-top-left-radius", border_radius_top_left),
    prop!("border-top-right-radius", border_radius_top_right),
    prop!("border-bottom-left-radius", border_radius_bottom_left),
    prop!("border-bottom-right-radius", border_radius_bottom_right),
    // List style position (same display gating note as list-style-type).
    prop!("list-style-position", list_style_position),
    // Visibility.
    prop!("visibility", visibility),
    // Note: language is stored but typically output via HTML lang attribute.
    //
    // Lookup-only entries below: never emitted in the CSS blob, used by the
    // KFX exporter via changed_property_value.
    prop!("min-width", min_width, in_blob: false),
    prop!("max-height", max_height, in_blob: false),
    prop!("clear", clear, in_blob: false),
    count_prop!("orphans", orphans),
    count_prop!("widows", widows),
    prop!("word-break", word_break, in_blob: false),
    prop!("border-collapse", border_collapse, in_blob: false),
    prop!("border-spacing", border_spacing, in_blob: false),
];

/// The default style every `emit` fn compares against. Built once: this is
/// consulted per property per style on the KFX export hot path, and
/// constructing the ~320-byte struct per lookup was measurable.
static DEFAULT_STYLE: std::sync::LazyLock<ComputedStyle> =
    std::sync::LazyLock::new(ComputedStyle::default);

/// Property-name → table-entry index, so per-name lookups don't linear-scan
/// the table's ~70 entries on the KFX export hot path.
static PROPERTY_INDEX: std::sync::LazyLock<rustc_hash::FxHashMap<&'static str, usize>> =
    std::sync::LazyLock::new(|| {
        PROPERTIES
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name, i))
            .collect()
    });

/// Visit every property that `ComputedStyle::to_css` emits — i.e. every blob
/// property differing from its default — in emission order, as
/// (css-property-name, css-value) pairs.
pub fn for_each_changed_property(style: &ComputedStyle, f: &mut dyn FnMut(&str, &str)) {
    let default = &*DEFAULT_STYLE;
    let mut value = String::new();
    for prop in PROPERTIES {
        if !prop.in_blob {
            continue;
        }
        value.clear();
        if (prop.emit)(style, default, &mut value) {
            f(prop.name, &value);
        }
    }
}

/// Look up one property in the canonical table: `Some(css_value)` when the
/// property differs from its default, `None` otherwise.
///
/// # Panics
///
/// Panics if `name` is not in the canonical table. Callers pass compile-time
/// constant names, so an unknown name is a bug (drift between the KFX schema
/// and the table) that must not be silently ignored.
pub fn changed_property_value(style: &ComputedStyle, name: &str) -> Option<String> {
    changed_property_value_from(style, &DEFAULT_STYLE, name)
}

/// Like [`changed_property_value`], but against an arbitrary baseline style
/// instead of the default. The KFX exporter passes the parent's computed
/// style for CSS-inherited properties: KFX styles inherit through nested
/// containers at render time, so a value equal to the parent's needs no
/// re-emission, while a value that *differs* must be emitted even when it
/// equals the CSS initial value (an explicit reset like `font-style: normal`
/// inside an italic ancestor).
pub fn changed_property_value_from(
    style: &ComputedStyle,
    baseline: &ComputedStyle,
    name: &str,
) -> Option<String> {
    let idx = *PROPERTY_INDEX
        .get(name)
        .unwrap_or_else(|| panic!("unknown CSS property name: {name}"));
    let prop = &PROPERTIES[idx];
    let mut value = String::new();
    (prop.emit)(style, baseline, &mut value).then_some(value)
}

impl ToCss for ComputedStyle {
    fn to_css(&self, buf: &mut String) {
        for_each_changed_property(self, &mut |name, value| {
            buf.push_str(name);
            buf.push_str(": ");
            buf.push_str(value);
            buf.push_str("; ");
        });
    }
}

/// CSS generic font families that must NOT be quoted.
const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];

/// Quote font-family names that need quoting in CSS.
///
/// A comma-separated font stack like `din next lt pro,sans-serif` becomes
/// `"din next lt pro",sans-serif` — generic families are left unquoted,
/// custom names with spaces or leading digits are quoted.
fn quote_font_family(buf: &mut String, family: &str) {
    for (i, part) in family.split(',').enumerate() {
        if i > 0 {
            buf.push(',');
        }
        let trimmed = part.trim();
        let is_generic = GENERIC_FAMILIES
            .iter()
            .any(|g| g.eq_ignore_ascii_case(trimmed));
        let needs_quoting = !is_generic
            && (trimmed.contains(' ')
                || trimmed.starts_with(|c: char| c.is_ascii_digit())
                || trimmed.contains('"')
                || trimmed.is_empty());
        if needs_quoting {
            buf.push('"');
            buf.push_str(trimmed);
            buf.push('"');
        } else {
            buf.push_str(trimmed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quoted(input: &str) -> String {
        let mut buf = String::new();
        quote_font_family(&mut buf, input);
        buf
    }

    #[test]
    fn test_font_family_quoting_spaces_and_digit_prefix() {
        // Real-world case from B003ZK58TA: unquoted name with spaces + leading digit
        assert_eq!(
            quoted("001_cvi_cover-din next lt pro,sans-serif"),
            r#""001_cvi_cover-din next lt pro",sans-serif"#
        );
    }

    #[test]
    fn test_font_family_quoting_spaces() {
        assert_eq!(
            quoted("DIN Next LT Pro,sans-serif"),
            r#""DIN Next LT Pro",sans-serif"#
        );
    }

    #[test]
    fn test_font_family_no_quoting_single_word() {
        assert_eq!(quoted("Helvetica"), "Helvetica");
    }

    #[test]
    fn test_font_family_generic_not_quoted() {
        assert_eq!(quoted("serif"), "serif");
        assert_eq!(quoted("sans-serif"), "sans-serif");
        assert_eq!(quoted("monospace"), "monospace");
    }

    #[test]
    fn test_font_family_generic_case_insensitive() {
        assert_eq!(quoted("Sans-Serif"), "Sans-Serif");
    }

    #[test]
    fn test_font_family_full_stack() {
        assert_eq!(
            quoted("031_next-reads-shift light,palatino,palatino linotype,georgia,serif"),
            r#""031_next-reads-shift light",palatino,"palatino linotype",georgia,serif"#
        );
    }

    #[test]
    fn test_font_family_leading_digit() {
        assert_eq!(quoted("123font"), r#""123font""#);
    }

    #[test]
    fn test_computed_style_font_family_quoted() {
        let style = ComputedStyle {
            font_family: Some("001_cvi_cover-din next lt pro,sans-serif".to_string()),
            ..Default::default()
        };
        let mut css = String::new();
        style.to_css(&mut css);
        assert!(
            css.contains(r#"font-family: "001_cvi_cover-din next lt pro",sans-serif;"#),
            "Expected quoted font-family in CSS output, got: {}",
            css
        );
    }

    #[test]
    fn test_property_names_unique() {
        for (i, a) in PROPERTIES.iter().enumerate() {
            for b in &PROPERTIES[i + 1..] {
                assert_ne!(a.name, b.name, "duplicate property name in table");
            }
        }
    }

    #[test]
    fn test_changed_property_value_matches_blob() {
        let style = ComputedStyle {
            font_weight: crate::style::FontWeight::BOLD,
            ..Default::default()
        };
        assert_eq!(
            changed_property_value(&style, "font-weight"),
            Some("bold".to_string())
        );
        assert_eq!(changed_property_value(&style, "font-style"), None);
    }

    #[test]
    #[should_panic(expected = "unknown CSS property name")]
    fn test_changed_property_value_unknown_name_panics() {
        changed_property_value(&ComputedStyle::default(), "not-a-property");
    }
}
