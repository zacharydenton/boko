//! Style registry for KFX export.
//!
//! Handles style deduplication and ID assignment during the two-pass export:
//! - Pass 1: Collect unique style combinations, assign IDs via hashing
//! - Pass 2: Emit style fragment with all definitions, reference by ID in content

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::ir as ir_style;
use crate::kfx::ion::IonValue;
use crate::kfx::style_schema::{KfxValue, StyleContext, StyleSchema, extract_ir_field};
use crate::kfx::symbols::KfxSymbol;

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
    pub fn set(&mut self, symbol: KfxSymbol, value: KfxValue) {
        // Remove any existing value for this symbol
        self.properties.retain(|(s, _)| *s != symbol);
        self.properties.push((symbol, value));
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

    /// Check if this style contains any block-only properties.
    pub fn has_block_properties(&self, schema: &StyleSchema) -> bool {
        for (symbol, _) in &self.properties {
            // Find the rule for this symbol
            for rule in schema.rules() {
                if rule.kfx_symbol == *symbol && rule.context == StyleContext::BlockOnly {
                    return true;
                }
            }
        }
        false
    }

    /// Check if this style contains any inline-safe properties.
    pub fn has_inline_properties(&self, schema: &StyleSchema) -> bool {
        for (symbol, _) in &self.properties {
            for rule in schema.rules() {
                if rule.kfx_symbol == *symbol && rule.context == StyleContext::InlineSafe {
                    return true;
                }
            }
        }
        false
    }

    /// Split into block and inline styles.
    ///
    /// Returns (block_style, inline_style) where each contains only
    /// properties appropriate for that context.
    pub fn split_by_context(&self, schema: &StyleSchema) -> (ComputedStyle, ComputedStyle) {
        let mut block = ComputedStyle::new();
        let mut inline = ComputedStyle::new();

        for (symbol, value) in &self.properties {
            let mut found_context = None;
            for rule in schema.rules() {
                if rule.kfx_symbol == *symbol {
                    found_context = Some(rule.context);
                    break;
                }
            }

            match found_context {
                Some(StyleContext::BlockOnly) => block.set(*symbol, value.clone()),
                Some(StyleContext::InlineSafe) => inline.set(*symbol, value.clone()),
                Some(StyleContext::Any) | None => {
                    // Properties with Any context go to both (or default to block)
                    block.set(*symbol, value.clone());
                }
            }
        }

        (block, inline)
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
    }
}

// ============================================================================
// Style Registry
// ============================================================================

/// Registry for collecting and deduplicating styles during export.
pub struct StyleRegistry {
    /// Hash -> (style_id, style_name_symbol, computed_style)
    styles: HashMap<u64, (u64, u64, ComputedStyle)>,

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

        if let Some((_, name_symbol, _)) = self.styles.get(&hash) {
            return *name_symbol;
        }

        // Assign new ID and create symbol name
        let style_id = self.next_style_id;
        self.next_style_id += 1;

        let style_name = format!("s{:X}", style_id);
        let name_symbol = symbols.get_or_intern(&style_name);

        self.styles.insert(hash, (style_id, name_symbol, style));

        name_symbol
    }

    /// Get the number of unique styles.
    pub fn len(&self) -> usize {
        self.styles.len()
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

        // Then all registered styles
        // Tuple is (style_id, name_symbol, computed_style)
        for (_, (style_id, name_symbol, style)) in self.styles.drain() {
            let ion = style.to_ion(name_symbol);
            // Use style_id for the fragment name (e.g., "s1", "s2", "sA")
            let name = format!("s{:X}", style_id);
            result.push((name, ion));
        }

        result
    }

    /// Get all styles without draining.
    pub fn styles(&self) -> impl Iterator<Item = (&u64, &ComputedStyle)> {
        self.styles.values().map(|(id, _, style)| (id, style))
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

    /// Apply a CSS property.
    pub fn apply(&mut self, property: &str, value: &str) -> &mut Self {
        // Handle shorthand properties first
        if let Some(expanded) = expand_shorthand(property, value) {
            for (prop, val) in expanded {
                self.apply_single(&prop, &val);
            }
        } else {
            self.apply_single(property, value);
        }
        self
    }

    /// Apply a single (non-shorthand) property.
    fn apply_single(&mut self, property: &str, value: &str) {
        if let Some(rule) = self.schema.get(property)
            && let Some(kfx_value) = rule.transform.apply(value) {
                self.style.set(rule.kfx_symbol, kfx_value);
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

/// Expand CSS shorthand properties into individual properties.
fn expand_shorthand(property: &str, value: &str) -> Option<Vec<(String, String)>> {
    let parts: Vec<&str> = value.split_whitespace().collect();

    match property {
        "margin" => Some(expand_box_shorthand("margin", &parts)),
        "padding" => Some(expand_box_shorthand("padding", &parts)),
        "border-width" => Some(
            expand_box_shorthand("border", &parts)
                .into_iter()
                .map(|(p, v)| (format!("{}-width", p), v))
                .collect(),
        ),
        "font" => expand_font_shorthand(value),
        _ => None,
    }
}

/// Expand a box model shorthand (margin, padding) into four individual properties.
fn expand_box_shorthand(prefix: &str, parts: &[&str]) -> Vec<(String, String)> {
    let (top, right, bottom, left) = match parts.len() {
        1 => (parts[0], parts[0], parts[0], parts[0]),
        2 => (parts[0], parts[1], parts[0], parts[1]),
        3 => (parts[0], parts[1], parts[2], parts[1]),
        4 => (parts[0], parts[1], parts[2], parts[3]),
        _ => return vec![],
    };

    vec![
        (format!("{}-top", prefix), top.to_string()),
        (format!("{}-right", prefix), right.to_string()),
        (format!("{}-bottom", prefix), bottom.to_string()),
        (format!("{}-left", prefix), left.to_string()),
    ]
}

/// Expand font shorthand (complex, partial support).
fn expand_font_shorthand(value: &str) -> Option<Vec<(String, String)>> {
    // font: [style] [weight] size[/line-height] family
    // This is complex; for now just extract what we can
    let mut result = Vec::new();
    let parts: Vec<&str> = value.split_whitespace().collect();

    for part in &parts {
        let lower = part.to_lowercase();
        if lower == "italic" || lower == "oblique" {
            result.push(("font-style".to_string(), lower));
        } else if lower == "bold" || lower == "normal" || lower == "lighter" || lower == "bolder" {
            result.push(("font-weight".to_string(), lower));
        } else if part.contains("px")
            || part.contains("em")
            || part.contains("pt")
            || part.contains('%')
        {
            // This might be size or size/line-height
            if part.contains('/') {
                let size_parts: Vec<&str> = part.split('/').collect();
                if size_parts.len() == 2 {
                    result.push(("font-size".to_string(), size_parts[0].to_string()));
                    result.push(("line-height".to_string(), size_parts[1].to_string()));
                }
            } else {
                result.push(("font-size".to_string(), part.to_string()));
            }
        }
        // Font family is harder to parse reliably, skip for now
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
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
    fn test_expand_margin_shorthand() {
        let parts = vec!["10px"];
        let expanded = expand_box_shorthand("margin", &parts);
        assert_eq!(expanded.len(), 4);
        assert_eq!(expanded[0], ("margin-top".to_string(), "10px".to_string()));

        let parts = vec!["10px", "20px"];
        let expanded = expand_box_shorthand("margin", &parts);
        assert_eq!(expanded[0].1, "10px"); // top
        assert_eq!(expanded[1].1, "20px"); // right
        assert_eq!(expanded[2].1, "10px"); // bottom
        assert_eq!(expanded[3].1, "20px"); // left
    }

    #[test]
    fn test_style_builder() {
        let schema = StyleSchema::standard();
        let mut builder = StyleBuilder::new(&schema);

        builder.apply("font-weight", "bold");
        builder.apply("font-style", "italic");

        let style = builder.build();
        assert_eq!(style.len(), 2);
        assert!(style.get(KfxSymbol::FontWeight).is_some());
        assert!(style.get(KfxSymbol::FontStyle).is_some());
    }
}
