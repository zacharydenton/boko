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

use std::collections::HashMap;

use crate::kfx::ion::IonValue;
use crate::kfx::symbols::KfxSymbol;
use crate::style as ir_style;

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

    /// Color parsing: CSS color -> packed KFX ARGB integer.
    ParseColor,

    /// Dimensioned value: wraps a number with a unit symbol.
    /// Example: 1.2 with unit=em -> { value: 1.2, unit: em }
    /// NOTE: This does NOT convert units.
    Dimensioned { unit: KfxSymbol },

    /// Wrap integer in a struct with a single field.
    /// Used for orphans/widows: `3` -> `{ first: 3 }` or `{ last: 3 }`
    WrapInStruct {
        /// Field name symbol (e.g., First or Last)
        field: KfxSymbol,
        /// Minimum value (KFX enforces min of 1 for orphans/widows)
        min_value: Option<i64>,
    },

    /// Preserve the original CSS unit as a KFX dimensioned value.
    /// Example: "10px" → { value: 10, unit: px }, "1.5em" → { value: 1.5, unit: em }
    PreserveUnit,

    /// A plain integer value (e.g. dropcap line/char counts).
    WrapInt,

    /// Hairline dimensions (border widths/radii, letter/word spacing):
    /// absolute px folds to pt at the CSS ratio (0.75pt/px); relative units
    /// pass through. Reference KFX emits pt for these.
    AbsolutePt,
}

/// The base KFX line box: 1lh = 1.2em, the reference line-height.
const LH_PER_EM: f64 = 1.0 / 1.2;
/// Kindle Previewer's layout viewport width, used to resolve percentage
/// spacing to absolute units (5% → 25.6px at 512px).
const KP_LAYOUT_VIEWPORT_PX: f64 = 512.0;
/// CSS reference pixels per em/rem.
const PX_PER_EM: f64 = 16.0;

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
    /// Multi-field struct accumulated from several StructField writes to the
    /// same KFX symbol (e.g. orphans + widows merging into
    /// `keep_lines_together: { first: N, last: M }`).
    StructFields(Vec<(KfxSymbol, i64)>),
    /// A list of symbols (e.g. `layout_hints: [treat_as_title]`).
    SymbolList(Vec<u64>),
}

impl KfxValue {
    /// Convert to IonValue for serialization.
    pub fn to_ion(&self) -> IonValue {
        match self {
            KfxValue::Symbol(sym) => IonValue::Symbol(*sym as u64),
            KfxValue::SymbolList(syms) => {
                IonValue::List(syms.iter().map(|&s| IonValue::Symbol(s)).collect())
            }
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
            KfxValue::StructFields(fields) => IonValue::Struct(
                fields
                    .iter()
                    .map(|(field, value)| (*field as u64, IonValue::Int(*value)))
                    .collect(),
            ),
        }
    }

    /// Merge two struct-shaped values for the same KFX symbol into one
    /// multi-field struct (later writes to the same field win). Returns
    /// `None` when either side is not struct-shaped; callers should then
    /// replace the old value.
    pub(crate) fn merge_struct_fields(&self, other: &KfxValue) -> Option<KfxValue> {
        let mut fields: Vec<(KfxSymbol, i64)> = match self {
            KfxValue::StructField { field, value } => vec![(*field, *value)],
            KfxValue::StructFields(fields) => fields.clone(),
            _ => return None,
        };
        let incoming: Vec<(KfxSymbol, i64)> = match other {
            KfxValue::StructField { field, value } => vec![(*field, *value)],
            KfxValue::StructFields(fields) => fields.clone(),
            _ => return None,
        };
        for (field, value) in incoming {
            if let Some(existing) = fields.iter_mut().find(|(f, _)| *f == field) {
                existing.1 = value;
            } else {
                fields.push((field, value));
            }
        }
        Some(KfxValue::StructFields(fields))
    }
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
    WhiteSpace,
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
    // Phase 12: Table properties
    BorderCollapse,
    BorderSpacing,
    DropcapLines,
    DropcapChars,
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
}

// ============================================================================
// Style Schema
// ============================================================================

/// The master schema for style property mappings.
///
/// Rules are stored in registration order and every iteration API walks them
/// in that order. This is load-bearing: property order in emitted KFX style
/// structs (and the winner when two rules share a KFX symbol) follows rule
/// order, so it must be deterministic for output to be byte-reproducible.
pub struct StyleSchema {
    /// All rules, in registration order.
    rules: Vec<StylePropertyRule>,
    /// Fast lookup from IR key -> indexes into `rules`.
    by_key: HashMap<&'static str, Vec<usize>>,
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
            rules: Vec::new(),
            by_key: HashMap::new(),
        }
    }

    /// Register a property rule.
    pub fn register(&mut self, rule: StylePropertyRule) {
        self.by_key
            .entry(rule.ir_key)
            .or_default()
            .push(self.rules.len());
        self.rules.push(rule);
    }

    /// Look up rules by IR key, in registration order.
    pub fn get<'a>(&'a self, ir_key: &str) -> impl Iterator<Item = &'a StylePropertyRule> + 'a {
        self.by_key
            .get(ir_key)
            .into_iter()
            .flatten()
            .map(|&i| &self.rules[i])
    }

    /// Look up the first rule by IR key (convenience for single-rule properties).
    #[cfg(test)]
    pub fn get_first(&self, ir_key: &str) -> Option<&StylePropertyRule> {
        self.get(ir_key).next()
    }

    /// Get all rules, in registration order.
    pub fn rules(&self) -> impl Iterator<Item = &StylePropertyRule> {
        self.rules.iter()
    }

    /// Get rules that have IR field mappings (for schema-driven IR extraction),
    /// in registration order.
    pub fn ir_mapped_rules(&self) -> impl Iterator<Item = &StylePropertyRule> {
        self.rules.iter().filter(|r| r.ir_field.is_some())
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
                // Reference output maps 300 to ultra_light (gold-master
                // verified); Light is reserved for the `lighter` keyword.
                ("300".into(), KfxValue::Symbol(KfxSymbol::UltraLight)),
                ("400".into(), KfxValue::Symbol(KfxSymbol::Normal)),
                ("500".into(), KfxValue::Symbol(KfxSymbol::Medium)),
                ("600".into(), KfxValue::Symbol(KfxSymbol::SemiBold)),
                ("700".into(), KfxValue::Symbol(KfxSymbol::Bold)),
                ("800".into(), KfxValue::Symbol(KfxSymbol::UltraBold)),
                ("900".into(), KfxValue::Symbol(KfxSymbol::Heavy)),
            ]),
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
        });

        schema.register(StylePropertyRule {
            ir_key: "font-size",
            ir_field: Some(IrField::FontSize),
            kfx_symbol: KfxSymbol::FontSize,
            transform: ValueTransform::PreserveUnit,
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
                // Logical start/end fold to the physical sides (LTR):
                // reference output never emits the Start/End symbols and
                // KFX consumers reject them in styles.
                ("start".into(), KfxValue::Symbol(KfxSymbol::Left)),
                ("end".into(), KfxValue::Symbol(KfxSymbol::Right)),
            ]),
        });

        schema.register(StylePropertyRule {
            ir_key: "text-indent",
            ir_field: Some(IrField::TextIndent),
            kfx_symbol: KfxSymbol::TextIndent,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "line-height",
            ir_field: Some(IrField::LineHeight),
            kfx_symbol: KfxSymbol::LineHeight,
            transform: ValueTransform::PreserveUnit,
        });

        // text-decoration: underline -> underline: solid (symbol, not bool)
        //
        // The style names are included so that import can round-trip
        // `underline: dotted` etc. back to the underline flag ("underline"
        // must stay first so Solid inverts to it). The specific style comes
        // from the `text-decoration-style` rule below, which shares this KFX
        // symbol and is registered later, so it deterministically overrides
        // the plain Solid emitted here whenever both are present.
        schema.register(StylePropertyRule {
            ir_key: "text-decoration",
            ir_field: Some(IrField::TextDecorationUnderline),
            kfx_symbol: KfxSymbol::Underline,
            transform: ValueTransform::Map(vec![
                ("underline".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("true".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("solid".into(), KfxValue::Symbol(KfxSymbol::Solid)),
                ("dotted".into(), KfxValue::Symbol(KfxSymbol::Dotted)),
                ("dashed".into(), KfxValue::Symbol(KfxSymbol::Dashed)),
                ("double".into(), KfxValue::Symbol(KfxSymbol::Double)),
                ("false".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ]),
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
        });

        // ====================================================================
        // Spacing Properties (Margins)
        // ====================================================================
        //
        // Margins use PreserveUnit to keep the original CSS units (px, em, %).
        // This matches the source CSS more closely.

        schema.register(StylePropertyRule {
            ir_key: "margin-top",
            ir_field: Some(IrField::MarginTop),
            kfx_symbol: KfxSymbol::MarginTop,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-bottom",
            ir_field: Some(IrField::MarginBottom),
            kfx_symbol: KfxSymbol::MarginBottom,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-left",
            ir_field: Some(IrField::MarginLeft),
            kfx_symbol: KfxSymbol::MarginLeft,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "margin-right",
            ir_field: Some(IrField::MarginRight),
            kfx_symbol: KfxSymbol::MarginRight,
            transform: ValueTransform::PreserveUnit,
        });

        // ====================================================================
        // Spacing Properties (Padding)
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "padding-top",
            ir_field: Some(IrField::PaddingTop),
            kfx_symbol: KfxSymbol::PaddingTop,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "padding-bottom",
            ir_field: Some(IrField::PaddingBottom),
            kfx_symbol: KfxSymbol::PaddingBottom,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "padding-left",
            ir_field: Some(IrField::PaddingLeft),
            kfx_symbol: KfxSymbol::PaddingLeft,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "padding-right",
            ir_field: Some(IrField::PaddingRight),
            kfx_symbol: KfxSymbol::PaddingRight,
            transform: ValueTransform::PreserveUnit,
        });

        // ====================================================================
        // Color Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "color",
            ir_field: Some(IrField::Color),
            kfx_symbol: KfxSymbol::TextColor,
            transform: ValueTransform::ParseColor,
        });

        // text_background_color: for inline text spans
        schema.register(StylePropertyRule {
            ir_key: "background-color",
            ir_field: Some(IrField::BackgroundColor),
            kfx_symbol: KfxSymbol::TextBackgroundColor,
            transform: ValueTransform::ParseColor,
        });

        // fill_color: for block container backgrounds
        schema.register(StylePropertyRule {
            ir_key: "background-color",
            ir_field: None, // Don't extract twice from IR
            kfx_symbol: KfxSymbol::FillColor,
            transform: ValueTransform::ParseColor,
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
        });

        // vertical-align → yj.vertical_align (for table cell alignment)
        // Multiple rules per key are now supported.
        schema.register(StylePropertyRule {
            ir_key: "vertical-align",
            ir_field: None, // Don't extract twice from IR
            kfx_symbol: KfxSymbol::YjVerticalAlign,
            transform: ValueTransform::Map(vec![
                ("top".into(), KfxValue::Symbol(KfxSymbol::Top)),
                ("middle".into(), KfxValue::Symbol(KfxSymbol::Center)),
                ("bottom".into(), KfxValue::Symbol(KfxSymbol::Bottom)),
            ]),
        });

        // ====================================================================
        // Phase 1: High-Priority Text Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "letter-spacing",
            ir_field: Some(IrField::LetterSpacing),
            kfx_symbol: KfxSymbol::Letterspacing,
            transform: ValueTransform::AbsolutePt,
        });

        schema.register(StylePropertyRule {
            ir_key: "word-spacing",
            ir_field: Some(IrField::WordSpacing),
            kfx_symbol: KfxSymbol::Wordspacing,
            transform: ValueTransform::AbsolutePt,
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
        });

        schema.register(StylePropertyRule {
            ir_key: "hyphens",
            ir_field: Some(IrField::Hyphens),
            kfx_symbol: KfxSymbol::Hyphens,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                // Reference output folds `manual` (break only at &shy;) to
                // none — KFX has no manual mode.
                ("manual".into(), KfxValue::Symbol(KfxSymbol::None)),
                ("none".into(), KfxValue::Symbol(KfxSymbol::None)),
            ]),
        });

        schema.register(StylePropertyRule {
            ir_key: "white-space",
            ir_field: Some(IrField::WhiteSpace),
            kfx_symbol: KfxSymbol::Nobreak,
            transform: ValueTransform::Map(vec![
                ("nowrap".into(), KfxValue::Bool(true)),
                ("normal".into(), KfxValue::Bool(false)),
            ]),
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
        });

        schema.register(StylePropertyRule {
            ir_key: "text-decoration-color",
            ir_field: Some(IrField::UnderlineColor),
            kfx_symbol: KfxSymbol::UnderlineColor,
            transform: ValueTransform::ParseColor,
        });

        // ====================================================================
        // Phase 3: Layout Properties
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "width",
            ir_field: Some(IrField::Width),
            kfx_symbol: KfxSymbol::Width,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "height",
            ir_field: Some(IrField::Height),
            kfx_symbol: KfxSymbol::Height,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "max-width",
            ir_field: Some(IrField::MaxWidth),
            kfx_symbol: KfxSymbol::MaxWidth,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "min-height",
            ir_field: Some(IrField::MinHeight),
            kfx_symbol: KfxSymbol::MinHeight,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "min-width",
            ir_field: Some(IrField::MinWidth),
            kfx_symbol: KfxSymbol::MinWidth,
            transform: ValueTransform::PreserveUnit,
        });

        schema.register(StylePropertyRule {
            ir_key: "max-height",
            ir_field: Some(IrField::MaxHeight),
            kfx_symbol: KfxSymbol::MaxHeight,
            transform: ValueTransform::PreserveUnit,
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
        });

        schema.register(StylePropertyRule {
            ir_key: "break-inside",
            ir_field: Some(IrField::BreakInside),
            kfx_symbol: KfxSymbol::BreakInside,
            transform: ValueTransform::Map(vec![
                ("auto".into(), KfxValue::Symbol(KfxSymbol::Auto)),
                ("avoid".into(), KfxValue::Symbol(KfxSymbol::Avoid)),
            ]),
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
        });

        schema.register(StylePropertyRule {
            ir_key: "border-right-style",
            ir_field: Some(IrField::BorderStyleRight),
            kfx_symbol: KfxSymbol::BorderStyleRight,
            transform: border_style_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-style",
            ir_field: Some(IrField::BorderStyleBottom),
            kfx_symbol: KfxSymbol::BorderStyleBottom,
            transform: border_style_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-left-style",
            ir_field: Some(IrField::BorderStyleLeft),
            kfx_symbol: KfxSymbol::BorderStyleLeft,
            transform: border_style_transform,
        });

        // Border widths: px folds to pt (reference KFX emits pt hairlines).
        let border_width_transform = ValueTransform::AbsolutePt;

        schema.register(StylePropertyRule {
            ir_key: "border-top-width",
            ir_field: Some(IrField::BorderWidthTop),
            kfx_symbol: KfxSymbol::BorderWeightTop,
            transform: border_width_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-right-width",
            ir_field: Some(IrField::BorderWidthRight),
            kfx_symbol: KfxSymbol::BorderWeightRight,
            transform: border_width_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-width",
            ir_field: Some(IrField::BorderWidthBottom),
            kfx_symbol: KfxSymbol::BorderWeightBottom,
            transform: border_width_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-left-width",
            ir_field: Some(IrField::BorderWidthLeft),
            kfx_symbol: KfxSymbol::BorderWeightLeft,
            transform: border_width_transform,
        });

        // Border colors
        let border_color_transform = ValueTransform::ParseColor;

        schema.register(StylePropertyRule {
            ir_key: "border-top-color",
            ir_field: Some(IrField::BorderColorTop),
            kfx_symbol: KfxSymbol::BorderColorTop,
            transform: border_color_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-right-color",
            ir_field: Some(IrField::BorderColorRight),
            kfx_symbol: KfxSymbol::BorderColorRight,
            transform: border_color_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-color",
            ir_field: Some(IrField::BorderColorBottom),
            kfx_symbol: KfxSymbol::BorderColorBottom,
            transform: border_color_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-left-color",
            ir_field: Some(IrField::BorderColorLeft),
            kfx_symbol: KfxSymbol::BorderColorLeft,
            transform: border_color_transform,
        });

        // Border radius - preserve original units
        let border_radius_transform = ValueTransform::AbsolutePt;

        schema.register(StylePropertyRule {
            ir_key: "border-top-left-radius",
            ir_field: Some(IrField::BorderRadiusTopLeft),
            kfx_symbol: KfxSymbol::BorderRadiusTopLeft,
            transform: border_radius_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-top-right-radius",
            ir_field: Some(IrField::BorderRadiusTopRight),
            kfx_symbol: KfxSymbol::BorderRadiusTopRight,
            transform: border_radius_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-left-radius",
            ir_field: Some(IrField::BorderRadiusBottomLeft),
            kfx_symbol: KfxSymbol::BorderRadiusBottomLeft,
            transform: border_radius_transform.clone(),
        });

        schema.register(StylePropertyRule {
            ir_key: "border-bottom-right-radius",
            ir_field: Some(IrField::BorderRadiusBottomRight),
            kfx_symbol: KfxSymbol::BorderRadiusBottomRight,
            transform: border_radius_transform,
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
        });

        // ====================================================================
        // Phase 7: Font Family (string value, not symbol)
        // ====================================================================

        schema.register(StylePropertyRule {
            ir_key: "font-family",
            ir_field: Some(IrField::FontFamily),
            kfx_symbol: KfxSymbol::FontFamily,
            transform: ValueTransform::Identity, // String passthrough
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
        });

        // Visibility. Reference KFX encodes this as an Ion boolean
        // (true = visible, false = hidden), not a symbol — readers flag
        // symbol values as unexpected style data.
        schema.register(StylePropertyRule {
            ir_key: "visibility",
            ir_field: Some(IrField::Visibility),
            kfx_symbol: KfxSymbol::Visibility,
            transform: ValueTransform::Map(vec![
                ("visible".into(), KfxValue::Bool(true)),
                ("hidden".into(), KfxValue::Bool(false)),
                ("collapse".into(), KfxValue::Bool(false)),
            ]),
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
        });

        // ====================================================================
        // Phase 12: Table Properties
        // ====================================================================

        // border-collapse → table_border_collapse (boolean: true=collapse, false=separate)
        schema.register(StylePropertyRule {
            ir_key: "border-collapse",
            ir_field: None, // import-only
            kfx_symbol: KfxSymbol::TableBorderCollapse,
            transform: ValueTransform::Map(vec![
                ("separate".into(), KfxValue::Bool(false)),
                ("collapse".into(), KfxValue::Bool(true)),
            ]),
        });

        // border-spacing → border_spacing_vertical
        schema.register(StylePropertyRule {
            ir_key: "border-spacing",
            ir_field: None, // import-only: reference KFX never emits border-spacing
            kfx_symbol: KfxSymbol::BorderSpacingVertical,
            transform: ValueTransform::AbsolutePt,
        });

        // border-spacing → border_spacing_horizontal (same value to both)
        schema.register(StylePropertyRule {
            ir_key: "border-spacing",
            ir_field: None, // Don't extract twice from IR
            kfx_symbol: KfxSymbol::BorderSpacingHorizontal,
            transform: ValueTransform::AbsolutePt,
        });

        // Dropcap: the leading letters of a paragraph rendered large,
        // spanning N text lines (detected from a floated large-font span at
        // paragraph start; see the dropcap-detection pass).
        schema.register(StylePropertyRule {
            ir_key: "dropcap-lines",
            ir_field: Some(IrField::DropcapLines),
            kfx_symbol: KfxSymbol::DropcapLines,
            transform: ValueTransform::WrapInt,
        });
        schema.register(StylePropertyRule {
            ir_key: "dropcap-chars",
            ir_field: Some(IrField::DropcapChars),
            kfx_symbol: KfxSymbol::DropcapChars,
            transform: ValueTransform::WrapInt,
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

            ValueTransform::ParseColor => {
                // Fully transparent colors (`transparent`, zero-alpha rgba)
                // parse to None and the property is omitted entirely —
                // coercing them to an opaque value would paint black boxes.
                let (r, g, b, a) = parse_css_color(raw)?;
                // KFX uses ARGB packing; alpha is preserved.
                let packed =
                    ((a as i64) << 24) | ((r as i64) << 16) | ((g as i64) << 8) | (b as i64);
                Some(KfxValue::Integer(packed))
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

            ValueTransform::PreserveUnit => {
                let (num, css_unit) = parse_css_length(raw)?;
                let kfx_unit = match css_unit.as_str() {
                    "px" => KfxSymbol::Px,
                    "em" => KfxSymbol::Em,
                    "rem" => KfxSymbol::Rem,
                    "%" => KfxSymbol::Percent,
                    "pt" => KfxSymbol::Pt,
                    // Device-unit strings pre-converted by extract_ir_field.
                    "lh" => KfxSymbol::Lh,
                    _ => KfxSymbol::Px, // Default fallback
                };
                Some(KfxValue::Dimensioned {
                    value: num,
                    unit: kfx_unit,
                })
            }

            ValueTransform::WrapInt => raw.trim().parse::<i64>().ok().map(KfxValue::Integer),

            ValueTransform::AbsolutePt => {
                let (num, css_unit) = parse_css_length(raw)?;
                let (value, unit) = match css_unit.as_str() {
                    "%" => (num, KfxSymbol::Percent),
                    "em" => (num, KfxSymbol::Em),
                    "rem" => (num, KfxSymbol::Rem),
                    "pt" => (num, KfxSymbol::Pt),
                    // px and bare numbers: CSS 1px = 0.75pt.
                    _ => (num * 0.75, KfxSymbol::Pt),
                };
                Some(KfxValue::Dimensioned { value, unit })
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

    // Find where the unit starts. Must be a byte offset (char_indices), not a
    // char count: a multi-byte character before the unit would otherwise make
    // the slices below panic on a non-char boundary.
    let unit_start = s
        .char_indices()
        .find(|(_, c)| c.is_alphabetic() || *c == '%')
        .map(|(i, _)| i)
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

/// Parse a CSS color into (r, g, b, a).
///
/// Returns `None` for unparseable colors and for **fully transparent** ones
/// (`transparent`, zero-alpha rgba/hex): an invisible color must be omitted,
/// not coerced to an opaque value.
fn parse_css_color(s: &str) -> Option<(u8, u8, u8, u8)> {
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
        "transparent" => return None,
        _ => None,
    };

    if let Some((r, g, b)) = named {
        return Some((r, g, b, 0xFF));
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

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8, u8)> {
    // All slicing below is by byte range; reject non-ASCII up front so a
    // multi-byte character can't land on a slice boundary and panic.
    if !hex.is_ascii() {
        return None;
    }
    match hex.len() {
        3 => {
            // #RGB -> #RRGGBB
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b, 0xFF))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b, 0xFF))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            if a == 0 {
                return None; // Fully transparent: omit the property
            }
            Some((r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgb_function(s: &str) -> Option<(u8, u8, u8, u8)> {
    // Extract content between parentheses
    let start = s.find('(')?;
    let end = s.find(')')?;
    // A malformed value like "rgb)x(" would put `end` before `start`.
    if end <= start {
        return None;
    }
    let content = &s[start + 1..end];

    // Split by comma, slash (rgb(r g b / a) syntax), or space
    let parts: Vec<&str> = content
        .split([',', ' ', '/'])
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() < 3 {
        return None;
    }

    let r = parse_color_component(parts[0])?;
    let g = parse_color_component(parts[1])?;
    let b = parse_color_component(parts[2])?;

    // Optional alpha: 0..1 float or percentage.
    let a = match parts.get(3) {
        Some(alpha) => {
            let alpha = alpha.trim();
            let value = if let Some(pct) = alpha.strip_suffix('%') {
                pct.parse::<f64>().ok()? / 100.0
            } else {
                alpha.parse::<f64>().ok()?
            };
            if value.is_nan() {
                return None;
            }
            (value.clamp(0.0, 1.0) * 255.0).round() as u8
        }
        None => 0xFF,
    };

    if a == 0 {
        return None; // Fully transparent: omit the property
    }

    Some((r, g, b, a))
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

// ============================================================================
// IR Field Extraction (Bidirectional Schema Bridge)
// ============================================================================

/// `Some(())` when the color is present and not pure black (default ink).
fn non_black(c: Option<crate::style::Color>) -> Option<()> {
    c.and_then(|c| ((c.r, c.g, c.b) != (0, 0, 0)).then_some(()))
}

/// Format a dimension with unit, rounding away float noise.
fn fmt_dim(v: f64, unit: &str) -> String {
    let rounded = (v * 1e6).round() / 1e6;
    format!("{}{}", rounded, unit)
}

/// Convert an absolute vertical margin (root-em) to the lh value the given
/// element's style emits — used by the margin-collapse pass to override
/// margins with position-dependent collapsed values.
pub(crate) fn margin_abs_em_to_lh(s: &ir_style::ComputedStyle, abs_em: f64) -> f64 {
    let em = abs_em / s.font_size_abs.0 as f64;
    em / (emitted_line_height_lh(s) * 1.2)
}

/// The element's line-height as emitted (in lh units): `normal` is 1lh;
/// authored leading converts at the 1.2em base line box and maps to 0.99lh
/// when at or below the base, exactly like reference output. The floor
/// applies to the *authored* value; the book's leading normalization
/// (`line_scale`) then scales the result, matching reference output which
/// lets normalized leading drop below the floor.
fn emitted_line_height_lh(s: &ir_style::ComputedStyle) -> f64 {
    match raw_line_height_em(s) {
        None => 1.0,
        Some(em) => {
            let lh = em / 1.2;
            let lh = if lh <= 1.0 { 0.99 } else { lh };
            lh * s.line_scale.0 as f64
        }
    }
}

/// The element's authored line-height resolved to em of its own font;
/// `None` means `normal`.
fn raw_line_height_em(s: &ir_style::ComputedStyle) -> Option<f64> {
    let abs = s.font_size_abs.0 as f64;
    match s.line_height {
        ir_style::Length::Auto => None,
        ir_style::Length::Em(x) => Some(x as f64),
        ir_style::Length::Percent(p) => Some(p as f64 / 100.0),
        ir_style::Length::Px(x) => Some(x as f64 / PX_PER_EM / abs),
        ir_style::Length::Rem(x) => Some(x as f64 / abs),
    }
}

/// Vertical spacing (margin/padding top+bottom) in lh units of the
/// element's own line box, so block spacing scales with both the font and
/// the device line-spacing setting — reference output divides by the
/// element's *actual* line-height, not the 1.2em base. Percent resolves
/// against Kindle Previewer's 512px layout viewport.
fn vertical_spacing_lh(len: ir_style::Length, s: &ir_style::ComputedStyle) -> Option<String> {
    let abs = s.font_size_abs.0 as f64;
    let em = match len {
        ir_style::Length::Auto => return None,
        ir_style::Length::Em(x) => x as f64,
        ir_style::Length::Rem(x) => x as f64 / abs,
        ir_style::Length::Px(x) => x as f64 / PX_PER_EM / abs,
        ir_style::Length::Percent(p) => p as f64 / 100.0 * KP_LAYOUT_VIEWPORT_PX / PX_PER_EM / abs,
    };
    if em == 0.0 {
        return None;
    }
    let line_em = emitted_line_height_lh(s) * 1.2;
    Some(fmt_dim(em / line_em, "lh"))
}

/// Horizontal spacing keeps the author's relative units (em/%); absolute
/// px/pt fold to em of the element's own font so side spacing scales with
/// the font size instead of freezing at a device-pixel size.
fn horizontal_spacing(len: ir_style::Length, s: &ir_style::ComputedStyle) -> Option<String> {
    let abs = s.font_size_abs.0 as f64;
    match len {
        ir_style::Length::Auto => None,
        ir_style::Length::Percent(p) if p != 0.0 => Some(fmt_dim(p as f64, "%")),
        ir_style::Length::Em(x) if x != 0.0 => Some(fmt_dim(x as f64, "em")),
        ir_style::Length::Rem(x) if x != 0.0 => Some(fmt_dim(x as f64 / abs, "em")),
        ir_style::Length::Px(x) if x != 0.0 => Some(fmt_dim(x as f64 / PX_PER_EM / abs, "em")),
        _ => None,
    }
}

/// Inherited spacing property with an explicit-reset path: `normal`
/// (`Length::Auto`) under a spaced ancestor emits `0em`; otherwise defer to
/// the parent-baseline table lookup.
fn spacing_reset(
    value: ir_style::Length,
    parent_value: ir_style::Length,
    fallback: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if value == parent_value {
        None
    } else if value == ir_style::Length::Auto {
        Some("0em".to_string())
    } else {
        fallback()
    }
}

/// Extract a CSS string from an IR ComputedStyle field.
///
/// This is the centralized extraction logic for the bidirectional schema.
/// The schema declares WHICH fields to extract (via `IrField` enum), and
/// this function provides the HOW. Most fields delegate to the canonical
/// property table in `crate::style::to_css`; fields with KFX-specific
/// semantics (device units, parent-inheritance baselines) are handled
/// explicitly.
///
/// Returns `None` if the field has nothing to emit.
///
/// `parent` is the computed style of the element's parent (the default style
/// at the root). KFX styles inherit through nested containers at render time
/// (verified on reference output: containers carry heritable properties and
/// their children omit them), so CSS-inherited properties are emitted only
/// when they differ from the parent — which both drops redundant
/// re-statements *and* emits explicit resets (`font-style: normal` inside an
/// italic ancestor) that comparing against the global default would prune.
pub fn extract_ir_field(
    ir_style: &ir_style::ComputedStyle,
    parent: &ir_style::ComputedStyle,
    field: IrField,
) -> Option<String> {
    let shared = |name: &str| ir_style::changed_property_value(ir_style, name);
    // Inherited properties: baseline is the parent's computed style.
    let inherited = |name: &str| ir_style::changed_property_value_from(ir_style, parent, name);

    match field {
        // ------------------------------------------------------------------
        // KFX-specific extractions (intentionally NOT shared with to_css).
        // ------------------------------------------------------------------
        // to_css combines both flags into one `text-decoration` value; KFX
        // needs each flag as a separate property.
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
        // Gate the decoration style on the underline flag: in CSS,
        // text-decoration-style without text-decoration-line: underline
        // draws nothing, and the KFX `underline` property this feeds
        // (shared with the text-decoration rule) would otherwise turn the
        // style into a phantom underline.
        IrField::UnderlineStyle => {
            if ir_style.text_decoration_underline {
                shared("text-decoration-style")
            } else {
                None
            }
        }
        // KNOWN DISCREPANCY: to_css emits `text-decoration-line: overline`,
        // while KFX maps the overline flag to a decoration style ("solid").
        // Kept as-is to preserve byte-identical output on both sides.
        IrField::Overline => {
            if ir_style.overline {
                Some("solid".to_string())
            } else {
                None
            }
        }
        // KNOWN DISCREPANCY: KFX gates list properties on display: list-item;
        // to_css emits them whenever they differ from the default.
        IrField::ListStylePosition => {
            if ir_style.display == ir_style::Display::ListItem {
                shared("list-style-position")
            } else {
                None
            }
        }
        IrField::ListStyleType => {
            if ir_style.display == ir_style::Display::ListItem {
                shared("list-style-type")
            } else {
                None
            }
        }
        // KNOWN DISCREPANCY: KFX uses the raw family string; to_css quotes
        // names that need quoting for CSS syntax. Inherited: skip when the
        // parent already carries the same family.
        IrField::FontFamily => {
            if ir_style.font_family == parent.font_family {
                None
            } else {
                ir_style.font_family.clone()
            }
        }
        // Language is not a CSS property (to_css emits it via the HTML lang
        // attribute instead). Inherited.
        IrField::Language => {
            if ir_style.language == parent.language {
                None
            } else {
                ir_style.language.clone()
            }
        }
        // BoxAlign: horizontal centering from `margin: <v> auto`. Unset
        // margins compute to `Px(0)`, so `Length::Auto` here is always the
        // author's explicit `auto` — the CSS centering idiom.
        IrField::BoxAlign => {
            let auto = ir_style::Length::Auto;
            if ir_style.margin_left == auto && ir_style.margin_right == auto {
                Some("center".to_string())
            } else {
                None
            }
        }
        // SizingBounds: Amazon auto-adds content-box when width/height is present.
        IrField::SizingBounds => {
            let default = ir_style::ComputedStyle::default();
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
        // ------------------------------------------------------------------
        // Device-unit extractions: font size and spacing emit their final
        // KFX units here (rem / lh / em), because the conversions need the
        // whole style — the element's absolute font size and its actual
        // line-height — not just one property's value.
        // ------------------------------------------------------------------
        // Font size: always the absolute size in rem, like reference output,
        // so the device font-size setting scales it and nested containers
        // never re-multiply relative em values. Omitted when it matches the
        // parent (renderer inheritance covers it).
        IrField::FontSize => {
            let abs = ir_style.font_size_abs.0 as f64;
            let parent_abs = parent.font_size_abs.0 as f64;
            if (abs - parent_abs).abs() < 1e-4 {
                None
            } else {
                Some(fmt_dim(abs, "rem"))
            }
        }
        // Line height in lh units (multiples of the 1.2em base line box),
        // floored at 0.99 like reference output — never below the readable
        // baseline, and never exactly 1.0, which means "normal". Inherited:
        // emitted only when it differs from the parent; an explicit
        // `line-height: normal` reset emits 1lh.
        IrField::LineHeight => {
            if ir_style.line_height == parent.line_height {
                None
            } else {
                Some(fmt_dim(emitted_line_height_lh(ir_style), "lh"))
            }
        }
        IrField::MarginTop => vertical_spacing_lh(ir_style.margin_top, ir_style),
        IrField::MarginBottom => vertical_spacing_lh(ir_style.margin_bottom, ir_style),
        IrField::MarginLeft => horizontal_spacing(ir_style.margin_left, ir_style),
        IrField::MarginRight => horizontal_spacing(ir_style.margin_right, ir_style),
        IrField::PaddingTop => vertical_spacing_lh(ir_style.padding_top, ir_style),
        IrField::PaddingBottom => vertical_spacing_lh(ir_style.padding_bottom, ir_style),
        IrField::PaddingLeft => horizontal_spacing(ir_style.padding_left, ir_style),
        IrField::PaddingRight => horizontal_spacing(ir_style.padding_right, ir_style),
        // Text indent is inherited: emit when it differs from the parent,
        // including an explicit reset to zero.
        IrField::TextIndent => {
            if ir_style.text_indent == parent.text_indent {
                None
            } else {
                horizontal_spacing(ir_style.text_indent, ir_style)
                    .or_else(|| Some("0em".to_string()))
            }
        }
        // Letter/word spacing are inherited; `normal` inside a spaced
        // ancestor must emit an explicit 0 (reference output does the same).
        IrField::LetterSpacing => {
            spacing_reset(ir_style.letter_spacing, parent.letter_spacing, || {
                inherited("letter-spacing")
            })
        }
        IrField::WordSpacing => spacing_reset(ir_style.word_spacing, parent.word_spacing, || {
            inherited("word-spacing")
        }),
        // ------------------------------------------------------------------
        // Shared extractions: canonical table in style/to_css.rs.
        // CSS-inherited properties diff against the parent, the rest
        // against the default style.
        // ------------------------------------------------------------------
        IrField::FontWeight => inherited("font-weight"),
        IrField::FontStyle => inherited("font-style"),
        IrField::FontVariant => inherited("font-variant"),
        IrField::TextAlign => inherited("text-align"),
        IrField::Color => {
            if ir_style.color == parent.color {
                None
            } else if parent.color.is_none()
                && ir_style
                    .color
                    .is_some_and(|c| c.r == 0 && c.g == 0 && c.b == 0)
            {
                // Authored black on default-ink context: reference output
                // drops it — explicit black forces black text in night mode.
                None
            } else {
                shared("color")
            }
        }
        IrField::BackgroundColor => shared("background-color"),
        IrField::VerticalAlign => shared("vertical-align"),
        IrField::TextTransform => inherited("text-transform"),
        IrField::Hyphens => inherited("hyphens"),
        IrField::WhiteSpace => shared("white-space"),
        IrField::UnderlineColor => shared("text-decoration-color"),
        IrField::Width => shared("width"),
        IrField::Height => shared("height"),
        IrField::MaxWidth => shared("max-width"),
        IrField::MinHeight => shared("min-height"),
        IrField::MinWidth => shared("min-width"),
        IrField::MaxHeight => shared("max-height"),
        IrField::Float => shared("float"),
        IrField::BreakBefore => shared("break-before"),
        IrField::BreakAfter => shared("break-after"),
        IrField::BreakInside => shared("break-inside"),
        IrField::BorderStyleTop => shared("border-style-top"),
        IrField::BorderStyleRight => shared("border-style-right"),
        IrField::BorderStyleBottom => shared("border-style-bottom"),
        IrField::BorderStyleLeft => shared("border-style-left"),
        IrField::BorderWidthTop => shared("border-width-top"),
        IrField::BorderWidthRight => shared("border-width-right"),
        IrField::BorderWidthBottom => shared("border-width-bottom"),
        IrField::BorderWidthLeft => shared("border-width-left"),
        // Black border colors are the renderer's default ink; reference
        // output never emits them (explicit black breaks night mode).
        IrField::BorderColorTop => {
            non_black(ir_style.border_color_top).and_then(|_| shared("border-top-color"))
        }
        IrField::BorderColorRight => {
            non_black(ir_style.border_color_right).and_then(|_| shared("border-right-color"))
        }
        IrField::BorderColorBottom => {
            non_black(ir_style.border_color_bottom).and_then(|_| shared("border-bottom-color"))
        }
        IrField::BorderColorLeft => {
            non_black(ir_style.border_color_left).and_then(|_| shared("border-left-color"))
        }
        IrField::BorderRadiusTopLeft => shared("border-top-left-radius"),
        IrField::BorderRadiusTopRight => shared("border-top-right-radius"),
        IrField::BorderRadiusBottomLeft => shared("border-bottom-left-radius"),
        IrField::BorderRadiusBottomRight => shared("border-bottom-right-radius"),
        IrField::Visibility => inherited("visibility"),
        IrField::Clear => shared("clear"),
        IrField::Orphans => shared("orphans"),
        IrField::Widows => shared("widows"),
        IrField::WordBreak => shared("word-break"),
        IrField::BorderCollapse => shared("border-collapse"),
        IrField::BorderSpacing => shared("border-spacing"),
        IrField::DropcapLines => shared("dropcap-lines"),
        IrField::DropcapChars => shared("dropcap-chars"),
    }
}

// ============================================================================
// KFX Import (Inverse Direction)
// ============================================================================

impl StyleSchema {
    /// Look up the first schema rule with the given KFX symbol
    /// (deterministic: registration order).
    pub fn get_by_kfx_symbol(&self, kfx_symbol: u64) -> Option<&StylePropertyRule> {
        self.rules_by_kfx_symbol(kfx_symbol).next()
    }

    /// Look up all schema rules with the given KFX symbol, in registration
    /// order. Several rules can share one symbol (e.g. orphans and widows
    /// both land in `keep_lines_together`), and import must consult each of
    /// them to recover every IR field encoded in the value.
    pub fn rules_by_kfx_symbol(&self, kfx_symbol: u64) -> impl Iterator<Item = &StylePropertyRule> {
        self.rules
            .iter()
            .filter(move |r| r.kfx_symbol as u64 == kfx_symbol)
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

            ValueTransform::WrapInt => value.as_int().map(|i| i.to_string()),

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

            ValueTransform::Dimensioned { .. }
            | ValueTransform::PreserveUnit
            | ValueTransform::AbsolutePt => {
                // Parse {value: N, unit: sym} struct
                // Value may be Int (whole numbers), Float, or Decimal (Amazon uses all three)
                let fields = value.as_struct()?;
                let value_field = get_field_by_symbol(fields, KfxSymbol::Value)?;
                let num = value_field
                    .as_float()
                    .or_else(|| value_field.as_int().map(|i| i as f64))
                    .or_else(|| {
                        if let IonValue::Decimal(s) = value_field {
                            s.parse::<f64>().ok()
                        } else {
                            None
                        }
                    })?;
                let unit_sym = get_field_by_symbol(fields, KfxSymbol::Unit)?.as_symbol()? as u32;

                // `lh` (reference line boxes) has no CSS spelling; convert
                // back to em at the 1.2em base line box.
                if unit_sym == KfxSymbol::Lh as u32 {
                    return Some(format!("{}em", num / LH_PER_EM));
                }

                // Convert unit symbol back to CSS unit string
                let unit_str = match unit_sym {
                    id if id == KfxSymbol::Em as u32 => "em",
                    id if id == KfxSymbol::Rem as u32 => "rem",
                    id if id == KfxSymbol::Percent as u32 => "%",
                    id if id == KfxSymbol::Px as u32 => "px",
                    id if id == KfxSymbol::Pt as u32 => "pt",
                    _ => "em", // Default fallback
                };

                Some(format!("{}{}", num, unit_str))
            }

            ValueTransform::ParseColor => {
                // Packed ARGB (0xAARRGGBB); the alpha byte is dropped since IR
                // colors have no alpha channel.
                let packed = value.as_int()? as u32;
                let r = (packed >> 16) & 0xFF;
                let g = (packed >> 8) & 0xFF;
                let b = packed & 0xFF;
                Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
            }

            ValueTransform::WrapInStruct { field, .. } => {
                // Parse struct { field: N } and extract integer value
                let fields = value.as_struct()?;
                let int_value = get_field_by_symbol(fields, *field)?.as_int()?;
                Some(int_value.to_string())
            }
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
            if let Some((r, g, b, _)) = parse_css_color(css_value) {
                ir_style.color = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BackgroundColor => {
            if let Some((r, g, b, _)) = parse_css_color(css_value) {
                ir_style.background_color = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::VerticalAlign => {
            if let Some(va) = ir_style::VerticalAlign::from_css(css_value) {
                ir_style.vertical_align = va;
            }
        }
        IrField::TextDecorationUnderline => {
            // A concrete decoration style (dotted, dashed, ...) coming back
            // from KFX `underline` also means the underline flag is on.
            ir_style.text_decoration_underline = matches!(
                css_value,
                "underline" | "true" | "solid" | "dotted" | "dashed" | "double"
            );
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
        IrField::WhiteSpace => {
            ir_style.white_space = match css_value {
                "normal" => ir_style::WhiteSpace::Normal,
                "nowrap" => ir_style::WhiteSpace::Nowrap,
                "pre" => ir_style::WhiteSpace::Pre,
                "pre-wrap" => ir_style::WhiteSpace::PreWrap,
                "pre-line" => ir_style::WhiteSpace::PreLine,
                _ => ir_style::WhiteSpace::Normal,
            };
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
            if let Some((r, g, b, _)) = parse_css_color(css_value) {
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
            if let Some((r, g, b, _)) = parse_css_color(css_value) {
                ir_style.border_color_top = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BorderColorRight => {
            if let Some((r, g, b, _)) = parse_css_color(css_value) {
                ir_style.border_color_right = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BorderColorBottom => {
            if let Some((r, g, b, _)) = parse_css_color(css_value) {
                ir_style.border_color_bottom = Some(ir_style::Color::rgb(r, g, b));
            }
        }
        IrField::BorderColorLeft => {
            if let Some((r, g, b, _)) = parse_css_color(css_value) {
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
        IrField::DropcapLines => {
            if let Ok(n) = css_value.parse::<u8>() {
                ir_style.dropcap_lines = n;
            }
        }
        IrField::DropcapChars => {
            if let Ok(n) = css_value.parse::<u8>() {
                ir_style.dropcap_chars = n;
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
        // Phase 12: Table properties
        IrField::BorderCollapse => {
            ir_style.border_collapse = match css_value {
                "collapse" => ir_style::BorderCollapse::Collapse,
                _ => ir_style::BorderCollapse::Separate,
            };
        }
        IrField::BorderSpacing => {
            if let Some(len) = parse_css_length_to_ir(css_value) {
                ir_style.border_spacing = len;
            }
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
        // Convert pt to px: 1pt = 96/72 px ≈ 1.333px
        "pt" => ir_style::Length::Px(value * 96.0 / 72.0),
        _ => ir_style::Length::Px(value),
    })
}

/// Import KFX style properties to an IR ComputedStyle using the schema.
///
/// This is the inverse of the export direction. Every rule sharing a KFX
/// symbol is consulted, because one KFX value can encode several IR fields
/// (`keep_lines_together: {first, last}` carries both orphans and widows;
/// `underline: dotted` carries both the underline flag and its style).
///
/// A second pass gives rules without their own IR field (e.g. `fill_color`,
/// which exists only to mirror `background-color` onto block containers) a
/// fallback: they populate their sibling key rule's field, but only when
/// that field wasn't already set directly — so a style carrying only
/// `fill_color` still imports a background color.
pub fn import_kfx_style(
    schema: &StyleSchema,
    props: &[(u64, IonValue)],
) -> ir_style::ComputedStyle {
    let mut style = ir_style::ComputedStyle::default();
    let mut applied: Vec<IrField> = Vec::new();

    // Reference KFX folds four equal border sides into uniform shorthand
    // symbols; expand them into the per-side symbols the rules know.
    let mut expanded: Vec<(u64, IonValue)> = Vec::with_capacity(props.len());
    for (kfx_symbol, kfx_value) in props {
        let sides: Option<[KfxSymbol; 4]> = match *kfx_symbol {
            s if s == KfxSymbol::BorderStyle as u64 => Some([
                KfxSymbol::BorderStyleTop,
                KfxSymbol::BorderStyleLeft,
                KfxSymbol::BorderStyleBottom,
                KfxSymbol::BorderStyleRight,
            ]),
            s if s == KfxSymbol::BorderWeight as u64 => Some([
                KfxSymbol::BorderWeightTop,
                KfxSymbol::BorderWeightLeft,
                KfxSymbol::BorderWeightBottom,
                KfxSymbol::BorderWeightRight,
            ]),
            s if s == KfxSymbol::BorderColor as u64 => Some([
                KfxSymbol::BorderColorTop,
                KfxSymbol::BorderColorLeft,
                KfxSymbol::BorderColorBottom,
                KfxSymbol::BorderColorRight,
            ]),
            _ => None,
        };
        match sides {
            Some(sides) => {
                expanded.extend(sides.iter().map(|&side| (side as u64, kfx_value.clone())))
            }
            None => expanded.push((*kfx_symbol, kfx_value.clone())),
        }
    }
    let props = &expanded[..];

    for (kfx_symbol, kfx_value) in props {
        for rule in schema.rules_by_kfx_symbol(*kfx_symbol) {
            if let Some(ir_field) = rule.ir_field
                && let Some(css_value) = rule.transform.inverse(kfx_value)
            {
                apply_ir_field(&mut style, ir_field, &css_value);
                if !applied.contains(&ir_field) {
                    applied.push(ir_field);
                }
            }
        }
    }

    for (kfx_symbol, kfx_value) in props {
        for rule in schema.rules_by_kfx_symbol(*kfx_symbol) {
            if rule.ir_field.is_none()
                && let Some(sibling_field) = schema.get(rule.ir_key).find_map(|r| r.ir_field)
                && !applied.contains(&sibling_field)
                && let Some(css_value) = rule.transform.inverse(kfx_value)
            {
                apply_ir_field(&mut style, sibling_field, &css_value);
                applied.push(sibling_field);
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
        assert_eq!(parse_hex_color("fff"), Some((255, 255, 255, 255)));
        assert_eq!(parse_hex_color("000"), Some((0, 0, 0, 255)));
        assert_eq!(parse_hex_color("ff0000"), Some((255, 0, 0, 255)));
        assert_eq!(parse_hex_color("00ff00"), Some((0, 255, 0, 255)));
        // Alpha is preserved; fully transparent is rejected outright.
        assert_eq!(parse_hex_color("ff000080"), Some((255, 0, 0, 128)));
        assert_eq!(parse_hex_color("ff000000"), None);
        // Multi-byte input must not panic on the byte slices.
        assert_eq!(parse_hex_color("aé"), None);
        assert_eq!(parse_hex_color("ééé"), None);
    }

    #[test]
    fn test_parse_named_color() {
        assert_eq!(parse_css_color("red"), Some((255, 0, 0, 255)));
        assert_eq!(parse_css_color("BLACK"), Some((0, 0, 0, 255)));
        assert_eq!(parse_css_color("White"), Some((255, 255, 255, 255)));
        // `transparent` must be omitted, never coerced to opaque black.
        assert_eq!(parse_css_color("transparent"), None);
        assert_eq!(parse_css_color("rgba(0, 0, 0, 0)"), None);
        assert_eq!(
            parse_css_color("rgba(10, 20, 30, 0.5)"),
            Some((10, 20, 30, 128))
        );
        // Malformed rgb with reversed parens must not panic.
        assert_eq!(parse_css_color("rgb)x("), None);
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
    fn test_schema_lookup() {
        let schema = StyleSchema::standard();

        assert!(schema.get_first("font-weight").is_some());
        assert!(schema.get_first("font-style").is_some());
        assert!(schema.get_first("text-align").is_some());
        assert!(schema.get_first("nonexistent").is_none());
    }

    #[test]
    fn test_font_weight_transform() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("font-weight").unwrap();

        let result = rule.transform.apply("bold");
        assert!(matches!(result, Some(KfxValue::Symbol(KfxSymbol::Bold))));

        let result = rule.transform.apply("normal");
        assert!(matches!(result, Some(KfxValue::Symbol(KfxSymbol::Normal))));
    }

    #[test]
    fn test_extract_ir_field_font_weight() {
        use crate::style::{ComputedStyle, FontWeight};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::FontWeight),
            None
        );

        let mut bold = ComputedStyle::default();
        bold.font_weight = FontWeight::BOLD;
        assert_eq!(
            extract_ir_field(&bold, &Default::default(), IrField::FontWeight),
            Some("bold".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_font_style() {
        use crate::style::{ComputedStyle, FontStyle};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::FontStyle),
            None
        );

        let mut italic = ComputedStyle::default();
        italic.font_style = FontStyle::Italic;
        assert_eq!(
            extract_ir_field(&italic, &Default::default(), IrField::FontStyle),
            Some("italic".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_color() {
        use crate::style::{Color, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::Color),
            None
        );

        let mut styled = ComputedStyle::default();
        styled.color = Some(Color::rgb(255, 0, 0));
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::Color),
            Some("#ff0000".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_margin() {
        use crate::style::{ComputedStyle, Length};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::MarginTop),
            None
        );

        // Vertical margins emit in lh units of the element's line box
        // (1.5em at the default 1.2em line-height = 1.25lh).
        let mut styled = ComputedStyle::default();
        styled.margin_top = Length::Em(1.5);
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::MarginTop),
            Some("1.25lh".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_all_variants_resolve() {
        // Drift guard: `changed_property_value` panics on a property name
        // missing from the canonical table in style/to_css.rs, so extracting
        // every IrField variant verifies every shared lookup resolves.
        const ALL_FIELDS: &[IrField] = &[
            IrField::FontWeight,
            IrField::FontStyle,
            IrField::FontSize,
            IrField::FontVariant,
            IrField::TextAlign,
            IrField::TextIndent,
            IrField::LineHeight,
            IrField::MarginTop,
            IrField::MarginBottom,
            IrField::MarginLeft,
            IrField::MarginRight,
            IrField::PaddingTop,
            IrField::PaddingBottom,
            IrField::PaddingLeft,
            IrField::PaddingRight,
            IrField::Color,
            IrField::BackgroundColor,
            IrField::VerticalAlign,
            IrField::TextDecorationUnderline,
            IrField::TextDecorationStrikethrough,
            IrField::LetterSpacing,
            IrField::WordSpacing,
            IrField::TextTransform,
            IrField::Hyphens,
            IrField::WhiteSpace,
            IrField::UnderlineStyle,
            IrField::Overline,
            IrField::UnderlineColor,
            IrField::Width,
            IrField::Height,
            IrField::MaxWidth,
            IrField::MinHeight,
            IrField::Float,
            IrField::BoxAlign,
            IrField::BreakBefore,
            IrField::BreakAfter,
            IrField::BreakInside,
            IrField::BorderStyleTop,
            IrField::BorderStyleRight,
            IrField::BorderStyleBottom,
            IrField::BorderStyleLeft,
            IrField::BorderWidthTop,
            IrField::BorderWidthRight,
            IrField::BorderWidthBottom,
            IrField::BorderWidthLeft,
            IrField::BorderColorTop,
            IrField::BorderColorRight,
            IrField::BorderColorBottom,
            IrField::BorderColorLeft,
            IrField::BorderRadiusTopLeft,
            IrField::BorderRadiusTopRight,
            IrField::BorderRadiusBottomLeft,
            IrField::BorderRadiusBottomRight,
            IrField::ListStylePosition,
            IrField::ListStyleType,
            IrField::FontFamily,
            IrField::Language,
            IrField::Visibility,
            IrField::SizingBounds,
            IrField::Clear,
            IrField::MinWidth,
            IrField::MaxHeight,
            IrField::Orphans,
            IrField::Widows,
            IrField::WordBreak,
            IrField::BorderCollapse,
            IrField::BorderSpacing,
        ];

        let default = ir_style::ComputedStyle::default();
        for &field in ALL_FIELDS {
            let extracted = extract_ir_field(&default, &Default::default(), field);
            // A fully-default style extracts nothing — including BoxAlign,
            // which no longer centers on unset (all-auto) margins.
            assert_eq!(extracted, None, "{field:?} non-None on default style");
        }
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
    fn test_css_length_with_whitespace() {
        // Leading/trailing whitespace
        assert_eq!(parse_css_length("  10px  "), Some((10.0, "px".into())));
        assert_eq!(parse_css_length("\t1.5em\n"), Some((1.5, "em".into())));
    }

    #[test]
    fn test_parse_color_with_whitespace() {
        // Colors should handle whitespace
        assert_eq!(parse_css_color("  red  "), Some((255, 0, 0, 255)));
        assert_eq!(parse_css_color("  #ff0000  "), Some((255, 0, 0, 255)));
    }

    #[test]
    fn test_rgb_function_parsing() {
        assert_eq!(parse_css_color("rgb(255, 0, 0)"), Some((255, 0, 0, 255)));
        assert_eq!(
            parse_css_color("rgb(0, 128, 255)"),
            Some((0, 128, 255, 255))
        );
        assert_eq!(
            parse_css_color("rgba(255, 255, 255, 0.5)"),
            Some((255, 255, 255, 128))
        );
    }

    #[test]
    fn test_rgb_percentage_parsing() {
        assert_eq!(parse_css_color("rgb(100%, 0%, 0%)"), Some((255, 0, 0, 255)));
        assert_eq!(
            parse_css_color("rgb(50%, 50%, 50%)"),
            Some((128, 128, 128, 255))
        );
    }

    #[test]
    fn test_negative_numbers() {
        // Negative values are valid in CSS (e.g., text-indent: -10px)
        assert_eq!(parse_css_length("-10px"), Some((-10.0, "px".into())));
        assert_eq!(parse_css_length("-1.5em"), Some((-1.5, "em".into())));
    }

    // ========================================================================
    // Amazon KFX Compatibility Tests
    // ========================================================================

    #[test]
    fn test_font_weight_full_range_symbols() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("font-weight").unwrap();

        // Verify all numeric weights map to correct symbols
        assert!(matches!(
            rule.transform.apply("100"),
            Some(KfxValue::Symbol(KfxSymbol::Thin))
        ));
        assert!(matches!(
            rule.transform.apply("200"),
            Some(KfxValue::Symbol(KfxSymbol::UltraLight))
        ));
        // 300 maps to ultra_light like reference output; Light is the
        // `lighter` keyword.
        assert!(matches!(
            rule.transform.apply("300"),
            Some(KfxValue::Symbol(KfxSymbol::UltraLight))
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
        let rule = schema.get_first("font-style").unwrap();

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
        let rule = schema.get_first("text-align").unwrap();

        // Logical start/end fold to the physical sides: reference output
        // never emits the Start/End symbols and KFX consumers reject them.
        assert!(matches!(
            rule.transform.apply("start"),
            Some(KfxValue::Symbol(KfxSymbol::Left))
        ));
        assert!(matches!(
            rule.transform.apply("end"),
            Some(KfxValue::Symbol(KfxSymbol::Right))
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
        let rule = schema.get_first("color").unwrap();

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
        let rule = schema.get_first("vertical-align").unwrap();

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
        let rule = schema.get_first("font-weight").unwrap();

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
        let rule = schema.get_first("margin-top").unwrap();

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

        // {value: "1.8", unit: pt} → "1.8pt" (Decimal - Amazon uses for border_weight)
        let kfx_value = IonValue::Struct(vec![
            (
                KfxSymbol::Value as u64,
                IonValue::Decimal("1.8".to_string()),
            ),
            (
                KfxSymbol::Unit as u64,
                IonValue::Symbol(KfxSymbol::Pt as u64),
            ),
        ]);
        assert_eq!(
            rule.transform.inverse(&kfx_value),
            Some("1.8pt".to_string())
        );
    }

    #[test]
    fn test_inverse_color_packed_to_hex() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("color").unwrap();

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
        use crate::style::{FontWeight, TextAlign};

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
        assert_eq!(ir_style.margin_top, crate::style::Length::Em(2.0));
    }

    // ========================================================================
    // Phase 1-7: New Style Properties Tests
    // ========================================================================

    #[test]
    fn test_letter_spacing_transform() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("letter-spacing").unwrap();

        // 0.1em should convert to dimensioned value
        let result = rule.transform.apply("0.1em");
        assert!(matches!(result, Some(KfxValue::Dimensioned { .. })));
    }

    #[test]
    fn test_text_transform_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("text-transform").unwrap();

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
        let rule = schema.get_first("hyphens").unwrap();

        assert!(matches!(
            rule.transform.apply("auto"),
            Some(KfxValue::Symbol(KfxSymbol::Auto))
        ));
        // manual folds to none like reference output (KFX has no manual mode).
        assert!(matches!(
            rule.transform.apply("manual"),
            Some(KfxValue::Symbol(KfxSymbol::None))
        ));
        assert!(matches!(
            rule.transform.apply("none"),
            Some(KfxValue::Symbol(KfxSymbol::None))
        ));
    }

    #[test]
    fn test_white_space_nobreak() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("white-space").unwrap();

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

        let rule = schema.get_first("break-before").unwrap();
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

        let rule = schema.get_first("break-inside").unwrap();
        assert!(matches!(
            rule.transform.apply("avoid"),
            Some(KfxValue::Symbol(KfxSymbol::Avoid))
        ));
    }

    #[test]
    fn test_float_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("float").unwrap();

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
        let rule = schema.get_first("border-top-style").unwrap();

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
        let rule = schema.get_first("list-style-position").unwrap();

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
        let rule = schema.get_first("visibility").unwrap();

        // Reference KFX encodes visibility as an Ion boolean, not a symbol.
        assert!(matches!(
            rule.transform.apply("visible"),
            Some(KfxValue::Bool(true))
        ));
        assert!(matches!(
            rule.transform.apply("hidden"),
            Some(KfxValue::Bool(false))
        ));
        assert!(matches!(
            rule.transform.apply("collapse"),
            Some(KfxValue::Bool(false))
        ));
    }

    #[test]
    fn test_extract_ir_field_letter_spacing() {
        use crate::style::{ComputedStyle, Length};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::LetterSpacing),
            None
        );

        let mut styled = ComputedStyle::default();
        styled.letter_spacing = Length::Em(0.1);
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::LetterSpacing),
            Some("0.1em".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_text_transform() {
        use crate::style::{ComputedStyle, TextTransform};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::TextTransform),
            None
        );

        let mut styled = ComputedStyle::default();
        styled.text_transform = TextTransform::Uppercase;
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::TextTransform),
            Some("uppercase".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_break_before() {
        use crate::style::{BreakValue, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::BreakBefore),
            None
        );

        let mut styled = ComputedStyle::default();
        styled.break_before = BreakValue::Always;
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::BreakBefore),
            Some("always".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_border_style() {
        use crate::style::{BorderStyle, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::BorderStyleTop),
            None
        );

        let mut styled = ComputedStyle::default();
        styled.border_style_top = BorderStyle::Solid;
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::BorderStyleTop),
            Some("solid".to_string())
        );
    }

    #[test]
    fn test_apply_ir_field_text_transform() {
        use crate::style::{ComputedStyle, TextTransform};

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
        use crate::style::{BorderStyle, ComputedStyle};

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
        use crate::style::{ComputedStyle, Length};

        // Negative letter-spacing is valid CSS
        let mut style = ComputedStyle::default();
        apply_ir_field(&mut style, IrField::LetterSpacing, "-0.05em");
        assert_eq!(style.letter_spacing, Length::Em(-0.05));
    }

    #[test]
    fn test_hyphens_default_is_manual() {
        use crate::style::{ComputedStyle, Hyphens};

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
        use crate::style::{ComputedStyle, Length};

        // When width is set, sizing_bounds should emit content-box (the CSS default)
        // This matches Amazon's converter behavior
        let mut style = ComputedStyle::default();
        style.width = Length::Percent(75.0);

        let result = extract_ir_field(&style, &Default::default(), IrField::SizingBounds);
        assert_eq!(result, Some("content-box".to_string()));
    }

    #[test]
    fn test_sizing_bounds_border_box() {
        use crate::style::{BoxSizing, ComputedStyle, Length};

        // Explicit border-box should emit border-box
        let mut style = ComputedStyle::default();
        style.box_sizing = BoxSizing::BorderBox;
        style.width = Length::Percent(100.0);

        let result = extract_ir_field(&style, &Default::default(), IrField::SizingBounds);
        assert_eq!(result, Some("border-box".to_string()));
    }

    #[test]
    fn test_sizing_bounds_not_emitted_without_dimensions() {
        use crate::style::ComputedStyle;

        // No width/height = no sizing_bounds
        let style = ComputedStyle::default();

        let result = extract_ir_field(&style, &Default::default(), IrField::SizingBounds);
        assert_eq!(result, None);
    }

    #[test]
    fn test_box_align_from_margin_auto() {
        use crate::style::{ComputedStyle, Length};

        // `margin: 0 auto` → left/right auto, top/bottom explicit → center.
        let mut style = ComputedStyle::default();
        style.margin_top = Length::Px(0.0);
        style.margin_bottom = Length::Px(0.0);
        style.margin_left = Length::Auto;
        style.margin_right = Length::Auto;

        let result = extract_ir_field(&style, &Default::default(), IrField::BoxAlign);
        assert_eq!(result, Some("center".to_string()));
    }

    #[test]
    fn test_box_align_not_emitted_for_unset_margins() {
        use crate::style::{ComputedStyle, Length};

        // A width-constrained box with UNSET margins (all four default Auto)
        // must NOT center — it renders left-aligned in CSS.
        let mut style = ComputedStyle::default();
        style.width = Length::Px(200.0);
        assert_eq!(
            extract_ir_field(&style, &Default::default(), IrField::BoxAlign),
            None
        );

        // Only margin-left: auto is not enough either.
        let mut style = ComputedStyle::default();
        style.margin_left = Length::Auto;
        style.margin_right = Length::Px(0.0);
        assert_eq!(
            extract_ir_field(&style, &Default::default(), IrField::BoxAlign),
            None
        );
    }

    // ========================================================================
    // Phase 12: Table Properties
    // ========================================================================

    #[test]
    fn test_border_collapse_mapping() {
        let schema = StyleSchema::standard();
        let rule = schema.get_first("border-collapse").unwrap();

        assert_eq!(rule.kfx_symbol, KfxSymbol::TableBorderCollapse);
        // KFX uses boolean: true = collapse, false = separate
        assert!(matches!(
            rule.transform.apply("collapse"),
            Some(KfxValue::Bool(true))
        ));
        assert!(matches!(
            rule.transform.apply("separate"),
            Some(KfxValue::Bool(false))
        ));
    }

    #[test]
    fn test_border_spacing_multiple_rules() {
        let schema = StyleSchema::standard();

        // border-spacing should have two rules (vertical and horizontal)
        let rules: Vec<_> = schema.get("border-spacing").collect();
        assert_eq!(rules.len(), 2);

        let symbols: Vec<_> = rules.iter().map(|r| r.kfx_symbol).collect();
        assert!(symbols.contains(&KfxSymbol::BorderSpacingVertical));
        assert!(symbols.contains(&KfxSymbol::BorderSpacingHorizontal));

        // Both transform "10px" to a pt hairline (1px = 0.75pt).
        for rule in rules {
            let result = rule.transform.apply("10px");
            assert!(matches!(
                result,
                Some(KfxValue::Dimensioned { value, unit }) if value == 7.5 && unit == KfxSymbol::Pt
            ));
        }
    }

    #[test]
    fn test_vertical_align_multiple_rules() {
        let schema = StyleSchema::standard();

        // vertical-align should have two rules (baseline_style and yj.vertical_align)
        let rules: Vec<_> = schema.get("vertical-align").collect();
        assert_eq!(rules.len(), 2);

        let symbols: Vec<_> = rules.iter().map(|r| r.kfx_symbol).collect();
        assert!(symbols.contains(&KfxSymbol::BaselineStyle));
        assert!(symbols.contains(&KfxSymbol::YjVerticalAlign));

        // "super" should only match baseline_style rule
        let super_results: Vec<_> = rules
            .iter()
            .filter_map(|r| r.transform.apply("super"))
            .collect();
        assert_eq!(super_results.len(), 1);
        assert!(matches!(
            super_results[0],
            KfxValue::Symbol(KfxSymbol::Superscript)
        ));

        // "top" should only match yj.vertical_align rule
        let top_results: Vec<_> = rules
            .iter()
            .filter_map(|r| r.transform.apply("top"))
            .collect();
        assert_eq!(top_results.len(), 1);
        assert!(matches!(top_results[0], KfxValue::Symbol(KfxSymbol::Top)));
    }

    #[test]
    fn test_extract_ir_field_border_collapse() {
        use crate::style::{BorderCollapse, ComputedStyle};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::BorderCollapse),
            None
        );

        let mut styled = ComputedStyle::default();
        styled.border_collapse = BorderCollapse::Collapse;
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::BorderCollapse),
            Some("collapse".to_string())
        );
    }

    #[test]
    fn test_extract_ir_field_border_spacing() {
        use crate::style::{ComputedStyle, Length};

        let default = ComputedStyle::default();
        assert_eq!(
            extract_ir_field(&default, &Default::default(), IrField::BorderSpacing),
            None
        );

        let mut styled = ComputedStyle::default();
        styled.border_spacing = Length::Px(5.0);
        assert_eq!(
            extract_ir_field(&styled, &Default::default(), IrField::BorderSpacing),
            Some("5px".to_string())
        );
    }

    #[test]
    fn test_preserve_unit_transform() {
        let schema = StyleSchema::standard();
        // width keeps the author's units verbatim (PreserveUnit).
        let rule = schema.get("width").next().unwrap();

        assert!(matches!(
            rule.transform.apply("10px"),
            Some(KfxValue::Dimensioned { value, unit }) if value == 10.0 && unit == KfxSymbol::Px
        ));
        assert!(matches!(
            rule.transform.apply("1.5em"),
            Some(KfxValue::Dimensioned { value, unit }) if value == 1.5 && unit == KfxSymbol::Em
        ));
        assert!(matches!(
            rule.transform.apply("50%"),
            Some(KfxValue::Dimensioned { value, unit }) if value == 50.0 && unit == KfxSymbol::Percent
        ));
    }

    #[test]
    fn unit_model_matches_reference_output() {
        use crate::style::{AbsFontSize, ComputedStyle, Length};

        // Extraction emits the final device unit; the schema transform is a
        // pass-through parse. Run both stages like ingest_ir_style does.
        let schema = StyleSchema::standard();
        let run = |style: &ComputedStyle, key: &str, field: IrField| {
            extract_ir_field(style, &Default::default(), field)
                .and_then(|css| schema.get(key).next().unwrap().transform.apply(&css))
        };

        // Font sizes emit the cascade-resolved absolute size in rem so the
        // device font-size setting scales them; never absolute px.
        let mut s = ComputedStyle::default();
        s.font_size = Length::Px(10.0);
        s.font_size_abs = AbsFontSize(0.625);
        assert!(matches!(
            run(&s, "font-size", IrField::FontSize),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 0.625).abs() < 1e-9 && unit == KfxSymbol::Rem
        ));

        // Line height converts to lh (1lh = 1.2em); authored leading at or
        // below the base maps to 0.99lh like reference output.
        let mut s = ComputedStyle::default();
        s.line_height = Length::Em(1.5);
        assert!(matches!(
            run(&s, "line-height", IrField::LineHeight),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 1.25).abs() < 1e-9 && unit == KfxSymbol::Lh
        ));
        let mut s = ComputedStyle::default();
        s.line_height = Length::Em(1.0);
        assert!(matches!(
            run(&s, "line-height", IrField::LineHeight),
            Some(KfxValue::Dimensioned { value, unit })
                if value == 0.99 && unit == KfxSymbol::Lh
        ));
        // px/pt line heights convert instead of being dropped (24px = 1.5em).
        let mut s = ComputedStyle::default();
        s.line_height = Length::Px(24.0);
        assert!(matches!(
            run(&s, "line-height", IrField::LineHeight),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 1.25).abs() < 1e-9 && unit == KfxSymbol::Lh
        ));

        // Vertical spacing: lh units of the element's own line box —
        // reference output divides by the actual line-height, not the base.
        let mut s = ComputedStyle::default();
        s.margin_top = Length::Em(1.0);
        assert!(matches!(
            run(&s, "margin-top", IrField::MarginTop),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 1.0 / 1.2).abs() < 1e-4 && unit == KfxSymbol::Lh
        ));
        let mut s = ComputedStyle::default();
        s.margin_top = Length::Em(1.0);
        s.line_height = Length::Em(2.0);
        assert!(matches!(
            run(&s, "margin-top", IrField::MarginTop),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 0.5).abs() < 1e-4 && unit == KfxSymbol::Lh
        ));
        // Percent vertical spacing resolves against KP's 512px viewport.
        let mut s = ComputedStyle::default();
        s.margin_top = Length::Percent(5.0);
        assert!(matches!(
            run(&s, "margin-top", IrField::MarginTop),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 4.0 / 3.0).abs() < 1e-5 && unit == KfxSymbol::Lh
        ));

        // Horizontal spacing keeps relative units; px folds to em of the
        // element's own font.
        let mut s = ComputedStyle::default();
        s.margin_left = Length::Px(40.0);
        assert!(matches!(
            run(&s, "margin-left", IrField::MarginLeft),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 2.5).abs() < 1e-9 && unit == KfxSymbol::Em
        ));
        let mut s = ComputedStyle::default();
        s.text_indent = Length::Percent(5.0);
        assert!(matches!(
            run(&s, "text-indent", IrField::TextIndent),
            Some(KfxValue::Dimensioned { value, unit })
                if value == 5.0 && unit == KfxSymbol::Percent
        ));
        let mut s = ComputedStyle::default();
        s.text_indent = Length::Em(-1.0);
        assert!(matches!(
            run(&s, "text-indent", IrField::TextIndent),
            Some(KfxValue::Dimensioned { value, unit })
                if value == -1.0 && unit == KfxSymbol::Em
        ));

        // Hairlines: px folds to pt at the CSS ratio (transform-level).
        let apply = |key: &str, raw: &str| schema.get(key).next().unwrap().transform.apply(raw);
        assert!(matches!(
            apply("border-top-width", "4px"),
            Some(KfxValue::Dimensioned { value, unit })
                if value == 3.0 && unit == KfxSymbol::Pt
        ));
        assert!(matches!(
            apply("letter-spacing", "0.1em"),
            Some(KfxValue::Dimensioned { value, unit })
                if (value - 0.1).abs() < 1e-9 && unit == KfxSymbol::Em
        ));
    }

    #[test]
    fn inherited_properties_diff_against_parent() {
        use crate::style::{AbsFontSize, ComputedStyle, FontStyle, Length, TextTransform};

        // Equal to parent: nothing to emit (renderer inherits).
        let mut parent = ComputedStyle::default();
        parent.font_style = FontStyle::Italic;
        let child = parent.clone();
        assert_eq!(extract_ir_field(&child, &parent, IrField::FontStyle), None);

        // Reset against an italic ancestor emits the explicit initial value.
        let mut reset = parent.clone();
        reset.font_style = FontStyle::Normal;
        assert_eq!(
            extract_ir_field(&reset, &parent, IrField::FontStyle),
            Some("normal".to_string())
        );

        // text-transform: none inside an uppercase ancestor.
        let mut parent = ComputedStyle::default();
        parent.text_transform = TextTransform::Uppercase;
        let mut reset = parent.clone();
        reset.text_transform = TextTransform::None;
        assert_eq!(
            extract_ir_field(&reset, &parent, IrField::TextTransform),
            Some("none".to_string())
        );

        // letter-spacing: normal inside a spaced ancestor emits explicit 0.
        let mut parent = ComputedStyle::default();
        parent.letter_spacing = Length::Px(2.0);
        let mut reset = parent.clone();
        reset.letter_spacing = Length::Auto;
        assert_eq!(
            extract_ir_field(&reset, &parent, IrField::LetterSpacing),
            Some("0em".to_string())
        );

        // Font size equal to the parent's absolute size: omitted; a reset
        // back to the root size inside a larger ancestor emits 1rem.
        let mut parent = ComputedStyle::default();
        parent.font_size_abs = AbsFontSize(1.44);
        let child = parent.clone();
        assert_eq!(extract_ir_field(&child, &parent, IrField::FontSize), None);
        let mut reset = parent.clone();
        reset.font_size_abs = AbsFontSize(1.0);
        assert_eq!(
            extract_ir_field(&reset, &parent, IrField::FontSize),
            Some("1rem".to_string())
        );
    }

    #[test]
    fn test_import_keep_lines_together_recovers_both_fields() {
        let schema = StyleSchema::standard();
        // keep_lines_together: { first: 3, last: 4 } encodes orphans AND
        // widows; import used to consult only one arbitrary rule and drop
        // the other.
        let props = vec![(
            KfxSymbol::KeepLinesTogether as u64,
            IonValue::Struct(vec![
                (KfxSymbol::First as u64, IonValue::Int(3)),
                (KfxSymbol::Last as u64, IonValue::Int(4)),
            ]),
        )];
        let style = import_kfx_style(schema, &props);
        assert_eq!(style.orphans, 3);
        assert_eq!(style.widows, 4);

        // A last-only struct must still populate widows.
        let props = vec![(
            KfxSymbol::KeepLinesTogether as u64,
            IonValue::Struct(vec![(KfxSymbol::Last as u64, IonValue::Int(5))]),
        )];
        let style = import_kfx_style(schema, &props);
        assert_eq!(style.widows, 5);
    }

    #[test]
    fn test_import_underline_dotted_sets_flag_and_style() {
        let schema = StyleSchema::standard();
        let props = vec![(
            KfxSymbol::Underline as u64,
            IonValue::Symbol(KfxSymbol::Dotted as u64),
        )];
        let style = import_kfx_style(schema, &props);
        assert!(style.text_decoration_underline);
        assert_eq!(style.underline_style, crate::style::DecorationStyle::Dotted);

        let props = vec![(
            KfxSymbol::Underline as u64,
            IonValue::Symbol(KfxSymbol::Solid as u64),
        )];
        let style = import_kfx_style(schema, &props);
        assert!(style.text_decoration_underline);
    }

    #[test]
    fn test_import_fill_color_falls_back_to_background() {
        let schema = StyleSchema::standard();
        // A style carrying only fill_color (typical for block containers)
        // must import as a background color instead of being dropped.
        let packed = 0xFF112233u32 as i64;
        let props = vec![(KfxSymbol::FillColor as u64, IonValue::Int(packed))];
        let style = import_kfx_style(schema, &props);
        assert_eq!(
            style.background_color,
            Some(crate::style::Color::rgb(0x11, 0x22, 0x33))
        );

        // When text_background_color is present it wins; fill_color must not
        // overwrite it.
        let props = vec![
            (
                KfxSymbol::TextBackgroundColor as u64,
                IonValue::Int(0xFFAABBCCu32 as i64),
            ),
            (KfxSymbol::FillColor as u64, IonValue::Int(packed)),
        ];
        let style = import_kfx_style(schema, &props);
        assert_eq!(
            style.background_color,
            Some(crate::style::Color::rgb(0xAA, 0xBB, 0xCC))
        );
    }

    #[test]
    fn test_import_border_width_from_kfx() {
        use crate::kfx::ion::IonValue;
        use crate::style::Length;

        let schema = StyleSchema::standard();

        // Simulate KFX: border_weight_top: { value: 0.45, unit: pt }
        // Note: In real KFX files, the struct uses field symbol IDs 4 (name) and similar,
        // not the KfxSymbol::Value/Unit IDs. Let me check the schema...
        let props = vec![(
            KfxSymbol::BorderWeightTop as u64,
            IonValue::Struct(vec![
                (KfxSymbol::Value as u64, IonValue::Float(0.45)),
                (
                    KfxSymbol::Unit as u64,
                    IonValue::Symbol(KfxSymbol::Pt as u64),
                ),
            ]),
        )];

        let style = import_kfx_style(schema, &props);

        // Should import as a non-default length (0.45pt ≈ 0.6px)
        assert!(
            !matches!(style.border_width_top, Length::Auto),
            "border_width_top should be set, got {:?}",
            style.border_width_top
        );

        // Check it's approximately 0.6px (0.45 * 96/72 ≈ 0.6)
        if let Length::Px(px) = style.border_width_top {
            assert!((px - 0.6).abs() < 0.01, "Expected ~0.6px, got {}px", px);
        } else {
            panic!("Expected Length::Px, got {:?}", style.border_width_top);
        }
    }

    #[test]
    fn test_import_border_width_verifies_schema_lookup() {
        use crate::kfx::ion::IonValue;

        // Debug test: verify the schema lookup works for BorderWeightTop
        let schema = StyleSchema::standard();

        // Check schema has the rule
        let rule = schema.get_by_kfx_symbol(94); // BorderWeightTop
        assert!(
            rule.is_some(),
            "Schema should have rule for symbol 94 (BorderWeightTop)"
        );

        let rule = rule.unwrap();
        eprintln!("Rule ir_key: {}", rule.ir_key);
        eprintln!("Rule kfx_symbol: {:?}", rule.kfx_symbol);
        eprintln!("Rule ir_field: {:?}", rule.ir_field);

        assert_eq!(rule.kfx_symbol, KfxSymbol::BorderWeightTop);

        // Test the inverse transform
        let kfx_value = IonValue::Struct(vec![
            (KfxSymbol::Value as u64, IonValue::Float(0.45)),
            (
                KfxSymbol::Unit as u64,
                IonValue::Symbol(KfxSymbol::Pt as u64),
            ),
        ]);

        let css_value = rule.transform.inverse(&kfx_value);
        eprintln!("Inverse transform result: {:?}", css_value);
        assert!(css_value.is_some(), "Inverse transform should succeed");
        assert_eq!(css_value.unwrap(), "0.45pt");
    }

    #[test]
    fn test_get_by_kfx_symbol_border_weight() {
        let schema = StyleSchema::standard();

        // Verify the schema has a rule for BorderWeightTop
        let rule = schema.get_by_kfx_symbol(KfxSymbol::BorderWeightTop as u64);
        assert!(
            rule.is_some(),
            "Schema should have a rule for BorderWeightTop"
        );

        let rule = rule.unwrap();
        assert_eq!(rule.ir_key, "border-top-width");
        assert_eq!(rule.ir_field, Some(IrField::BorderWidthTop));
    }
}
