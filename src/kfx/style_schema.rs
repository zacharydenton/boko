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
// Constants
// ============================================================================

/// Default base font size in pixels.
///
/// Used for converting CSS units (em, rem, %) to absolute values.
/// 16px is the standard browser default.
pub const DEFAULT_BASE_FONT_SIZE: f64 = 16.0;

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
    /// NOTE: This does NOT convert units - use ConvertToDimensioned for that.
    Dimensioned { unit: KfxSymbol },

    /// Convert CSS units and output as KFX dimensioned value.
    ///
    /// This is the proper transform for block layout properties:
    /// 1. Parses CSS length (e.g., "20px", "1.5em", "10%")
    /// 2. Converts to target unit using base_font_size
    /// 3. Outputs as { value: N, unit: $symbol }
    ///
    /// Percentages are preserved as-is with the `percent` unit:
    /// - "75%" → { value: 75., unit: percent }
    /// - "20px" with base=16, target=Em → { value: 1.25, unit: em }
    ConvertToDimensioned {
        /// Base font size in pixels (for em/rem conversion)
        base_pixels: f64,
        /// Target KFX unit (used for non-percentage values)
        target_unit: KfxSymbol,
    },

    /// Symbol lookup: converts string to KFX symbol ID.
    ToSymbol,

    /// Wrap integer in a struct with a single field.
    /// Used for orphans/widows: `3` -> `{ first: 3 }` or `{ last: 3 }`
    WrapInStruct {
        /// Field name symbol (e.g., First or Last)
        field: KfxSymbol,
        /// Minimum value (KFX enforces min of 1 for orphans/widows)
        min_value: Option<i64>,
    },
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
    Dimensioned {
        value: f64,
        unit: KfxSymbol,
    },
    /// Single-field struct: { field: value }
    /// Used for orphans/widows: { first: N } or { last: N }
    StructField {
        field: KfxSymbol,
        value: i64,
    },
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
                (
                    KfxSymbol::Value as u64,
                    IonValue::Decimal(value.to_string()),
                ),
                (KfxSymbol::Unit as u64, IonValue::Symbol(*unit as u64)),
            ]),
            KfxValue::StructField { field, value } => {
                IonValue::Struct(vec![(*field as u64, IonValue::Int(*value))])
            }
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
    FontVariant,
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
    // Phase 1: Text properties
    LetterSpacing,
    WordSpacing,
    TextTransform,
    Hyphens,
    NoBreak,
    // Phase 2: Text decoration extensions
    UnderlineStyle,
    Overline,
    UnderlineColor,
    // Phase 3: Layout properties
    Width,
    Height,
    MaxWidth,
    MinHeight,
    Float,
    /// Derived from margin-left: auto + margin-right: auto
    BoxAlign,
    // Phase 4: Page break properties
    BreakBefore,
    BreakAfter,
    BreakInside,
    // Phase 5: Border properties
    BorderStyleTop,
    BorderStyleRight,
    BorderStyleBottom,
    BorderStyleLeft,
    BorderWidthTop,
    BorderWidthRight,
    BorderWidthBottom,
    BorderWidthLeft,
    BorderColorTop,
    BorderColorRight,
    BorderColorBottom,
    BorderColorLeft,
    BorderRadiusTopLeft,
    BorderRadiusTopRight,
    BorderRadiusBottomLeft,
    BorderRadiusBottomRight,
    // Phase 6: List properties
    ListStylePosition,
    ListStyleType,
    // Phase 7: Font family (string value)
    FontFamily,
    // Phase 8: Amazon properties
    Language,
    Visibility,
    /// Maps CSS box-sizing to KFX sizing_bounds
    SizingBounds,
    // Phase 9: Additional layout properties
    Clear,
    MinWidth,
    MaxHeight,
    // Phase 10: Pagination control
    Orphans,
    Widows,
    // Phase 11: Text wrapping
    WordBreak,
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

impl Default for StyleSchema {
    fn default() -> Self {
        Self::new()
    }
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

    /// Get the standard KFX style schema (cached).
    pub fn standard() -> &'static Self {
        use std::sync::LazyLock;
        static STANDARD: LazyLock<StyleSchema> = LazyLock::new(StyleSchema::build_standard);
        &STANDARD
    }

    /// Build the standard KFX style schema.
    fn build_standard() -> Self {
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
            transform: ValueTransform::Dimensioned {
                unit: KfxSymbol::Rem,
            },
            context: StyleContext::Any,
        });

        // font-variant: small-caps -> glyph_transform: small_caps
        schema.register(StylePropertyRule {
            ir_key: "font-variant",
            ir_field: Some(IrField::FontVariant),
            kfx_symbol: KfxSymbol::GlyphTransform,
            transform: ValueTransform::Map(vec![(
                "small-caps".into(),
                KfxValue::Symbol(KfxSymbol::SmallCaps),
            )]),
            context: StyleContext::InlineSafe,
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
            // Convert CSS indent (px/em/%) to KFX em units
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "line-height",
            ir_field: Some(IrField::LineHeight),
            kfx_symbol: KfxSymbol::LineHeight,
            // Convert CSS line-height (px/em/%) to KFX em units
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        // text-decoration: underline -> underline: solid (symbol, not bool)
        schema.register(StylePropertyRule {
            ir_key: "text-decoration",
            ir_field: Some(IrField::TextDecorationUnderline),
            kfx_symbol: KfxSymbol::Underline,
            transform: ValueTransform::Map(vec![
                ("underline".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("true".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("false".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ]),
            context: StyleContext::InlineSafe,
        });

        // text-decoration: line-through -> strikethrough: solid (symbol, not bool)
        schema.register(StylePropertyRule {
            ir_key: "text-decoration-strikethrough",
            ir_field: Some(IrField::TextDecorationStrikethrough),
            kfx_symbol: KfxSymbol::Strikethrough,
            transform: ValueTransform::Map(vec![
                ("line-through".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("true".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("false".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ]),
            context: StyleContext::InlineSafe,
        });

        // ====================================================================
        // Spacing Properties (Margins)
        // ====================================================================
        //
        // Margins use ConvertToDimensioned for proper px→em conversion.
        // KFX expects em-based dimensions for consistent scaling.

        schema.register(StylePropertyRule {
            ir_key: "margin-top",
            ir_field: Some(IrField::MarginTop),
            kfx_symbol: KfxSymbol::MarginTop,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-bottom",
            ir_field: Some(IrField::MarginBottom),
            kfx_symbol: KfxSymbol::MarginBottom,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-left",
            ir_field: Some(IrField::MarginLeft),
            kfx_symbol: KfxSymbol::MarginLeft,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-right",
            ir_field: Some(IrField::MarginRight),
            kfx_symbol: KfxSymbol::MarginRight,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Spacing Properties (Padding)
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "padding-top",
            ir_field: Some(IrField::PaddingTop),
            kfx_symbol: KfxSymbol::PaddingTop,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "padding-bottom",
            ir_field: Some(IrField::PaddingBottom),
            kfx_symbol: KfxSymbol::PaddingBottom,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "padding-left",
            ir_field: Some(IrField::PaddingLeft),
            kfx_symbol: KfxSymbol::PaddingLeft,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "padding-right",
            ir_field: Some(IrField::PaddingRight),
            kfx_symbol: KfxSymbol::PaddingRight,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
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

        // baseline_style: for inline super/sub positioning (text baseline shift)
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

        // NOTE: yj.vertical_align for top/middle/bottom is handled separately
        // in StyleBuilder::ingest_ir_style() since the schema only supports
        // one rule per key.

        // ====================================================================
        // Phase 1: High-Priority Text Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "letter-spacing",
            ir_field: Some(IrField::LetterSpacing),
            kfx_symbol: KfxSymbol::Letterspacing,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "word-spacing",
            ir_field: Some(IrField::WordSpacing),
            kfx_symbol: KfxSymbol::Wordspacing,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "text-transform",
            ir_field: Some(IrField::TextTransform),
            kfx_symbol: KfxSymbol::TextTransform,
            transform: ValueTransform::Map(vec![
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("uppercase".into(), KfxValue::Symbol(KfxSymbol::Uppercase)),
                ("lowercase".into(), KfxValue::Symbol(KfxSymbol::Lowercase)),
                ("capitalize".into(), KfxValue::Symbol(KfxSymbol::Titlecase)),
            ]),
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "hyphens",
            ir_field: Some(IrField::Hyphens),
            kfx_symbol: KfxSymbol::Hyphens,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                ("manual".into(), KfxValue::Symbol(KfxSymbol::Manual)),
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ]),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "white-space",
            ir_field: Some(IrField::NoBreak),
            kfx_symbol: KfxSymbol::Nobreak,
            transform: ValueTransform::Map(vec![
                ("nowrap".into(), KfxValue::Bool(true)),
                ("normal".into(), KfxValue::Bool(false)),
            ]),
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 2: Text Decoration Extensions
        // ====================================================================

        // Underline style (solid/dotted/dashed/double)
        // Note: This extends the existing underline property with style info
        schema.register(StylePropertyRule {
            ir_key: "text-decoration-style",
            ir_field: Some(IrField::UnderlineStyle),
            kfx_symbol: KfxSymbol::Underline,
            transform: ValueTransform::Map(vec![
                ("solid".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("dotted".into(), KfxValue::Symbol(KfxSymbol::Dotted)),
                ("dashed".into(), KfxValue::Symbol(KfxSymbol::Dashed)),
                ("double".into(), KfxValue::Symbol(KfxSymbol::Double)),
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ]),
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "overline",
            ir_field: Some(IrField::Overline),
            kfx_symbol: KfxSymbol::Overline,
            transform: ValueTransform::Map(vec![
                ("true".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("solid".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("false".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ]),
            context: StyleContext::InlineSafe,
        });

        schema.register(StylePropertyRule {
            ir_key: "text-decoration-color",
            ir_field: Some(IrField::UnderlineColor),
            kfx_symbol: KfxSymbol::UnderlineColor,
            transform: ValueTransform::ParseColor {
                output_format: ColorFormat::PackedInt,
            },
            context: StyleContext::InlineSafe,
        });

        // ====================================================================
        // Phase 3: Layout Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "width",
            ir_field: Some(IrField::Width),
            kfx_symbol: KfxSymbol::Width,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "height",
            ir_field: Some(IrField::Height),
            kfx_symbol: KfxSymbol::Height,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "max-width",
            ir_field: Some(IrField::MaxWidth),
            kfx_symbol: KfxSymbol::MaxWidth,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "min-height",
            ir_field: Some(IrField::MinHeight),
            kfx_symbol: KfxSymbol::MinHeight,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "min-width",
            ir_field: Some(IrField::MinWidth),
            kfx_symbol: KfxSymbol::MinWidth,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "max-height",
            ir_field: Some(IrField::MaxHeight),
            kfx_symbol: KfxSymbol::MaxHeight,
            transform: ValueTransform::ConvertToDimensioned {
                base_pixels: DEFAULT_BASE_FONT_SIZE,
                target_unit: KfxSymbol::Em,
            },
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "float",
            ir_field: Some(IrField::Float),
            kfx_symbol: KfxSymbol::Float,
            transform: ValueTransform::Map(vec![
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("left".into(), KfxValue::Symbol(KfxSymbol::Left)),
                ("right".into(), KfxValue::Symbol(KfxSymbol::Right)),
            ]),
            context: StyleContext::BlockOnly,
        });

        // box_align: derived from margin-left: auto + margin-right: auto
        schema.register(StylePropertyRule {
            ir_key: "box-align",
            ir_field: Some(IrField::BoxAlign),
            kfx_symbol: KfxSymbol::BoxAlign,
            transform: ValueTransform::Map(vec![(
                "center".into(),
                KfxValue::Symbol(KfxSymbol::Center),
            )]),
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 4: Page Break Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "break-before",
            ir_field: Some(IrField::BreakBefore),
            kfx_symbol: KfxSymbol::BreakBefore,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                ("always".into(), KfxValue::Symbol(KfxSymbol::Always)),
                ("avoid".into(), KfxValue::Symbol(KfxSymbol::Avoid)),
                ("column".into(), KfxValue::Symbol(KfxSymbol::Column)),
                // Legacy CSS2 page-break-* values
                ("page".into(), KfxValue::Symbol(KfxSymbol::Always)),
            ]),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "break-after",
            ir_field: Some(IrField::BreakAfter),
            kfx_symbol: KfxSymbol::BreakAfter,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                ("always".into(), KfxValue::Symbol(KfxSymbol::Always)),
                ("avoid".into(), KfxValue::Symbol(KfxSymbol::Avoid)),
                ("column".into(), KfxValue::Symbol(KfxSymbol::Column)),
                ("page".into(), KfxValue::Symbol(KfxSymbol::Always)),
            ]),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "break-inside",
            ir_field: Some(IrField::BreakInside),
            kfx_symbol: KfxSymbol::BreakInside,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                ("avoid".into(), KfxValue::Symbol(KfxSymbol::Avoid)),
            ]),
            context: StyleContext::BlockOnly,
        });

        // Kindle-specific break properties (yj_break_before/after)
        // These use the same IR fields but different KFX symbols
        schema.register(StylePropertyRule {
            ir_key: "yj-break-before",
            ir_field: Some(IrField::BreakBefore),
            kfx_symbol: KfxSymbol::YjBreakBefore,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                ("always".into(), KfxValue::Symbol(KfxSymbol::Always)),
                ("avoid".into(), KfxValue::Symbol(KfxSymbol::Avoid)),
            ]),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "yj-break-after",
            ir_field: Some(IrField::BreakAfter),
            kfx_symbol: KfxSymbol::YjBreakAfter,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                ("always".into(), KfxValue::Symbol(KfxSymbol::Always)),
                ("avoid".into(), KfxValue::Symbol(KfxSymbol::Avoid)),
            ]),
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 5: Border Properties
        // ====================================================================

        // Border style transform (shared by all sides)
        let border_style_transform = ValueTransform::Map(vec![
            ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ("solid".into(), KfxValue::Symbol(KfxSymbol::Solid)),
            ("dotted".into(), KfxValue::Symbol(KfxSymbol::Dotted)),
            ("dashed".into(), KfxValue::Symbol(KfxSymbol::Dashed)),
            ("double".into(), KfxValue::Symbol(KfxSymbol::Double)),
            ("groove".into(), KfxValue::Symbol(KfxSymbol::Groove)),
            ("ridge".into(), KfxValue::Symbol(KfxSymbol::Ridge)),
            ("inset".into(), KfxValue::Symbol(KfxSymbol::Inset)),
            ("outset".into(), KfxValue::Symbol(KfxSymbol::Outset)),
        ]);

        schema.register(StylePropertyRule {
            ir_key: "border-top-style",
            ir_field: Some(IrField::BorderStyleTop),
            kfx_symbol: KfxSymbol::BorderStyleTop,
            transform: border_style_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-right-style",
            ir_field: Some(IrField::BorderStyleRight),
            kfx_symbol: KfxSymbol::BorderStyleRight,
            transform: border_style_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-style",
            ir_field: Some(IrField::BorderStyleBottom),
            kfx_symbol: KfxSymbol::BorderStyleBottom,
            transform: border_style_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-left-style",
            ir_field: Some(IrField::BorderStyleLeft),
            kfx_symbol: KfxSymbol::BorderStyleLeft,
            transform: border_style_transform,
            context: StyleContext::BlockOnly,
        });

        // Border widths
        let border_width_transform = ValueTransform::ConvertToDimensioned {
            base_pixels: DEFAULT_BASE_FONT_SIZE,
            target_unit: KfxSymbol::Em,
        };

        schema.register(StylePropertyRule {
            ir_key: "border-top-width",
            ir_field: Some(IrField::BorderWidthTop),
            kfx_symbol: KfxSymbol::BorderWeightTop,
            transform: border_width_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-right-width",
            ir_field: Some(IrField::BorderWidthRight),
            kfx_symbol: KfxSymbol::BorderWeightRight,
            transform: border_width_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-width",
            ir_field: Some(IrField::BorderWidthBottom),
            kfx_symbol: KfxSymbol::BorderWeightBottom,
            transform: border_width_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-left-width",
            ir_field: Some(IrField::BorderWidthLeft),
            kfx_symbol: KfxSymbol::BorderWeightLeft,
            transform: border_width_transform,
            context: StyleContext::BlockOnly,
        });

        // Border colors
        let border_color_transform = ValueTransform::ParseColor {
            output_format: ColorFormat::PackedInt,
        };

        schema.register(StylePropertyRule {
            ir_key: "border-top-color",
            ir_field: Some(IrField::BorderColorTop),
            kfx_symbol: KfxSymbol::BorderColorTop,
            transform: border_color_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-right-color",
            ir_field: Some(IrField::BorderColorRight),
            kfx_symbol: KfxSymbol::BorderColorRight,
            transform: border_color_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-color",
            ir_field: Some(IrField::BorderColorBottom),
            kfx_symbol: KfxSymbol::BorderColorBottom,
            transform: border_color_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-left-color",
            ir_field: Some(IrField::BorderColorLeft),
            kfx_symbol: KfxSymbol::BorderColorLeft,
            transform: border_color_transform,
            context: StyleContext::BlockOnly,
        });

        // Border radius
        let border_radius_transform = ValueTransform::ConvertToDimensioned {
            base_pixels: DEFAULT_BASE_FONT_SIZE,
            target_unit: KfxSymbol::Em,
        };

        schema.register(StylePropertyRule {
            ir_key: "border-top-left-radius",
            ir_field: Some(IrField::BorderRadiusTopLeft),
            kfx_symbol: KfxSymbol::BorderRadiusTopLeft,
            transform: border_radius_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-top-right-radius",
            ir_field: Some(IrField::BorderRadiusTopRight),
            kfx_symbol: KfxSymbol::BorderRadiusTopRight,
            transform: border_radius_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-left-radius",
            ir_field: Some(IrField::BorderRadiusBottomLeft),
            kfx_symbol: KfxSymbol::BorderRadiusBottomLeft,
            transform: border_radius_transform.clone(),
            context: StyleContext::BlockOnly,
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-right-radius",
            ir_field: Some(IrField::BorderRadiusBottomRight),
            kfx_symbol: KfxSymbol::BorderRadiusBottomRight,
            transform: border_radius_transform,
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 6: List Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "list-style-position",
            ir_field: Some(IrField::ListStylePosition),
            kfx_symbol: KfxSymbol::ListStylePosition,
            transform: ValueTransform::Map(vec![
                ("outside".into(), KfxValue::Symbol(KfxSymbol::Outside)),
                ("inside".into(), KfxValue::Symbol(KfxSymbol::Inside)),
            ]),
            context: StyleContext::BlockOnly,
        });

        // list-style-type → list_style (symbol values)
        schema.register(StylePropertyRule {
            ir_key: "list-style-type",
            ir_field: Some(IrField::ListStyleType),
            kfx_symbol: KfxSymbol::ListStyle,
            transform: ValueTransform::Map(vec![
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("disc".into(), KfxValue::Symbol(KfxSymbol::Disc)),
                ("circle".into(), KfxValue::Symbol(KfxSymbol::Circle)),
                ("square".into(), KfxValue::Symbol(KfxSymbol::Square)),
                ("decimal".into(), KfxValue::Symbol(KfxSymbol::Numeric)),
                (
                    "lower-roman".into(),
                    KfxValue::Symbol(KfxSymbol::RomanLower),
                ),
                (
                    "upper-roman".into(),
                    KfxValue::Symbol(KfxSymbol::RomanUpper),
                ),
                (
                    "lower-alpha".into(),
                    KfxValue::Symbol(KfxSymbol::AlphaLower),
                ),
                (
                    "upper-alpha".into(),
                    KfxValue::Symbol(KfxSymbol::AlphaUpper),
                ),
                // CSS aliases
                (
                    "lower-latin".into(),
                    KfxValue::Symbol(KfxSymbol::AlphaLower),
                ),
                (
                    "upper-latin".into(),
                    KfxValue::Symbol(KfxSymbol::AlphaUpper),
                ),
            ]),
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 7: Font Family (string value, not symbol)
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "font-family",
            ir_field: Some(IrField::FontFamily),
            kfx_symbol: KfxSymbol::FontFamily,
            transform: ValueTransform::Identity, // String passthrough
            context: StyleContext::InlineSafe,
        });

        // ====================================================================
        // Phase 8: Amazon Properties
        // ====================================================================

        // Language (maps to HTML lang attribute in CSS, stored as string in KFX)
        schema.register(StylePropertyRule {
            ir_key: "language",
            ir_field: Some(IrField::Language),
            kfx_symbol: KfxSymbol::Language,
            transform: ValueTransform::Identity,
            context: StyleContext::Any,
        });

        // Visibility
        schema.register(StylePropertyRule {
            ir_key: "visibility",
            ir_field: Some(IrField::Visibility),
            kfx_symbol: KfxSymbol::Visibility,
            transform: ValueTransform::Map(vec![
                ("visible".into(), KfxValue::Symbol(KfxSymbol::Show)),
                ("hidden".into(), KfxValue::Symbol(KfxSymbol::Hide)),
                ("collapse".into(), KfxValue::Symbol(KfxSymbol::Hide)),
            ]),
            context: StyleContext::Any,
        });

        // Box-sizing → sizing_bounds
        // Amazon auto-adds content-box when width/height is present
        schema.register(StylePropertyRule {
            ir_key: "box-sizing",
            ir_field: Some(IrField::SizingBounds),
            kfx_symbol: KfxSymbol::SizingBounds,
            transform: ValueTransform::Map(vec![
                (
                    "content-box".into(),
                    KfxValue::Symbol(KfxSymbol::ContentBounds),
                ),
                (
                    "border-box".into(),
                    KfxValue::Symbol(KfxSymbol::BorderBounds),
                ),
            ]),
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 8: Additional Layout Properties
        // ====================================================================

        // clear → yj.float_clear
        schema.register(StylePropertyRule {
            ir_key: "clear",
            ir_field: Some(IrField::Clear),
            kfx_symbol: KfxSymbol::YjFloatClear,
            transform: ValueTransform::Map(vec![
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("left".into(), KfxValue::Symbol(KfxSymbol::Left)),
                ("right".into(), KfxValue::Symbol(KfxSymbol::Right)),
                ("both".into(), KfxValue::Symbol(KfxSymbol::Both)),
            ]),
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 9: Pagination Control (orphans/widows)
        // ====================================================================

        // orphans → keep_lines_together: { first: N }
        schema.register(StylePropertyRule {
            ir_key: "orphans",
            ir_field: Some(IrField::Orphans),
            kfx_symbol: KfxSymbol::KeepLinesTogether,
            transform: ValueTransform::WrapInStruct {
                field: KfxSymbol::First,
                min_value: Some(1), // KFX enforces minimum of 1
            },
            context: StyleContext::BlockOnly,
        });

        // widows → keep_lines_together: { last: N }
        schema.register(StylePropertyRule {
            ir_key: "widows",
            ir_field: Some(IrField::Widows),
            kfx_symbol: KfxSymbol::KeepLinesTogether,
            transform: ValueTransform::WrapInStruct {
                field: KfxSymbol::Last,
                min_value: Some(1), // KFX enforces minimum of 1
            },
            context: StyleContext::BlockOnly,
        });

        // ====================================================================
        // Phase 10: Text Wrapping
        // ====================================================================

        // word-break → word_break
        // Note: Only normal and break-all are supported by KFX.
        // keep-all and break-word are not in the KFX symbol table.
        schema.register(StylePropertyRule {
            ir_key: "word-break",
            ir_field: Some(IrField::WordBreak),
            kfx_symbol: KfxSymbol::WordBreak,
            transform: ValueTransform::Map(vec![
                ("normal".into(), KfxValue::Symbol(KfxSymbol::Normal)),
                ("break-all".into(), KfxValue::Symbol(KfxSymbol::BreakAll)),
            ]),
            context: StyleContext::BlockOnly,
        });

        schema
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
                        // KFX uses ARGB format with 0xFF alpha for opaque colors
                        let packed = (0xFF_i64 << 24)
                            | ((color.0 as i64) << 16)
                            | ((color.1 as i64) << 8)
                            | (color.2 as i64);
                        Some(KfxValue::Integer(packed))
                    }
                    ColorFormat::RgbStruct => {
                        // Would need a KfxValue variant for this
                        let packed = (0xFF_i64 << 24)
                            | ((color.0 as i64) << 16)
                            | ((color.1 as i64) << 8)
                            | (color.2 as i64);
                        Some(KfxValue::Integer(packed))
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
                let (num, css_unit) = parse_css_length(raw)?;
                // Preserve percentage values (don't replace % with the rule's unit)
                let actual_unit = if css_unit == "%" {
                    KfxSymbol::Percent
                } else {
                    *unit
                };
                Some(KfxValue::Dimensioned {
                    value: num,
                    unit: actual_unit,
                })
            }

            ValueTransform::ConvertToDimensioned {
                base_pixels,
                target_unit,
            } => {
                let (num, css_unit) = parse_css_length(raw)?;

                // Preserve percentage values as-is (don't convert to em)
                if css_unit == "%" {
                    return Some(KfxValue::Dimensioned {
                        value: num,
                        unit: KfxSymbol::Percent,
                    });
                }

                // Guard against zero base
                if *base_pixels == 0.0 {
                    return None;
                }

                // Convert CSS value to pixels first
                let pixels = convert_to_pixels(num, &css_unit, *base_pixels);

                // Then convert pixels to target unit
                // KFX uses Em for most relative dimensions
                let result = match target_unit {
                    // Em/Rem: relative to base font size (most common for KFX)
                    KfxSymbol::Em | KfxSymbol::Rem => pixels / base_pixels,
                    // Percentage: 100% = base_pixels
                    KfxSymbol::YjPercentage => pixels / base_pixels * 100.0,
                    // Default: convert to em (safest for KFX compatibility)
                    _ => pixels / base_pixels,
                };

                // Guard against NaN/Infinity
                if result.is_nan() || result.is_infinite() {
                    return None;
                }

                Some(KfxValue::Dimensioned {
                    value: result,
                    unit: *target_unit,
                })
            }

            ValueTransform::ToSymbol => Some(KfxValue::String(raw.to_string())),

            ValueTransform::WrapInStruct { field, min_value } => {
                let num = parse_number(raw)? as i64;
                // Apply minimum value (KFX enforces min of 1 for orphans/widows)
                let clamped = if let Some(min) = min_value {
                    num.max(*min)
                } else {
                    num
                };
                Some(KfxValue::StructField {
                    field: *field,
                    value: clamped,
                })
            }
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
        .split([',', ' '])
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
        IrField::FontVariant => {
            if ir_style.font_variant != default.font_variant {
                Some(ir_style.font_variant.to_css_string())
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
            if ir_style.vertical_align != ir_style::VerticalAlign::Baseline {
                Some(ir_style.vertical_align.to_css_string())
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
        // Phase 1: Text properties
        IrField::LetterSpacing => {
            if ir_style.letter_spacing != default.letter_spacing {
                Some(ir_style.letter_spacing.to_css_string())
            } else {
                None
            }
        }
        IrField::WordSpacing => {
            if ir_style.word_spacing != default.word_spacing {
                Some(ir_style.word_spacing.to_css_string())
            } else {
                None
            }
        }
        IrField::TextTransform => {
            if ir_style.text_transform != default.text_transform {
                Some(ir_style.text_transform.to_css_string())
            } else {
                None
            }
        }
        IrField::Hyphens => {
            if ir_style.hyphens != default.hyphens {
                Some(ir_style.hyphens.to_css_string())
            } else {
                None
            }
        }
        IrField::NoBreak => {
            if ir_style.no_break {
                Some("nowrap".to_string())
            } else {
                None
            }
        }
        // Phase 2: Text decoration extensions
        IrField::UnderlineStyle => {
            if ir_style.underline_style != default.underline_style {
                Some(ir_style.underline_style.to_css_string())
            } else {
                None
            }
        }
        IrField::Overline => {
            if ir_style.overline {
                Some("solid".to_string())
            } else {
                None
            }
        }
        IrField::UnderlineColor => ir_style.underline_color.map(|c| c.to_css_string()),
        // Phase 3: Layout properties
        IrField::Width => {
            if ir_style.width != default.width {
                Some(ir_style.width.to_css_string())
            } else {
                None
            }
        }
        IrField::Height => {
            if ir_style.height != default.height {
                Some(ir_style.height.to_css_string())
            } else {
                None
            }
        }
        IrField::MaxWidth => {
            if ir_style.max_width != default.max_width {
                Some(ir_style.max_width.to_css_string())
            } else {
                None
            }
        }
        IrField::MinHeight => {
            if ir_style.min_height != default.min_height {
                Some(ir_style.min_height.to_css_string())
            } else {
                None
            }
        }
        IrField::MinWidth => {
            if ir_style.min_width != default.min_width {
                Some(ir_style.min_width.to_css_string())
            } else {
                None
            }
        }
        IrField::MaxHeight => {
            if ir_style.max_height != default.max_height {
                Some(ir_style.max_height.to_css_string())
            } else {
                None
            }
        }
        IrField::Float => {
            if ir_style.float != default.float {
                Some(ir_style.float.to_css_string())
            } else {
                None
            }
        }
        // Phase 4: Page break properties
        IrField::BreakBefore => {
            if ir_style.break_before != default.break_before {
                Some(ir_style.break_before.to_css_string())
            } else {
                None
            }
        }
        IrField::BreakAfter => {
            if ir_style.break_after != default.break_after {
                Some(ir_style.break_after.to_css_string())
            } else {
                None
            }
        }
        IrField::BreakInside => {
            if ir_style.break_inside != default.break_inside {
                Some(ir_style.break_inside.to_css_string())
            } else {
                None
            }
        }
        // Phase 5: Border properties
        IrField::BorderStyleTop => {
            if ir_style.border_style_top != default.border_style_top {
                Some(ir_style.border_style_top.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderStyleRight => {
            if ir_style.border_style_right != default.border_style_right {
                Some(ir_style.border_style_right.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderStyleBottom => {
            if ir_style.border_style_bottom != default.border_style_bottom {
                Some(ir_style.border_style_bottom.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderStyleLeft => {
            if ir_style.border_style_left != default.border_style_left {
                Some(ir_style.border_style_left.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderWidthTop => {
            if ir_style.border_width_top != default.border_width_top {
                Some(ir_style.border_width_top.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderWidthRight => {
            if ir_style.border_width_right != default.border_width_right {
                Some(ir_style.border_width_right.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderWidthBottom => {
            if ir_style.border_width_bottom != default.border_width_bottom {
                Some(ir_style.border_width_bottom.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderWidthLeft => {
            if ir_style.border_width_left != default.border_width_left {
                Some(ir_style.border_width_left.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderColorTop => ir_style.border_color_top.map(|c| c.to_css_string()),
        IrField::BorderColorRight => ir_style.border_color_right.map(|c| c.to_css_string()),
        IrField::BorderColorBottom => ir_style.border_color_bottom.map(|c| c.to_css_string()),
        IrField::BorderColorLeft => ir_style.border_color_left.map(|c| c.to_css_string()),
        IrField::BorderRadiusTopLeft => {
            if ir_style.border_radius_top_left != default.border_radius_top_left {
                Some(ir_style.border_radius_top_left.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderRadiusTopRight => {
            if ir_style.border_radius_top_right != default.border_radius_top_right {
                Some(ir_style.border_radius_top_right.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderRadiusBottomLeft => {
            if ir_style.border_radius_bottom_left != default.border_radius_bottom_left {
                Some(ir_style.border_radius_bottom_left.to_css_string())
            } else {
                None
            }
        }
        IrField::BorderRadiusBottomRight => {
            if ir_style.border_radius_bottom_right != default.border_radius_bottom_right {
                Some(ir_style.border_radius_bottom_right.to_css_string())
            } else {
                None
            }
        }
        // Phase 6: List properties
        IrField::ListStylePosition => {
            // Only applies to display: list-item
            if ir_style.display == ir_style::Display::ListItem
                && ir_style.list_style_position != default.list_style_position
            {
                Some(ir_style.list_style_position.to_css_string())
            } else {
                None
            }
        }
        IrField::ListStyleType => {
            // Only applies to display: list-item
            if ir_style.display == ir_style::Display::ListItem
                && ir_style.list_style_type != default.list_style_type
            {
                Some(ir_style.list_style_type.to_css_string())
            } else {
                None
            }
        }
        // Phase 7: Font family
        IrField::FontFamily => ir_style.font_family.clone(),
        // Phase 8: Amazon properties
        IrField::Language => ir_style.language.clone(),
        IrField::Visibility => {
            if ir_style.visibility != default.visibility {
                Some(ir_style.visibility.to_css_string())
            } else {
                None
            }
        }
        // BoxAlign: derived from margin-left: auto + margin-right: auto
        IrField::BoxAlign => {
            if ir_style.margin_left == ir_style::Length::Auto
                && ir_style.margin_right == ir_style::Length::Auto
            {
                Some("center".to_string())
            } else {
                None
            }
        }
        // SizingBounds: Amazon auto-adds content-box when width/height is present
        IrField::SizingBounds => {
            // If explicitly border-box, emit it
            if ir_style.box_sizing == ir_style::BoxSizing::BorderBox {
                Some("border-box".to_string())
            // Otherwise, if width or height is set, emit content-box (CSS default)
            } else if ir_style.width != default.width || ir_style.height != default.height {
                Some("content-box".to_string())
            } else {
                None
            }
        }
        // Phase 8: Additional layout properties
        IrField::Clear => {
            if ir_style.clear != default.clear {
                Some(ir_style.clear.to_css_string())
            } else {
                None
            }
        }
        // Phase 9: Pagination control
        IrField::Orphans => {
            if ir_style.orphans != default.orphans {
                Some(ir_style.orphans.to_string())
            } else {
                None
            }
        }
        IrField::Widows => {
            if ir_style.widows != default.widows {
                Some(ir_style.widows.to_string())
            } else {
                None
            }
        }
        // Phase 10: Text wrapping
        IrField::WordBreak => {
            if ir_style.word_break != default.word_break {
                Some(ir_style.word_break.to_css_string())
            } else {
                None
            }
        }
    }
}

// ============================================================================
// KFX Import (Inverse Direction)
// ============================================================================

impl StyleSchema {
    /// Look up a schema rule by its KFX symbol.
    pub fn get_by_kfx_symbol(&self, kfx_symbol: u64) -> Option<&StylePropertyRule> {
        self.rules
            .values()
            .find(|r| r.kfx_symbol as u64 == kfx_symbol)
    }
}

impl ValueTransform {
    /// Apply inverse transform: convert KFX value back to CSS string.
    ///
    /// This is the reverse of `apply()` - used during import.
    pub fn inverse(&self, value: &IonValue) -> Option<String> {
        match self {
            ValueTransform::Identity => {
                // Identity: extract string or symbol text
                value.as_string().map(|s| s.to_string())
            }

            ValueTransform::Map(mappings) => {
                // Reverse map lookup: find CSS value for KFX value
                if let Some(sym_id) = value.as_symbol() {
                    for (css_val, kfx_val) in mappings {
                        if let KfxValue::Symbol(kfx_sym) = kfx_val
                            && *kfx_sym as u64 == sym_id
                        {
                            return Some(css_val.clone());
                        }
                    }
                }
                if let Some(i) = value.as_int() {
                    for (css_val, kfx_val) in mappings {
                        if let KfxValue::Integer(kfx_int) = kfx_val
                            && *kfx_int == i
                        {
                            return Some(css_val.clone());
                        }
                    }
                }
                if let Some(b) = value.as_bool() {
                    for (css_val, kfx_val) in mappings {
                        if let KfxValue::Bool(kfx_bool) = kfx_val
                            && *kfx_bool == b
                        {
                            return Some(css_val.clone());
                        }
                    }
                }
                None
            }

            ValueTransform::Dimensioned { unit }
            | ValueTransform::ConvertToDimensioned {
                target_unit: unit, ..
            } => {
                // Parse {value: N, unit: sym} struct
                // Value may be Int (whole numbers) or Float
                let fields = value.as_struct()?;
                let value_field = get_field_by_symbol(fields, KfxSymbol::Value)?;
                let num = value_field
                    .as_float()
                    .or_else(|| value_field.as_int().map(|i| i as f64))?;
                let unit_sym = get_field_by_symbol(fields, KfxSymbol::Unit)?.as_symbol()? as u32;

                // Convert unit symbol back to CSS unit string
                let unit_str = match unit_sym {
                    id if id == KfxSymbol::Em as u32 => "em",
                    id if id == KfxSymbol::Rem as u32 => "rem",
                    id if id == KfxSymbol::Percent as u32 => "%",
                    id if id == KfxSymbol::Px as u32 => "px",
                    _ => {
                        // Use the expected unit from the rule
                        match *unit as u32 {
                            id if id == KfxSymbol::Em as u32 => "em",
                            id if id == KfxSymbol::Rem as u32 => "rem",
                            id if id == KfxSymbol::Percent as u32 => "%",
                            _ => "em",
                        }
                    }
                };

                Some(format!("{}{}", num, unit_str))
            }

            ValueTransform::ParseColor { .. } => {
                // Packed integer: 0xRRGGBB
                let packed = value.as_int()? as u32;
                let r = (packed >> 16) & 0xFF;
                let g = (packed >> 8) & 0xFF;
                let b = packed & 0xFF;
                Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
            }

            ValueTransform::ScaleFloat { factor, .. } => {
                // Reverse scaling
                let i = value.as_int()?;
                let original = i as f64 / factor;
                Some(original.to_string())
            }

            ValueTransform::WrapInStruct { field, .. } => {
                // Parse struct { field: N } and extract integer value
                let fields = value.as_struct()?;
                let int_value = get_field_by_symbol(fields, *field)?.as_int()?;
                Some(int_value.to_string())
            }

            _ => None, // Other transforms not commonly used for styles
        }
    }
}

/// Helper to get a field from an Ion struct by KfxSymbol.
fn get_field_by_symbol(fields: &[(u64, IonValue)], sym: KfxSymbol) -> Option<&IonValue> {
    fields
        .iter()
        .find(|(k, _)| *k == sym as u64)
        .map(|(_, v)| v)
}

/// Apply a CSS value to an IR ComputedStyle field.
///
/// This is the inverse of `extract_ir_field` - sets the field instead of reading it.
pub fn apply_ir_field(ir_style: &mut ir_style::ComputedStyle, field: IrField, css_value: &str) {
    match field {
        IrField::FontWeight => {
            // Parse CSS font-weight (number or keyword)
            ir_style.font_weight = match css_value {
                "bold" => ir_style::FontWeight::BOLD,
                "normal" => ir_style::FontWeight::NORMAL,
                s => s
                    .parse::<u16>()
                    .map(ir_style::FontWeight)
                    .unwrap_or(ir_style::FontWeight::NORMAL),
            };
        }
        IrField::FontStyle => {
            ir_style.font_style = match css_value {
                "italic" => ir_style::FontStyle::Italic,
                "oblique" => ir_style::FontStyle::Oblique,
                _ => ir_style::FontStyle::Normal,
            };
        }
        IrField::FontSize => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.font_size = len;
            }
        }
        IrField::FontVariant => {
            ir_style.font_variant = match css_value {
                "small-caps" => ir_style::FontVariant::SmallCaps,
                _ => ir_style::FontVariant::Normal,
            };
        }
        IrField::TextAlign => {
            ir_style.text_align = match css_value {
                "left" => ir_style::TextAlign::Left,
                "right" => ir_style::TextAlign::Right,
                "center" => ir_style::TextAlign::Center,
                "justify" => ir_style::TextAlign::Justify,
                "start" => ir_style::TextAlign::Start,
                "end" => ir_style::TextAlign::End,
                _ => ir_style::TextAlign::Start,
            };
        }
        IrField::TextIndent => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.text_indent = len;
            }
        }
        IrField::LineHeight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.line_height = len;
            }
        }
        IrField::MarginTop => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.margin_top = len;
            }
        }
        IrField::MarginBottom => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.margin_bottom = len;
            }
        }
        IrField::MarginLeft => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.margin_left = len;
            }
        }
        IrField::MarginRight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.margin_right = len;
            }
        }
        IrField::PaddingTop => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.padding_top = len;
            }
        }
        IrField::PaddingBottom => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.padding_bottom = len;
            }
        }
        IrField::PaddingLeft => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.padding_left = len;
            }
        }
        IrField::PaddingRight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.padding_right = len;
            }
        }
        IrField::Color => {
            if let Some((r, g, b)) = parse_css_color(css_value) {
                ir_style.color = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BackgroundColor => {
            if let Some((r, g, b)) = parse_css_color(css_value) {
                ir_style.background_color = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::VerticalAlign => {
            if let Some(va) = ir_style::VerticalAlign::from_css(css_value) {
                ir_style.vertical_align = va;
            }
        }
        IrField::TextDecorationUnderline => {
            ir_style.text_decoration_underline = css_value == "underline";
        }
        IrField::TextDecorationStrikethrough => {
            ir_style.text_decoration_line_through = css_value == "line-through";
        }
        // Phase 1: Text properties
        IrField::LetterSpacing => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.letter_spacing = len;
            }
        }
        IrField::WordSpacing => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.word_spacing = len;
            }
        }
        IrField::TextTransform => {
            ir_style.text_transform = match css_value {
                "uppercase" => ir_style::TextTransform::Uppercase,
                "lowercase" => ir_style::TextTransform::Lowercase,
                "capitalize" => ir_style::TextTransform::Capitalize,
                _ => ir_style::TextTransform::None,
            };
        }
        IrField::Hyphens => {
            ir_style.hyphens = match css_value {
                "auto" => ir_style::Hyphens::Auto,
                "manual" => ir_style::Hyphens::Manual,
                _ => ir_style::Hyphens::None,
            };
        }
        IrField::NoBreak => {
            ir_style.no_break = css_value == "nowrap";
        }
        // Phase 2: Text decoration extensions
        IrField::UnderlineStyle => {
            ir_style.underline_style = match css_value {
                "solid" => ir_style::DecorationStyle::Solid,
                "dotted" => ir_style::DecorationStyle::Dotted,
                "dashed" => ir_style::DecorationStyle::Dashed,
                "double" => ir_style::DecorationStyle::Double,
                _ => ir_style::DecorationStyle::None,
            };
        }
        IrField::Overline => {
            ir_style.overline = css_value == "solid" || css_value == "true";
        }
        IrField::UnderlineColor => {
            if let Some((r, g, b)) = parse_css_color(css_value) {
                ir_style.underline_color = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        // Phase 3: Layout properties
        IrField::Width => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.width = len;
            }
        }
        IrField::Height => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.height = len;
            }
        }
        IrField::MaxWidth => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.max_width = len;
            }
        }
        IrField::MinHeight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.min_height = len;
            }
        }
        IrField::MinWidth => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.min_width = len;
            }
        }
        IrField::MaxHeight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.max_height = len;
            }
        }
        IrField::Float => {
            ir_style.float = match css_value {
                "left" => ir_style::Float::Left,
                "right" => ir_style::Float::Right,
                _ => ir_style::Float::None,
            };
        }
        // Phase 4: Page break properties
        IrField::BreakBefore => {
            ir_style.break_before = match css_value {
                "always" | "page" => ir_style::BreakValue::Always,
                "avoid" => ir_style::BreakValue::Avoid,
                "column" => ir_style::BreakValue::Column,
                _ => ir_style::BreakValue::Auto,
            };
        }
        IrField::BreakAfter => {
            ir_style.break_after = match css_value {
                "always" | "page" => ir_style::BreakValue::Always,
                "avoid" => ir_style::BreakValue::Avoid,
                "column" => ir_style::BreakValue::Column,
                _ => ir_style::BreakValue::Auto,
            };
        }
        IrField::BreakInside => {
            ir_style.break_inside = match css_value {
                "avoid" => ir_style::BreakValue::Avoid,
                _ => ir_style::BreakValue::Auto,
            };
        }
        // Phase 5: Border properties
        IrField::BorderStyleTop => {
            ir_style.border_style_top = parse_border_style(css_value);
        }
        IrField::BorderStyleRight => {
            ir_style.border_style_right = parse_border_style(css_value);
        }
        IrField::BorderStyleBottom => {
            ir_style.border_style_bottom = parse_border_style(css_value);
        }
        IrField::BorderStyleLeft => {
            ir_style.border_style_left = parse_border_style(css_value);
        }
        IrField::BorderWidthTop => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_width_top = len;
            }
        }
        IrField::BorderWidthRight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_width_right = len;
            }
        }
        IrField::BorderWidthBottom => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_width_bottom = len;
            }
        }
        IrField::BorderWidthLeft => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_width_left = len;
            }
        }
        IrField::BorderColorTop => {
            if let Some((r, g, b)) = parse_css_color(css_value) {
                ir_style.border_color_top = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BorderColorRight => {
            if let Some((r, g, b)) = parse_css_color(css_value) {
                ir_style.border_color_right = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BorderColorBottom => {
            if let Some((r, g, b)) = parse_css_color(css_value) {
                ir_style.border_color_bottom = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BorderColorLeft => {
            if let Some((r, g, b)) = parse_css_color(css_value) {
                ir_style.border_color_left = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BorderRadiusTopLeft => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_radius_top_left = len;
            }
        }
        IrField::BorderRadiusTopRight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_radius_top_right = len;
            }
        }
        IrField::BorderRadiusBottomLeft => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_radius_bottom_left = len;
            }
        }
        IrField::BorderRadiusBottomRight => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_radius_bottom_right = len;
            }
        }
        // Phase 6: List properties
        IrField::ListStylePosition => {
            ir_style.list_style_position = match css_value {
                "inside" => ir_style::ListStylePosition::Inside,
                _ => ir_style::ListStylePosition::Outside,
            };
        }
        IrField::ListStyleType => {
            ir_style.list_style_type = match css_value {
                "none" => ir_style::ListStyleType::None,
                "disc" => ir_style::ListStyleType::Disc,
                "circle" => ir_style::ListStyleType::Circle,
                "square" => ir_style::ListStyleType::Square,
                "decimal" => ir_style::ListStyleType::Decimal,
                "lower-roman" => ir_style::ListStyleType::LowerRoman,
                "upper-roman" => ir_style::ListStyleType::UpperRoman,
                "lower-alpha" | "lower-latin" => ir_style::ListStyleType::LowerAlpha,
                "upper-alpha" | "upper-latin" => ir_style::ListStyleType::UpperAlpha,
                _ => ir_style::ListStyleType::Disc, // CSS default
            };
        }
        // Phase 7: Font family
        IrField::FontFamily => {
            ir_style.font_family = Some(css_value.to_string());
        }
        // Phase 8: Amazon properties
        IrField::Language => {
            ir_style.language = Some(css_value.to_string());
        }
        IrField::Visibility => {
            ir_style.visibility = match css_value {
                "hidden" | "collapse" => ir_style::Visibility::Hidden,
                _ => ir_style::Visibility::Visible,
            };
        }
        // BoxAlign: reverse of export - set both margins to auto
        IrField::BoxAlign => {
            if css_value == "center" {
                ir_style.margin_left = ir_style::Length::Auto;
                ir_style.margin_right = ir_style::Length::Auto;
            }
        }
        // SizingBounds: CSS box-sizing
        IrField::SizingBounds => {
            ir_style.box_sizing = match css_value {
                "border-box" => ir_style::BoxSizing::BorderBox,
                _ => ir_style::BoxSizing::ContentBox,
            };
        }
        // Phase 8: Additional layout properties
        IrField::Clear => {
            ir_style.clear = match css_value {
                "left" => ir_style::Clear::Left,
                "right" => ir_style::Clear::Right,
                "both" => ir_style::Clear::Both,
                _ => ir_style::Clear::None,
            };
        }
        // Phase 9: Pagination control
        IrField::Orphans => {
            if let Ok(n) = css_value.parse::<u32>() {
                ir_style.orphans = n;
            }
        }
        IrField::Widows => {
            if let Ok(n) = css_value.parse::<u32>() {
                ir_style.widows = n;
            }
        }
        // Phase 10: Text wrapping
        IrField::WordBreak => {
            ir_style.word_break = match css_value {
                "break-all" => ir_style::WordBreak::BreakAll,
                "keep-all" => ir_style::WordBreak::KeepAll,
                "break-word" => ir_style::WordBreak::BreakWord,
                _ => ir_style::WordBreak::Normal,
            };
        }
    }
}

/// Parse a CSS border-style value to IR BorderStyle.
fn parse_border_style(css_value: &str) -> ir_style::BorderStyle {
    match css_value {
        "solid" => ir_style::BorderStyle::Solid,
        "dotted" => ir_style::BorderStyle::Dotted,
        "dashed" => ir_style::BorderStyle::Dashed,
        "double" => ir_style::BorderStyle::Double,
        "groove" => ir_style::BorderStyle::Groove,
        "ridge" => ir_style::BorderStyle::Ridge,
        "inset" => ir_style::BorderStyle::Inset,
        "outset" => ir_style::BorderStyle::Outset,
        _ => ir_style::BorderStyle::None,
    }
}

/// Parse a CSS length string to IR Length.
fn parse_css_length_to_ir(s: &str) -> Option<ir_style::Length> {
    let (value, unit) = parse_css_length(s)?;
    let value = value as f32;
    Some(match unit.as_str() {
        "px" => ir_style::Length::Px(value),
        "em" => ir_style::Length::Em(value),
        "rem" => ir_style::Length::Rem(value),
        "%" => ir_style::Length::Percent(value),
        _ => ir_style::Length::Px(value),
    })
}

/// Import KFX style properties to an IR ComputedStyle using the schema.
///
/// This is the inverse of the export direction:
/// 1. For each KFX property, look up the schema rule by kfx_symbol
/// 2. Apply inverse transform to get CSS value
/// 3. Apply CSS value to IR field
pub fn import_kfx_style(
    schema: &StyleSchema,
    props: &[(u64, IonValue)],
) -> ir_style::ComputedStyle {
    let mut style = ir_style::ComputedStyle::default();

    for (kfx_symbol, kfx_value) in props {
        // Look up the schema rule for this KFX symbol
        if let Some(rule) = schema.get_by_kfx_symbol(*kfx_symbol) {
            // Apply inverse transform to get CSS value
            if let Some(css_value) = rule.transform.inverse(kfx_value) {
                // Apply to IR field (if the rule has an IR field mapping)
                if let Some(ir_field) = rule.ir_field {
                    apply_ir_field(&mut style, ir_field, &css_value);
                }
            }
        }
    }

    style
}

/// Check if a style should be treated as block-like for KFX export.
///
/// KFX doesn't have native `display: inline-block`. Elements with this
/// display type should be emitted as block containers instead of inline spans.
pub fn is_block_display(style: &ir_style::ComputedStyle) -> bool {
    matches!(
        style.display,
        ir_style::Display::Block | ir_style::Display::InlineBlock
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
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
        assert_eq!(
            extract_ir_field(&bold, IrField::FontWeight),
            Some("bold".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_font_style() {
        use crate::ir::{ComputedStyle, FontStyle};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::FontStyle), None);

        let mut italic = ComputedStyle::default();
        italic.font_style = FontStyle::Italic;
        assert_eq!(
            extract_ir_field(&italic, IrField::FontStyle),
            Some("italic".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_color() {
        use crate::ir::{Color, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::Color), None);

        let mut styled = ComputedStyle::default();
        styled.color = Some(Color::rgb(255, 0, 0));
        assert_eq!(
            extract_ir_field(&styled, IrField::Color),
            Some("#ff0000".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_margin() {
        use crate::ir::{ComputedStyle, Length};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::MarginTop), None);

        let mut styled = ComputedStyle::default();
        styled.margin_top = Length::Em(1.5);
        assert_eq!(
            extract_ir_field(&styled, IrField::MarginTop),
            Some("1.5em".to_string())
        );
    }

    #[test]
    fn test_schema_ir_mapped_rules() {
        let schema = StyleSchema::standard();

        // Count rules with IR field mappings
        let mapped_count = schema.ir_mapped_rules().count();
        assert!(
            mapped_count > 10,
            "Expected >10 IR-mapped rules, got {}",
            mapped_count
        );

        // All mapped rules should have ir_field set
        for rule in schema.ir_mapped_rules() {
            assert!(
                rule.ir_field.is_some(),
                "Rule {} has no ir_field",
                rule.ir_key
            );
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
        let _parts = ["10px", "20px"];
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
        assert_eq!(
            parse_css_color("rgba(255, 255, 255, 0.5)"),
            Some((255, 255, 255))
        );
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
        assert!(matches!(
            rule.transform.apply("100"),
            Some(KfxValue::Symbol(KfxSymbol::Thin))
        ));
        assert!(matches!(
            rule.transform.apply("200"),
            Some(KfxValue::Symbol(KfxSymbol::UltraLight))
        ));
        assert!(matches!(
            rule.transform.apply("300"),
            Some(KfxValue::Symbol(KfxSymbol::Light))
        ));
        assert!(matches!(
            rule.transform.apply("400"),
            Some(KfxValue::Symbol(KfxSymbol::Normal))
        ));
        assert!(matches!(
            rule.transform.apply("500"),
            Some(KfxValue::Symbol(KfxSymbol::Medium))
        ));
        assert!(matches!(
            rule.transform.apply("600"),
            Some(KfxValue::Symbol(KfxSymbol::SemiBold))
        ));
        assert!(matches!(
            rule.transform.apply("700"),
            Some(KfxValue::Symbol(KfxSymbol::Bold))
        ));
        assert!(matches!(
            rule.transform.apply("800"),
            Some(KfxValue::Symbol(KfxSymbol::UltraBold))
        ));
        assert!(matches!(
            rule.transform.apply("900"),
            Some(KfxValue::Symbol(KfxSymbol::Heavy))
        ));
    }

    #[test]
    fn test_font_style_oblique_distinct() {
        let schema = StyleSchema::standard();
        let rule = schema.get("font-style").unwrap();

        // Oblique should map to Oblique, NOT Italic (per Amazon's ElementEnums.data)
        assert!(matches!(
            rule.transform.apply("oblique"),
            Some(KfxValue::Symbol(KfxSymbol::Oblique))
        ));
        assert!(matches!(
            rule.transform.apply("italic"),
            Some(KfxValue::Symbol(KfxSymbol::Italic))
        ));
    }

    #[test]
    fn test_text_alignment_start_end_distinct() {
        let schema = StyleSchema::standard();
        let rule = schema.get("text-align").unwrap();

        // Start/End should be distinct from Left/Right for RTL support
        assert!(matches!(
            rule.transform.apply("start"),
            Some(KfxValue::Symbol(KfxSymbol::Start))
        ));
        assert!(matches!(
            rule.transform.apply("end"),
            Some(KfxValue::Symbol(KfxSymbol::End))
        ));
        assert!(matches!(
            rule.transform.apply("left"),
            Some(KfxValue::Symbol(KfxSymbol::Left))
        ));
        assert!(matches!(
            rule.transform.apply("right"),
            Some(KfxValue::Symbol(KfxSymbol::Right))
        ));
    }

    #[test]
    fn test_color_packed_integer() {
        let schema = StyleSchema::standard();
        let rule = schema.get("color").unwrap();

        // Colors should output packed ARGB integers with 0xFF alpha
        // 0xFFFF0000 = 4294901760
        let result = rule.transform.apply("#ff0000");
        assert!(matches!(result, Some(KfxValue::Integer(4294901760))));

        // 0xFF0080FF = 4278223103
        let result = rule.transform.apply("rgb(0, 128, 255)");
        assert!(matches!(result, Some(KfxValue::Integer(4278223103))));
    }

    #[test]
    fn test_baseline_style_field() {
        let schema = StyleSchema::standard();
        let rule = schema.get("vertical-align").unwrap();

        // Should use BaselineStyle symbol, not TextBaseline
        assert_eq!(rule.kfx_symbol, KfxSymbol::BaselineStyle);

        assert!(matches!(
            rule.transform.apply("super"),
            Some(KfxValue::Symbol(KfxSymbol::Superscript))
        ));
        assert!(matches!(
            rule.transform.apply("sub"),
            Some(KfxValue::Symbol(KfxSymbol::Subscript))
        ));
    }

    // ========================================================================
    // Inverse Transform Tests (KFX → IR Import Direction)
    // ========================================================================

    #[test]
    fn test_inverse_font_weight_symbol_to_css() {
        let schema = StyleSchema::standard();
        let rule = schema.get("font-weight").unwrap();

        // Bold symbol → "bold" CSS string
        let kfx_value = IonValue::Symbol(KfxSymbol::Bold as u64);
        assert_eq!(rule.transform.inverse(&kfx_value), Some("bold".to_string()));

        // Normal symbol → "normal" CSS string
        let kfx_value = IonValue::Symbol(KfxSymbol::Normal as u64);
        assert_eq!(
            rule.transform.inverse(&kfx_value),
            Some("normal".to_string())
        );
    }

    #[test]
    fn test_inverse_dimensioned_value() {
        let schema = StyleSchema::standard();
        let rule = schema.get("margin-top").unwrap();

        // {value: 1.5, unit: em} → "1.5em" (Float)
        let kfx_value = IonValue::Struct(vec![
            (KfxSymbol::Value as u64, IonValue::Float(1.5)),
            (
                KfxSymbol::Unit as u64,
                IonValue::Symbol(KfxSymbol::Em as u64),
            ),
        ]);
        assert_eq!(
            rule.transform.inverse(&kfx_value),
            Some("1.5em".to_string())
        );

        // {value: 2, unit: em} → "2em" (Int - Amazon may store whole numbers as Int)
        let kfx_value = IonValue::Struct(vec![
            (KfxSymbol::Value as u64, IonValue::Int(2)),
            (
                KfxSymbol::Unit as u64,
                IonValue::Symbol(KfxSymbol::Em as u64),
            ),
        ]);
        assert_eq!(rule.transform.inverse(&kfx_value), Some("2em".to_string()));
    }

    #[test]
    fn test_inverse_color_packed_to_hex() {
        let schema = StyleSchema::standard();
        let rule = schema.get("color").unwrap();

        // 0xFF0000 → "#ff0000"
        let kfx_value = IonValue::Int(0xFF0000);
        assert_eq!(
            rule.transform.inverse(&kfx_value),
            Some("#ff0000".to_string())
        );

        // 0x00FF00 → "#00ff00"
        let kfx_value = IonValue::Int(0x00FF00);
        assert_eq!(
            rule.transform.inverse(&kfx_value),
            Some("#00ff00".to_string())
        );
    }

    #[test]
    fn test_import_kfx_style_full() {
        use crate::ir::{FontWeight, TextAlign};

        let schema = StyleSchema::standard();

        // Build KFX style properties
        let props = vec![
            (
                KfxSymbol::FontWeight as u64,
                IonValue::Symbol(KfxSymbol::Bold as u64),
            ),
            (
                KfxSymbol::TextAlignment as u64,
                IonValue::Symbol(KfxSymbol::Center as u64),
            ),
            (
                KfxSymbol::MarginTop as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::Value as u64, IonValue::Float(2.0)),
                    (
                        KfxSymbol::Unit as u64,
                        IonValue::Symbol(KfxSymbol::Em as u64),
                    ),
                ]),
            ),
        ];

        // Import using schema
        let ir_style = import_kfx_style(schema, &props);

        // Verify fields were set correctly
        assert_eq!(ir_style.font_weight, FontWeight::BOLD);
        assert_eq!(ir_style.text_align, TextAlign::Center);
        assert_eq!(ir_style.margin_top, crate::ir::Length::Em(2.0));
    }

    #[test]
    fn test_convert_to_dimensioned_transform() {
        // Test: CSS "32px" with 16px base → 2em
        let transform = ValueTransform::ConvertToDimensioned {
            base_pixels: 16.0,
            target_unit: KfxSymbol::Em,
        };

        let result = transform.apply("32px").unwrap();
        match result {
            KfxValue::Dimensioned { value, unit } => {
                assert!(
                    (value - 2.0).abs() < 0.001,
                    "32px / 16px should be 2em, got {}",
                    value
                );
                assert_eq!(unit, KfxSymbol::Em);
            }
            _ => panic!("Expected Dimensioned, got {:?}", result),
        }

        // Test: CSS "1.5em" → 1.5em (em to em is 1:1 when using base)
        let result = transform.apply("1.5em").unwrap();
        match result {
            KfxValue::Dimensioned { value, unit } => {
                // 1.5em * 16 = 24px, 24px / 16 = 1.5em
                assert!(
                    (value - 1.5).abs() < 0.001,
                    "1.5em should stay 1.5em, got {}",
                    value
                );
                assert_eq!(unit, KfxSymbol::Em);
            }
            _ => panic!("Expected Dimensioned, got {:?}", result),
        }

        // Test: CSS "50%" → preserved as { value: 50, unit: percent }
        let result = transform.apply("50%").unwrap();
        match result {
            KfxValue::Dimensioned { value, unit } => {
                assert!(
                    (value - 50.0).abs() < 0.001,
                    "50% should be preserved as 50, got {}",
                    value
                );
                assert_eq!(unit, KfxSymbol::Percent);
            }
            _ => panic!("Expected Dimensioned, got {:?}", result),
        }
    }

    #[test]
    fn test_convert_to_dimensioned_inverse() {
        // Test inverse: {value: 2.0, unit: em} → "2em"
        let transform = ValueTransform::ConvertToDimensioned {
            base_pixels: 16.0,
            target_unit: KfxSymbol::Em,
        };

        let kfx_value = IonValue::Struct(vec![
            (KfxSymbol::Value as u64, IonValue::Float(2.0)),
            (
                KfxSymbol::Unit as u64,
                IonValue::Symbol(KfxSymbol::Em as u64),
            ),
        ]);

        let css = transform.inverse(&kfx_value).unwrap();
        assert_eq!(css, "2em");

        // Test with Int value (Amazon sometimes stores whole numbers as Int)
        let kfx_value = IonValue::Struct(vec![
            (KfxSymbol::Value as u64, IonValue::Int(3)),
            (
                KfxSymbol::Unit as u64,
                IonValue::Symbol(KfxSymbol::Em as u64),
            ),
        ]);

        let css = transform.inverse(&kfx_value).unwrap();
        assert_eq!(css, "3em");
    }

    // ========================================================================
    // Phase 1-7: New Style Properties Tests
    // ========================================================================

    #[test]
    fn test_letter_spacing_transform() {
        let schema = StyleSchema::standard();
        let rule = schema.get("letter-spacing").unwrap();

        // 0.1em should convert to dimensioned value
        let result = rule.transform.apply("0.1em");
        assert!(matches!(result, Some(KfxValue::Dimensioned { .. })));
    }

    #[test]
    fn test_text_transform_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get("text-transform").unwrap();

        assert!(matches!(
            rule.transform.apply("uppercase"),
            Some(KfxValue::Symbol(KfxSymbol::Uppercase))
        ));
        assert!(matches!(
            rule.transform.apply("lowercase"),
            Some(KfxValue::Symbol(KfxSymbol::Lowercase))
        ));
        assert!(matches!(
            rule.transform.apply("capitalize"),
            Some(KfxValue::Symbol(KfxSymbol::Titlecase))
        ));
        assert!(matches!(
            rule.transform.apply("none"),
            Some(KfxValue::Symbol(KfxSymbol::None))
        ));
    }

    #[test]
    fn test_hyphens_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get("hyphens").unwrap();

        assert!(matches!(
            rule.transform.apply("auto"),
            Some(KfxValue::Symbol(KfxSymbol::Auto))
        ));
        assert!(matches!(
            rule.transform.apply("manual"),
            Some(KfxValue::Symbol(KfxSymbol::Manual))
        ));
        assert!(matches!(
            rule.transform.apply("none"),
            Some(KfxValue::Symbol(KfxSymbol::None))
        ));
    }

    #[test]
    fn test_white_space_nobreak() {
        let schema = StyleSchema::standard();
        let rule = schema.get("white-space").unwrap();

        assert!(matches!(
            rule.transform.apply("nowrap"),
            Some(KfxValue::Bool(true))
        ));
        assert!(matches!(
            rule.transform.apply("normal"),
            Some(KfxValue::Bool(false))
        ));
    }

    #[test]
    fn test_break_properties() {
        let schema = StyleSchema::standard();

        let rule = schema.get("break-before").unwrap();
        assert!(matches!(
            rule.transform.apply("always"),
            Some(KfxValue::Symbol(KfxSymbol::Always))
        ));
        assert!(matches!(
            rule.transform.apply("avoid"),
            Some(KfxValue::Symbol(KfxSymbol::Avoid))
        ));
        assert!(matches!(
            rule.transform.apply("auto"),
            Some(KfxValue::Symbol(KfxSymbol::Auto))
        ));

        let rule = schema.get("break-inside").unwrap();
        assert!(matches!(
            rule.transform.apply("avoid"),
            Some(KfxValue::Symbol(KfxSymbol::Avoid))
        ));
    }

    #[test]
    fn test_float_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get("float").unwrap();

        assert!(matches!(
            rule.transform.apply("left"),
            Some(KfxValue::Symbol(KfxSymbol::Left))
        ));
        assert!(matches!(
            rule.transform.apply("right"),
            Some(KfxValue::Symbol(KfxSymbol::Right))
        ));
        assert!(matches!(
            rule.transform.apply("none"),
            Some(KfxValue::Symbol(KfxSymbol::None))
        ));
    }

    #[test]
    fn test_border_style_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get("border-top-style").unwrap();

        assert!(matches!(
            rule.transform.apply("solid"),
            Some(KfxValue::Symbol(KfxSymbol::Solid))
        ));
        assert!(matches!(
            rule.transform.apply("dashed"),
            Some(KfxValue::Symbol(KfxSymbol::Dashed))
        ));
        assert!(matches!(
            rule.transform.apply("dotted"),
            Some(KfxValue::Symbol(KfxSymbol::Dotted))
        ));
        assert!(matches!(
            rule.transform.apply("double"),
            Some(KfxValue::Symbol(KfxSymbol::Double))
        ));
        assert!(matches!(
            rule.transform.apply("groove"),
            Some(KfxValue::Symbol(KfxSymbol::Groove))
        ));
        assert!(matches!(
            rule.transform.apply("none"),
            Some(KfxValue::Symbol(KfxSymbol::None))
        ));
    }

    #[test]
    fn test_list_style_position() {
        let schema = StyleSchema::standard();
        let rule = schema.get("list-style-position").unwrap();

        assert!(matches!(
            rule.transform.apply("inside"),
            Some(KfxValue::Symbol(KfxSymbol::Inside))
        ));
        assert!(matches!(
            rule.transform.apply("outside"),
            Some(KfxValue::Symbol(KfxSymbol::Outside))
        ));
    }

    #[test]
    fn test_visibility_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get("visibility").unwrap();

        assert!(matches!(
            rule.transform.apply("visible"),
            Some(KfxValue::Symbol(KfxSymbol::Show))
        ));
        assert!(matches!(
            rule.transform.apply("hidden"),
            Some(KfxValue::Symbol(KfxSymbol::Hide))
        ));
    }

    #[test]
    fn test_extract_ir_field_letter_spacing() {
        use crate::ir::{ComputedStyle, Length};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::LetterSpacing), None);

        let mut styled = ComputedStyle::default();
        styled.letter_spacing = Length::Em(0.1);
        assert_eq!(
            extract_ir_field(&styled, IrField::LetterSpacing),
            Some("0.1em".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_text_transform() {
        use crate::ir::{ComputedStyle, TextTransform};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::TextTransform), None);

        let mut styled = ComputedStyle::default();
        styled.text_transform = TextTransform::Uppercase;
        assert_eq!(
            extract_ir_field(&styled, IrField::TextTransform),
            Some("uppercase".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_break_before() {
        use crate::ir::{BreakValue, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::BreakBefore), None);

        let mut styled = ComputedStyle::default();
        styled.break_before = BreakValue::Always;
        assert_eq!(
            extract_ir_field(&styled, IrField::BreakBefore),
            Some("always".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_border_style() {
        use crate::ir::{BorderStyle, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(extract_ir_field(&default, IrField::BorderStyleTop), None);

        let mut styled = ComputedStyle::default();
        styled.border_style_top = BorderStyle::Solid;
        assert_eq!(
            extract_ir_field(&styled, IrField::BorderStyleTop),
            Some("solid".to_string())
        );
    }

    #[test]
    fn test_apply_ir_field_text_transform() {
        use crate::ir::{ComputedStyle, TextTransform};

        let mut style = ComputedStyle::default();
        apply_ir_field(&mut style, IrField::TextTransform, "uppercase");
        assert_eq!(style.text_transform, TextTransform::Uppercase);

        apply_ir_field(&mut style, IrField::TextTransform, "lowercase");
        assert_eq!(style.text_transform, TextTransform::Lowercase);

        apply_ir_field(&mut style, IrField::TextTransform, "capitalize");
        assert_eq!(style.text_transform, TextTransform::Capitalize);
    }

    #[test]
    fn test_apply_ir_field_border_style() {
        use crate::ir::{BorderStyle, ComputedStyle};

        let mut style = ComputedStyle::default();
        apply_ir_field(&mut style, IrField::BorderStyleTop, "solid");
        assert_eq!(style.border_style_top, BorderStyle::Solid);

        apply_ir_field(&mut style, IrField::BorderStyleTop, "dashed");
        assert_eq!(style.border_style_top, BorderStyle::Dashed);

        apply_ir_field(&mut style, IrField::BorderStyleTop, "groove");
        assert_eq!(style.border_style_top, BorderStyle::Groove);
    }

    #[test]
    fn test_negative_letter_spacing() {
        use crate::ir::{ComputedStyle, Length};

        // Negative letter-spacing is valid CSS
        let mut style = ComputedStyle::default();
        apply_ir_field(&mut style, IrField::LetterSpacing, "-0.05em");
        assert_eq!(style.letter_spacing, Length::Em(-0.05));
    }

    #[test]
    fn test_hyphens_default_is_manual() {
        use crate::ir::{ComputedStyle, Hyphens};

        // Default is Manual so explicit hyphens: auto is emitted in KFX output
        let default = ComputedStyle::default();
        assert_eq!(default.hyphens, Hyphens::Manual);
    }

    #[test]
    fn test_ir_mapped_rules_count() {
        let schema = StyleSchema::standard();

        // Count rules with IR field mappings (should be ~50+ now with all phases)
        let mapped_count = schema.ir_mapped_rules().count();
        assert!(
            mapped_count >= 40,
            "Expected >=40 IR-mapped rules, got {}",
            mapped_count
        );
    }

    #[test]
    fn test_sizing_bounds_auto_emit_with_width() {
        use crate::ir::{ComputedStyle, Length};

        // When width is set, sizing_bounds should emit content-box (the CSS default)
        // This matches Amazon's converter behavior
        let mut style = ComputedStyle::default();
        style.width = Length::Percent(75.0);

        let result = extract_ir_field(&style, IrField::SizingBounds);
        assert_eq!(result, Some("content-box".to_string()));
    }

    #[test]
    fn test_sizing_bounds_border_box() {
        use crate::ir::{BoxSizing, ComputedStyle, Length};

        // Explicit border-box should emit border-box
        let mut style = ComputedStyle::default();
        style.box_sizing = BoxSizing::BorderBox;
        style.width = Length::Percent(100.0);

        let result = extract_ir_field(&style, IrField::SizingBounds);
        assert_eq!(result, Some("border-box".to_string()));
    }

    #[test]
    fn test_sizing_bounds_not_emitted_without_dimensions() {
        use crate::ir::ComputedStyle;

        // No width/height = no sizing_bounds
        let style = ComputedStyle::default();

        let result = extract_ir_field(&style, IrField::SizingBounds);
        assert_eq!(result, None);
    }

    #[test]
    fn test_box_align_from_margin_auto() {
        use crate::ir::{ComputedStyle, Length};

        // margin-left: auto + margin-right: auto → box_align: center
        let mut style = ComputedStyle::default();
        style.margin_left = Length::Auto;
        style.margin_right = Length::Auto;

        let result = extract_ir_field(&style, IrField::BoxAlign);
        assert_eq!(result, Some("center".to_string()));
    }

    #[test]
    fn test_box_align_not_emitted_without_both_auto() {
        use crate::ir::{ComputedStyle, Length};

        // Only margin-left: auto is not enough
        let mut style = ComputedStyle::default();
        style.margin_left = Length::Auto;
        style.margin_right = Length::Px(0.0);

        let result = extract_ir_field(&style, IrField::BoxAlign);
        assert_eq!(result, None);
    }
}
