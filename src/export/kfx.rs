//! KFX format exporter.
//!
//! This module provides the `KfxExporter` which implements the `Exporter` trait
//! for writing books in Amazon's KFX format.

use std::io::{self, Seek, Write};

use crate::book::Book;
use crate::export::Exporter;
use crate::import::ChapterId;
use crate::ir::{IRChapter, NodeId, Role};
use crate::kfx::context::ExportContext;
use crate::kfx::fragment::KfxFragment;
use crate::kfx::ion::IonValue;
use crate::kfx::serialization::{
    create_entity_data, generate_container_id, serialize_annotated_ion, serialize_container,
    SerializedEntity,
};
use crate::kfx::symbols::KfxSymbol;

/// KFX export configuration.
#[derive(Debug, Clone, Default)]
pub struct KfxConfig {
    // Future: compression, DRM settings, etc.
}

/// KFX format exporter.
///
/// Converts books to Amazon's KFX format for Kindle devices.
pub struct KfxExporter {
    #[allow(dead_code)]
    config: KfxConfig,
}

impl KfxExporter {
    /// Create a new KfxExporter with default configuration.
    pub fn new() -> Self {
        Self {
            config: KfxConfig::default(),
        }
    }

    /// Create a new KfxExporter with custom configuration.
    pub fn with_config(config: KfxConfig) -> Self {
        Self { config }
    }
}

impl Default for KfxExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter for KfxExporter {
    fn export<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> io::Result<()> {
        // Build the KFX container
        let data = build_kfx_container(book)?;
        writer.write_all(&data)?;
        Ok(())
    }
}

/// Build a complete KFX container from a book.
///
/// This follows a strict Two-Pass architecture:
/// - Pass 1 (Survey): Walk IR, build position map, intern symbols - NO ION GENERATION
/// - Pass 2 (Synthesis): Generate Ion using pre-computed positions
fn build_kfx_container(book: &mut Book) -> io::Result<Vec<u8>> {
    let container_id = generate_container_id();
    let mut ctx = ExportContext::new();

    // ========================================================================
    // PASS 1: SURVEY (Read-Only / State Accumulation)
    // Goal: Populate ctx.symbols, ctx.position_map, ctx.chapter_fragments
    // NO ION GENERATION HERE!
    // ========================================================================

    // Collect spine info first to avoid borrow conflicts
    // Generate clean short section names (like 'c0', 'c1', etc.)
    let spine_info: Vec<_> = book
        .spine()
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            // Use short identifiers like the reference KFX files do
            let section_name = format!("c{}", idx);
            (entry.id, section_name)
        })
        .collect();

    // 1a. Survey each chapter: assign fragment IDs, build position map
    for (chapter_id, section_name) in &spine_info {
        // Register section name as symbol
        let _section_id = ctx.register_section(section_name);

        // Load and survey chapter
        if let Ok(chapter) = book.load_chapter(*chapter_id) {
            survey_chapter(&chapter, *chapter_id, &mut ctx);
        }
    }

    // 1b. Register TOC strings
    register_toc_symbols(book.toc(), &mut ctx);

    // 1c. Register resource paths
    let asset_paths: Vec<_> = book.list_assets();
    for asset_path in &asset_paths {
        if is_media_asset(asset_path) {
            let href = asset_path.to_string_lossy().to_string();
            ctx.resource_registry.register(&href, &mut ctx.symbols);
        }
    }

    // After Pass 1: ctx.symbols is COMPLETE, ctx.position_map has all EIDs

    // ========================================================================
    // PASS 2: SYNTHESIS (Generate Ion)
    // Now ctx.position_map is populated. We can resolve links correctly.
    // ========================================================================

    let mut fragments = Vec::new();

    // 2a. Metadata fragment
    fragments.push(build_metadata_fragment(book));

    // 2b. Format capabilities fragment
    fragments.push(build_format_capabilities_fragment());

    // 2c. Reading order fragment (uses ctx.section_ids)
    fragments.push(build_reading_orders_fragment(book, &ctx));

    // 2d. Book navigation fragment (uses ctx.position_map for TOC links)
    fragments.push(build_book_navigation_fragment_with_positions(book, &ctx));

    // 2e. Chapter entities (Content + Storyline + Section for each chapter)
    // Uses the Assembler pattern: Schema handles element semantics,
    // Assembler handles entity topology.
    for (chapter_id, section_name) in &spine_info {
        if let Ok(chapter) = book.load_chapter(*chapter_id) {
            let chapter_frags = build_chapter_entities(
                &chapter,
                *chapter_id,
                section_name,
                &mut ctx,
            );
            fragments.extend(chapter_frags);
        }
    }

    // 2f. Resource fragments (images, fonts, etc.)
    // Each resource gets two entities: external_resource (metadata) + bcRawMedia (bytes)
    for asset_path in &asset_paths {
        if is_media_asset(asset_path) {
            if let Ok(data) = book.load_asset(asset_path) {
                let href = asset_path.to_string_lossy().to_string();
                // external_resource ($164) - metadata about the resource
                fragments.push(build_external_resource_fragment(&href, &data, &mut ctx));
                // bcRawMedia ($417) - the actual bytes
                fragments.push(build_resource_fragment(&href, &data, &mut ctx));
            }
        }
    }

    // 2g. Anchor fragments (for internal link targets)
    fragments.extend(build_anchor_fragments(&ctx));

    // Build symbol table ION using context
    let local_syms = ctx.symbols.local_symbols();
    let symtab_ion = build_symbol_table_ion(local_syms);

    // Build format capabilities ION
    let format_caps_ion = build_format_capabilities_ion();

    // Serialize fragments to entities
    let entities = serialize_fragments(&fragments, ctx.symbols.local_symbols());

    // ========================================================================
    // PASS 3: SERIALIZATION
    // ========================================================================

    Ok(serialize_container(
        &container_id,
        &entities,
        &symtab_ion,
        &format_caps_ion,
    ))
}

// ============================================================================
// Pass 1: Survey Functions (NO ION GENERATION)
// ============================================================================

/// Survey a chapter during Pass 1.
///
/// This walks the IR tree to:
/// - Assign a fragment ID to this chapter
/// - Build position map entries for every node
/// - Intern all text and attribute strings
/// - Track text offsets for link resolution
///
/// NO ION GENERATION happens here.
fn survey_chapter(chapter: &IRChapter, chapter_id: ChapterId, ctx: &mut ExportContext) {
    // Begin surveying this chapter
    let _fragment_id = ctx.begin_chapter_survey(chapter_id);

    // Walk the IR tree
    survey_node(chapter, chapter.root(), ctx);

    // End surveying
    ctx.end_chapter_survey();
}

/// Recursively survey a node and its children.
fn survey_node(chapter: &IRChapter, node_id: NodeId, ctx: &mut ExportContext) {
    let node = match chapter.node(node_id) {
        Some(n) => n,
        None => return,
    };

    // Skip root node processing but walk children
    if node.role == Role::Root {
        for child in chapter.children(node_id) {
            survey_node(chapter, child, ctx);
        }
        return;
    }

    // Record position for this node (for link targets)
    ctx.record_position(node_id);

    // If node has an anchor ID, record it
    if let Some(anchor_id) = chapter.semantics.id(node_id) {
        ctx.record_anchor(anchor_id, node_id);
    }

    // Intern semantic attributes
    if let Some(href) = chapter.semantics.href(node_id) {
        ctx.intern(href);
    }
    if let Some(src) = chapter.semantics.src(node_id) {
        ctx.intern(src);
        // Also register as resource
        ctx.resource_registry.register(src, &mut ctx.symbols);
    }
    if let Some(alt) = chapter.semantics.alt(node_id) {
        ctx.intern(alt);
    }

    // Track text content and advance offset
    if !node.text.is_empty() {
        let text = chapter.text(node.text);
        ctx.advance_text_offset(text.len());
        // We don't need to intern plain text content
    }

    // Recurse into children
    for child in chapter.children(node_id) {
        survey_node(chapter, child, ctx);
    }
}

/// Register TOC strings in the symbol table.
fn register_toc_symbols(entries: &[crate::book::TocEntry], ctx: &mut ExportContext) {
    for entry in entries {
        ctx.intern(&entry.title);
        ctx.intern(&entry.href);
        if !entry.children.is_empty() {
            register_toc_symbols(&entry.children, ctx);
        }
    }
}

/// Build the metadata fragment.
fn build_metadata_fragment(book: &Book) -> KfxFragment {
    let mut fields = Vec::new();
    let meta = book.metadata();

    // Add title
    fields.push((
        KfxSymbol::Title as u64,
        IonValue::String(meta.title.clone()),
    ));

    // Add author if present (first author only for simplicity)
    if let Some(author) = meta.authors.first() {
        fields.push((
            KfxSymbol::Author as u64,
            IonValue::String(author.clone()),
        ));
    }

    // Add language
    fields.push((
        KfxSymbol::Language as u64,
        IonValue::String(meta.language.clone()),
    ));

    // Add publisher if present
    if let Some(publisher) = &meta.publisher {
        fields.push((
            KfxSymbol::Publisher as u64,
            IonValue::String(publisher.to_string()),
        ));
    }

    // Create book_metadata wrapper
    let book_metadata = IonValue::Struct(fields);

    // Wrap in metadata struct
    let metadata = IonValue::Struct(vec![(KfxSymbol::BookMetadata as u64, book_metadata)]);

    KfxFragment::singleton(KfxSymbol::Metadata, metadata)
}

/// Build the format capabilities fragment.
fn build_format_capabilities_fragment() -> KfxFragment {
    // Minimal format capabilities
    let features = IonValue::List(vec![]);

    let format_caps = IonValue::Struct(vec![
        (KfxSymbol::MajorVersion as u64, IonValue::Int(1)),
        (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
        (KfxSymbol::Features as u64, features),
    ]);

    KfxFragment::singleton(KfxSymbol::FormatCapabilities, format_caps)
}

/// Build the reading orders fragment.
fn build_reading_orders_fragment(book: &Book, ctx: &ExportContext) -> KfxFragment {
    // Build section list from context's registered sections
    let sections: Vec<IonValue> = if ctx.section_ids.is_empty() {
        // Fallback: use spine indices if no sections registered
        book.spine()
            .iter()
            .enumerate()
            .map(|(i, _)| {
                IonValue::Symbol(crate::kfx::symbols::KFX_SYMBOL_TABLE_SIZE as u64 + i as u64)
            })
            .collect()
    } else {
        ctx.section_ids
            .iter()
            .map(|&id| IonValue::Symbol(id))
            .collect()
    };

    // reading_order_name should be a STRING (not a symbol) per KFX spec
    // Only the section REFERENCES need to be symbols
    let reading_order = IonValue::Struct(vec![
        (
            KfxSymbol::ReadingOrderName as u64,
            IonValue::String("default".to_string()),
        ),
        (KfxSymbol::Sections as u64, IonValue::List(sections)),
    ]);

    let reading_orders = IonValue::List(vec![reading_order]);

    KfxFragment::singleton(KfxSymbol::ReadingOrders, reading_orders)
}

/// Build the book navigation fragment with resolved positions.
///
/// Uses ctx.position_map to generate correct fid:off positions for TOC entries.
fn build_book_navigation_fragment_with_positions(book: &Book, ctx: &ExportContext) -> KfxFragment {
    let mut nav_containers = Vec::new();

    // Add TOC nav container if there are TOC entries
    if !book.toc().is_empty() {
        let toc_entries = build_toc_entries_with_positions(book.toc(), ctx);
        let toc_container = IonValue::Struct(vec![
            (
                KfxSymbol::NavType as u64,
                IonValue::Symbol(KfxSymbol::Toc as u64),
            ),
            (
                KfxSymbol::NavContainerName as u64,
                IonValue::String("toc".to_string()),
            ),
            (KfxSymbol::Entries as u64, IonValue::List(toc_entries)),
        ]);
        nav_containers.push(toc_container);
    }

    // Add headings nav container (empty for now)
    let headings_container = IonValue::Struct(vec![
        (
            KfxSymbol::NavType as u64,
            IonValue::Symbol(KfxSymbol::Headings as u64),
        ),
        (
            KfxSymbol::NavContainerName as u64,
            IonValue::String("headings".to_string()),
        ),
        (KfxSymbol::Entries as u64, IonValue::List(vec![])),
    ]);
    nav_containers.push(headings_container);

    let book_nav = IonValue::Struct(vec![(
        KfxSymbol::NavContainers as u64,
        IonValue::List(nav_containers),
    )]);

    KfxFragment::singleton(KfxSymbol::BookNavigation, book_nav)
}

/// Build TOC entries recursively with resolved positions.
fn build_toc_entries_with_positions(
    entries: &[crate::book::TocEntry],
    ctx: &ExportContext,
) -> Vec<IonValue> {
    entries
        .iter()
        .map(|entry| {
            let mut fields = Vec::new();

            // Add representation with label
            let representation = IonValue::Struct(vec![(
                KfxSymbol::Label as u64,
                IonValue::String(entry.title.clone()),
            )]);
            fields.push((KfxSymbol::Representation as u64, representation));

            // Resolve target position from href
            // The href might be "chapter1.xhtml#anchor" or just "chapter1.xhtml"
            let (fragment_id, offset) = resolve_toc_position(&entry.href, ctx);

            let target = IonValue::Struct(vec![
                (KfxSymbol::Id as u64, IonValue::Int(fragment_id as i64)),
                (KfxSymbol::Offset as u64, IonValue::Int(offset as i64)),
            ]);
            fields.push((KfxSymbol::TargetPosition as u64, target));

            // Add children if present
            if !entry.children.is_empty() {
                let child_entries = build_toc_entries_with_positions(&entry.children, ctx);
                fields.push((KfxSymbol::Entries as u64, IonValue::List(child_entries)));
            }

            IonValue::Struct(fields)
        })
        .collect()
}

/// Resolve a TOC href to (fragment_id, offset).
fn resolve_toc_position(href: &str, ctx: &ExportContext) -> (u64, usize) {
    // Extract base path and anchor from href
    let (base_path, _anchor) = if let Some(hash_pos) = href.find('#') {
        (&href[..hash_pos], Some(&href[hash_pos + 1..]))
    } else {
        (href, None)
    };

    // Try to find the chapter fragment ID by matching section name
    // The section IDs in ctx correspond to source paths
    if let Some(symbol_id) = ctx.symbols.get(base_path) {
        // Find which chapter this corresponds to
        for (&chapter_id, &frag_id) in &ctx.chapter_fragments {
            // Check if this chapter's section_id matches
            if ctx.section_ids.iter().any(|&sid| sid == symbol_id) {
                // For now, return offset 0 for the chapter start
                // TODO: If we have an anchor, look it up in position_map
                return (frag_id, 0);
            }
            // Suppress unused warning
            let _ = chapter_id;
        }
    }

    // Fallback: first fragment or 0
    if let Some(&frag_id) = ctx.chapter_fragments.values().next() {
        (frag_id, 0)
    } else {
        (0, 0)
    }
}

// ============================================================================
// Entity Assembler: Packages Schema output into KFX Entity Hierarchy
// ============================================================================

/// Build the three KFX entities for a chapter: Content, Storyline, Section.
///
/// This is the "Assembler" (Macro layer) that:
/// 1. Sets up naming for this chapter's entity triad
/// 2. Calls schema-driven token generation (`ir_to_tokens`)
/// 3. Calls `tokens_to_ion` which SPLITS data:
///    - Structure → Ion (for Storyline)
///    - Text → ctx.text_accumulator (for Content)
/// 4. Packages results into three KFX fragments
///
/// The Assembler knows about KFX Entity topology but NOT about element semantics.
/// Element semantics are handled by the Schema.
fn build_chapter_entities(
    chapter: &IRChapter,
    chapter_id: ChapterId,
    section_name: &str,
    ctx: &mut ExportContext,
) -> Vec<KfxFragment> {
    use crate::kfx::storyline::{ir_to_tokens, tokens_to_ion};

    let mut fragments = Vec::new();

    // =========================================================================
    // 1. SETUP: Naming for this chapter's entity triad
    // =========================================================================
    let story_name = format!("story_{}", section_name);
    let content_name = format!("content_{}", section_name);

    let section_name_symbol = ctx.symbols.get_or_intern(section_name);
    let story_name_symbol = ctx.symbols.get_or_intern(&story_name);
    let content_name_symbol = ctx.symbols.get_or_intern(&content_name);

    // Tell tokens_to_ion what content name to use for references
    ctx.begin_chapter(&content_name);

    // Get the section fragment ID assigned during Pass 1
    let section_id = ctx.get_chapter_fragment(chapter_id).unwrap_or_else(|| ctx.next_fragment_id());

    // =========================================================================
    // 2. GENERATE: Schema-driven token generation + text/structure split
    // =========================================================================
    // ir_to_tokens uses the Schema to convert IR → Tokens
    // tokens_to_ion SPLITS: Structure → Ion, Text → ctx.text_accumulator
    let tokens = ir_to_tokens(chapter, ctx);
    let storyline_content_list = tokens_to_ion(&tokens, ctx);

    // Drain the accumulated text strings (captured during tokens_to_ion)
    let content_strings = ctx.drain_text();

    // =========================================================================
    // 3. ASSEMBLE: Package into three KFX Entities
    // =========================================================================

    // Entity A: CONTENT ($145) - Holds the raw text strings
    if !content_strings.is_empty() {
        let content_ion = IonValue::Struct(vec![
            (KfxSymbol::Name as u64, IonValue::Symbol(content_name_symbol)),
            (
                KfxSymbol::ContentList as u64,
                IonValue::List(content_strings.into_iter().map(IonValue::String).collect()),
            ),
        ]);
        fragments.push(KfxFragment::new(KfxSymbol::Content, &content_name, content_ion));
    }

    // Entity B: STORYLINE ($259) - Holds the structure, references Content by name
    let storyline_ion = IonValue::Struct(vec![
        (KfxSymbol::StoryName as u64, IonValue::Symbol(story_name_symbol)),
        (KfxSymbol::ContentList as u64, storyline_content_list),
    ]);
    fragments.push(KfxFragment::new(KfxSymbol::Storyline, &story_name, storyline_ion));

    // Entity C: SECTION ($260) - Entry point, references Storyline by story_name
    let page_template = IonValue::Struct(vec![
        (KfxSymbol::Id as u64, IonValue::Int(section_id as i64)),
        (KfxSymbol::StoryName as u64, IonValue::Symbol(story_name_symbol)),
        (KfxSymbol::Type as u64, IonValue::Symbol(KfxSymbol::Text as u64)),
    ]);

    let section_ion = IonValue::Struct(vec![
        (KfxSymbol::SectionName as u64, IonValue::Symbol(section_name_symbol)),
        (KfxSymbol::PageTemplates as u64, IonValue::List(vec![page_template])),
    ]);
    fragments.push(KfxFragment::new_with_id(
        KfxSymbol::Section,
        section_id,
        section_name,
        section_ion,
    ));

    fragments
}


/// Build the $ion_symbol_table ION.
fn build_symbol_table_ion(local_symbols: &[String]) -> Vec<u8> {
    // Import statement for YJ_symbols
    let imports = IonValue::List(vec![IonValue::Struct(vec![
        (
            KfxSymbol::Name as u64,
            IonValue::String("YJ_symbols".to_string()),
        ),
        (KfxSymbol::Version as u64, IonValue::Int(10)),
        (
            KfxSymbol::MaxId as u64,
            // max_id is the highest symbol ID, not the count
            IonValue::Int(crate::kfx::symbols::KFX_MAX_SYMBOL_ID as i64),
        ),
    ])]);

    // Local symbols list
    let symbols = IonValue::List(
        local_symbols
            .iter()
            .map(|s| IonValue::String(s.clone()))
            .collect(),
    );

    let symtab = IonValue::Struct(vec![
        (KfxSymbol::Imports as u64, imports),
        (KfxSymbol::Symbols as u64, symbols),
    ]);

    // Annotate with $ion_symbol_table (symbol 3)
    serialize_annotated_ion(3, &symtab)
}

/// Build format capabilities ION.
fn build_format_capabilities_ion() -> Vec<u8> {
    let caps = IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String("yj".to_string()),
        ),
        (KfxSymbol::MajorVersion as u64, IonValue::Int(1)),
        (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
        (KfxSymbol::Features as u64, IonValue::List(vec![])),
    ]);

    // Annotate with $593 (format_capabilities)
    serialize_annotated_ion(KfxSymbol::FormatCapabilities as u64, &caps)
}

/// Build an external_resource fragment ($164) - metadata about a resource.
fn build_external_resource_fragment(href: &str, data: &[u8], ctx: &mut ExportContext) -> KfxFragment {
    // Generate a short resource name (e.g., "e0", "e1", etc.)
    let resource_name = generate_resource_name(href, ctx);
    let resource_name_symbol = ctx.symbols.get_or_intern(&resource_name);

    let mut fields = Vec::new();

    // resource_name - the symbolic name for this resource
    fields.push((
        KfxSymbol::ResourceName as u64,
        IonValue::Symbol(resource_name_symbol),
    ));

    // location - path to the bcRawMedia entity
    let location = format!("resource/{}", resource_name);
    fields.push((
        KfxSymbol::Location as u64,
        IonValue::String(location),
    ));

    // format - file type symbol
    let format_symbol = detect_format_symbol(href, data);
    fields.push((KfxSymbol::Format as u64, IonValue::Symbol(format_symbol)));

    // For images, try to extract dimensions
    if let Some((width, height)) = crate::util::extract_image_dimensions(data) {
        fields.push((KfxSymbol::ResourceWidth as u64, IonValue::Int(width as i64)));
        fields.push((KfxSymbol::ResourceHeight as u64, IonValue::Int(height as i64)));
    }

    // mime type for images
    if let Some(mime) = crate::util::detect_mime_type(href, data) {
        fields.push((
            KfxSymbol::Mime as u64,
            IonValue::String(mime.to_string()),
        ));
    }

    let ion = IonValue::Struct(fields);
    KfxFragment::new(KfxSymbol::ExternalResource, &resource_name, ion)
}

/// Build a resource fragment (bcRawMedia $417) - the actual bytes.
fn build_resource_fragment(href: &str, data: &[u8], ctx: &mut ExportContext) -> KfxFragment {
    // Use the same resource name as external_resource
    let resource_name = generate_resource_name(href, ctx);

    // Register the resource
    ctx.resource_registry.register(href, &mut ctx.symbols);

    // Create raw fragment for binary resources
    KfxFragment::raw(KfxSymbol::Bcrawmedia as u64, &resource_name, data.to_vec())
}

/// Build anchor fragments ($266) for all recorded anchors.
fn build_anchor_fragments(ctx: &ExportContext) -> Vec<KfxFragment> {
    let mut fragments = Vec::new();

    // Create anchor for each entry in the anchor_map
    for (anchor_id, (chapter_id, node_id)) in &ctx.anchor_map {
        // Look up the position for this anchor
        if let Some(position) = ctx.position_map.get(&(*chapter_id, *node_id)) {
            // Get the interned symbol for the anchor name
            if let Some(anchor_symbol) = ctx.symbols.get(anchor_id) {
                let mut pos_fields = Vec::new();
                pos_fields.push((KfxSymbol::Id as u64, IonValue::Int(position.fragment_id as i64)));
                if position.offset > 0 {
                    pos_fields.push((KfxSymbol::Offset as u64, IonValue::Int(position.offset as i64)));
                }

                let ion = IonValue::Struct(vec![
                    (KfxSymbol::AnchorName as u64, IonValue::Symbol(anchor_symbol)),
                    (KfxSymbol::Position as u64, IonValue::Struct(pos_fields)),
                ]);

                fragments.push(KfxFragment::new(KfxSymbol::Anchor, anchor_id, ion));
            }
        }
    }

    fragments
}

/// Generate a short resource name for a given href.
fn generate_resource_name(href: &str, ctx: &mut ExportContext) -> String {
    // Check if we already have a name for this resource
    if let Some(_) = ctx.resource_registry.get(href) {
        // Extract existing name from the resource:href symbol
        // For simplicity, generate based on registry count
    }

    // Generate short name like "e0", "e1", etc.
    let count = ctx.resource_registry.iter().count();
    format!("e{}", count)
}

/// Detect format symbol from file extension/magic bytes.
fn detect_format_symbol(href: &str, data: &[u8]) -> u64 {
    let href_lower = href.to_lowercase();

    if href_lower.ends_with(".jpg") || href_lower.ends_with(".jpeg") {
        return ctx_intern_static("jpg");
    }
    if href_lower.ends_with(".png") {
        return ctx_intern_static("png");
    }
    if href_lower.ends_with(".gif") {
        return ctx_intern_static("gif");
    }
    if href_lower.ends_with(".svg") {
        return ctx_intern_static("svg");
    }
    if href_lower.ends_with(".webp") {
        return ctx_intern_static("webp");
    }

    // Check magic bytes
    if data.len() >= 4 {
        if data[0] == 0xFF && data[1] == 0xD8 {
            return ctx_intern_static("jpg");
        }
        if data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47 {
            return ctx_intern_static("png");
        }
        if data[0] == 0x47 && data[1] == 0x49 && data[2] == 0x46 {
            return ctx_intern_static("gif");
        }
    }

    ctx_intern_static("bin")
}

/// Get symbol ID for static format strings.
/// These are common symbols that should be in the shared table.
fn ctx_intern_static(s: &str) -> u64 {
    // Look up in shared symbol table
    match s {
        "jpg" => KfxSymbol::Jpg as u64,
        "png" => KfxSymbol::Png as u64,
        "gif" => KfxSymbol::Gif as u64,
        // svg and other formats not in shared table, use jpg as fallback
        _ => KfxSymbol::Jpg as u64,
    }
}

/// Check if a path is a media asset (image, font, etc.)
fn is_media_asset(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "ttf" | "otf" | "woff" | "woff2"
    )
}

/// Serialize fragments to entities.
fn serialize_fragments(
    fragments: &[KfxFragment],
    local_symbols: &[String],
) -> Vec<SerializedEntity> {
    fragments
        .iter()
        .map(|frag| {
            let id = if frag.is_singleton() {
                0 // Singleton marker
            } else {
                // Look up local symbol ID
                local_symbols
                    .iter()
                    .position(|s| s == &frag.fid)
                    .map(|i| (crate::kfx::symbols::KFX_SYMBOL_TABLE_SIZE + i) as u32)
                    .unwrap_or(0)
            };

            let data = match &frag.data {
                crate::kfx::fragment::FragmentData::Ion(value) => create_entity_data(value),
                crate::kfx::fragment::FragmentData::Raw(bytes) => {
                    crate::kfx::serialization::create_raw_media_data(bytes)
                }
            };

            SerializedEntity {
                id,
                entity_type: frag.ftype as u32,
                data,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_symbol_table_ion() {
        let symbols = vec!["section-1".to_string(), "section-2".to_string()];
        let ion = build_symbol_table_ion(&symbols);

        // Should start with Ion BVM
        assert_eq!(&ion[..4], &[0xe0, 0x01, 0x00, 0xea]);
    }

    #[test]
    fn test_build_format_capabilities_ion() {
        let ion = build_format_capabilities_ion();

        // Should start with Ion BVM
        assert_eq!(&ion[..4], &[0xe0, 0x01, 0x00, 0xea]);
    }
}
