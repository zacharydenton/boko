//! Export context for KFX generation.
//!
//! The ExportContext is the central state management for KFX export.
//! All shared state flows through this context, avoiding the pitfalls of
//! scattered symbol tables, ID collision, and orphaned references.

use std::collections::{HashMap, HashSet};

use crate::import::ChapterId;
use crate::model::{GlobalNodeId, LandmarkType, NodeId, TocEntry};
use crate::style::StyleId;

use super::style_registry::StyleRegistry;
use super::symbols::KFX_SYMBOL_TABLE_SIZE;
use super::transforms::encode_base32;

/// Symbol table for KFX export.
///
/// Maintains a mapping between strings and symbol IDs for the exported file.
/// Local symbols start after the shared YJ_symbols table.
pub struct SymbolTable {
    /// Local symbols (book-specific IDs)
    local_symbols: Vec<String>,
    /// Map from symbol name to ID
    symbol_map: HashMap<String, u64>,
    /// Next local symbol ID (starts after YJ_symbols max_id)
    next_id: u64,
}

impl SymbolTable {
    /// Local symbol IDs start here (after YJ_symbols shared table).
    pub const LOCAL_MIN_ID: u64 = KFX_SYMBOL_TABLE_SIZE as u64;

    /// Create a new empty symbol table.
    pub fn new() -> Self {
        Self {
            local_symbols: Vec::new(),
            symbol_map: HashMap::new(),
            next_id: Self::LOCAL_MIN_ID,
        }
    }

    /// Get or create a symbol ID for a name.
    ///
    /// If the name starts with `$` followed by a number, it's treated as
    /// a shared symbol reference and the number is returned directly.
    pub fn get_or_intern(&mut self, name: &str) -> u64 {
        // Check if it's a shared symbol reference (starts with $)
        if let Some(id_str) = name.strip_prefix('$')
            && let Ok(id) = id_str.parse::<u64>()
        {
            return id;
        }

        // Check if already interned
        if let Some(&id) = self.symbol_map.get(name) {
            return id;
        }

        // Create new local symbol
        let id = self.next_id;
        self.next_id += 1;
        self.local_symbols.push(name.to_string());
        self.symbol_map.insert(name.to_string(), id);
        id
    }

    /// Get symbol ID without interning (returns None if not found).
    pub fn get(&self, name: &str) -> Option<u64> {
        if let Some(id_str) = name.strip_prefix('$')
            && let Ok(id) = id_str.parse::<u64>()
        {
            return Some(id);
        }
        self.symbol_map.get(name).copied()
    }

    /// Get local symbols for $ion_symbol_table fragment.
    pub fn local_symbols(&self) -> &[String] {
        &self.local_symbols
    }

    /// Get the number of local symbols.
    pub fn len(&self) -> usize {
        self.local_symbols.len()
    }

    /// Check if the symbol table is empty.
    pub fn is_empty(&self) -> bool {
        self.local_symbols.is_empty()
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Fragment ID generator.
///
/// Generates unique IDs for fragments, starting at 200 to avoid
/// collision with system fragments (0-199 are reserved).
pub struct IdGenerator {
    next_id: u64,
}

impl IdGenerator {
    /// Fragment IDs start here (matching reference KFX format).
    pub const FRAGMENT_MIN_ID: u64 = 866;

    /// Create a new ID generator.
    pub fn new() -> Self {
        Self {
            next_id: Self::FRAGMENT_MIN_ID,
        }
    }

    /// Generate the next unique ID.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Get the current next ID without incrementing.
    pub fn peek(&self) -> u64 {
        self.next_id
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Resource registry for tracking resources (images, fonts, etc.).
#[derive(Debug)]
pub struct ResourceRegistry {
    /// href → resource symbol ID
    resources: HashMap<String, u64>,
    /// href → short resource name (e.g., "e0", "e1")
    resource_names: HashMap<String, String>,
    /// Counter for generating unique names
    next_resource_id: usize,
}

impl ResourceRegistry {
    /// Create a new empty resource registry.
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
            resource_names: HashMap::new(),
            next_resource_id: 0,
        }
    }

    /// Register a resource and get its symbol ID.
    pub fn register(&mut self, href: &str, symbols: &mut SymbolTable) -> u64 {
        if let Some(&id) = self.resources.get(href) {
            return id;
        }

        let symbol_name = format!("resource:{}", href);
        let id = symbols.get_or_intern(&symbol_name);
        self.resources.insert(href.to_string(), id);
        id
    }

    /// Get or generate a short resource name (e.g., "e0", "e1").
    ///
    /// Returns the same name for the same href on subsequent calls.
    pub fn get_or_create_name(&mut self, href: &str) -> String {
        if let Some(name) = self.resource_names.get(href) {
            return name.clone();
        }

        let name = format!("e{:X}", self.next_resource_id);
        self.next_resource_id += 1;
        self.resource_names.insert(href.to_string(), name.clone());
        name
    }

    /// Get the symbol ID for a resource (if registered).
    pub fn get(&self, href: &str) -> Option<u64> {
        self.resources.get(href).copied()
    }

    /// Get the short name for a resource (if assigned).
    pub fn get_name(&self, href: &str) -> Option<&str> {
        self.resource_names.get(href).map(|s| s.as_str())
    }

    /// Iterate over all registered resources.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &u64)> {
        self.resources.iter()
    }

    /// Get the number of resources registered.
    pub fn len(&self) -> usize {
        self.resource_names.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.resource_names.is_empty()
    }
}

impl Default for ResourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Text accumulator for content entities.
///
/// Tracks text content during export and provides offset information
/// for position maps.
#[derive(Default)]
pub struct TextAccumulator {
    /// Accumulated text segments
    segments: Vec<String>,
    /// Total accumulated length
    total_len: usize,
}

impl TextAccumulator {
    /// Create a new empty text accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push text and return the segment index.
    pub fn push(&mut self, text: &str) -> usize {
        let index = self.segments.len();
        self.total_len += text.len();
        self.segments.push(text.to_string());
        index
    }

    /// Get the total accumulated length.
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// Check if the accumulator is empty.
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Get all accumulated text segments.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Clear the accumulator and return the segments.
    pub fn drain(&mut self) -> Vec<String> {
        self.total_len = 0;
        std::mem::take(&mut self.segments)
    }
}

/// Position entry for a node: (fragment_id, byte_offset).
#[derive(Debug, Clone, Copy)]
pub struct Position {
    /// Fragment ID where this node lives.
    pub fragment_id: u64,
    /// Byte offset within the fragment's text content.
    pub offset: usize,
}

/// Resolved anchor with position information (for internal links).
#[derive(Debug, Clone)]
pub struct AnchorPosition {
    /// The anchor symbol name (e.g., "a0", "a1")
    pub symbol: String,
    /// Content fragment ID where this anchor points (for anchor.position.id)
    pub fragment_id: u64,
    /// Section's page_template ID (for position_map grouping)
    pub section_id: u64,
    /// Byte offset within the fragment (0 if at start)
    pub offset: usize,
}

/// External anchor with URI (for external links like http/https URLs).
#[derive(Debug, Clone)]
pub struct ExternalAnchor {
    /// The anchor symbol name (e.g., "a0", "a1")
    pub symbol: String,
    /// The external URI (e.g., `https://standardebooks.org/`)
    pub uri: String,
}

/// Anchor registry for link resolution in KFX export.
///
/// KFX uses indirect anchor references: links point to anchor symbols,
/// and anchor entities ($266) define where those symbols resolve to.
///
/// ## Design
///
/// The registry supports two lookup patterns:
/// - **By GlobalNodeId**: For internal targets from `ResolvedLinks`
/// - **By href string**: For link_to lookups in storyline generation
///
/// Both patterns share the same anchor symbols, ensuring consistency.
///
/// ## Example Flow
///
/// 1. ResolvedLinks says node X in chapter Y is an internal target
/// 2. Registry assigns symbol "a0" to GlobalNodeId(Y, X)
/// 3. Href "chapter.xhtml#id" is also mapped to "a0"
/// 4. During storyline gen, link_to lookup uses href to get "a0"
/// 5. During anchor creation, GlobalNodeId lookup confirms it's a target
#[derive(Debug, Default)]
pub struct AnchorRegistry {
    /// GlobalNodeId → anchor symbol name (e.g., "a0", "a1")
    node_to_symbol: HashMap<GlobalNodeId, String>,

    /// ChapterId → anchor symbol (for chapter-level targets)
    chapter_to_symbol: HashMap<ChapterId, String>,

    /// href string → anchor symbol (for link_to lookups)
    /// Populated alongside node_to_symbol for href-based access
    href_to_symbol: HashMap<String, String>,

    /// Symbols that have been resolved to positions (for deduplication)
    resolved_symbols: HashSet<String>,

    /// Resolved internal anchors ready for entity emission
    resolved: Vec<AnchorPosition>,

    /// External anchors ready for entity emission
    external_anchors: Vec<ExternalAnchor>,

    /// Counter for generating unique anchor symbols
    next_anchor_id: usize,

    /// Node positions for TOC lookup: GlobalNodeId → (content_fragment_id, offset)
    node_positions: HashMap<GlobalNodeId, (u64, usize)>,

    /// Chapter positions for TOC lookup: ChapterId → content_fragment_id
    chapter_positions: HashMap<ChapterId, u64>,
}

impl AnchorRegistry {
    /// Create a new empty anchor registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an internal link target (a node that links point to).
    ///
    /// Also registers the href for href-based lookup.
    /// Returns the anchor symbol for use in `link_to` style events.
    pub fn register_internal_target(&mut self, target: GlobalNodeId, href: &str) -> String {
        if let Some(symbol) = self.node_to_symbol.get(&target) {
            // Ensure href is also mapped
            self.href_to_symbol.insert(href.to_string(), symbol.clone());
            return symbol.clone();
        }

        let symbol = format!("a{:X}", self.next_anchor_id);
        self.next_anchor_id += 1;
        self.node_to_symbol.insert(target, symbol.clone());
        self.href_to_symbol.insert(href.to_string(), symbol.clone());
        symbol
    }

    /// Register a chapter-level link target.
    ///
    /// Also registers the href for href-based lookup.
    /// Returns the anchor symbol for use in `link_to` style events.
    pub fn register_chapter_target(&mut self, chapter: ChapterId, href: &str) -> String {
        if let Some(symbol) = self.chapter_to_symbol.get(&chapter) {
            self.href_to_symbol.insert(href.to_string(), symbol.clone());
            return symbol.clone();
        }

        let symbol = format!("a{:X}", self.next_anchor_id);
        self.next_anchor_id += 1;
        self.chapter_to_symbol.insert(chapter, symbol.clone());
        self.href_to_symbol.insert(href.to_string(), symbol.clone());
        symbol
    }

    /// Register an external link target (http/https URL).
    ///
    /// Returns the anchor symbol for use in `link_to` style events.
    pub fn register_external(&mut self, url: &str) -> String {
        if let Some(symbol) = self.href_to_symbol.get(url) {
            return symbol.clone();
        }

        let symbol = format!("a{:X}", self.next_anchor_id);
        self.next_anchor_id += 1;

        self.href_to_symbol.insert(url.to_string(), symbol.clone());
        self.external_anchors.push(ExternalAnchor {
            symbol: symbol.clone(),
            uri: url.to_string(),
        });

        symbol
    }

    /// Get the anchor symbol for an href (for link_to lookups).
    ///
    /// This is the primary lookup method used during storyline generation.
    /// Returns the symbol if the href was registered, or creates a new one.
    pub fn get_or_create_href_symbol(&mut self, href: &str) -> String {
        // Check if already registered
        if let Some(symbol) = self.href_to_symbol.get(href) {
            return symbol.clone();
        }

        // Check if this is an external link
        if href.starts_with("http://") || href.starts_with("https://") {
            return self.register_external(href);
        }

        // Unknown internal link - create a symbol but it won't have an anchor entity
        // This handles links that weren't in ResolvedLinks (shouldn't happen normally)
        let symbol = format!("a{:X}", self.next_anchor_id);
        self.next_anchor_id += 1;
        self.href_to_symbol.insert(href.to_string(), symbol.clone());
        symbol
    }

    /// Get the anchor symbol for a node target (if registered).
    pub fn get_symbol(&self, target: GlobalNodeId) -> Option<&str> {
        self.node_to_symbol.get(&target).map(|s| s.as_str())
    }

    /// Get the anchor symbol for a chapter target (if registered).
    pub fn get_chapter_symbol(&self, chapter: ChapterId) -> Option<&str> {
        self.chapter_to_symbol.get(&chapter).map(|s| s.as_str())
    }

    /// Get the anchor symbol for an href (if registered).
    pub fn get_href_symbol(&self, href: &str) -> Option<&str> {
        self.href_to_symbol.get(href).map(|s| s.as_str())
    }

    /// Check if a node is a registered internal target.
    pub fn is_internal_target(&self, target: GlobalNodeId) -> bool {
        self.node_to_symbol.contains_key(&target)
    }

    /// Check if a chapter is a registered target.
    pub fn is_chapter_target(&self, chapter: ChapterId) -> bool {
        self.chapter_to_symbol.contains_key(&chapter)
    }

    /// Create an anchor entity for a node target.
    ///
    /// Call this during Pass 2 when processing a node that's a link target.
    /// Returns the symbol if the anchor was created, None if already resolved.
    pub fn create_anchor(
        &mut self,
        target: GlobalNodeId,
        content_fragment_id: u64,
        section_id: u64,
        offset: usize,
    ) -> Option<String> {
        let symbol = self.node_to_symbol.get(&target)?.clone();

        // Skip if already resolved
        if self.resolved_symbols.contains(&symbol) {
            return None;
        }

        self.resolved_symbols.insert(symbol.clone());
        self.resolved.push(AnchorPosition {
            symbol: symbol.clone(),
            fragment_id: content_fragment_id,
            section_id,
            offset,
        });

        // Record position for TOC lookup
        self.node_positions
            .insert(target, (content_fragment_id, offset));

        Some(symbol)
    }

    /// Create an anchor entity for a chapter-level target.
    ///
    /// Call this during Pass 2 when generating the first content for a chapter
    /// that's a link target.
    pub fn create_chapter_anchor(
        &mut self,
        chapter: ChapterId,
        content_fragment_id: u64,
        section_id: u64,
    ) -> Option<String> {
        let symbol = self.chapter_to_symbol.get(&chapter)?.clone();

        // Skip if already resolved
        if self.resolved_symbols.contains(&symbol) {
            return None;
        }

        self.resolved_symbols.insert(symbol.clone());
        self.resolved.push(AnchorPosition {
            symbol: symbol.clone(),
            fragment_id: content_fragment_id,
            section_id,
            offset: 0,
        });

        // Record position for TOC lookup
        self.chapter_positions.insert(chapter, content_fragment_id);

        Some(symbol)
    }

    /// Record the position of a node (for TOC/navigation lookup).
    ///
    /// This stores the position without creating an anchor entity.
    pub fn record_node_position(&mut self, target: GlobalNodeId, fragment_id: u64, offset: usize) {
        self.node_positions
            .entry(target)
            .or_insert((fragment_id, offset));
    }

    /// Record the position of a chapter start.
    pub fn record_chapter_position(&mut self, chapter: ChapterId, fragment_id: u64) {
        self.chapter_positions.entry(chapter).or_insert(fragment_id);
    }

    /// Get the content position for a node (for TOC resolution).
    pub fn get_node_position(&self, target: GlobalNodeId) -> Option<(u64, usize)> {
        self.node_positions.get(&target).copied()
    }

    /// Get the content position for a chapter (for TOC resolution).
    pub fn get_chapter_position(&self, chapter: ChapterId) -> Option<u64> {
        self.chapter_positions.get(&chapter).copied()
    }

    /// Drain all resolved internal anchors for entity emission.
    pub fn drain_anchors(&mut self) -> Vec<AnchorPosition> {
        std::mem::take(&mut self.resolved)
    }

    /// Drain all external anchors for entity emission.
    pub fn drain_external_anchors(&mut self) -> Vec<ExternalAnchor> {
        std::mem::take(&mut self.external_anchors)
    }

    /// Get the number of registered targets.
    pub fn len(&self) -> usize {
        self.node_to_symbol.len() + self.chapter_to_symbol.len() + self.external_anchors.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.node_to_symbol.is_empty()
            && self.chapter_to_symbol.is_empty()
            && self.external_anchors.is_empty()
    }
}

/// Central context for KFX export.
///
/// All shared state flows through this context:
/// - symbols: String → Symbol ID mapping
/// - fragment_ids: Unique fragment ID generator
/// - resource_registry: href → resource symbol mapping
/// - section_ids: Section IDs in spine order (for reading order)
/// - position_map: NodeId → (fragment_id, offset) for link resolution
///
/// The context also bridges the Schema (Micro) and Assembler (Macro) layers:
/// - During `tokens_to_ion`, text strings are captured in `text_accumulator`
/// - The assembler then packages these into separate Content entities
pub struct ExportContext {
    /// Global symbol table - strings → symbol IDs.
    pub symbols: SymbolTable,

    /// Fragment ID generator (starts at 200).
    pub fragment_ids: IdGenerator,

    /// Resource tracking: href → resource symbol.
    pub resource_registry: ResourceRegistry,

    /// Section IDs in spine order (for reading order).
    pub section_ids: Vec<u64>,

    /// Text accumulator for current content entity.
    /// Captures strings "falling out" of token conversion for the Assembler.
    text_accumulator: TextAccumulator,

    /// Current content entity name (symbol ID).
    /// Set by the Assembler before calling tokens_to_ion.
    pub current_content_name: u64,

    /// Position map: (ChapterId, NodeId) → Position.
    /// Populated during Pass 1 survey for landmark resolution.
    pub position_map: HashMap<(ChapterId, NodeId), Position>,

    /// Chapter to fragment ID mapping.
    /// Populated during Pass 1 to resolve section references.
    pub chapter_fragments: HashMap<ChapterId, u64>,

    /// Current chapter being processed.
    current_chapter: Option<ChapterId>,

    /// Current fragment ID being built.
    current_fragment_id: u64,

    /// Current text offset within the fragment.
    current_text_offset: usize,

    /// Path to fragment ID mapping.
    /// Maps source file paths (e.g., "chapter1.xhtml") to fragment IDs.
    pub path_to_fragment: HashMap<String, u64>,

    /// Default style symbol ID.
    /// All storyline elements reference this style for Kindle rendering.
    pub default_style_symbol: u64,

    /// Style registry for deduplicating and tracking KFX styles.
    pub style_registry: StyleRegistry,

    /// Anchor registry for link target resolution.
    pub anchor_registry: AnchorRegistry,

    /// Resolved landmarks mapping LandmarkType to (fragment ID, offset, label).
    /// Populated during survey from IR landmarks and heuristics.
    pub landmark_fragments: HashMap<LandmarkType, LandmarkTarget>,

    /// Nav container name symbols (registered during Pass 1).
    pub nav_container_symbols: NavContainerSymbols,

    /// Heading positions tracked during survey for headings navigation.
    pub heading_positions: Vec<HeadingPosition>,

    /// Fragment ID for standalone cover section (if EPUB has cover image not in spine).
    pub cover_fragment_id: Option<u64>,

    /// Content fragment ID for standalone cover.
    pub cover_content_id: Option<u64>,

    /// Chapters that need chapter-start anchors.
    chapters_needing_anchor: HashSet<ChapterId>,

    /// Current pending chapter-start anchor.
    pending_chapter_anchor: Option<ChapterId>,

    /// First content fragment ID for each chapter.
    pub first_content_ids: HashMap<ChapterId, u64>,

    /// All content fragment IDs for each chapter.
    pub content_ids_by_chapter: HashMap<ChapterId, Vec<u64>>,

    /// Text length for each content fragment ID.
    pub content_id_lengths: HashMap<u64, usize>,
}

/// Position of a heading element for navigation.
#[derive(Debug, Clone)]
pub struct HeadingPosition {
    /// Heading level (1-6).
    pub level: u8,
    /// Fragment ID containing the heading.
    pub fragment_id: u64,
    /// Byte offset within the fragment.
    pub offset: usize,
}

/// Target position for a landmark.
#[derive(Debug, Clone)]
pub struct LandmarkTarget {
    /// Fragment ID containing the landmark target.
    pub fragment_id: u64,
    /// Byte offset within the fragment (0 for chapter start).
    pub offset: u64,
    /// Display label for the landmark.
    pub label: String,
}

/// Pre-registered symbol IDs for nav container names.
#[derive(Debug, Clone, Default)]
pub struct NavContainerSymbols {
    pub toc: u64,
    pub headings: u64,
    pub landmarks: u64,
}

impl ExportContext {
    /// Create a new export context.
    pub fn new() -> Self {
        let mut symbols = SymbolTable::new();
        let default_style_symbol = symbols.get_or_intern("s0");

        Self {
            symbols,
            fragment_ids: IdGenerator::new(),
            resource_registry: ResourceRegistry::new(),
            section_ids: Vec::new(),
            text_accumulator: TextAccumulator::new(),
            current_content_name: 0,
            position_map: HashMap::new(),
            chapter_fragments: HashMap::new(),
            current_chapter: None,
            current_fragment_id: 0,
            current_text_offset: 0,
            path_to_fragment: HashMap::new(),
            default_style_symbol,
            style_registry: StyleRegistry::new(default_style_symbol),
            anchor_registry: AnchorRegistry::new(),
            landmark_fragments: HashMap::new(),
            nav_container_symbols: NavContainerSymbols::default(),
            heading_positions: Vec::new(),
            cover_fragment_id: None,
            cover_content_id: None,
            chapters_needing_anchor: HashSet::new(),
            pending_chapter_anchor: None,
            first_content_ids: HashMap::new(),
            content_ids_by_chapter: HashMap::new(),
            content_id_lengths: HashMap::new(),
        }
    }

    /// Prepare context for processing a new chapter.
    pub fn begin_chapter(&mut self, content_name: &str) -> u64 {
        self.text_accumulator = TextAccumulator::new();
        self.current_content_name = self.symbols.get_or_intern(content_name);
        self.current_content_name
    }

    /// Begin Pass 2 export for a chapter.
    pub fn begin_chapter_export(&mut self, chapter_id: ChapterId) {
        self.current_chapter = Some(chapter_id);

        // Check if this chapter needs a chapter-start anchor
        if self.chapters_needing_anchor.contains(&chapter_id) {
            self.pending_chapter_anchor = Some(chapter_id);
        } else {
            self.pending_chapter_anchor = None;
        }
    }

    /// Intern a string into the symbol table, returning its ID.
    pub fn intern(&mut self, s: &str) -> u64 {
        self.symbols.get_or_intern(s)
    }

    /// Track text and return (segment_index, offset).
    pub fn append_text(&mut self, text: &str) -> (usize, usize) {
        let offset = self.text_accumulator.len();
        let index = self.text_accumulator.push(text);
        (index, offset)
    }

    /// Get the text accumulator.
    pub fn text_accumulator(&self) -> &TextAccumulator {
        &self.text_accumulator
    }

    /// Drain the text accumulator.
    pub fn drain_text(&mut self) -> Vec<String> {
        self.text_accumulator.drain()
    }

    /// Generate a new unique fragment ID.
    pub fn next_fragment_id(&mut self) -> u64 {
        self.fragment_ids.next_id()
    }

    /// Register a section and return its symbol ID.
    pub fn register_section(&mut self, name: &str) -> u64 {
        let id = self.intern(name);
        self.section_ids.push(id);
        id
    }

    /// Register an IR style and return its KFX style symbol.
    pub fn register_ir_style(&mut self, ir_style: &crate::style::ComputedStyle) -> u64 {
        let schema = crate::kfx::style_schema::StyleSchema::standard();
        let mut builder = crate::kfx::style_registry::StyleBuilder::new(schema);
        builder.ingest_ir_style(ir_style);
        let kfx_style = builder.build();
        self.style_registry.register(kfx_style, &mut self.symbols)
    }

    /// Register an IR style by StyleId.
    pub fn register_style_id(
        &mut self,
        style_id: StyleId,
        style_pool: &crate::style::StylePool,
    ) -> u64 {
        if style_id == StyleId::DEFAULT {
            return self.default_style_symbol;
        }

        if let Some(ir_style) = style_pool.get(style_id) {
            self.register_ir_style(ir_style)
        } else {
            self.default_style_symbol
        }
    }

    // =========================================================================
    // Pass 1: Survey / Position Tracking
    // =========================================================================

    /// Begin surveying a chapter.
    pub fn begin_chapter_survey(&mut self, chapter_id: ChapterId, path: &str) -> u64 {
        let fragment_id = self.fragment_ids.next_id();
        self.chapter_fragments.insert(chapter_id, fragment_id);
        self.path_to_fragment.insert(path.to_string(), fragment_id);
        self.current_chapter = Some(chapter_id);
        self.current_fragment_id = fragment_id;
        self.current_text_offset = 0;

        // Mark chapter-start anchor if this chapter is a target
        if self.anchor_registry.is_chapter_target(chapter_id) {
            self.chapters_needing_anchor.insert(chapter_id);
        }

        fragment_id
    }

    /// End surveying a chapter.
    pub fn end_chapter_survey(&mut self) {
        self.current_chapter = None;
    }

    /// Get the fragment ID for a given source path.
    pub fn get_fragment_for_path(&self, path: &str) -> Option<u64> {
        self.path_to_fragment.get(path).copied()
    }

    /// Record position for a node during Pass 1.
    pub fn record_position(&mut self, node_id: NodeId) {
        if let Some(chapter_id) = self.current_chapter {
            self.position_map.insert(
                (chapter_id, node_id),
                Position {
                    fragment_id: self.current_fragment_id,
                    offset: self.current_text_offset,
                },
            );
        }
    }

    /// Record a heading position for headings navigation.
    pub fn record_heading(&mut self, level: u8) {
        self.heading_positions.push(HeadingPosition {
            level,
            fragment_id: self.current_fragment_id,
            offset: self.current_text_offset,
        });
    }

    /// Record heading position during Pass 2 with actual content fragment ID.
    pub fn record_heading_with_id(&mut self, level: u8, fragment_id: u64) {
        self.heading_positions.push(HeadingPosition {
            level,
            fragment_id,
            offset: 0,
        });
    }

    /// Create the pending chapter-start anchor with the first content fragment ID.
    pub fn resolve_pending_chapter_anchor(&mut self, first_content_id: u64) {
        // Record first content ID for this chapter
        if let Some(chapter_id) = self.current_chapter {
            self.first_content_ids
                .entry(chapter_id)
                .or_insert(first_content_id);

            // Record chapter position for TOC lookup
            self.anchor_registry
                .record_chapter_position(chapter_id, first_content_id);
        }

        // Get section ID for position_map grouping
        let section_id = self
            .current_chapter
            .and_then(|ch| self.chapter_fragments.get(&ch).copied())
            .unwrap_or(first_content_id);

        // Create chapter-start anchor if pending
        if let Some(chapter_id) = self.pending_chapter_anchor.take()
            && let Some(symbol) =
                self.anchor_registry
                    .create_chapter_anchor(chapter_id, first_content_id, section_id)
        {
            self.symbols.get_or_intern(&symbol);
        }
    }

    /// Process a node during storyline building.
    ///
    /// If the node is a link target, creates an anchor entity.
    pub fn create_anchor_if_needed(&mut self, node_id: NodeId, content_id: u64, offset: usize) {
        let Some(chapter_id) = self.current_chapter else {
            return;
        };

        let gid = GlobalNodeId::new(chapter_id, node_id);

        // Get section ID for position_map grouping
        let section_id = self
            .chapter_fragments
            .get(&chapter_id)
            .copied()
            .unwrap_or(content_id);

        // Always record position for TOC/navigation lookup
        self.anchor_registry
            .record_node_position(gid, content_id, offset);

        // Only create anchor entity if this is a link target
        if let Some(symbol) = self
            .anchor_registry
            .create_anchor(gid, content_id, section_id, offset)
        {
            self.symbols.get_or_intern(&symbol);
        }
    }

    /// Record a content fragment ID for the current chapter.
    pub fn record_content_id(&mut self, content_id: u64) {
        if let Some(chapter_id) = self.current_chapter {
            self.content_ids_by_chapter
                .entry(chapter_id)
                .or_default()
                .push(content_id);
        }
    }

    /// Record text length for a content fragment ID.
    pub fn record_content_length(&mut self, content_id: u64, text_len: usize) {
        self.content_id_lengths.insert(content_id, text_len);
    }

    /// Advance the text offset during survey (Pass 1).
    pub fn advance_text_offset(&mut self, text_len: usize) {
        self.current_text_offset += text_len;
    }

    /// Get the current fragment ID being surveyed.
    pub fn current_fragment_id(&self) -> u64 {
        self.current_fragment_id
    }

    /// Get the current text offset during survey.
    pub fn current_text_offset(&self) -> usize {
        self.current_text_offset
    }

    // =========================================================================
    // Pass 2: Position Lookup
    // =========================================================================

    /// Look up position for a node.
    pub fn get_position(&self, chapter_id: ChapterId, node_id: NodeId) -> Option<Position> {
        self.position_map.get(&(chapter_id, node_id)).copied()
    }

    /// Get fragment ID for a chapter.
    pub fn get_chapter_fragment(&self, chapter_id: ChapterId) -> Option<u64> {
        self.chapter_fragments.get(&chapter_id).copied()
    }

    /// Get the maximum EID used.
    pub fn max_eid(&self) -> u64 {
        if self.fragment_ids.peek() > IdGenerator::FRAGMENT_MIN_ID {
            self.fragment_ids.peek() - 1
        } else {
            0
        }
    }

    /// Format a position as a Kindle position string.
    pub fn format_kindle_pos(fragment_id: u64, offset: usize) -> String {
        let fid_encoded = encode_base32(fragment_id as u32, 4);
        let off_encoded = encode_base32(offset as u32, 10);
        format!("kindle:pos:fid:{}:off:{}", fid_encoded, off_encoded)
    }

    // =========================================================================
    // TOC Anchor Management
    // =========================================================================

    /// Register TOC entries to mark their targets for anchor creation.
    ///
    /// Uses the pre-resolved `target` field from `ResolvedLinks`.
    pub fn register_toc_targets(&mut self, entries: &[TocEntry]) {
        for entry in entries {
            // The target is already resolved by resolve_links()
            // We just need to ensure it's registered for anchor creation
            // (which happens when we process internal_targets from ResolvedLinks)

            // Recurse into children
            if !entry.children.is_empty() {
                self.register_toc_targets(&entry.children);
            }
        }
    }

    /// Update landmark fragment IDs to use storyline content IDs.
    pub fn fix_landmark_content_ids(&mut self) {
        for target in self.landmark_fragments.values_mut() {
            // Try to find which chapter this fragment_id belongs to
            let mut found_chapter = None;
            for (cid, &fid) in &self.chapter_fragments {
                if fid == target.fragment_id {
                    found_chapter = Some(*cid);
                    break;
                }
            }

            // If we found the chapter, look up the first content ID
            if let Some(chapter_id) = found_chapter
                && let Some(&content_id) = self.first_content_ids.get(&chapter_id)
            {
                target.fragment_id = content_id;
            }
        }
    }

    /// Get the current chapter ID.
    pub fn current_chapter(&self) -> Option<ChapterId> {
        self.current_chapter
    }

    /// Check if a node is a registered link/TOC target.
    pub fn is_registered_target(&self, node_id: NodeId) -> bool {
        let Some(chapter_id) = self.current_chapter else {
            return false;
        };
        let gid = GlobalNodeId::new(chapter_id, node_id);
        self.anchor_registry.is_internal_target(gid)
    }
}

impl Default for ExportContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table_shared_symbols() {
        let mut symtab = SymbolTable::new();
        assert_eq!(symtab.get_or_intern("$260"), 260);
        assert_eq!(symtab.get_or_intern("$145"), 145);
    }

    #[test]
    fn test_symbol_table_local_symbols() {
        let mut symtab = SymbolTable::new();
        let id1 = symtab.get_or_intern("section-1");
        let id2 = symtab.get_or_intern("section-2");
        assert!(id1 >= SymbolTable::LOCAL_MIN_ID);
        assert_eq!(id2, id1 + 1);
        assert_eq!(symtab.get_or_intern("section-1"), id1);
    }

    #[test]
    fn test_id_generator() {
        let mut id_gen = IdGenerator::new();
        assert_eq!(id_gen.next_id(), 866);
        assert_eq!(id_gen.next_id(), 867);
        assert_eq!(id_gen.next_id(), 868);
    }

    #[test]
    fn test_resource_registry() {
        let mut symbols = SymbolTable::new();
        let mut registry = ResourceRegistry::new();

        let id1 = registry.register("images/cover.jpg", &mut symbols);
        let id2 = registry.register("images/cover.jpg", &mut symbols);
        let id3 = registry.register("images/other.jpg", &mut symbols);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_anchor_registry_internal() {
        let mut registry = AnchorRegistry::new();

        let target = GlobalNodeId::new(ChapterId(1), NodeId(42));
        let symbol = registry.register_internal_target(target, "chapter.xhtml#id42");

        assert_eq!(symbol, "a0");
        assert!(registry.is_internal_target(target));
        assert_eq!(registry.get_symbol(target), Some("a0"));
        // Also accessible by href
        assert_eq!(registry.get_href_symbol("chapter.xhtml#id42"), Some("a0"));
    }

    #[test]
    fn test_anchor_registry_chapter() {
        let mut registry = AnchorRegistry::new();

        let chapter = ChapterId(5);
        let symbol = registry.register_chapter_target(chapter, "chapter5.xhtml");

        assert_eq!(symbol, "a0");
        assert!(registry.is_chapter_target(chapter));
        assert_eq!(registry.get_chapter_symbol(chapter), Some("a0"));
        // Also accessible by href
        assert_eq!(registry.get_href_symbol("chapter5.xhtml"), Some("a0"));
    }

    #[test]
    fn test_anchor_registry_external() {
        let mut registry = AnchorRegistry::new();

        let url = "https://example.com/";
        let symbol = registry.register_external(url);

        assert_eq!(symbol, "a0");
        assert_eq!(registry.get_href_symbol(url), Some("a0"));

        let externals = registry.drain_external_anchors();
        assert_eq!(externals.len(), 1);
        assert_eq!(externals[0].uri, url);
    }

    #[test]
    fn test_anchor_registry_create_anchor() {
        let mut registry = AnchorRegistry::new();

        let target = GlobalNodeId::new(ChapterId(1), NodeId(42));
        registry.register_internal_target(target, "chapter.xhtml#id42");

        // Create anchor
        let symbol = registry.create_anchor(target, 100, 200, 50);
        assert_eq!(symbol, Some("a0".to_string()));

        // Second call should return None (already resolved)
        let symbol2 = registry.create_anchor(target, 100, 200, 50);
        assert_eq!(symbol2, None);

        // Position should be recorded
        assert_eq!(registry.get_node_position(target), Some((100, 50)));
    }

    #[test]
    fn test_export_context() {
        let mut ctx = ExportContext::new();

        let id1 = ctx.intern("section-1");
        let id2 = ctx.intern("section-1");
        assert_eq!(id1, id2);

        let fid1 = ctx.next_fragment_id();
        let fid2 = ctx.next_fragment_id();
        assert_eq!(fid1, 866);
        assert_eq!(fid2, 867);
    }
}
