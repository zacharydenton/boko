//! Export context for KFX generation.
//!
//! The ExportContext is the central state management for KFX export.
//! All shared state flows through this context, avoiding the pitfalls of
//! scattered symbol tables, ID collision, and orphaned references.

use std::collections::{HashMap, HashSet};

use crate::book::LandmarkType;
use crate::import::ChapterId;
use crate::ir::{NodeId, StyleId};

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
        if let Some(id_str) = name.strip_prefix('$') {
            if let Ok(id) = id_str.parse::<u64>() {
                return id;
            }
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
        if let Some(id_str) = name.strip_prefix('$') {
            if let Ok(id) = id_str.parse::<u64>() {
                return Some(id);
            }
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
    pub fn next(&mut self) -> u64 {
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

/// Resolved anchor with position information.
#[derive(Debug, Clone)]
pub struct AnchorPosition {
    /// The anchor symbol name (e.g., "a0", "a1")
    pub symbol: String,
    /// The original anchor name from href fragment (e.g., "note-1")
    pub anchor_name: String,
    /// Fragment/section ID where this anchor lives
    pub fragment_id: u64,
    /// Byte offset within the fragment (0 if at start)
    pub offset: usize,
}

/// Anchor registry for link resolution in KFX export.
///
/// KFX uses indirect anchor references: links point to anchor symbols,
/// and anchor entities ($266) define where those symbols resolve to.
///
/// ## Example Flow
///
/// 1. Link `href="chapter2.xhtml#note-1"` → `register_link_target("chapter2.xhtml#note-1")`
/// 2. Registry returns symbol "a0" for use in `link_to: a0`
/// 3. Later, call `resolve_anchor("note-1", fragment_id, offset)` when position is known
/// 4. At end, `drain_anchors()` returns entities to emit:
///    `{ anchor_name: a0, position: { id: 204, offset: 123 } }`
#[derive(Debug, Default)]
pub struct AnchorRegistry {
    /// href → anchor symbol name (e.g., "chapter2.xhtml#note-1" → "a0")
    link_to_symbol: HashMap<String, String>,

    /// anchor_name (fragment ID) → symbol (e.g., "note-1" → "a0")
    /// Used for resolving when we encounter the target element
    anchor_to_symbol: HashMap<String, String>,

    /// Resolved anchors ready for entity emission
    resolved: Vec<AnchorPosition>,

    /// Counter for generating unique anchor symbols
    next_anchor_id: usize,
}

impl AnchorRegistry {
    /// Create a new empty anchor registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a link target and return its anchor symbol.
    ///
    /// Call this when encountering a link with href. Returns the symbol
    /// to use in the `link_to` field of style_events.
    ///
    /// The href can be:
    /// - Full path: "chapter2.xhtml#note-1"
    /// - Fragment only: "#note-1"
    /// - Path without fragment: "chapter2.xhtml"
    pub fn register_link_target(&mut self, href: &str) -> String {
        // Check if already registered
        if let Some(symbol) = self.link_to_symbol.get(href) {
            return symbol.clone();
        }

        // Generate new anchor symbol
        let symbol = format!("a{:X}", self.next_anchor_id);
        self.next_anchor_id += 1;

        // Store href → symbol mapping
        self.link_to_symbol.insert(href.to_string(), symbol.clone());

        // Extract anchor name (fragment) if present and store reverse mapping
        if let Some(fragment) = extract_fragment(href) {
            self.anchor_to_symbol.insert(fragment.to_string(), symbol.clone());
        }

        symbol
    }

    /// Get the anchor symbol for a link target (if already registered).
    pub fn get_symbol(&self, href: &str) -> Option<&str> {
        self.link_to_symbol.get(href).map(|s| s.as_str())
    }

    /// Get the anchor symbol for an anchor name/fragment (if registered).
    pub fn get_symbol_for_anchor(&self, anchor_name: &str) -> Option<&str> {
        self.anchor_to_symbol.get(anchor_name).map(|s| s.as_str())
    }

    /// Resolve an anchor's position.
    ///
    /// Call this when we know where an anchor target lives (e.g., when
    /// processing an element with `id="note-1"`).
    pub fn resolve_anchor(&mut self, anchor_name: &str, fragment_id: u64, offset: usize) {
        if let Some(symbol) = self.anchor_to_symbol.get(anchor_name).cloned() {
            self.resolved.push(AnchorPosition {
                symbol,
                anchor_name: anchor_name.to_string(),
                fragment_id,
                offset,
            });
        }
    }

    /// Check if an anchor name is needed (has a link pointing to it).
    pub fn is_anchor_needed(&self, anchor_name: &str) -> bool {
        self.anchor_to_symbol.contains_key(anchor_name)
    }

    /// Drain all resolved anchors for entity emission.
    pub fn drain_anchors(&mut self) -> Vec<AnchorPosition> {
        std::mem::take(&mut self.resolved)
    }

    /// Get the number of registered link targets.
    pub fn len(&self) -> usize {
        self.link_to_symbol.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.link_to_symbol.is_empty()
    }
}

/// Extract the fragment (anchor) part from an href.
///
/// Examples:
/// - "chapter2.xhtml#note-1" → Some("note-1")
/// - "#note-1" → Some("note-1")
/// - "chapter2.xhtml" → None
fn extract_fragment(href: &str) -> Option<&str> {
    href.find('#').map(|i| &href[i + 1..])
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
    /// Populated during Pass 1 for TOC and internal link generation.
    pub position_map: HashMap<(ChapterId, NodeId), Position>,

    /// Chapter to fragment ID mapping.
    /// Populated during Pass 1 to resolve section references.
    pub chapter_fragments: HashMap<ChapterId, u64>,

    /// Current chapter being processed (for position tracking).
    current_chapter: Option<ChapterId>,

    /// Current fragment ID being built.
    current_fragment_id: u64,

    /// Current text offset within the fragment.
    current_text_offset: usize,

    /// Anchor map: anchor_id → (ChapterId, NodeId).
    /// Populated during Pass 1 survey when nodes have IDs that are link targets.
    pub anchor_map: HashMap<String, (ChapterId, NodeId)>,

    /// Path to fragment ID mapping.
    /// Maps source file paths (e.g., "chapter1.xhtml") to fragment IDs.
    /// Used for resolving TOC hrefs to positions.
    pub path_to_fragment: HashMap<String, u64>,

    /// Set of anchor IDs that are actually needed (targets of links or TOC).
    /// Only anchors in this set will be emitted to avoid bloat.
    needed_anchors: HashSet<String>,

    /// Default style symbol ID.
    /// All storyline elements reference this style for Kindle rendering.
    pub default_style_symbol: u64,

    /// Style registry for deduplicating and tracking KFX styles.
    pub style_registry: StyleRegistry,

    /// Anchor registry for link target resolution.
    /// Maps link hrefs to anchor symbols and tracks positions for entity emission.
    pub anchor_registry: AnchorRegistry,

    /// Resolved landmarks mapping LandmarkType to (fragment ID, offset, label).
    /// Populated during survey from IR landmarks and heuristics.
    pub landmark_fragments: HashMap<LandmarkType, LandmarkTarget>,

    /// Nav container name symbols (registered during Pass 1).
    pub nav_container_symbols: NavContainerSymbols,

    /// Heading positions tracked during survey for headings navigation.
    /// Grouped by heading level (2-6, h1 is typically not used in body).
    pub heading_positions: Vec<HeadingPosition>,
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
        // Register the default style name during initialization
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
            anchor_map: HashMap::new(),
            path_to_fragment: HashMap::new(),
            needed_anchors: HashSet::new(),
            default_style_symbol,
            style_registry: StyleRegistry::new(default_style_symbol),
            anchor_registry: AnchorRegistry::new(),
            landmark_fragments: HashMap::new(),
            nav_container_symbols: NavContainerSymbols::default(),
            heading_positions: Vec::new(),
        }
    }

    /// Prepare context for processing a new chapter.
    ///
    /// Called by the Assembler before generating tokens for a chapter.
    /// Sets up the content name and clears the text accumulator.
    pub fn begin_chapter(&mut self, content_name: &str) -> u64 {
        self.text_accumulator = TextAccumulator::new();
        self.current_content_name = self.symbols.get_or_intern(content_name);
        self.current_content_name
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
        self.fragment_ids.next()
    }

    /// Register a section and return its symbol ID.
    pub fn register_section(&mut self, name: &str) -> u64 {
        let id = self.intern(name);
        self.section_ids.push(id);
        id
    }

    /// Register an IR style and return its KFX style symbol.
    ///
    /// Converts the IR ComputedStyle to KFX format via the schema-driven
    /// StyleBuilder pipeline, then deduplicates via the StyleRegistry.
    /// Returns the symbol ID to use in storyline elements.
    pub fn register_ir_style(
        &mut self,
        ir_style: &crate::ir::ComputedStyle,
    ) -> u64 {
        // Use the schema-driven pipeline (single source of truth)
        let schema = crate::kfx::style_schema::StyleSchema::standard();
        let mut builder = crate::kfx::style_registry::StyleBuilder::new(&schema);
        builder.ingest_ir_style(ir_style);
        let kfx_style = builder.build();

        // Register and get symbol (handles deduplication)
        self.style_registry.register(kfx_style, &mut self.symbols)
    }

    /// Register an IR style by StyleId, looking it up in the style pool.
    ///
    /// Returns the KFX style symbol. For DEFAULT style, returns the default symbol.
    pub fn register_style_id(
        &mut self,
        style_id: StyleId,
        style_pool: &crate::ir::StylePool,
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

    /// Begin surveying a chapter. Call this at the start of Pass 1 for each chapter.
    ///
    /// The `path` is the source file path (e.g., "chapter1.xhtml") used to resolve
    /// TOC hrefs to positions.
    pub fn begin_chapter_survey(&mut self, chapter_id: ChapterId, path: &str) -> u64 {
        let fragment_id = self.fragment_ids.next();
        self.chapter_fragments.insert(chapter_id, fragment_id);
        self.path_to_fragment.insert(path.to_string(), fragment_id);
        self.current_chapter = Some(chapter_id);
        self.current_fragment_id = fragment_id;
        self.current_text_offset = 0;
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
    /// Call this when encountering a node that might be a link target.
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
    /// Call this when encountering a heading node during Pass 1.
    pub fn record_heading(&mut self, level: u8) {
        self.heading_positions.push(HeadingPosition {
            level,
            fragment_id: self.current_fragment_id,
            offset: self.current_text_offset,
        });
    }

    /// Register an anchor as needed (it's a link target or TOC destination).
    ///
    /// Call this during the initial survey when encountering href="#anchor".
    pub fn register_needed_anchor(&mut self, anchor_id: &str) {
        self.needed_anchors.insert(anchor_id.to_string());
    }

    /// Check if an anchor is needed (has a link pointing to it).
    pub fn is_anchor_needed(&self, anchor_id: &str) -> bool {
        self.needed_anchors.contains(anchor_id)
    }

    /// Record position for a node with a specific anchor ID.
    /// Only records if the anchor is actually needed (has incoming links).
    pub fn record_anchor(&mut self, anchor_id: &str, node_id: NodeId) {
        // Only create anchors for IDs that are actually link targets
        if !self.needed_anchors.contains(anchor_id) {
            return;
        }

        // Intern the anchor for later lookup
        self.intern(anchor_id);
        self.record_position(node_id);

        // Store mapping from anchor_id to position key
        if let Some(chapter_id) = self.current_chapter {
            self.anchor_map
                .insert(anchor_id.to_string(), (chapter_id, node_id));
        }
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

    /// Look up position for a node. Used during Pass 2 for link generation.
    pub fn get_position(&self, chapter_id: ChapterId, node_id: NodeId) -> Option<Position> {
        self.position_map.get(&(chapter_id, node_id)).copied()
    }

    /// Get fragment ID for a chapter.
    pub fn get_chapter_fragment(&self, chapter_id: ChapterId) -> Option<u64> {
        self.chapter_fragments.get(&chapter_id).copied()
    }

    /// Get the maximum EID used.
    ///
    /// This returns the highest element ID that has been assigned,
    /// used for the `max_id` field in document_data.
    pub fn max_eid(&self) -> u64 {
        // The next ID minus 1 gives us the last used ID
        // If no IDs have been used yet, return 0
        if self.fragment_ids.peek() > IdGenerator::FRAGMENT_MIN_ID {
            self.fragment_ids.peek() - 1
        } else {
            0
        }
    }

    /// Format a position as a Kindle position string: "kindle:pos:fid:XXXX:off:YYYY"
    pub fn format_kindle_pos(fragment_id: u64, offset: usize) -> String {
        // KFX uses base-32 encoding for positions (4 digits each)
        let fid_encoded = encode_base32(fragment_id as u32, 4);
        let off_encoded = encode_base32(offset as u32, 10);
        format!("kindle:pos:fid:{}:off:{}", fid_encoded, off_encoded)
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

        // Shared symbols should return their ID directly
        assert_eq!(symtab.get_or_intern("$260"), 260);
        assert_eq!(symtab.get_or_intern("$145"), 145);
    }

    #[test]
    fn test_symbol_table_local_symbols() {
        let mut symtab = SymbolTable::new();

        // Local symbols should get new IDs starting at LOCAL_MIN_ID
        let id1 = symtab.get_or_intern("section-1");
        let id2 = symtab.get_or_intern("section-2");
        assert!(id1 >= SymbolTable::LOCAL_MIN_ID);
        assert_eq!(id2, id1 + 1);

        // Same symbol should return same ID
        assert_eq!(symtab.get_or_intern("section-1"), id1);
    }

    #[test]
    fn test_id_generator() {
        let mut id_gen = IdGenerator::new();

        assert_eq!(id_gen.next(), 866);
        assert_eq!(id_gen.next(), 867);
        assert_eq!(id_gen.next(), 868);
    }

    #[test]
    fn test_resource_registry() {
        let mut symbols = SymbolTable::new();
        let mut registry = ResourceRegistry::new();

        let id1 = registry.register("images/cover.jpg", &mut symbols);
        let id2 = registry.register("images/cover.jpg", &mut symbols);
        let id3 = registry.register("images/other.jpg", &mut symbols);

        // Same resource should return same ID
        assert_eq!(id1, id2);
        // Different resource should return different ID
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_resource_registry_unique_names() {
        let mut registry = ResourceRegistry::new();

        // Each resource should get a unique short name
        let name1 = registry.get_or_create_name("images/cover.jpg");
        let name2 = registry.get_or_create_name("images/photo.png");
        let name3 = registry.get_or_create_name("images/logo.gif");

        assert_eq!(name1, "e0");
        assert_eq!(name2, "e1");
        assert_eq!(name3, "e2");

        // Same href should return the same name (idempotent)
        assert_eq!(registry.get_or_create_name("images/cover.jpg"), "e0");
        assert_eq!(registry.get_or_create_name("images/photo.png"), "e1");

        // Verify get_name lookup
        assert_eq!(registry.get_name("images/cover.jpg"), Some("e0"));
        assert_eq!(registry.get_name("images/unknown.jpg"), None);
    }

    #[test]
    fn test_text_accumulator() {
        let mut acc = TextAccumulator::new();

        let idx1 = acc.push("Hello");
        let idx2 = acc.push(" World");

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(acc.len(), 11);
        assert_eq!(acc.segments().len(), 2);
    }

    #[test]
    fn test_export_context() {
        let mut ctx = ExportContext::new();

        // Test interning
        let id1 = ctx.intern("section-1");
        let id2 = ctx.intern("section-1");
        assert_eq!(id1, id2);

        // Test fragment ID generation
        let fid1 = ctx.next_fragment_id();
        let fid2 = ctx.next_fragment_id();
        assert_eq!(fid1, 866);
        assert_eq!(fid2, 867);

        // Test text accumulation
        let (idx, offset) = ctx.append_text("Hello");
        assert_eq!(idx, 0);
        assert_eq!(offset, 0);

        let (idx, offset) = ctx.append_text("World");
        assert_eq!(idx, 1);
        assert_eq!(offset, 5);
    }
}
