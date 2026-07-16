//! Style registry for KFX export.
//!
//! Handles style deduplication and ID assignment during the two-pass export:
//! - Pass 1: Collect unique style combinations, assign IDs via hashing
//! - Pass 2: Emit style fragment with all definitions, reference by ID in content

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::kfx::ion::IonValue;
use crate::kfx::style_schema::{KfxValue, StyleSchema, extract_ir_field};
use crate::kfx::symbols::KfxSymbol;
use crate::style as ir_style;

// ============================================================================
// Computed Style
// ============================================================================

/// A computed style is a set of resolved KFX property values.
///
/// This is what we hash for deduplication - identical property sets
/// get the same style ID.
#[derive(Debug, Clone, Default)]
pub struct ComputedStyle {
    /// Resolved properties: (KfxSymbol, KfxValue)
    properties: Vec<(KfxSymbol, KfxValue)>,
}

impl ComputedStyle {
    /// Create an empty computed style.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a property to this style.
    ///
    /// Setting the same symbol twice replaces the value, except when both
    /// values are struct-shaped: those merge into one multi-field struct so
    /// that orphans (`{first: N}`) and widows (`{last: M}`) can coexist in a
    /// single `keep_lines_together` instead of one silently clobbering the
    /// other.
    pub fn set(&mut self, symbol: KfxSymbol, value: KfxValue) {
        if let Some(existing) = self.properties.iter_mut().find(|(s, _)| *s == symbol) {
            existing.1 = existing.1.merge_struct_fields(&value).unwrap_or(value);
        } else {
            self.properties.push((symbol, value));
        }
    }

    /// Get a property value.
    pub fn get(&self, symbol: KfxSymbol) -> Option<&KfxValue> {
        self.properties
            .iter()
            .find(|(s, _)| *s == symbol)
            .map(|(_, v)| v)
    }

    /// Check if the style is empty.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// Get the number of properties.
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// Iterate over properties.
    pub fn iter(&self) -> impl Iterator<Item = &(KfxSymbol, KfxValue)> {
        self.properties.iter()
    }

    /// Order-independent equality of the property sets. Used to confirm a
    /// [`compute_hash`](Self::compute_hash) match is genuine rather than a
    /// 64-bit collision between two distinct styles.
    fn same_properties(&self, other: &ComputedStyle) -> bool {
        if self.properties.len() != other.properties.len() {
            return false;
        }
        let mut a = self.properties.clone();
        let mut b = other.properties.clone();
        a.sort_by_key(|(s, _)| *s as u64);
        b.sort_by_key(|(s, _)| *s as u64);
        a == b
    }

    /// Compute a hash for this style (for deduplication).
    pub fn compute_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;

        // Sort properties for consistent hashing
        let mut sorted: Vec<_> = self.properties.clone();
        sorted.sort_by_key(|(s, _)| *s as u64);

        let mut hasher = DefaultHasher::new();
        for (symbol, value) in &sorted {
            (*symbol as u64).hash(&mut hasher);
            hash_kfx_value(value, &mut hasher);
        }
        hasher.finish()
    }

    /// Convert to KFX Ion struct for the style entity.
    pub fn to_ion(&self, style_name_symbol: u64) -> IonValue {
        let mut fields = Vec::new();

        // style_name field first
        fields.push((
            KfxSymbol::StyleName as u64,
            IonValue::Symbol(style_name_symbol),
        ));

        // Add all properties
        for (symbol, value) in &self.properties {
            fields.push((*symbol as u64, value.to_ion()));
        }

        IonValue::Struct(fields)
    }
}

/// Hash a KfxValue for style deduplication.
fn hash_kfx_value<H: Hasher>(value: &KfxValue, hasher: &mut H) {
    // Discriminant first
    std::mem::discriminant(value).hash(hasher);
    match value {
        KfxValue::Symbol(s) => (*s as u64).hash(hasher),
        KfxValue::SymbolId(id) => id.hash(hasher),
        KfxValue::Integer(n) => n.hash(hasher),
        KfxValue::Float(f) => f.to_bits().hash(hasher),
        KfxValue::String(s) => s.hash(hasher),
        KfxValue::Bool(b) => b.hash(hasher),
        KfxValue::Null => 0u8.hash(hasher),
        KfxValue::Dimensioned { value, unit } => {
            value.to_bits().hash(hasher);
            (*unit as u64).hash(hasher);
        }
        KfxValue::StructField { field, value } => {
            (*field as u64).hash(hasher);
            value.hash(hasher);
        }
        KfxValue::StructFields(fields) => {
            for (field, value) in fields {
                (*field as u64).hash(hasher);
                value.hash(hasher);
            }
        }
    }
}

// ============================================================================
// Style Registry
// ============================================================================

/// Registry for collecting and deduplicating styles during export.
pub struct StyleRegistry {
    /// Hash -> bucket of (style_id, style_name_symbol, computed_style).
    /// Bucketed so a 64-bit hash collision between two *different* styles is
    /// resolved by comparing the actual properties, rather than silently
    /// merging them.
    styles: HashMap<u64, Vec<(u64, u64, ComputedStyle)>>,

    /// Next style ID to assign
    next_style_id: u64,

    /// The default style ID (for elements without specific styles)
    default_style_id: u64,

    /// Default style name symbol
    default_style_symbol: u64,
}

impl StyleRegistry {
    /// Create a new style registry.
    ///
    /// The `default_style_symbol` is the symbol ID for "s0" (or similar),
    /// pre-registered in the symbol table.
    pub fn new(default_style_symbol: u64) -> Self {
        Self {
            styles: HashMap::new(),
            next_style_id: 1, // Start at 1, 0 is default
            default_style_id: 0,
            default_style_symbol,
        }
    }

    /// Get the default style ID.
    pub fn default_style_id(&self) -> u64 {
        self.default_style_id
    }

    /// Get the default style symbol.
    pub fn default_style_symbol(&self) -> u64 {
        self.default_style_symbol
    }

    /// Register a computed style and get its ID.
    ///
    /// If an identical style was already registered, returns the existing ID.
    /// Otherwise, assigns a new ID.
    pub fn register(
        &mut self,
        style: ComputedStyle,
        symbols: &mut crate::kfx::context::SymbolTable,
    ) -> u64 {
        if style.is_empty() {
            return self.default_style_symbol;
        }

        let hash = style.compute_hash();

        let bucket = self.styles.entry(hash).or_default();
        // Confirm a hash hit is a real match, not a collision.
        if let Some((_, name_symbol, _)) = bucket
            .iter()
            .find(|(_, _, existing)| existing.same_properties(&style))
        {
            return *name_symbol;
        }

        // Assign new ID and create symbol name
        let style_id = self.next_style_id;
        self.next_style_id += 1;

        let style_name = format!("s{:X}", style_id);
        let name_symbol = symbols.get_or_intern(&style_name);

        bucket.push((style_id, name_symbol, style));

        name_symbol
    }

    /// Get the number of unique styles.
    pub fn len(&self) -> usize {
        self.styles.values().map(Vec::len).sum()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Drain all styles into KFX fragments.
    ///
    /// Returns a list of (style_name, IonValue) pairs for building style entities.
    pub fn drain_to_ion(&mut self) -> Vec<(String, IonValue)> {
        let mut result = Vec::new();

        // First, add the default style
        let default_ion = IonValue::Struct(vec![(
            KfxSymbol::StyleName as u64,
            IonValue::Symbol(self.default_style_symbol),
        )]);
        result.push(("s0".to_string(), default_ion));

        // Then all registered styles. Sort by style_id so the emitted fragment
        // order is deterministic across runs (HashMap drain order is not),
        // which keeps KFX output byte-for-byte reproducible.
        let mut styles: Vec<(u64, u64, ComputedStyle)> =
            self.styles.drain().flat_map(|(_, v)| v).collect();
        styles.sort_by_key(|(style_id, _, _)| *style_id);
        for (style_id, name_symbol, style) in styles {
            let ion = style.to_ion(name_symbol);
            // Use style_id for the fragment name (e.g., "s1", "s2", "sA")
            let name = format!("s{:X}", style_id);
            result.push((name, ion));
        }

        result
    }

    /// Get all styles without draining.
    pub fn styles(&self) -> impl Iterator<Item = (&u64, &ComputedStyle)> {
        self.styles
            .values()
            .flatten()
            .map(|(id, _, style)| (id, style))
    }
}

impl Default for StyleRegistry {
    fn default() -> Self {
        Self::new(0)
    }
}

// ============================================================================
// Style Builder
// ============================================================================

/// Builds a ComputedStyle from IR style properties using the schema.
pub struct StyleBuilder<'a> {
    schema: &'a StyleSchema,
    style: ComputedStyle,
}

impl<'a> StyleBuilder<'a> {
    /// Create a new style builder.
    pub fn new(schema: &'a StyleSchema) -> Self {
        Self {
            schema,
            style: ComputedStyle::new(),
        }
    }

    /// Apply a single (non-shorthand) property.
    fn apply_single(&mut self, property: &str, value: &str) {
        for rule in self.schema.get(property) {
            if let Some(kfx_value) = rule.transform.apply(value) {
                self.style.set(rule.kfx_symbol, kfx_value);
            }
        }
    }

    /// Ingest an IR ComputedStyle through the schema pipeline.
    ///
    /// This is the **single source of truth** for converting IR styles to KFX.
    /// The schema drives which properties to extract and how to transform them:
    /// 1. Iterate over schema rules that have IR field mappings
    /// 2. Extract CSS string value from IR struct via `extract_ir_field()`
    /// 3. Apply schema transform to convert CSS → KFX
    ///
    /// Adding new properties only requires:
    /// 1. Add variant to `IrField` enum
    /// 2. Add extraction case to `extract_ir_field()`
    /// 3. Add schema rule with `ir_field: Some(IrField::NewField)`
    pub fn ingest_ir_style(&mut self, ir_style: &ir_style::ComputedStyle) -> &mut Self {
        // Iterate over all schema rules that have IR field mappings
        for rule in self.schema.ir_mapped_rules() {
            if let Some(ir_field) = rule.ir_field {
                // Extract CSS string from IR struct (returns None for default values)
                if let Some(css_value) = extract_ir_field(ir_style, ir_field) {
                    // Apply schema transform to convert CSS → KFX
                    self.apply_single(rule.ir_key, &css_value);
                }
            }
        }

        self
    }

    /// Build the final computed style.
    pub fn build(self) -> ComputedStyle {
        self.style
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn test_computed_style_hash_consistency() {
        let mut style1 = ComputedStyle::new();
        style1.set(KfxSymbol::FontWeight, KfxValue::Integer(700));
        style1.set(KfxSymbol::FontStyle, KfxValue::Integer(1));

        let mut style2 = ComputedStyle::new();
        style2.set(KfxSymbol::FontStyle, KfxValue::Integer(1));
        style2.set(KfxSymbol::FontWeight, KfxValue::Integer(700));

        // Order shouldn't matter
        assert_eq!(style1.compute_hash(), style2.compute_hash());
    }

    #[test]
    fn test_computed_style_hash_difference() {
        let mut style1 = ComputedStyle::new();
        style1.set(KfxSymbol::FontWeight, KfxValue::Integer(700));

        let mut style2 = ComputedStyle::new();
        style2.set(KfxSymbol::FontWeight, KfxValue::Integer(400));

        assert_ne!(style1.compute_hash(), style2.compute_hash());
    }

    #[test]
    fn test_style_registry_deduplication() {
        let mut symbols = crate::kfx::context::SymbolTable::new();
        let default_sym = symbols.get_or_intern("s0");
        let mut registry = StyleRegistry::new(default_sym);

        let mut style1 = ComputedStyle::new();
        style1.set(KfxSymbol::FontWeight, KfxValue::Integer(700));

        let mut style2 = ComputedStyle::new();
        style2.set(KfxSymbol::FontWeight, KfxValue::Integer(700));

        let id1 = registry.register(style1, &mut symbols);
        let id2 = registry.register(style2, &mut symbols);

        // Same style should get same ID
        assert_eq!(id1, id2);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_orphans_widows_merge_into_one_struct() {
        use crate::kfx::style_schema::StyleSchema;

        let mut ir = crate::style::ComputedStyle::default();
        ir.orphans = 3;
        ir.widows = 4;

        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);
        let style = builder.build();

        // Both must survive in a single keep_lines_together value; before the
        // merge fix, whichever rule ran last silently clobbered the other.
        match style.get(KfxSymbol::KeepLinesTogether) {
            Some(KfxValue::StructFields(fields)) => {
                assert_eq!(
                    fields.as_slice(),
                    &[(KfxSymbol::First, 3), (KfxSymbol::Last, 4),]
                );
            }
            other => panic!("expected merged StructFields, got {other:?}"),
        }
    }

    #[test]
    fn test_underline_style_overrides_plain_underline() {
        use crate::kfx::style_schema::StyleSchema;

        // Flag only -> solid underline.
        let mut ir = crate::style::ComputedStyle::default();
        ir.text_decoration_underline = true;
        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);
        assert_eq!(
            builder.build().get(KfxSymbol::Underline),
            Some(&KfxValue::Symbol(KfxSymbol::Solid))
        );

        // Flag + style -> the specific style wins, deterministically.
        let mut ir = crate::style::ComputedStyle::default();
        ir.text_decoration_underline = true;
        ir.underline_style = crate::style::DecorationStyle::Dotted;
        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);
        assert_eq!(
            builder.build().get(KfxSymbol::Underline),
            Some(&KfxValue::Symbol(KfxSymbol::Dotted))
        );

        // Style without the flag draws nothing in CSS -> no KFX underline.
        let mut ir = crate::style::ComputedStyle::default();
        ir.underline_style = crate::style::DecorationStyle::Dotted;
        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);
        assert_eq!(builder.build().get(KfxSymbol::Underline), None);
    }

    #[test]
    fn test_font_size_preserves_css_unit() {
        use crate::kfx::style_schema::StyleSchema;

        // 24px must stay 24px — the old Dimensioned{Rem} transform relabeled
        // it as 24rem (~38x too large on device).
        let mut ir = crate::style::ComputedStyle::default();
        ir.font_size = crate::style::Length::Px(24.0);
        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);
        match builder.build().get(KfxSymbol::FontSize) {
            Some(KfxValue::Dimensioned { value, unit }) => {
                assert_eq!(*value, 24.0);
                assert_eq!(*unit, KfxSymbol::Px);
            }
            other => panic!("expected dimensioned font-size, got {other:?}"),
        }

        let mut ir = crate::style::ComputedStyle::default();
        ir.font_size = crate::style::Length::Em(0.833);
        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);
        match builder.build().get(KfxSymbol::FontSize) {
            Some(KfxValue::Dimensioned { unit, .. }) => assert_eq!(*unit, KfxSymbol::Em),
            other => panic!("expected dimensioned font-size, got {other:?}"),
        }
    }

    #[test]
    fn test_transparent_background_is_omitted() {
        let mut ir = crate::style::ComputedStyle::default();
        ir.background_color = Some(crate::style::Color::TRANSPARENT);
        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);
        let style = builder.build();
        // `transparent` used to export as opaque black fill/text background.
        assert!(style.get(KfxSymbol::FillColor).is_none());
        assert!(style.get(KfxSymbol::TextBackgroundColor).is_none());
    }

    #[test]
    fn test_ingest_is_deterministic_registration_order() {
        use crate::kfx::style_schema::StyleSchema;

        let mut ir = crate::style::ComputedStyle::default();
        ir.text_decoration_underline = true;
        ir.orphans = 2;
        ir.widows = 3;
        ir.font_size = crate::style::Length::Px(12.0);
        ir.background_color = Some(crate::style::Color::rgb(1, 2, 3));

        let build = || {
            let mut b = StyleBuilder::new(StyleSchema::standard());
            b.ingest_ir_style(&ir);
            b.build()
        };
        let a = build();
        let b = build();
        // Property *order* must match, not just the set: emitted Ion structs
        // follow this order and KFX output must be byte-reproducible.
        assert_eq!(a.iter().collect::<Vec<_>>(), b.iter().collect::<Vec<_>>());
    }

    #[test]
    fn test_style_builder() {
        let mut ir = crate::style::ComputedStyle::default();
        ir.font_weight = crate::style::FontWeight::BOLD;
        ir.font_style = crate::style::FontStyle::Italic;

        let mut builder = StyleBuilder::new(StyleSchema::standard());
        builder.ingest_ir_style(&ir);

        let style = builder.build();
        assert!(style.get(KfxSymbol::FontWeight).is_some());
        assert!(style.get(KfxSymbol::FontStyle).is_some());
    }
}
