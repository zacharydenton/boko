//! Export context for KFX generation.
//!
//! The ExportContext is the central state management for KFX export.
//! All shared state flows through this context, avoiding the pitfalls of
//! scattered symbol tables, ID collision, and orphaned references.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rustc_hash::FxHashMap;

use crate::import::ChapterId;
use crate::model::{GlobalNodeId, LandmarkType, NodeId};
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
/// Generates unique IDs for fragments, starting at `FRAGMENT_MIN_ID` (866)
/// to avoid collision with symbols in the base and local symbol tables.
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

/// Maximum bytes of text per $145 content fragment.
///
/// Amazon's tooling rolls to a new content fragment once the accumulated
/// UTF-8 size reaches this bound (the element that crosses it may overshoot),
/// and its format checks treat larger fragments as errors.
pub const MAX_CONTENT_CHUNK_BYTES: usize = 8192;

/// Book-global text accumulator for content entities ($145).
///
/// Text is packed into `content_1..content_N` chunks of at most
/// [`MAX_CONTENT_CHUNK_BYTES`] (measured in UTF-8 bytes), spanning chapter
/// boundaries, matching Amazon-produced KFX.
#[derive(Default)]
pub struct TextAccumulator {
    /// Finished chunks: (chunk_number, segments).
    finished: Vec<(usize, Vec<String>)>,
    /// Segments of the chunk currently being filled.
    segments: Vec<String>,
    /// UTF-8 byte size of the current chunk.
    current_bytes: usize,
    /// Number of the current chunk (0 = none started yet).
    current_chunk: usize,
}

impl TextAccumulator {
    /// Create a new empty text accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push text, returning `(chunk_number, index_within_chunk)`.
    ///
    /// Mirrors the reference chunker: a new chunk starts when the current one
    /// has already reached the size bound *before* this push, so a single
    /// oversized segment lands whole rather than being split.
    pub fn push(&mut self, text: &str) -> (usize, usize) {
        if self.current_chunk == 0 {
            self.current_chunk = 1;
        } else if self.current_bytes >= MAX_CONTENT_CHUNK_BYTES {
            self.finished
                .push((self.current_chunk, std::mem::take(&mut self.segments)));
            self.current_chunk += 1;
            self.current_bytes = 0;
        }
        let index = self.segments.len();
        self.current_bytes += text.len();
        self.segments.push(text.to_string());
        (self.current_chunk, index)
    }

    /// Take all chunks (finished plus the one in progress), resetting.
    pub fn drain_chunks(&mut self) -> Vec<(usize, Vec<String>)> {
        let mut chunks = std::mem::take(&mut self.finished);
        if !self.segments.is_empty() {
            chunks.push((self.current_chunk, std::mem::take(&mut self.segments)));
        }
        self.current_bytes = 0;
        self.current_chunk = 0;
        chunks
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

    /// Register an external link target (http/https/mailto/tel URL — see
    /// [`crate::kfx::transforms::is_external_url`]).
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

        // Check if this is an external link. This must use the same predicate
        // as the KFX import parser (`transforms::parse_kfx_link`): if import
        // classifies a URL (e.g. `mailto:`) as external but export treats it
        // as internal, the storyline would reference an anchor entity that is
        // never emitted.
        if crate::kfx::transforms::is_external_url(href) {
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

    /// Zero every recorded offset that points into `fragment_id`.
    ///
    /// Called when an element's accumulated text turns out to be
    /// anchor-marker zero-width spaces only and is dropped from the output:
    /// offsets counted against that phantom text (e.g. a second anchor after
    /// a marker) would point past the element's actual (empty) content,
    /// which readers cannot locate.
    pub fn clamp_offsets_at(&mut self, fragment_id: u64) {
        for pos in self.node_positions.values_mut() {
            if pos.0 == fragment_id {
                pos.1 = 0;
            }
        }
        for anchor in &mut self.resolved {
            if anchor.fragment_id == fragment_id {
                anchor.offset = 0;
            }
        }
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

    /// Symbols handed out to links that never resolved to a position.
    ///
    /// Call after draining resolved anchors: whatever was registered (via
    /// node, chapter, or href lookups) but never resolved needs a fallback
    /// $266 fragment, or the emitted `link_to` references dangle.
    pub fn unresolved_symbols(&self) -> Vec<String> {
        let mut symbols: BTreeSet<&String> = BTreeSet::new();
        symbols.extend(self.node_to_symbol.values());
        symbols.extend(self.chapter_to_symbol.values());
        symbols.extend(self.href_to_symbol.values());
        symbols
            .into_iter()
            .filter(|s| !self.resolved_symbols.contains(*s))
            // External links resolve through their own $266 form (yj.external
            // URI); they are not "unresolved" and must not get a fallback.
            .filter(|s| !self.external_anchors.iter().any(|e| e.symbol == **s))
            .cloned()
            .collect()
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

    /// Fragment ID generator (starts at `IdGenerator::FRAGMENT_MIN_ID`, 866).
    pub fragment_ids: IdGenerator,

    /// Resource tracking: href → resource symbol.
    pub resource_registry: ResourceRegistry,

    /// Section IDs in spine order (for reading order).
    pub section_ids: Vec<u64>,

    /// Spine (section symbol, chapter) pairs in spine order.
    ///
    /// Populated by `register_spine_section` during Pass 1. Consumers must
    /// pair sections with chapters through this keyed association rather
    /// than by positional index: a chapter that fails to load never enters
    /// `chapter_fragments`, and index-based pairing would silently shift
    /// every later section onto the wrong chapter's EIDs.
    pub spine_section_chapters: Vec<(u64, ChapterId)>,

    /// Text accumulator for current content entity.
    /// Captures strings "falling out" of token conversion for the Assembler.
    text_accumulator: TextAccumulator,

    /// Symbol ID of the content chunk currently being filled.
    /// Maintained by `append_text` as chunks roll over.
    pub current_content_name: u64,

    /// Number of the content chunk `current_content_name` refers to.
    current_content_chunk: usize,

    /// Position map: (ChapterId, NodeId) → Position.
    /// Populated during Pass 1 survey for landmark resolution.
    pub position_map: FxHashMap<(ChapterId, NodeId), Position>,

    /// Chapter to fragment ID mapping.
    /// Populated during Pass 1 to resolve section references.
    pub chapter_fragments: FxHashMap<ChapterId, u64>,

    /// Current chapter being processed.
    current_chapter: Option<ChapterId>,

    /// Current fragment ID being built.
    current_fragment_id: u64,

    /// Current text offset within the fragment.
    current_text_offset: usize,

    /// Path to fragment ID mapping.
    /// Maps source file paths (e.g., "chapter1.xhtml") to fragment IDs.
    pub path_to_fragment: FxHashMap<String, u64>,

    /// Default style symbol ID.
    /// All storyline elements reference this style for Kindle rendering.
    pub default_style_symbol: u64,

    /// Style registry for deduplicating and tracking KFX styles.
    pub style_registry: StyleRegistry,

    /// Memo for `register_style_id`: chapter-local (StyleId, parent StyleId)
    /// → KFX style symbol. Keyed by the pair because inherited properties
    /// are emitted as a diff against the parent's style. StyleIds are only
    /// meaningful within one chapter's StylePool, so this is cleared by
    /// `begin_chapter`.
    ir_style_memo: FxHashMap<(StyleId, StyleId), u64>,

    /// Memo for `register_inline_style_id` (same lifecycle as
    /// `ir_style_memo`, separate because the inline projection of a style
    /// registers under a different symbol than its block form).
    ir_inline_style_memo: FxHashMap<(StyleId, StyleId), u64>,

    /// Memo for `register_style_id_adjusted` (same lifecycle): the key adds
    /// the margin-collapse override bits to the (style, parent) pair.
    #[allow(clippy::type_complexity)]
    ir_adjusted_style_memo: FxHashMap<
        (
            StyleId,
            StyleId,
            Option<u32>,
            Option<u32>,
            Option<crate::kfx::symbols::KfxSymbol>,
            Option<u32>,
        ),
        u64,
    >,

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

    /// Any image resource exceeds the classic 1920px bound (drives the
    /// `yj_hdv` content feature). Set during the resource pass.
    pub has_hdv_image: bool,

    /// Any JPEG resource contains restart markers FF D0-D7 (drives the
    /// `yj_jpg_rst_marker_present` content feature). Set during the resource
    /// pass.
    pub jpg_rst_marker_present: bool,

    /// The book contains table elements (drives the `yj_table` and
    /// `yj_table_viewer` content features). Set during storyline export.
    pub has_tables: bool,

    /// Something referenced the default style (s0); when nothing does, the
    /// fragment is not emitted (an unreferenced style is a conformance error).
    pub default_style_used: bool,

    /// Set while emitting a dropcap paragraph's inline content: the first
    /// styled run (the floated dropcap span) has its float and large font
    /// projected away, because the native KFX dropcap on the paragraph
    /// replaces them. Consumed by the first inline run.
    pub dropcap_suppress: bool,

    /// Text bytes per absolute font size (keyed by the exact f32 bits),
    /// accumulated during the survey pass to find the dominant body size.
    font_size_weights: FxHashMap<u32, u64>,

    /// Global font scale so the dominant body size renders at 1rem — the
    /// user's chosen device font size. Reference KFX normalizes the same
    /// way: a book whose stylesheet sets body text to 13px must not render
    /// 19% smaller than every other book on the device.
    pub font_scale: f32,

    /// Text bytes per line-height (em of the element's font, exact f32
    /// bits), accumulated during the survey pass.
    line_height_weights: FxHashMap<u32, u64>,

    /// Global line-height scale so the dominant leading renders at 1lh —
    /// the user's line-spacing setting. Reference KFX normalizes authored
    /// body leading the same way (a book forcing line-height: 1.6 must not
    /// override the reader's chosen spacing); authored leading elsewhere
    /// keeps its ratio to the body.
    pub line_scale: f32,

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

    /// Lazily loaded math font for KVG typesetting (None until first use;
    /// inner None = no font on this system → math falls back to text runs).
    math_font: Option<Option<crate::math::kvg::MathFont>>,
    /// Book-level shared KVG glyph outline bundle ($692 fragment "p0").
    pub math_bundle: crate::math::kvg::PathBundle,

    /// Per-section image resource dependencies.
    /// Maps section_name → set of resource short names (e.g., "e6") referenced by that section.
    /// Used to build the container_entity_map dependency graph so Kindle can locate images.
    pub section_resource_deps: BTreeMap<String, BTreeSet<String>>,
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
    pub page_list: u64,
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
            spine_section_chapters: Vec::new(),
            text_accumulator: TextAccumulator::new(),
            current_content_name: 0,
            current_content_chunk: 0,
            position_map: FxHashMap::default(),
            chapter_fragments: FxHashMap::default(),
            current_chapter: None,
            current_fragment_id: 0,
            current_text_offset: 0,
            path_to_fragment: FxHashMap::default(),
            default_style_symbol,
            style_registry: StyleRegistry::new(default_style_symbol),
            ir_style_memo: FxHashMap::default(),
            ir_inline_style_memo: FxHashMap::default(),
            ir_adjusted_style_memo: FxHashMap::default(),
            anchor_registry: AnchorRegistry::new(),
            landmark_fragments: HashMap::new(),
            nav_container_symbols: NavContainerSymbols::default(),
            heading_positions: Vec::new(),
            cover_fragment_id: None,
            cover_content_id: None,
            has_hdv_image: false,
            jpg_rst_marker_present: false,
            has_tables: false,
            default_style_used: false,
            dropcap_suppress: false,
            font_size_weights: FxHashMap::default(),
            font_scale: 1.0,
            line_height_weights: FxHashMap::default(),
            line_scale: 1.0,
            chapters_needing_anchor: HashSet::new(),
            pending_chapter_anchor: None,
            first_content_ids: HashMap::new(),
            content_ids_by_chapter: HashMap::new(),
            content_id_lengths: HashMap::new(),
            math_font: None,
            math_bundle: crate::math::kvg::PathBundle::new(),
            section_resource_deps: BTreeMap::new(),
        }
    }

    /// Record that a section references a given image resource (by short name).
    pub fn record_section_image_ref(&mut self, section_name: &str, short_name: &str) {
        self.section_resource_deps
            .entry(section_name.to_string())
            .or_default()
            .insert(short_name.to_string());
    }

    /// Reset the per-chapter StyleId → style-symbol memo.
    ///
    /// The memo is keyed by chapter-local `StyleId`, which is only meaningful
    /// within a single chapter's `StylePool`. Call this whenever the active
    /// chapter (and thus pool) changes, before registering its styles.
    pub fn reset_style_memo(&mut self) {
        self.ir_style_memo.clear();
        self.ir_inline_style_memo.clear();
        self.ir_adjusted_style_memo.clear();
    }

    /// Prepare context for processing a new chapter.
    ///
    /// The text accumulator is deliberately NOT reset: content chunks are
    /// book-global and span chapter boundaries.
    pub fn begin_chapter(&mut self) {
        self.ir_style_memo.clear();
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

    /// Append text to the current content chunk.
    ///
    /// Returns `(content_name_symbol, index_within_chunk)` — the pair a
    /// storyline content_ref needs. Rolls to a new `content_{N}` chunk when
    /// the current one has reached [`MAX_CONTENT_CHUNK_BYTES`].
    pub fn append_text(&mut self, text: &str) -> (u64, usize) {
        let (chunk, index) = self.text_accumulator.push(text);
        if chunk != self.current_content_chunk {
            self.current_content_chunk = chunk;
            self.current_content_name = self.symbols.get_or_intern(&format!("content_{chunk}"));
        }
        (self.current_content_name, index)
    }

    /// Whether math renders as a KVG-bearing container (a math font is
    /// available) or falls back to inline text runs.
    pub fn math_renders_as_container(&mut self) -> bool {
        self.math_font
            .get_or_insert_with(crate::math::kvg::MathFont::load_system)
            .is_some()
    }

    /// Typeset a math expression into KVG shapes, deduplicating outlines
    /// into the book bundle. `None` when no font is available or the
    /// expression contains unmodeled content.
    pub fn typeset_math(
        &mut self,
        math: &crate::math::Math,
    ) -> Option<crate::math::kvg::KvgEquation> {
        let font = self
            .math_font
            .get_or_insert_with(crate::math::kvg::MathFont::load_system)
            .as_ref()?;
        let layout = crate::math::kvg::typeset(font, &math.expr, math.display)?;
        Some(crate::math::kvg::emit(font, &layout, &mut self.math_bundle))
    }

    /// Take all content chunks as (fragment_name, segments) pairs.
    pub fn take_content_chunks(&mut self) -> Vec<(String, Vec<String>)> {
        self.text_accumulator
            .drain_chunks()
            .into_iter()
            .map(|(chunk, segments)| (format!("content_{chunk}"), segments))
            .collect()
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

    /// Register a spine section together with the chapter it belongs to.
    ///
    /// Records the (section symbol, chapter) pair in `spine_section_chapters`
    /// so downstream consumers (e.g. the position map) can associate sections
    /// with chapters by key instead of by positional index. Returns the
    /// section symbol ID.
    pub fn register_spine_section(&mut self, name: &str, chapter_id: ChapterId) -> u64 {
        let id = self.register_section(name);
        self.spine_section_chapters.push((id, chapter_id));
        id
    }

    /// Record text weight for one absolute font size and line-height
    /// (em of the element's font), from the survey pass.
    pub fn record_text_metrics(&mut self, abs: f32, line_em: f32, bytes: usize) {
        *self.font_size_weights.entry(abs.to_bits()).or_insert(0) += bytes as u64;
        *self
            .line_height_weights
            .entry(line_em.to_bits())
            .or_insert(0) += bytes as u64;
    }

    /// Resolve the global font scale from the surveyed weights: the
    /// text-dominant absolute size maps to 1rem. No-op when the dominant
    /// size is already within 2% of 1rem; clamped to a sane range.
    pub fn resolve_font_scale(&mut self) {
        let Some((&key, _)) = self.font_size_weights.iter().max_by_key(|&(_, &w)| w) else {
            return;
        };
        let dominant = f32::from_bits(key);
        if dominant > 0.0 && (dominant - 1.0).abs() > 0.02 {
            self.font_scale = (1.0 / dominant).clamp(0.5, 2.0);
        }
        if let Some((&key, _)) = self.line_height_weights.iter().max_by_key(|&(_, &w)| w) {
            let dominant = f32::from_bits(key);
            if dominant > 0.0 && (dominant - 1.2).abs() > 0.024 {
                self.line_scale = (1.2 / dominant).clamp(0.5, 2.0);
            }
        }
    }

    /// A copy of `style` with the global font and line-height scales
    /// applied. Emission derives every font-relative conversion from
    /// `font_size_abs`, so scaling it here normalizes the whole style;
    /// authored line-heights scale toward the 1.2em base (unset leading is
    /// already the base).
    fn scaled_style(&self, style: &crate::style::ComputedStyle) -> crate::style::ComputedStyle {
        let mut scaled = style.clone();
        scaled.font_size_abs = crate::style::AbsFontSize(style.font_size_abs.0 * self.font_scale);
        scaled.line_scale = crate::style::AbsFontSize(self.line_scale);
        scaled
    }

    /// Build the KFX property set for a style under the book's font scale.
    /// `parent_is_default` marks the root inheritance context, which is the
    /// renderer environment — 1rem absolutely, never rescaled.
    fn build_kfx_style(
        &mut self,
        ir_style: &crate::style::ComputedStyle,
        parent: &crate::style::ComputedStyle,
        parent_is_default: bool,
    ) -> crate::kfx::style_registry::ComputedStyle {
        let schema = crate::kfx::style_schema::StyleSchema::standard();
        let mut builder = crate::kfx::style_registry::StyleBuilder::new(schema);
        if self.font_scale != 1.0 || self.line_scale != 1.0 {
            let scaled_parent = if parent_is_default {
                parent.clone()
            } else {
                self.scaled_style(parent)
            };
            builder.ingest_ir_style_with_parent(&self.scaled_style(ir_style), &scaled_parent);
        } else {
            builder.ingest_ir_style_with_parent(ir_style, parent);
        }
        let mut kfx_style = builder.build();
        // Block backgrounds paint the box, not the text run: reference
        // output uses fill_color on block styles (text_background_color is
        // for inline style_events; see register_inline_style_id).
        use crate::kfx::symbols::KfxSymbol;
        if let Some(value) = kfx_style.remove(KfxSymbol::TextBackgroundColor) {
            kfx_style.set(KfxSymbol::FillColor, value);
        }
        kfx_style
    }

    /// Register an IR style and return its KFX style symbol. `parent` is the
    /// parent element's computed style — the inheritance baseline for
    /// CSS-inherited properties (see `extract_ir_field`).
    pub fn register_ir_style(
        &mut self,
        ir_style: &crate::style::ComputedStyle,
        parent: &crate::style::ComputedStyle,
    ) -> u64 {
        let kfx_style = self.build_kfx_style(ir_style, parent, false);
        if kfx_style.is_empty() {
            self.default_style_used = true;
            return self.default_style_symbol;
        }
        self.style_registry.register(kfx_style, &mut self.symbols)
    }

    /// Register an IR style by StyleId. `parent_id` is the style of the
    /// nearest styled ancestor — emission depends on the (style, parent)
    /// pair, so the memo is keyed by both.
    pub fn register_style_id(
        &mut self,
        style_id: StyleId,
        parent_id: StyleId,
        style_pool: &crate::style::StylePool,
    ) -> u64 {
        if style_id == StyleId::DEFAULT && parent_id == StyleId::DEFAULT {
            return self.default_style_symbol;
        }

        if let Some(&symbol) = self.ir_style_memo.get(&(style_id, parent_id)) {
            return symbol;
        }

        let symbol = if let Some(ir_style) = style_pool.get(style_id) {
            let default = crate::style::ComputedStyle::default();
            let parent = style_pool.get(parent_id).unwrap_or(&default);
            let kfx_style = self.build_kfx_style(ir_style, parent, parent_id == StyleId::DEFAULT);
            if kfx_style.is_empty() {
                self.default_style_used = true;
                self.default_style_symbol
            } else {
                self.style_registry.register(kfx_style, &mut self.symbols)
            }
        } else {
            self.default_style_symbol
        };
        self.ir_style_memo.insert((style_id, parent_id), symbol);
        symbol
    }

    /// Register an IR style with margin-collapse overrides applied: the
    /// built KFX style's margin-top/bottom are replaced with the collapsed
    /// values (in lh of the element's line box) or removed when collapsed
    /// to zero. Memoized by (style, parent, override bits) — collapsed
    /// sequences repeat, so identical adjusted styles dedup.
    pub fn register_style_id_adjusted(
        &mut self,
        style_id: StyleId,
        parent_id: StyleId,
        style_pool: &crate::style::StylePool,
        adjust: crate::kfx::storyline::MarginAdjust,
        layout_hint: Option<crate::kfx::symbols::KfxSymbol>,
        link_color: Option<u32>,
    ) -> u64 {
        if adjust.is_identity() && layout_hint.is_none() && link_color.is_none() {
            return self.register_style_id(style_id, parent_id, style_pool);
        }
        let key = (
            style_id,
            parent_id,
            adjust.top_abs_em.map(f32::to_bits),
            adjust.bottom_abs_em.map(f32::to_bits),
            layout_hint,
            link_color,
        );
        if let Some(&symbol) = self.ir_adjusted_style_memo.get(&key) {
            return symbol;
        }

        let default = crate::style::ComputedStyle::default();
        let ir_style = style_pool.get(style_id);
        let parent = style_pool.get(parent_id).unwrap_or(&default);
        let ir_style_ref = ir_style.unwrap_or(&default);

        let mut kfx_style =
            self.build_kfx_style(ir_style_ref, parent, parent_id == StyleId::DEFAULT);

        use crate::kfx::style_schema::KfxValue;
        use crate::kfx::symbols::KfxSymbol;
        // Margin overrides are in unscaled absolute em; the unscaled style
        // converts them to the element's own em (the scale cancels).
        let mut apply = |symbol: KfxSymbol, abs_em: Option<f32>| {
            if let Some(v) = abs_em {
                if v == 0.0 {
                    kfx_style.remove(symbol);
                } else {
                    let lh = crate::kfx::style_schema::margin_abs_em_to_lh(ir_style_ref, v as f64);
                    // Round like extract_ir_field's dimension formatting.
                    let lh = (lh * 1e6).round() / 1e6;
                    kfx_style.set(
                        symbol,
                        KfxValue::Dimensioned {
                            value: lh,
                            unit: KfxSymbol::Lh,
                        },
                    );
                }
            }
        };
        apply(KfxSymbol::MarginTop, adjust.top_abs_em);
        apply(KfxSymbol::MarginBottom, adjust.bottom_abs_em);

        if let Some(hint) = layout_hint {
            kfx_style.set(
                KfxSymbol::LayoutHints,
                KfxValue::SymbolList(vec![hint as u64]),
            );
        }

        // Blocks containing colored links carry the link color as
        // link_unvisited_style/link_visited_style, like reference output —
        // event styles alone don't restyle the link text on device.
        if let Some(argb) = link_color {
            for symbol in [KfxSymbol::LinkUnvisitedStyle, KfxSymbol::LinkVisitedStyle] {
                kfx_style.set(
                    symbol,
                    KfxValue::StructField {
                        field: KfxSymbol::TextColor,
                        value: argb as i64,
                    },
                );
            }
        }

        let symbol = if kfx_style.is_empty() {
            self.default_style_used = true;
            self.default_style_symbol
        } else {
            self.style_registry.register(kfx_style, &mut self.symbols)
        };
        self.ir_adjusted_style_memo.insert(key, symbol);
        symbol
    }

    /// Register an IR style for an inline run (style_event), projecting away
    /// block-only properties.
    ///
    /// `box_align` centers a *block* within its container; readers only
    /// consume it on block elements, and a style carrying it from an inline
    /// run survives into the output as unexpected data. Reference KFX never
    /// puts block alignment on style_events.
    pub fn register_inline_style_id(
        &mut self,
        style_id: StyleId,
        parent_id: StyleId,
        style_pool: &crate::style::StylePool,
    ) -> u64 {
        self.register_inline_style_id_inner(style_id, parent_id, style_pool, false)
    }

    /// As [`Self::register_inline_style_id`], but when `suppress_dropcap` is
    /// set the run's float and large font are projected away (a dropcap
    /// paragraph's leading span — the native dropcap replaces them). Not
    /// memoized, so the same span emits normally elsewhere.
    pub fn register_inline_style_id_inner(
        &mut self,
        style_id: StyleId,
        parent_id: StyleId,
        style_pool: &crate::style::StylePool,
        suppress_dropcap: bool,
    ) -> u64 {
        if style_id == StyleId::DEFAULT && parent_id == StyleId::DEFAULT {
            return self.default_style_symbol;
        }

        if !suppress_dropcap
            && let Some(&symbol) = self.ir_inline_style_memo.get(&(style_id, parent_id))
        {
            return symbol;
        }

        let symbol = if let Some(ir_style) = style_pool.get(style_id) {
            let default = crate::style::ComputedStyle::default();
            let parent = style_pool.get(parent_id).unwrap_or(&default);
            let ir_style = ir_style.clone();
            let mut kfx_style =
                self.build_kfx_style(&ir_style, parent, parent_id == StyleId::DEFAULT);
            kfx_style.remove(crate::kfx::symbols::KfxSymbol::BoxAlign);
            if suppress_dropcap {
                use crate::kfx::symbols::KfxSymbol;
                kfx_style.remove(KfxSymbol::Float);
                kfx_style.remove(KfxSymbol::FontSize);
                kfx_style.remove(KfxSymbol::LineHeight);
            }
            if kfx_style.is_empty() {
                self.default_style_used = true;
                self.default_style_symbol
            } else {
                self.style_registry.register(kfx_style, &mut self.symbols)
            }
        } else {
            self.default_style_symbol
        };
        if !suppress_dropcap {
            self.ir_inline_style_memo
                .insert((style_id, parent_id), symbol);
        }
        symbol
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
    fn test_href_symbol_mailto_registers_external_anchor() {
        // Regression: `get_or_create_href_symbol` must classify external URLs
        // the same way the import parser does. A `mailto:` link previously
        // fell into the "unknown internal" branch, handing out a link_to
        // symbol with no backing anchor entity (a dangling KFX anchor).
        let mut registry = AnchorRegistry::new();

        let mailto_sym = registry.get_or_create_href_symbol("mailto:test@example.com");
        let tel_sym = registry.get_or_create_href_symbol("tel:+15551234567");
        assert_ne!(mailto_sym, tel_sym);

        let externals = registry.drain_external_anchors();
        assert_eq!(externals.len(), 2);
        // Every symbol handed out has a backing external anchor entity.
        assert!(
            externals
                .iter()
                .any(|a| a.symbol == mailto_sym && a.uri == "mailto:test@example.com")
        );
        assert!(
            externals
                .iter()
                .any(|a| a.symbol == tel_sym && a.uri == "tel:+15551234567")
        );

        // Internal-looking hrefs still don't create external anchor entities.
        let mut registry = AnchorRegistry::new();
        registry.get_or_create_href_symbol("chapter2.xhtml#note-1");
        assert!(registry.drain_external_anchors().is_empty());
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
