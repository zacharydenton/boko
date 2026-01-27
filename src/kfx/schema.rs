//! KFX Schema: The instruction set for bidirectional KFX ↔ IR conversion.
//!
//! This module treats KFX symbols as **opcodes** and defines strategies for each.
//! The schema is the single source of truth for both import and export.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         Schema                                  │
//! │  ┌─────────────────┐          ┌─────────────────┐              │
//! │  │   import_table  │          │  export_table   │              │
//! │  │  (ID → Strategy)│          │ (Role → ID)     │              │
//! │  └─────────────────┘          └─────────────────┘              │
//! └─────────────────────────────────────────────────────────────────┘
//!              │                            │
//!              ▼                            ▼
//!     ┌─────────────────┐          ┌─────────────────┐
//!     │ Import: KFX→IR  │          │ Export: IR→KFX  │
//!     │  (interpreter)  │          │    (future)     │
//!     └─────────────────┘          └─────────────────┘
//! ```
//!
//! ## Key Design Principles
//!
//! 1. **Declarative Truth**: All mapping logic lives in this schema
//! 2. **Generic Interpreter**: The parser only knows Strategy, not semantics
//! 3. **Bidirectional**: Every import rule has export metadata
//! 4. **Transformers**: Complex value conversions are encapsulated in traits

use crate::book::LandmarkType;
use crate::ir::{ComputedStyle, FontStyle, FontWeight, Role};
use crate::kfx::symbols::KfxSymbol;
use crate::kfx::transforms::{
    AttributeTransform, IdentityTransform, KfxLinkTransform, ResourceTransform,
};
use std::collections::HashMap;

// ============================================================================
// Strategy: The "Opcode" definitions
// ============================================================================

/// Strategy tells the interpreter what to do when encountering a KFX symbol.
///
/// This is the "assembly language" for format conversion.
#[derive(Clone, Debug)]
pub enum Strategy {
    /// Create a new structural node in the IR tree.
    ///
    /// Usage: Paragraphs, containers, lists, tables.
    Structure {
        /// The IR Role to assign.
        role: Role,
        /// KFX type symbol for export.
        kfx_type: KfxSymbol,
    },

    /// Create a structural node where the role depends on an attribute value.
    ///
    /// Usage: Text elements that become Heading when heading_level is present.
    StructureWithModifier {
        /// Default role if modifier is not present.
        default_role: Role,
        /// Attribute that modifies the role (e.g., heading_level).
        modifier_attr: KfxSymbol,
        /// How to transform the role based on attribute value.
        modifier_effect: ModifierEffect,
        /// KFX type symbol for export.
        kfx_type: KfxSymbol,
    },

    /// Apply a style modifier without creating a new node.
    ///
    /// Usage: Bold, italic wrappers in style_events.
    Style {
        /// The style modification to apply.
        modifier: StyleModifier,
        /// KFX type symbol for export.
        kfx_type: KfxSymbol,
    },

    /// Dynamic role based on attribute presence.
    ///
    /// Usage: Format spans that become Link when link_to is present.
    Dynamic {
        /// Default role if trigger attribute is absent.
        default_role: Role,
        /// Attribute that triggers role change.
        trigger_attr: KfxSymbol,
        /// Role to use when trigger attribute is present.
        trigger_role: Role,
        /// KFX type symbol for export.
        kfx_type: KfxSymbol,
    },

    /// Pass-through: process children without creating a node.
    ///
    /// Usage: Wrapper elements that have no semantic meaning.
    Transparent {
        /// KFX type symbol for export.
        kfx_type: KfxSymbol,
    },
}

impl Strategy {
    /// Get the KFX type symbol for export.
    pub fn kfx_type(&self) -> KfxSymbol {
        match self {
            Strategy::Structure { kfx_type, .. } => *kfx_type,
            Strategy::StructureWithModifier { kfx_type, .. } => *kfx_type,
            Strategy::Style { kfx_type, .. } => *kfx_type,
            Strategy::Dynamic { kfx_type, .. } => *kfx_type,
            Strategy::Transparent { kfx_type } => *kfx_type,
        }
    }
}

/// How a modifier attribute affects the role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierEffect {
    /// Attribute value becomes heading level: Role::Heading(value).
    HeadingLevel,
    /// Attribute presence switches to ordered list.
    ListOrdered,
}

/// Style modifications that can be applied without creating nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleModifier {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Superscript,
    Subscript,
}

impl StyleModifier {
    /// Apply this modifier to a ComputedStyle.
    pub fn apply(&self, style: &mut ComputedStyle) {
        match self {
            StyleModifier::Bold => style.font_weight = FontWeight::BOLD,
            StyleModifier::Italic => style.font_style = FontStyle::Italic,
            StyleModifier::Underline => style.text_decoration_underline = true,
            StyleModifier::Strikethrough => style.text_decoration_line_through = true,
            StyleModifier::Superscript => style.vertical_align_super = true,
            StyleModifier::Subscript => style.vertical_align_sub = true,
        }
    }
}

/// Semantic attribute targets for attribute mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticTarget {
    /// `semantics.href` (for links)
    Href,
    /// `semantics.src` (for images)
    Src,
    /// `semantics.alt` (for images)
    Alt,
    /// `semantics.id` (for anchors)
    Id,
}

// ============================================================================
// Attribute Extraction Rules
// ============================================================================

/// Rule for extracting an attribute from KFX to IR semantics.
#[derive(Clone, Debug)]
pub struct AttrRule {
    /// KFX field to read.
    pub kfx_field: KfxSymbol,
    /// IR semantic target.
    pub target: SemanticTarget,
    /// Value transformer for bidirectional conversion.
    pub transform: Box<dyn AttributeTransform>,
}

impl AttrRule {
    /// Create a new attribute rule with the identity transform.
    pub fn new(kfx_field: KfxSymbol, target: SemanticTarget) -> Self {
        Self {
            kfx_field,
            target,
            transform: Box::new(IdentityTransform),
        }
    }

    /// Create a new attribute rule with a custom transform.
    pub fn with_transform(
        kfx_field: KfxSymbol,
        target: SemanticTarget,
        transform: Box<dyn AttributeTransform>,
    ) -> Self {
        Self {
            kfx_field,
            target,
            transform,
        }
    }
}

// ============================================================================
// Schema Registry
// ============================================================================

/// The KFX schema registry with bidirectional lookup tables.
pub struct KfxSchema {
    /// Import: KFX symbol ID → Strategy
    import_table: HashMap<u32, Strategy>,
    /// Import: Attribute extraction rules per symbol ID
    attr_rules: HashMap<u32, Vec<AttrRule>>,
    /// Export: IR Role → KFX symbol ID (for structural strategies)
    export_role_table: HashMap<Role, u32>,
    /// Export: IR Role → Strategy (for complete export info)
    export_strategy_table: HashMap<Role, Strategy>,
    /// Span rules: for style_events (checked in order)
    span_rules: Vec<SpanRule>,
    /// Bidirectional landmark mapping: (IR LandmarkType, KFX symbol)
    landmark_mapping: Vec<(LandmarkType, KfxSymbol)>,
}

/// Rule for interpreting style_event spans.
#[derive(Clone, Debug)]
pub struct SpanRule {
    /// Attribute that identifies this span type.
    pub indicator: KfxSymbol,
    /// Strategy to apply.
    pub strategy: Strategy,
    /// Attribute rules for this span.
    pub attr_rules: Vec<AttrRule>,
}

impl KfxSchema {
    /// Create a new schema with all mapping rules.
    pub fn new() -> Self {
        let mut schema = Self {
            import_table: HashMap::new(),
            attr_rules: HashMap::new(),
            export_role_table: HashMap::new(),
            export_strategy_table: HashMap::new(),
            span_rules: Vec::new(),
            landmark_mapping: Vec::new(),
        };
        schema.register_element_rules();
        schema.register_span_rules();
        schema.register_landmark_rules();
        schema
    }

    /// Register element (block-level) rules.
    fn register_element_rules(&mut self) {
        // Text: becomes Paragraph or Heading based on heading_level
        self.register_element(
            KfxSymbol::Text,
            Strategy::StructureWithModifier {
                default_role: Role::Paragraph,
                modifier_attr: KfxSymbol::YjSemanticsHeadingLevel,
                modifier_effect: ModifierEffect::HeadingLevel,
                kfx_type: KfxSymbol::Text,
            },
            vec![],
        );

        // Also register Role::Text directly for export (IR text nodes)
        // This ensures text leaf nodes get type: text in the output
        self.export_strategy_table.insert(
            Role::Text,
            Strategy::Structure {
                role: Role::Text,
                kfx_type: KfxSymbol::Text,
            },
        );

        // Register Role::Inline for inline spans (default inline wrapper)
        self.export_strategy_table.insert(
            Role::Inline,
            Strategy::Structure {
                role: Role::Inline,
                kfx_type: KfxSymbol::Text, // Inline content uses text type
            },
        );

        // Register Role::Link for hyperlinks
        self.export_strategy_table.insert(
            Role::Link,
            Strategy::Structure {
                role: Role::Link,
                kfx_type: KfxSymbol::Text, // Links are text type with link_to attr
            },
        );

        // Container - maps to type: text (not type: container)
        // In KFX, type: container is only for special layout containers with
        // layout/fixed_width/fit_width attributes. Regular structural elements
        // with children use type: text.
        self.register_element(
            KfxSymbol::Container,
            Strategy::Structure {
                role: Role::Container,
                kfx_type: KfxSymbol::Text,
            },
            vec![],
        );

        // Image with src and alt attributes
        self.register_element(
            KfxSymbol::Image,
            Strategy::Structure {
                role: Role::Image,
                kfx_type: KfxSymbol::Image,
            },
            vec![
                AttrRule::with_transform(
                    KfxSymbol::ResourceName,
                    SemanticTarget::Src,
                    Box::new(ResourceTransform),
                ),
                AttrRule::new(KfxSymbol::AltText, SemanticTarget::Alt),
            ],
        );

        // List: UnorderedList by default, OrderedList if list_style: numeric
        self.register_element(
            KfxSymbol::List,
            Strategy::StructureWithModifier {
                default_role: Role::UnorderedList,
                modifier_attr: KfxSymbol::ListStyle,
                modifier_effect: ModifierEffect::ListOrdered,
                kfx_type: KfxSymbol::List,
            },
            vec![],
        );

        // Also register OrderedList for export (same KFX type, but with list_style)
        self.export_strategy_table.insert(
            Role::OrderedList,
            Strategy::StructureWithModifier {
                default_role: Role::UnorderedList,
                modifier_attr: KfxSymbol::ListStyle,
                modifier_effect: ModifierEffect::ListOrdered,
                kfx_type: KfxSymbol::List,
            },
        );

        // List item
        self.register_element(
            KfxSymbol::Listitem,
            Strategy::Structure {
                role: Role::ListItem,
                kfx_type: KfxSymbol::Listitem,
            },
            vec![],
        );

        // Table elements
        self.register_element(
            KfxSymbol::Table,
            Strategy::Structure {
                role: Role::Table,
                kfx_type: KfxSymbol::Table,
            },
            vec![],
        );
        self.register_element(
            KfxSymbol::TableRow,
            Strategy::Structure {
                role: Role::TableRow,
                kfx_type: KfxSymbol::TableRow,
            },
            vec![],
        );

        // Sidebar
        self.register_element(
            KfxSymbol::Sidebar,
            Strategy::Structure {
                role: Role::Sidebar,
                kfx_type: KfxSymbol::Sidebar,
            },
            vec![],
        );

        // Horizontal rule
        self.register_element(
            KfxSymbol::HorizontalRule,
            Strategy::Structure {
                role: Role::Rule,
                kfx_type: KfxSymbol::HorizontalRule,
            },
            vec![],
        );
    }

    /// Register span (inline) rules for style_events.
    fn register_span_rules(&mut self) {
        // Link: detected by presence of link_to field
        // Uses KfxLinkTransform for kindle:pos:fid:... parsing
        self.span_rules.push(SpanRule {
            indicator: KfxSymbol::LinkTo,
            strategy: Strategy::Dynamic {
                default_role: Role::Inline,
                trigger_attr: KfxSymbol::LinkTo,
                trigger_role: Role::Link,
                kfx_type: KfxSymbol::LinkTo, // For export
            },
            attr_rules: vec![AttrRule::with_transform(
                KfxSymbol::LinkTo,
                SemanticTarget::Href,
                Box::new(KfxLinkTransform),
            )],
        });

        // Note: Additional span rules (emphasis, strong) would check style
        // definitions. For now, anything without link_to becomes Inline.
    }

    /// Register landmark type mappings.
    fn register_landmark_rules(&mut self) {
        self.landmark_mapping = vec![
            (LandmarkType::Cover, KfxSymbol::CoverPage),
            (LandmarkType::StartReading, KfxSymbol::Srl),
            (LandmarkType::TitlePage, KfxSymbol::Titlepage),
            (LandmarkType::Toc, KfxSymbol::Toc),
            (LandmarkType::BodyMatter, KfxSymbol::Bodymatter),
            (LandmarkType::FrontMatter, KfxSymbol::Frontmatter),
            (LandmarkType::BackMatter, KfxSymbol::Backmatter),
            (LandmarkType::Acknowledgements, KfxSymbol::Acknowledgements),
            (LandmarkType::Preface, KfxSymbol::Preface),
            (LandmarkType::Bibliography, KfxSymbol::Bibliography),
            (LandmarkType::Glossary, KfxSymbol::Glossary),
            (LandmarkType::Index, KfxSymbol::Index),
            (LandmarkType::Loi, KfxSymbol::Loi),
            (LandmarkType::Lot, KfxSymbol::Lot),
            // Note: LandmarkType::Endnotes has no direct KFX equivalent
        ];
    }

    /// Register an element rule with attributes.
    fn register_element(
        &mut self,
        symbol: KfxSymbol,
        strategy: Strategy,
        attr_rules: Vec<AttrRule>,
    ) {
        let id = symbol as u32;
        self.import_table.insert(id, strategy.clone());

        if !attr_rules.is_empty() {
            self.attr_rules.insert(id, attr_rules);
        }

        // Register export mappings
        match &strategy {
            Strategy::Structure { role, .. } => {
                self.export_role_table.insert(*role, id);
                self.export_strategy_table.insert(*role, strategy.clone());
            }
            Strategy::StructureWithModifier {
                default_role,
                modifier_effect,
                ..
            } => {
                self.export_role_table.insert(*default_role, id);
                self.export_strategy_table
                    .insert(*default_role, strategy.clone());

                // Also register modified roles for export
                if *modifier_effect == ModifierEffect::HeadingLevel {
                    for level in 1..=6 {
                        self.export_role_table.insert(Role::Heading(level), id);
                        self.export_strategy_table
                            .insert(Role::Heading(level), strategy.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // =========================================================================
    // Import API
    // =========================================================================

    /// Get the strategy for a KFX element type.
    pub fn element_strategy(&self, kfx_type_id: u32) -> Option<&Strategy> {
        self.import_table.get(&kfx_type_id)
    }

    /// Get attribute rules for a KFX element type.
    pub fn element_attr_rules(&self, kfx_type_id: u32) -> &[AttrRule] {
        self.attr_rules
            .get(&kfx_type_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Resolve a KFX element to IR Role.
    ///
    /// # Arguments
    /// * `kfx_type_id` - The KFX type symbol ID.
    /// * `get_attr` - Closure to look up attribute values.
    pub fn resolve_element_role<F>(&self, kfx_type_id: u32, get_attr: F) -> Role
    where
        F: Fn(KfxSymbol) -> Option<i64>,
    {
        let strategy = match self.element_strategy(kfx_type_id) {
            Some(s) => s,
            None => return Role::Container, // default
        };

        self.execute_strategy_for_role(strategy, get_attr)
    }

    /// Find the matching span rule for a style_event.
    ///
    /// # Arguments
    /// * `has_field` - Closure that returns true if the span has a given field.
    pub fn span_rule<F>(&self, has_field: F) -> Option<&SpanRule>
    where
        F: Fn(KfxSymbol) -> bool,
    {
        self.span_rules.iter().find(|rule| has_field(rule.indicator))
    }

    /// Resolve a style_event to IR Role.
    pub fn resolve_span_role<F>(&self, has_field: F) -> Role
    where
        F: Fn(KfxSymbol) -> bool,
    {
        match self.span_rule(&has_field) {
            Some(rule) => {
                // Convert has_field to get_attr: if field exists, return Some(1)
                // This allows Dynamic strategies to detect attribute presence
                self.execute_strategy_for_role(&rule.strategy, |sym| {
                    if has_field(sym) {
                        Some(1)
                    } else {
                        None
                    }
                })
            }
            None => Role::Inline, // default span role
        }
    }

    /// Get attribute rules for a span.
    pub fn span_attr_rules<F>(&self, has_field: F) -> &[AttrRule]
    where
        F: Fn(KfxSymbol) -> bool,
    {
        match self.span_rule(has_field) {
            Some(rule) => &rule.attr_rules,
            None => &[],
        }
    }

    /// Execute a strategy to determine the Role.
    fn execute_strategy_for_role<F>(&self, strategy: &Strategy, get_attr: F) -> Role
    where
        F: Fn(KfxSymbol) -> Option<i64>,
    {
        match strategy {
            Strategy::Structure { role, .. } => *role,

            Strategy::StructureWithModifier {
                default_role,
                modifier_attr,
                modifier_effect,
                ..
            } => {
                if let Some(value) = get_attr(*modifier_attr) {
                    match modifier_effect {
                        ModifierEffect::HeadingLevel => Role::Heading(value as u8),
                        ModifierEffect::ListOrdered => {
                            // list_style is a symbol; check for numeric (343)
                            if value == KfxSymbol::Numeric as i64 {
                                Role::OrderedList
                            } else {
                                *default_role
                            }
                        }
                    }
                } else {
                    *default_role
                }
            }

            Strategy::Dynamic {
                default_role,
                trigger_attr,
                trigger_role,
                ..
            } => {
                // For Dynamic, check if trigger attribute exists
                if get_attr(*trigger_attr).is_some() {
                    *trigger_role
                } else {
                    *default_role
                }
            }

            Strategy::Style { .. } => Role::Inline, // Style strategies don't create structure
            Strategy::Transparent { .. } => Role::Container,
        }
    }

    // =========================================================================
    // Export API
    // =========================================================================

    /// Find the KFX symbol ID for an IR Role.
    pub fn kfx_symbol_for_role(&self, role: Role) -> Option<u32> {
        self.export_role_table.get(&role).copied()
    }

    /// Get the strategy for exporting an IR Role.
    pub fn export_strategy(&self, role: Role) -> Option<&Strategy> {
        self.export_strategy_table.get(&role)
    }

    /// Get the KFX type symbol for an IR Role.
    pub fn kfx_type_for_role(&self, role: Role) -> Option<KfxSymbol> {
        self.export_strategy(role).map(|s| s.kfx_type())
    }

    /// Export attributes for a role using registered transforms.
    ///
    /// This is the schema-driven way to export attributes. Instead of hardcoding
    /// attribute extraction, call this method to get properly transformed KFX
    /// attribute values.
    ///
    /// # Arguments
    /// * `role` - The IR role being exported
    /// * `get_semantic` - Closure to get semantic values by target
    /// * `export_ctx` - Export context for transformations (spine map, etc.)
    ///
    /// # Returns
    /// Vector of (KFX field ID, transformed string value) pairs
    pub fn export_attributes<F>(
        &self,
        role: Role,
        get_semantic: F,
        export_ctx: &crate::kfx::transforms::ExportContext,
    ) -> Vec<(u64, String)>
    where
        F: Fn(SemanticTarget) -> Option<String>,
    {
        let mut attrs = Vec::new();

        // Find the KFX type ID for this role
        let kfx_type_id = match self.kfx_symbol_for_role(role) {
            Some(id) => id,
            None => return attrs,
        };

        // Apply attribute rules in reverse (IR → KFX)
        for rule in self.element_attr_rules(kfx_type_id) {
            if let Some(value) = get_semantic(rule.target) {
                // Wrap as ParsedAttribute::String for transformation
                let parsed = crate::kfx::transforms::ParsedAttribute::String(value);

                // Apply transformer's export direction
                let kfx_value = rule.transform.export(&parsed, export_ctx);

                attrs.push((rule.kfx_field as u64, kfx_value));
            }
        }

        attrs
    }

    /// Check if a role should be treated as an inline span during export.
    ///
    /// Inline spans are rendered as style_events in KFX, not as nested containers.
    /// This includes: Link, Inline (for bold/italic spans).
    pub fn is_inline_role(&self, role: Role) -> bool {
        matches!(role, Role::Link | Role::Inline)
    }

    /// Export span attributes for an inline role.
    ///
    /// Similar to export_attributes but uses span rules instead of element rules.
    /// Used when generating style_events for inline spans.
    pub fn export_span_attributes<F>(
        &self,
        role: Role,
        get_semantic: F,
        export_ctx: &crate::kfx::transforms::ExportContext,
    ) -> Vec<(u64, String)>
    where
        F: Fn(SemanticTarget) -> Option<String>,
    {
        let mut attrs = Vec::new();

        // Find the matching span rule for this role
        // For export, we match by examining the strategy's trigger_role or role
        for span_rule in &self.span_rules {
            let rule_matches = match &span_rule.strategy {
                Strategy::Dynamic { trigger_role, .. } => *trigger_role == role,
                Strategy::Structure { role: r, .. } => *r == role,
                Strategy::StructureWithModifier { default_role, .. } => *default_role == role,
                Strategy::Style { .. } => role == Role::Inline,
                Strategy::Transparent { .. } => false,
            };

            if rule_matches {
                // Apply attribute rules for this span type
                for attr_rule in &span_rule.attr_rules {
                    if let Some(value) = get_semantic(attr_rule.target) {
                        let parsed = crate::kfx::transforms::ParsedAttribute::String(value);
                        let kfx_value = attr_rule.transform.export(&parsed, export_ctx);
                        attrs.push((attr_rule.kfx_field as u64, kfx_value));
                    }
                }
                break;
            }
        }

        attrs
    }

    // =========================================================================
    // Landmark API
    // =========================================================================

    /// Convert a KFX landmark symbol ID to IR LandmarkType.
    pub fn landmark_from_kfx(&self, symbol_id: u64) -> Option<LandmarkType> {
        self.landmark_mapping
            .iter()
            .find(|(_, kfx)| *kfx as u64 == symbol_id)
            .map(|(ir, _)| *ir)
    }

    /// Convert an IR LandmarkType to KFX symbol.
    pub fn landmark_to_kfx(&self, landmark_type: LandmarkType) -> Option<KfxSymbol> {
        self.landmark_mapping
            .iter()
            .find(|(ir, _)| *ir == landmark_type)
            .map(|(_, kfx)| *kfx)
    }
}

impl Default for KfxSchema {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Global Schema Instance
// ============================================================================

static SCHEMA: std::sync::OnceLock<KfxSchema> = std::sync::OnceLock::new();

/// Get the global KFX schema.
pub fn schema() -> &'static KfxSchema {
    SCHEMA.get_or_init(KfxSchema::new)
}

// ============================================================================
// Default Roles
// ============================================================================

/// Default element role when no rule matches.
pub const DEFAULT_ELEMENT_ROLE: Role = Role::Container;

/// Default span role when no rule matches.
pub const DEFAULT_SPAN_ROLE: Role = Role::Inline;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_paragraph() {
        let schema = KfxSchema::new();
        let role = schema.resolve_element_role(KfxSymbol::Text as u32, |_| None);
        assert_eq!(role, Role::Paragraph);
    }

    #[test]
    fn test_resolve_heading_with_modifier() {
        let schema = KfxSchema::new();
        let role = schema.resolve_element_role(KfxSymbol::Text as u32, |field| {
            if field == KfxSymbol::YjSemanticsHeadingLevel {
                Some(2)
            } else {
                None
            }
        });
        assert_eq!(role, Role::Heading(2));
    }

    #[test]
    fn test_resolve_image() {
        let schema = KfxSchema::new();
        let role = schema.resolve_element_role(KfxSymbol::Image as u32, |_| None);
        assert_eq!(role, Role::Image);

        let attrs = schema.element_attr_rules(KfxSymbol::Image as u32);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].kfx_field, KfxSymbol::ResourceName);
        assert_eq!(attrs[0].target, SemanticTarget::Src);
        assert_eq!(attrs[1].kfx_field, KfxSymbol::AltText);
        assert_eq!(attrs[1].target, SemanticTarget::Alt);
    }

    #[test]
    fn test_resolve_unknown_element() {
        let schema = KfxSchema::new();
        let role = schema.resolve_element_role(9999, |_| None);
        assert_eq!(role, DEFAULT_ELEMENT_ROLE);
    }

    #[test]
    fn test_resolve_link_span() {
        let schema = KfxSchema::new();
        let role = schema.resolve_span_role(|field| field == KfxSymbol::LinkTo);
        assert_eq!(role, Role::Link);

        let attrs = schema.span_attr_rules(|field| field == KfxSymbol::LinkTo);
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].target, SemanticTarget::Href);
    }

    #[test]
    fn test_resolve_generic_span() {
        let schema = KfxSchema::new();
        let role = schema.resolve_span_role(|_| false);
        assert_eq!(role, DEFAULT_SPAN_ROLE);
    }

    #[test]
    fn test_export_lookup_paragraph() {
        let schema = KfxSchema::new();
        assert_eq!(
            schema.kfx_symbol_for_role(Role::Paragraph),
            Some(KfxSymbol::Text as u32)
        );
    }

    #[test]
    fn test_export_lookup_heading() {
        let schema = KfxSchema::new();
        // All heading levels should map to Text
        for level in 1..=6 {
            assert_eq!(
                schema.kfx_symbol_for_role(Role::Heading(level)),
                Some(KfxSymbol::Text as u32)
            );
        }
    }

    #[test]
    fn test_export_lookup_image() {
        let schema = KfxSchema::new();
        assert_eq!(
            schema.kfx_symbol_for_role(Role::Image),
            Some(KfxSymbol::Image as u32)
        );
    }

    #[test]
    fn test_export_strategy_includes_kfx_type() {
        let schema = KfxSchema::new();
        let strategy = schema.export_strategy(Role::Paragraph).unwrap();
        assert_eq!(strategy.kfx_type(), KfxSymbol::Text);
    }

    #[test]
    fn test_style_modifier_apply() {
        let mut style = ComputedStyle::default();
        StyleModifier::Bold.apply(&mut style);
        assert!(style.is_bold());

        let mut style = ComputedStyle::default();
        StyleModifier::Italic.apply(&mut style);
        assert!(style.is_italic());
    }

    #[test]
    fn test_landmark_from_kfx() {
        let s = schema();
        assert_eq!(
            s.landmark_from_kfx(KfxSymbol::CoverPage as u64),
            Some(LandmarkType::Cover)
        );
        assert_eq!(
            s.landmark_from_kfx(KfxSymbol::Srl as u64),
            Some(LandmarkType::StartReading)
        );
        assert_eq!(
            s.landmark_from_kfx(KfxSymbol::Bodymatter as u64),
            Some(LandmarkType::BodyMatter)
        );
        assert_eq!(s.landmark_from_kfx(9999), None);
    }

    #[test]
    fn test_landmark_to_kfx() {
        let s = schema();
        assert_eq!(
            s.landmark_to_kfx(LandmarkType::Cover),
            Some(KfxSymbol::CoverPage)
        );
        assert_eq!(
            s.landmark_to_kfx(LandmarkType::StartReading),
            Some(KfxSymbol::Srl)
        );
        assert_eq!(
            s.landmark_to_kfx(LandmarkType::BodyMatter),
            Some(KfxSymbol::Bodymatter)
        );
        // Endnotes has no KFX equivalent
        assert_eq!(s.landmark_to_kfx(LandmarkType::Endnotes), None);
    }

    #[test]
    fn test_landmark_roundtrip() {
        let s = schema();
        // All mapped landmark types should roundtrip correctly
        for (ir_type, kfx_sym) in &s.landmark_mapping {
            let kfx_id = *kfx_sym as u64;
            assert_eq!(s.landmark_from_kfx(kfx_id), Some(*ir_type));
            assert_eq!(s.landmark_to_kfx(*ir_type), Some(*kfx_sym));
        }
    }
}
