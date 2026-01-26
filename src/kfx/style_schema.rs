//! Declarative style schema for KFX export.
//!
//! This module defines the schema-driven approach to style conversion.
//! Instead of imperative CSS parsing, we define property rules that map
//! IR style properties to KFX Ion structures.
//!
//! ## Architecture
//!
//! 1. **StylePropertyRule** - Declares how a single property maps (e.g., font-weight → fontWeight)
//! 2. **ValueTransform** - Defines the conversion logic (enum lookup, unit scaling, etc.)
//! 3. **StyleSchema** - Registry of all rules with fast lookup
//! 4. **StyleContext** - Whether a property can be inline or requires a block container

use std::collections::HashMap;

use crate::ir::{self as ir_style, ToCss};
use crate::kfx::ion::IonValue;
use crate::kfx::symbols::KfxSymbol;

// ============================================================================
// Value Transform System
// ============================================================================

/// Defines how a raw value from IR is converted into a KFX-native Ion Value.
#[derive(Debug, Clone)]
pub enum ValueTransform {
    /// Pass-through: value is identical in both formats.
    /// Example: "center" -> "center"
    Identity,

    /// Dictionary lookup: maps specific strings to KFX values.
    /// Example: "bold" -> Symbol(fontWeight_bold)
    Map(Vec<(String, KfxValue)>),

    /// Number scaling: multiply input by factor, clamp, round.
    /// Example: line-height 1.5 -> 150 (factor=100)
    ScaleFloat {
        factor: f64,
        min: Option<f64>,
        max: Option<f64>,
        precision: RoundingMode,
    },

    /// Unit conversion: CSS units (px, em, rem) -> KFX units.
    ConvertUnit {
        base_pixels: f64,
        target_unit: KfxUnitType,
    },

    /// Color parsing: CSS color -> KFX integer or struct.
    ParseColor { output_format: ColorFormat },

    /// Shorthand extraction: extracts Nth component from CSS shorthand.
    /// Example: "margin: 10px 20px" with index=1 extracts "20px"
    ExtractShorthand {
        index: usize,
        default_value: Option<KfxValue>,
    },

    /// Dimensioned value: wraps a number with a unit symbol.
    /// Example: 1.2 with unit=em -> { value: 1.2, unit: em }
    Dimensioned { unit: KfxSymbol },

    /// Symbol lookup: converts string to KFX symbol ID.
    ToSymbol,
}

/// KFX value representation for transforms.
#[derive(Debug, Clone, PartialEq)]
pub enum KfxValue {
    Symbol(KfxSymbol),
    SymbolId(u64),
    Integer(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
    /// Dimensioned value: { value: N, unit: Symbol }
    Dimensioned { value: f64, unit: KfxSymbol },
}

impl KfxValue {
    /// Convert to IonValue for serialization.
    pub fn to_ion(&self) -> IonValue {
        match self {
            KfxValue::Symbol(sym) => IonValue::Symbol(*sym as u64),
            KfxValue::SymbolId(id) => IonValue::Symbol(*id),
            KfxValue::Integer(n) => IonValue::Int(*n),
            KfxValue::Float(f) => IonValue::Float(*f),
            KfxValue::String(s) => IonValue::String(s.clone()),
            KfxValue::Bool(b) => IonValue::Bool(*b),
            KfxValue::Null => IonValue::Null,
            KfxValue::Dimensioned { value, unit } => IonValue::Struct(vec![
                (KfxSymbol::Value as u64, IonValue::Float(*value)),
                (KfxSymbol::Unit as u64, IonValue::Symbol(*unit as u64)),
            ]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RoundingMode {
    Floor,
    Ceil,
    Round,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KfxUnitType {
    /// Model pixels (1/260th inch, Kindle specific)
    ModelPixels,
    /// Percentage as integer (100% = 1000)
    Percentage1000,
    /// Percentage as integer (100% = 100)
    Percentage100,
    /// Em units (relative to font size)
    Em,
    /// Rem units (relative to root font size)
    Rem,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorFormat {
    /// Pack as 0xRRGGBB integer
    PackedInt,
    /// Struct with r, g, b fields
    RgbStruct,
}

// ============================================================================
// Style Context
// ============================================================================

/// Defines where a style property can be applied in KFX.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StyleContext {
    /// Can apply to text spans (color, bold, italic)
    InlineSafe,
    /// Requires a structural container (margins, alignment)
    BlockOnly,
    /// Can apply anywhere
    Any,
}

// ============================================================================
// Style Property Rule
// ============================================================================

/// Identifies which IR struct field a property comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IrField {
    FontWeight,
    FontStyle,
    FontSize,
    TextAlign,
    TextIndent,
    LineHeight,
    MarginTop,
    MarginBottom,
    MarginLeft,
    MarginRight,
    PaddingTop,
    PaddingBottom,
    PaddingLeft,
    PaddingRight,
    Color,
    BackgroundColor,
    VerticalAlign,
    TextDecorationUnderline,
    TextDecorationStrikethrough,
}

/// Declarative definition for how a style property maps from IR to KFX.
#[derive(Debug, Clone)]
pub struct StylePropertyRule {
    /// The key in IR (e.g., "font-weight", "margin-top")
    pub ir_key: &'static str,

    /// Which IR struct field this maps from (for bidirectional schema)
    pub ir_field: Option<IrField>,

    /// The KFX symbol for this property
    pub kfx_symbol: KfxSymbol,

    /// How to convert the raw value
    pub transform: ValueTransform,

    /// Where this property can be applied
    pub context: StyleContext,
}

// ============================================================================
// Style Schema
// ============================================================================

/// The master schema for style property mappings.
pub struct StyleSchema {
    /// Fast lookup from IR key -> Rule
    rules: HashMap<&'static str, StylePropertyRule>,
}

impl StyleSchema {
    /// Create an empty schema.
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
        }
    }

    /// Register a property rule.
    pub fn register(&mut self, rule: StylePropertyRule) {
        self.rules.insert(rule.ir_key, rule);
    }

    /// Look up a rule by IR key.
    pub fn get(&self, ir_key: &str) -> Option<&StylePropertyRule> {
        self.rules.get(ir_key)
    }

    /// Get all rules.
    pub fn rules(&self) -> impl Iterator<Item = &StylePropertyRule> {
        self.rules.values()
    }

    /// Get rules that have IR field mappings (for schema-driven IR extraction).
    pub fn ir_mapped_rules(&self) -> impl Iterator<Item = &StylePropertyRule> {
        self.rules.values().filter(|r| r.ir_field.is_some())
    }

    /// Create the standard KFX style schema.
    pub fn standard() -> Self {
        let mut schema = Self::new();

        // ====================================================================
        // Font Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "font-weight",
            ir_field: Some(IrField::FontWeight),
            kfx_symbol: KfxSymbol::FontWeight,
            transform: ValueTransform::Map(vec![
                // Named values
                ("bold".into(), KfxValue::Symbol(KfxSymbol::Bold)),
                ("normal".into(), KfxValue::Symbol(KfxSymbol::Normal)),
                ("lighter".into(), KfxValue::Symbol(KfxSymbol::Light)),
                ("bolder".into(), KfxValue::Symbol(KfxSymbol::Bold)),
                // Numeric values (100-900 scale per CSS spec)
                ("100".into(), KfxValue::Symbol(KfxSymbol::Thin)),
                ("200".into(), KfxValue::Symbol(KfxSymbol::UltraLight)),
                ("300".into(), KfxValue::Symbol(KfxSymbol::Light)),
                ("400".into(), KfxValue::Symbol(KfxSymbol::Normal)),
                ("500".into(), KfxValue::Symbol(KfxSymbol::Medium)),
                ("600".into(), KfxValue::Symbol(KfxSymbol::SemiBold)),
                ("700".into(), KfxValue::Symbol(KfxSymbol::Bold)),
                ("800".into(), KfxValue::Symbol(KfxSymbol::UltraBold)),
                ("900".into(), KfxValue::Symbol(KfxSymbol::Heavy)),
            ]),
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "font-style",
            ir_field: Some(IrField::FontStyle),
            kfx_symbol: KfxSymbol::FontStyle,
            transform: ValueTransform::Map(vec![
                ("italic".into(), KfxValue::Symbol(KfxSymbol::Italic)),
                ("oblique".into(), KfxValue::Symbol(KfxSymbol::Oblique)),
                ("normal".into(), KfxValue::Symbol(KfxSymbol::Normal)),
            ]),
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "font-size",
            ir_field: Some(IrField::FontSize),
            kfx_symbol: KfxSymbol::FontSize,
            transform: ValueTransform::Dimensioned { unit: KfxSymbol::Rem },
            context: StyleContext::Any,
        });

        // ====================================================================
        // Text Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "text-align",
            ir_field: Some(IrField::TextAlign),
            kfx_symbol: KfxSymbol::TextAlignment,
            transform: ValueTransform::Map(vec![
                ("left".into(), KfxValue::Symbol(KfxSymbol::Left)),
                ("center".into(), KfxValue::Symbol(KfxSymbol::Center)),
                ("right".into(), KfxValue::Symbol(KfxSymbol::Right)),
                ("justify".into(), KfxValue::Symbol(KfxSymbol::Justify)),
                // Start/End are distinct for RTL language support
                ("start".into(), KfxValue::Symbol(KfxSymbol::Start)),
                ("end".into(), KfxValue::Symbol(KfxSymbol::End)),
            ]),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "text-indent",
            ir_field: Some(IrField::TextIndent),
            kfx_symbol: KfxSymbol::TextIndent,
            transform: ValueTransform::Dimensioned { unit: KfxSymbol::Em },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "line-height",
            ir_field: Some(IrField::LineHeight),
            kfx_symbol: KfxSymbol::LineHeight,
            transform: ValueTransform::Dimensioned { unit: KfxSymbol::Em },
            context: StyleContext::BlockOnly,
        });

        // text-decoration: underline -> underline: true
        schema.register(StylePropertyRule {
            ir_key: "text-decoration",
            ir_field: Some(IrField::TextDecorationUnderline),
            kfx_symbol: KfxSymbol::Underline,
            transform: ValueTransform::Map(vec![
                ("underline".into(), KfxValue::Bool(true)),
                ("true".into(), KfxValue::Bool(true)),
                ("false".into(), KfxValue::Bool(false)),
                ("none".into(), KfxValue::Bool(false)),
            ]),
            context: StyleContext::InlineSafe,
        });

        // text-decoration: line-through -> strikethrough: true
        schema.register(StylePropertyRule {
            ir_key: "text-decoration-strikethrough",
            ir_field: Some(IrField::TextDecorationStrikethrough),
            kfx_symbol: KfxSymbol::Strikethrough,
            transform: ValueTransform::Map(vec![
                ("line-through".into(), KfxValue::Bool(true)),
                ("true".into(), KfxValue::Bool(true)),
                ("false".into(), KfxValue::Bool(false)),
                ("none".into(), KfxValue::Bool(false)),
            ]),
            context: StyleContext::InlineSafe,
        });

        // ====================================================================
        // Spacing Properties (Margins)
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "margin-top",
            ir_field: Some(IrField::MarginTop),
            kfx_symbol: KfxSymbol::MarginTop,
            transform: ValueTransform::Dimensioned { unit: KfxSymbol::Em },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-bottom",
            ir_field: Some(IrField::MarginBottom),
            kfx_symbol: KfxSymbol::MarginBottom,
            transform: ValueTransform::Dimensioned { unit: KfxSymbol::Em },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-left",
            ir_field: Some(IrField::MarginLeft),
            kfx_symbol: KfxSymbol::MarginLeft,
            transform: ValueTransform::Dimensioned { unit: KfxSymbol::Em },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-right",
            ir_field: Some(IrField::MarginRight),
            kfx_symbol: KfxSymbol::MarginRight,
            transform: ValueTransform::Dimensioned { unit: KfxSymbol::Em },
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Color Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "color",
            ir_field: Some(IrField::Color),
            kfx_symbol: KfxSymbol::TextColor,
            transform: ValueTransform::ParseColor {
                output_format: ColorFormat::PackedInt,
            },
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "background-color",
            ir_field: Some(IrField::BackgroundColor),
            kfx_symbol: KfxSymbol::TextBackgroundColor,
            transform: ValueTransform::ParseColor {
                output_format: ColorFormat::PackedInt,
            },
            context: StyleContext::Any,
        });

        // ====================================================================
        // Vertical Alignment (for superscript/subscript)
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "vertical-align",
            ir_field: Some(IrField::VerticalAlign),
            kfx_symbol: KfxSymbol::BaselineStyle,
            transform: ValueTransform::Map(vec![
                ("super".into(), KfxValue::Symbol(KfxSymbol::Superscript)),
                ("sub".into(), KfxValue::Symbol(KfxSymbol::Subscript)),
                ("baseline".into(), KfxValue::Symbol(KfxSymbol::TextBaseline)),
            ]),
            context: StyleContext::InlineSafe,
        });

        schema
    }
}

impl Default for StyleSchema {
    fn default() -> Self {
        Self::standard()
    }
}

// ============================================================================
// Transform Execution
// ============================================================================

impl ValueTransform {
    /// Apply this transform to a raw string value.
    pub fn apply(&self, raw: &str) -> Option<KfxValue> {
        match self {
            ValueTransform::Identity => Some(KfxValue::String(raw.to_string())),

            ValueTransform::Map(mappings) => {
                let normalized = raw.trim().to_lowercase();
                mappings
                    .iter()
                    .find(|(k, _)| k == &normalized)
                    .map(|(_, v)| v.clone())
            }

            ValueTransform::ScaleFloat {
                factor,
                min,
                max,
                precision,
            } => {
                let num = parse_number(raw)?;
                let mut scaled = num * factor;

                // Guard against NaN from multiplication
                if scaled.is_nan() {
                    return None;
                }

                if let Some(min_val) = min {
                    scaled = scaled.max(*min_val);
                }
                if let Some(max_val) = max {
                    scaled = scaled.min(*max_val);
                }

                let result = match precision {
                    RoundingMode::Floor => scaled.floor(),
                    RoundingMode::Ceil => scaled.ceil(),
                    RoundingMode::Round => scaled.round(),
                };

                // Guard against Infinity after clamping (shouldn't happen with proper min/max)
                if result.is_infinite() || result.is_nan() {
                    return None;
                }

                // Safe conversion: clamp to i64 range
                let clamped = result.clamp(i64::MIN as f64, i64::MAX as f64);
                Some(KfxValue::Integer(clamped as i64))
            }

            ValueTransform::ConvertUnit {
                base_pixels,
                target_unit,
            } => {
                let (num, unit) = parse_css_length(raw)?;
                let pixels = convert_to_pixels(num, &unit, *base_pixels);

                // Guard against division by zero
                if *base_pixels == 0.0 {
                    return None;
                }

                let result = match target_unit {
                    KfxUnitType::ModelPixels => pixels * (260.0 / 96.0), // 96 DPI -> 260 DPI
                    KfxUnitType::Percentage1000 => pixels / base_pixels * 1000.0,
                    KfxUnitType::Percentage100 => pixels / base_pixels * 100.0,
                    KfxUnitType::Em => pixels / base_pixels,
                    KfxUnitType::Rem => pixels / base_pixels,
                };

                // Guard against NaN/Infinity
                if result.is_nan() || result.is_infinite() {
                    return None;
                }

                Some(KfxValue::Float(result))
            }

            ValueTransform::ParseColor { output_format } => {
                let color = parse_css_color(raw)?;
                match output_format {
                    ColorFormat::PackedInt => {
                        let packed = ((color.0 as i64) << 16)
                            | ((color.1 as i64) << 8)
                            | (color.2 as i64);
                        Some(KfxValue::Integer(packed))
                    }
                    ColorFormat::RgbStruct => {
                        // Would need a KfxValue variant for this
                        Some(KfxValue::Integer(
                            ((color.0 as i64) << 16)
                                | ((color.1 as i64) << 8)
                                | (color.2 as i64),
                        ))
                    }
                }
            }

            ValueTransform::ExtractShorthand {
                index,
                default_value,
            } => {
                let parts: Vec<&str> = raw.split_whitespace().collect();
                extract_shorthand_value(&parts, *index, default_value.clone())
            }

            ValueTransform::Dimensioned { unit } => {
                let (num, _css_unit) = parse_css_length(raw)?;
                Some(KfxValue::Dimensioned { value: num, unit: *unit })
            }

            ValueTransform::ToSymbol => Some(KfxValue::String(raw.to_string())),
        }
    }
}

// ============================================================================
// CSS Parsing Helpers
// ============================================================================

/// Parse a number from a string.
fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    // Remove any unit suffix
    let numeric: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    numeric.parse().ok()
}

/// Parse a CSS length value into (number, unit).
fn parse_css_length(s: &str) -> Option<(f64, String)> {
    let s = s.trim();

    // Find where the unit starts
    let unit_start = s
        .chars()
        .position(|c| c.is_alphabetic() || c == '%')
        .unwrap_or(s.len());

    let num_str = &s[..unit_start];
    let unit_str = &s[unit_start..];

    let num: f64 = num_str.parse().ok()?;
    let unit = if unit_str.is_empty() {
        "px".to_string() // Default to pixels
    } else {
        unit_str.to_lowercase()
    };

    Some((num, unit))
}

/// Convert a value with CSS unit to pixels.
fn convert_to_pixels(value: f64, unit: &str, base_font_size: f64) -> f64 {
    match unit {
        "px" => value,
        "em" => value * base_font_size,
        "rem" => value * base_font_size,
        "pt" => value * (96.0 / 72.0), // 72 pt per inch, 96 px per inch
        "%" => value * base_font_size / 100.0,
        "in" => value * 96.0,
        "cm" => value * (96.0 / 2.54),
        "mm" => value * (96.0 / 25.4),
        _ => value, // Unknown unit, assume pixels
    }
}

/// Parse a CSS color into (r, g, b).
fn parse_css_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().to_lowercase();

    // Named colors
    let named = match s.as_str() {
        "black" => Some((0, 0, 0)),
        "white" => Some((255, 255, 255)),
        "red" => Some((255, 0, 0)),
        "green" => Some((0, 128, 0)),
        "blue" => Some((0, 0, 255)),
        "yellow" => Some((255, 255, 0)),
        "cyan" => Some((0, 255, 255)),
        "magenta" => Some((255, 0, 255)),
        "gray" | "grey" => Some((128, 128, 128)),
        "darkgray" | "darkgrey" => Some((169, 169, 169)),
        "lightgray" | "lightgrey" => Some((211, 211, 211)),
        "orange" => Some((255, 165, 0)),
        "purple" => Some((128, 0, 128)),
        "brown" => Some((165, 42, 42)),
        "pink" => Some((255, 192, 203)),
        "navy" => Some((0, 0, 128)),
        "teal" => Some((0, 128, 128)),
        "olive" => Some((128, 128, 0)),
        "maroon" => Some((128, 0, 0)),
        "silver" => Some((192, 192, 192)),
        "lime" => Some((0, 255, 0)),
        "aqua" => Some((0, 255, 255)),
        "fuchsia" => Some((255, 0, 255)),
        "transparent" => Some((0, 0, 0)), // Treat as black
        _ => None,
    };

    if let Some(color) = named {
        return Some(color);
    }

    // Hex colors
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    // rgb() / rgba()
    if s.starts_with("rgb") {
        return parse_rgb_function(&s);
    }

    None
}

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    match hex.len() {
        3 => {
            // #RGB -> #RRGGBB
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        8 => {
            // #RRGGBBAA - ignore alpha
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn parse_rgb_function(s: &str) -> Option<(u8, u8, u8)> {
    // Extract content between parentheses
    let start = s.find('(')?;
    let end = s.find(')')?;
    let content = &s[start + 1..end];

    // Split by comma or space
    let parts: Vec<&str> = content
        .split(|c| c == ',' || c == ' ')
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() < 3 {
        return None;
    }

    let r = parse_color_component(parts[0])?;
    let g = parse_color_component(parts[1])?;
    let b = parse_color_component(parts[2])?;

    Some((r, g, b))
}

fn parse_color_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let pct: f64 = pct.parse().ok()?;
        Some((pct * 255.0 / 100.0).round() as u8)
    } else {
        s.parse().ok()
    }
}

/// Extract a value from a CSS shorthand property.
///
/// CSS shorthands expand in specific ways:
/// - 1 value: applies to all sides (top, right, bottom, left)
/// - 2 values: (top/bottom, left/right)
/// - 3 values: (top, left/right, bottom)
/// - 4 values: (top, right, bottom, left)
fn extract_shorthand_value(
    parts: &[&str],
    index: usize,
    default: Option<KfxValue>,
) -> Option<KfxValue> {
    let value = match (parts.len(), index) {
        // 1 value: all sides get the same value
        (1, _) => parts.first(),

        // 2 values: (vertical, horizontal)
        // index 0,2 = top/bottom = parts[0]
        // index 1,3 = left/right = parts[1]
        (2, 0) | (2, 2) => parts.first(),
        (2, 1) | (2, 3) => parts.get(1),

        // 3 values: (top, horizontal, bottom)
        // index 0 = top = parts[0]
        // index 1,3 = left/right = parts[1]
        // index 2 = bottom = parts[2]
        (3, 0) => parts.first(),
        (3, 1) | (3, 3) => parts.get(1),
        (3, 2) => parts.get(2),

        // 4 values: (top, right, bottom, left)
        (4, i) => parts.get(i),

        _ => None,
    };

    match value {
        Some(v) => Some(KfxValue::String((*v).to_string())),
        None => default,
    }
}

// ============================================================================
// IR Field Extraction (Bidirectional Schema Bridge)
// ============================================================================

/// Extract a CSS string from an IR ComputedStyle field.
///
/// This is the centralized extraction logic for the bidirectional schema.
/// The schema declares WHICH fields to extract (via `IrField` enum), and
/// this function provides the HOW (accessing the struct field, checking defaults).
///
/// Returns `None` if the field has its default value (nothing to emit).
pub fn extract_ir_field(ir_style: &ir_style::ComputedStyle, field: IrField) -> Option<String> {
    let default = ir_style::ComputedStyle::default();

    match field {
        IrField::FontWeight => {
            if ir_style.font_weight != default.font_weight {
                Some(ir_style.font_weight.to_css_string())
            } else {
                None
            }
        }
        IrField::FontStyle => {
            if ir_style.font_style != default.font_style {
                Some(ir_style.font_style.to_css_string())
            } else {
                None
            }
        }
        IrField::FontSize => {
            if ir_style.font_size != default.font_size {
                Some(ir_style.font_size.to_css_string())
            } else {
                None
            }
        }
        IrField::TextAlign => {
            if ir_style.text_align != default.text_align {
                Some(ir_style.text_align.to_css_string())
            } else {
                None
            }
        }
        IrField::TextIndent => {
            if ir_style.text_indent != default.text_indent {
                Some(ir_style.text_indent.to_css_string())
            } else {
                None
            }
        }
        IrField::LineHeight => {
            if ir_style.line_height != default.line_height {
                Some(ir_style.line_height.to_css_string())
            } else {
                None
            }
        }
        IrField::MarginTop => {
            if ir_style.margin_top != default.margin_top {
                Some(ir_style.margin_top.to_css_string())
            } else {
                None
            }
        }
        IrField::MarginBottom => {
            if ir_style.margin_bottom != default.margin_bottom {
                Some(ir_style.margin_bottom.to_css_string())
            } else {
                None
            }
        }
        IrField::MarginLeft => {
            if ir_style.margin_left != default.margin_left {
                Some(ir_style.margin_left.to_css_string())
            } else {
                None
            }
        }
        IrField::MarginRight => {
            if ir_style.margin_right != default.margin_right {
                Some(ir_style.margin_right.to_css_string())
            } else {
                None
            }
        }
        IrField::PaddingTop => {
            if ir_style.padding_top != default.padding_top {
                Some(ir_style.padding_top.to_css_string())
            } else {
                None
            }
        }
        IrField::PaddingBottom => {
            if ir_style.padding_bottom != default.padding_bottom {
                Some(ir_style.padding_bottom.to_css_string())
            } else {
                None
            }
        }
        IrField::PaddingLeft => {
            if ir_style.padding_left != default.padding_left {
                Some(ir_style.padding_left.to_css_string())
            } else {
                None
            }
        }
        IrField::PaddingRight => {
            if ir_style.padding_right != default.padding_right {
                Some(ir_style.padding_right.to_css_string())
            } else {
                None
            }
        }
        IrField::Color => ir_style.color.map(|c| c.to_css_string()),
        IrField::BackgroundColor => ir_style.background_color.map(|c| c.to_css_string()),
        IrField::VerticalAlign => {
            if ir_style.vertical_align_super {
                Some("super".to_string())
            } else if ir_style.vertical_align_sub {
                Some("sub".to_string())
            } else {
                None
            }
        }
        IrField::TextDecorationUnderline => {
            if ir_style.text_decoration_underline {
                Some("underline".to_string())
            } else {
                None
            }
        }
        IrField::TextDecorationStrikethrough => {
            if ir_style.text_decoration_line_through {
                Some("line-through".to_string())
            } else {
                None
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_css_length() {
        assert_eq!(parse_css_length("10px"), Some((10.0, "px".into())));
        assert_eq!(parse_css_length("1.5em"), Some((1.5, "em".into())));
        assert_eq!(parse_css_length("100%"), Some((100.0, "%".into())));
        assert_eq!(parse_css_length("12pt"), Some((12.0, "pt".into())));
    }

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_color("000"), Some((0, 0, 0)));
        assert_eq!(parse_hex_color("ff0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex_color("00ff00"), Some((0, 255, 0)));
    }

    #[test]
    fn test_parse_named_color() {
        assert_eq!(parse_css_color("red"), Some((255, 0, 0)));
        assert_eq!(parse_css_color("BLACK"), Some((0, 0, 0)));
        assert_eq!(parse_css_color("White"), Some((255, 255, 255)));
    }

    #[test]
    fn test_extract_shorthand_one_value() {
        let parts = vec!["10px"];
        assert!(extract_shorthand_value(&parts, 0, None).is_some());
        assert!(extract_shorthand_value(&parts, 1, None).is_some());
        assert!(extract_shorthand_value(&parts, 2, None).is_some());
        assert!(extract_shorthand_value(&parts, 3, None).is_some());
    }

    #[test]
    fn test_extract_shorthand_two_values() {
        let parts = vec!["10px", "20px"];
        // top/bottom = 10px
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 0, None) {
            assert_eq!(s, "10px");
        }
        // left/right = 20px
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 1, None) {
            assert_eq!(s, "20px");
        }
    }

    #[test]
    fn test_extract_shorthand_four_values() {
        let parts = vec!["1px", "2px", "3px", "4px"];
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 0, None) {
            assert_eq!(s, "1px"); // top
        }
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 1, None) {
            assert_eq!(s, "2px"); // right
        }
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 2, None) {
            assert_eq!(s, "3px"); // bottom
        }
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 3, None) {
            assert_eq!(s, "4px"); // left
        }
    }

    #[test]
    fn test_map_transform() {
        let transform = ValueTransform::Map(vec![
            ("bold".into(), KfxValue::Integer(700)),
            ("normal".into(), KfxValue::Integer(400)),
        ]);

        assert_eq!(transform.apply("bold"), Some(KfxValue::Integer(700)));
        assert_eq!(transform.apply("BOLD"), Some(KfxValue::Integer(700)));
        assert_eq!(transform.apply("normal"), Some(KfxValue::Integer(400)));
        assert_eq!(transform.apply("unknown"), None);
    }

    #[test]
    fn test_scale_float_transform() {
        let transform = ValueTransform::ScaleFloat {
            factor: 100.0,
            min: Some(0.0),
            max: Some(1000.0),
            precision: RoundingMode::Round,
        };

        assert_eq!(transform.apply("1.5"), Some(KfxValue::Integer(150)));
        assert_eq!(transform.apply("0.5"), Some(KfxValue::Integer(50)));
    }

    #[test]
    fn test_schema_lookup() {
        let schema = StyleSchema::standard();

        assert!(schema.get("font-weight").is_some());
        assert!(schema.get("font-style").is_some());
        assert!(schema.get("text-align").is_some());
        assert!(schema.get("nonexistent").is_none());
    }

    #[test]
    fn test_font_weight_transform() {
        let schema = StyleSchema::standard();
        let rule = schema.get("font-weight").unwrap();

        let result = rule.transform.apply("bold");
        assert!(matches!(result, Some(KfxValue::Symbol(KfxSymbol::Bold))));

        let result = rule.transform.apply("normal");
        assert!(matches!(result, Some(KfxValue::Symbol(KfxSymbol::Normal))));
    }

    #[test]
    fn test_extract_ir_field_font_weight() {
        use crate::ir::{ComputedStyle, FontWeight};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::FontWeight), None);

        let mut bold = ComputedStyle::default();
        bold.font_weight = FontWeight::BOLD;
        assert_eq!(extract_ir_field(&bold, IrField::FontWeight), Some("bold".to_string()));
    }

    #[test]
    fn test_extract_ir_field_font_style() {
        use crate::ir::{ComputedStyle, FontStyle};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::FontStyle), None);

        let mut italic = ComputedStyle::default();
        italic.font_style = FontStyle::Italic;
        assert_eq!(extract_ir_field(&italic, IrField::FontStyle), Some("italic".to_string()));
    }

    #[test]
    fn test_extract_ir_field_color() {
        use crate::ir::{Color, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::Color), None);

        let mut styled = ComputedStyle::default();
        styled.color = Some(Color::rgb(255, 0, 0));
        assert_eq!(extract_ir_field(&styled, IrField::Color), Some("#ff0000".to_string()));
    }

    #[test]
    fn test_extract_ir_field_margin() {
        use crate::ir::{ComputedStyle, Length};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::MarginTop), None);

        let mut styled = ComputedStyle::default();
        styled.margin_top = Length::Em(1.5);
        assert_eq!(extract_ir_field(&styled, IrField::MarginTop), Some("1.5em".to_string()));
    }

    #[test]
    fn test_schema_ir_mapped_rules() {
        let schema = StyleSchema::standard();

        // Count rules with IR field mappings
        let mapped_count = schema.ir_mapped_rules().count();
        assert!(mapped_count > 10, "Expected >10 IR-mapped rules, got {}", mapped_count);

        // All mapped rules should have ir_field set
        for rule in schema.ir_mapped_rules() {
            assert!(rule.ir_field.is_some(), "Rule {} has no ir_field", rule.ir_key);
        }
    }

    // ========================================================================
    // Additional Edge Case Tests
    // ========================================================================

    #[test]
    fn test_extract_shorthand_three_values() {
        // 3 values: (top, horizontal, bottom)
        let parts = vec!["1px", "2px", "3px"];

        // index 0 = top
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 0, None) {
            assert_eq!(s, "1px");
        } else {
            panic!("Expected Some for index 0");
        }

        // index 1 = right (uses horizontal)
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 1, None) {
            assert_eq!(s, "2px");
        } else {
            panic!("Expected Some for index 1");
        }

        // index 2 = bottom
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 2, None) {
            assert_eq!(s, "3px");
        } else {
            panic!("Expected Some for index 2");
        }

        // index 3 = left (uses horizontal)
        if let Some(KfxValue::String(s)) = extract_shorthand_value(&parts, 3, None) {
            assert_eq!(s, "2px");
        } else {
            panic!("Expected Some for index 3");
        }
    }

    #[test]
    fn test_whitespace_handling() {
        // ExtractShorthand with excessive whitespace
        let transform = ValueTransform::ExtractShorthand {
            index: 1,
            default_value: None,
        };

        // Multiple spaces between values
        let result = transform.apply("  10px    20px  ");
        if let Some(KfxValue::String(s)) = result {
            assert_eq!(s, "20px");
        } else {
            panic!("Expected Some with whitespace handling");
        }
    }

    #[test]
    fn test_css_length_with_whitespace() {
        // Leading/trailing whitespace
        assert_eq!(parse_css_length("  10px  "), Some((10.0, "px".into())));
        assert_eq!(parse_css_length("\t1.5em\n"), Some((1.5, "em".into())));
    }

    #[test]
    fn test_scale_float_clamping() {
        let transform = ValueTransform::ScaleFloat {
            factor: 100.0,
            min: Some(0.0),
            max: Some(500.0),
            precision: RoundingMode::Round,
        };

        // Should clamp to max
        assert_eq!(transform.apply("10.0"), Some(KfxValue::Integer(500)));

        // Should clamp to min
        assert_eq!(transform.apply("-5.0"), Some(KfxValue::Integer(0)));

        // Within range
        assert_eq!(transform.apply("2.5"), Some(KfxValue::Integer(250)));
    }

    #[test]
    fn test_scale_float_rounding_modes() {
        // Test Floor
        let floor_transform = ValueTransform::ScaleFloat {
            factor: 1.0,
            min: None,
            max: None,
            precision: RoundingMode::Floor,
        };
        assert_eq!(floor_transform.apply("1.9"), Some(KfxValue::Integer(1)));
        assert_eq!(floor_transform.apply("-1.1"), Some(KfxValue::Integer(-2)));

        // Test Ceil
        let ceil_transform = ValueTransform::ScaleFloat {
            factor: 1.0,
            min: None,
            max: None,
            precision: RoundingMode::Ceil,
        };
        assert_eq!(ceil_transform.apply("1.1"), Some(KfxValue::Integer(2)));
        assert_eq!(ceil_transform.apply("-1.9"), Some(KfxValue::Integer(-1)));

        // Test Round
        let round_transform = ValueTransform::ScaleFloat {
            factor: 1.0,
            min: None,
            max: None,
            precision: RoundingMode::Round,
        };
        assert_eq!(round_transform.apply("1.4"), Some(KfxValue::Integer(1)));
        assert_eq!(round_transform.apply("1.5"), Some(KfxValue::Integer(2)));
    }

    #[test]
    fn test_scale_float_handles_nan() {
        let transform = ValueTransform::ScaleFloat {
            factor: 100.0,
            min: None,
            max: None,
            precision: RoundingMode::Round,
        };

        // Invalid input that would produce NaN
        assert_eq!(transform.apply("not_a_number"), None);
        assert_eq!(transform.apply(""), None);
    }

    #[test]
    fn test_convert_unit_division_by_zero() {
        let transform = ValueTransform::ConvertUnit {
            base_pixels: 0.0, // Division by zero
            target_unit: KfxUnitType::Em,
        };

        // Should return None, not panic
        assert_eq!(transform.apply("10px"), None);
    }

    #[test]
    fn test_shorthand_empty_input() {
        let parts: Vec<&str> = vec![];
        let default = Some(KfxValue::String("0px".to_string()));

        // Should return default when no parts
        let result = extract_shorthand_value(&parts, 0, default.clone());
        assert_eq!(result, default);
    }

    #[test]
    fn test_shorthand_out_of_bounds_uses_default() {
        let parts = vec!["10px", "20px"];
        let default = Some(KfxValue::String("0px".to_string()));

        // Index 4 is out of bounds for 2 values, but our logic handles it
        // Actually for 2 values, index 0-3 are all valid due to expansion
        // Let's test with 4 values and index 5
        let parts4 = vec!["1px", "2px", "3px", "4px"];
        let result = extract_shorthand_value(&parts4, 5, default.clone());
        // parts.get(5) returns None, so should use default
        assert_eq!(result, default);
    }

    #[test]
    fn test_parse_color_with_whitespace() {
        // Colors should handle whitespace
        assert_eq!(parse_css_color("  red  "), Some((255, 0, 0)));
        assert_eq!(parse_css_color("  #ff0000  "), Some((255, 0, 0)));
    }

    #[test]
    fn test_rgb_function_parsing() {
        assert_eq!(parse_css_color("rgb(255, 0, 0)"), Some((255, 0, 0)));
        assert_eq!(parse_css_color("rgb(0, 128, 255)"), Some((0, 128, 255)));
        assert_eq!(parse_css_color("rgba(255, 255, 255, 0.5)"), Some((255, 255, 255)));
    }

    #[test]
    fn test_rgb_percentage_parsing() {
        assert_eq!(parse_css_color("rgb(100%, 0%, 0%)"), Some((255, 0, 0)));
        assert_eq!(parse_css_color("rgb(50%, 50%, 50%)"), Some((128, 128, 128)));
    }

    #[test]
    fn test_negative_numbers() {
        // Negative values are valid in CSS (e.g., text-indent: -10px)
        assert_eq!(parse_css_length("-10px"), Some((-10.0, "px".into())));
        assert_eq!(parse_css_length("-1.5em"), Some((-1.5, "em".into())));
    }

    #[test]
    fn test_unit_conversion_factors() {
        // Verify unit conversion is correct
        let base = 16.0; // Standard base font size

        assert_eq!(convert_to_pixels(1.0, "px", base), 1.0);
        assert_eq!(convert_to_pixels(1.0, "em", base), 16.0);
        assert_eq!(convert_to_pixels(1.0, "rem", base), 16.0);
        assert_eq!(convert_to_pixels(72.0, "pt", base), 96.0); // 72pt = 1 inch = 96px
        assert_eq!(convert_to_pixels(100.0, "%", base), 16.0); // 100% of 16px
        assert_eq!(convert_to_pixels(1.0, "in", base), 96.0);
    }

    // ========================================================================
    // Amazon KFX Compatibility Tests
    // ========================================================================

    #[test]
    fn test_font_weight_full_range_symbols() {
        let schema = StyleSchema::standard();
        let rule = schema.get("font-weight").unwrap();

        // Verify all numeric weights map to correct symbols
        assert!(matches!(rule.transform.apply("100"), Some(KfxValue::Symbol(KfxSymbol::Thin))));
        assert!(matches!(rule.transform.apply("200"), Some(KfxValue::Symbol(KfxSymbol::UltraLight))));
        assert!(matches!(rule.transform.apply("300"), Some(KfxValue::Symbol(KfxSymbol::Light))));
        assert!(matches!(rule.transform.apply("400"), Some(KfxValue::Symbol(KfxSymbol::Normal))));
        assert!(matches!(rule.transform.apply("500"), Some(KfxValue::Symbol(KfxSymbol::Medium))));
        assert!(matches!(rule.transform.apply("600"), Some(KfxValue::Symbol(KfxSymbol::SemiBold))));
        assert!(matches!(rule.transform.apply("700"), Some(KfxValue::Symbol(KfxSymbol::Bold))));
        assert!(matches!(rule.transform.apply("800"), Some(KfxValue::Symbol(KfxSymbol::UltraBold))));
        assert!(matches!(rule.transform.apply("900"), Some(KfxValue::Symbol(KfxSymbol::Heavy))));
    }

    #[test]
    fn test_font_style_oblique_distinct() {
        let schema = StyleSchema::standard();
        let rule = schema.get("font-style").unwrap();

        // Oblique should map to Oblique, NOT Italic (per Amazon's ElementEnums.data)
        assert!(matches!(rule.transform.apply("oblique"), Some(KfxValue::Symbol(KfxSymbol::Oblique))));
        assert!(matches!(rule.transform.apply("italic"), Some(KfxValue::Symbol(KfxSymbol::Italic))));
    }

    #[test]
    fn test_text_alignment_start_end_distinct() {
        let schema = StyleSchema::standard();
        let rule = schema.get("text-align").unwrap();

        // Start/End should be distinct from Left/Right for RTL support
        assert!(matches!(rule.transform.apply("start"), Some(KfxValue::Symbol(KfxSymbol::Start))));
        assert!(matches!(rule.transform.apply("end"), Some(KfxValue::Symbol(KfxSymbol::End))));
        assert!(matches!(rule.transform.apply("left"), Some(KfxValue::Symbol(KfxSymbol::Left))));
        assert!(matches!(rule.transform.apply("right"), Some(KfxValue::Symbol(KfxSymbol::Right))));
    }

    #[test]
    fn test_color_packed_integer() {
        let schema = StyleSchema::standard();
        let rule = schema.get("color").unwrap();

        // Colors should output packed integers, not strings
        let result = rule.transform.apply("#ff0000");
        assert!(matches!(result, Some(KfxValue::Integer(0xFF0000))));

        let result = rule.transform.apply("rgb(0, 128, 255)");
        assert!(matches!(result, Some(KfxValue::Integer(0x0080FF))));
    }

    #[test]
    fn test_baseline_style_field() {
        let schema = StyleSchema::standard();
        let rule = schema.get("vertical-align").unwrap();

        // Should use BaselineStyle symbol, not TextBaseline
        assert_eq!(rule.kfx_symbol, KfxSymbol::BaselineStyle);

        assert!(matches!(rule.transform.apply("super"), Some(KfxValue::Symbol(KfxSymbol::Superscript))));
        assert!(matches!(rule.transform.apply("sub"), Some(KfxValue::Symbol(KfxSymbol::Subscript))));
    }
}
