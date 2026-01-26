//! Export context for KFX generation.
//!
//! The ExportContext is the central state management for KFX export.
//! All shared state flows through this context, avoiding the pitfalls of
//! scattered symbol tables, ID collision, and orphaned references.

use std::collections::HashMap;

use crate::import::ChapterId;
use crate::ir::NodeId;

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
    /// Fragment IDs start here (0-199 reserved for system).
    pub const FRAGMENT_MIN_ID: u64 = 200;

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
    /// Populated during Pass 1 survey when nodes have IDs.
    pub anchor_map: HashMap<String, (ChapterId, NodeId)>,
}

impl ExportContext {
    /// Create a new export context.
    pub fn new() -> Self {
        Self {
            symbols: SymbolTable::new(),
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

    // =========================================================================
    // Pass 1: Survey / Position Tracking
    // =========================================================================

    /// Begin surveying a chapter. Call this at the start of Pass 1 for each chapter.
    pub fn begin_chapter_survey(&mut self, chapter_id: ChapterId) -> u64 {
        let fragment_id = self.fragment_ids.next();
        self.chapter_fragments.insert(chapter_id, fragment_id);
        self.current_chapter = Some(chapter_id);
        self.current_fragment_id = fragment_id;
        self.current_text_offset = 0;
        fragment_id
    }

    /// End surveying a chapter.
    pub fn end_chapter_survey(&mut self) {
        self.current_chapter = None;
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

    /// Record position for a node with a specific anchor ID.
    /// This allows looking up by anchor name later.
    pub fn record_anchor(&mut self, anchor_id: &str, node_id: NodeId) {
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

        assert_eq!(id_gen.next(), 200);
        assert_eq!(id_gen.next(), 201);
        assert_eq!(id_gen.next(), 202);
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
        assert_eq!(fid1, 200);
        assert_eq!(fid2, 201);

        // Test text accumulation
        let (idx, offset) = ctx.append_text("Hello");
        assert_eq!(idx, 0);
        assert_eq!(offset, 0);

        let (idx, offset) = ctx.append_text("World");
        assert_eq!(idx, 1);
        assert_eq!(offset, 5);
    }
}
