//! KFX format exporter.
//!
//! This module provides the `KfxExporter` which implements the `Exporter` trait
//! for writing books in Amazon's KFX format.

use std::collections::HashMap;
use std::io::{self, Seek, Write};

use crate::book::{Book, LandmarkType};
use crate::export::Exporter;
use crate::import::ChapterId;
use crate::ir::{IRChapter, NodeId, Role};
use crate::kfx::context::{ExportContext, LandmarkTarget};
use crate::kfx::cover::{build_cover_section, is_image_only_chapter, needs_standalone_cover, normalize_cover_path};
use crate::kfx::fragment::KfxFragment;
use crate::kfx::ion::IonValue;
use crate::kfx::serialization::{
    create_entity_data, generate_container_id, serialize_annotated_ion, serialize_container,
    SerializedEntity,
};
use crate::kfx::metadata::{build_category_entries, generate_book_id, MetadataCategory, MetadataContext};
use crate::kfx::symbols::KfxSymbol;
use crate::kfx::transforms::format_to_kfx_symbol;
use crate::util::detect_media_format;

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

    // Check if we need a standalone cover section
    // This happens when the EPUB cover image differs from the first spine chapter's image
    let asset_paths: Vec<_> = book.list_assets();
    let cover_image = book.metadata().cover_image.clone();
    let first_chapter_id = book.spine().first().map(|e| e.id);

    let standalone_cover_path: Option<String> = match (cover_image, first_chapter_id) {
        (Some(cover_img), Some(first_id)) => {
            let normalized = normalize_cover_path(&cover_img, &asset_paths);
            book.load_chapter(first_id).ok().and_then(|first_chapter| {
                if needs_standalone_cover(&normalized, &first_chapter) {
                    Some(normalized)
                } else {
                    None
                }
            })
        }
        _ => None,
    };

    // If standalone cover needed, section offset starts at 1 (c0 reserved for cover)
    let section_offset = if standalone_cover_path.is_some() { 1 } else { 0 };

    // Collect spine info with appropriate offset
    // Generate clean short section names (like 'c0', 'c1', etc.)
    let spine_info: Vec<_> = book
        .spine()
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            // Use short identifiers like the reference KFX files do
            let section_name = format!("c{}", idx + section_offset);
            (entry.id, section_name)
        })
        .collect();

    // Register cover section (c0) in Pass 1 if standalone cover is needed
    // This ensures it appears in reading_orders.sections and landmarks point to it
    if standalone_cover_path.is_some() {
        ctx.register_section("c0");
        // Assign fragment ID for cover section now (used by landmarks)
        let cover_section_id = ctx.next_fragment_id();
        ctx.cover_fragment_id = Some(cover_section_id);
        // Register Cover landmark pointing to the standalone cover section
        ctx.landmark_fragments.insert(
            LandmarkType::Cover,
            LandmarkTarget {
                fragment_id: cover_section_id,
                offset: 0,
                label: "cover-nav-unit".to_string(),
            },
        );
    }

    // 1a. Collect needed anchors FIRST (before survey)
    // Only IDs that are link targets or TOC destinations need anchor entities.
    // This prevents creating anchors for every element ID in the source.
    collect_needed_anchors_from_toc(book.toc(), &mut ctx);
    for (chapter_id, _) in &spine_info {
        if let Ok(chapter) = book.load_chapter(*chapter_id) {
            collect_needed_anchors_from_chapter(&chapter, chapter.root(), &mut ctx);
        }
    }

    // 1b. Survey each chapter: assign fragment IDs, build position map
    // Also build a map from source paths to chapter IDs for landmark resolution
    let mut source_to_chapter: HashMap<String, ChapterId> = HashMap::new();

    for (chapter_id, section_name) in &spine_info {
        // Register section name as symbol
        let _section_id = ctx.register_section(section_name);

        // Get the source path for this chapter (for TOC resolution)
        let source_path = book.source_id(*chapter_id).unwrap_or("").to_string();

        // Map source path to chapter ID for landmark resolution
        if !source_path.is_empty() {
            source_to_chapter.insert(source_path.clone(), *chapter_id);
        }

        // Load and survey chapter
        if let Ok(chapter) = book.load_chapter(*chapter_id) {
            survey_chapter(&chapter, *chapter_id, &source_path, &mut ctx);
        }
    }

    // 1b2. Resolve landmarks to fragment IDs
    // First try IR landmarks, then fall back to heuristics for Cover/StartReading
    resolve_landmarks_from_ir(book, &source_to_chapter, &mut ctx);

    // Fall back to heuristics if IR didn't provide Cover or StartReading
    let has_cover = ctx.landmark_fragments.contains_key(&LandmarkType::Cover);
    let has_srl = ctx.landmark_fragments.contains_key(&LandmarkType::StartReading);

    if !has_cover || !has_srl {
        for (chapter_id, _section_name) in &spine_info {
            if let Ok(chapter) = book.load_chapter(*chapter_id) {
                let is_cover = is_image_only_chapter(&chapter);
                let fragment_id = ctx.chapter_fragments.get(chapter_id).copied();

                if let Some(fid) = fragment_id {
                    if is_cover && !ctx.landmark_fragments.contains_key(&LandmarkType::Cover) {
                        ctx.landmark_fragments.insert(
                            LandmarkType::Cover,
                            LandmarkTarget {
                                fragment_id: fid,
                                offset: 0,
                                label: "cover-nav-unit".to_string(),
                            },
                        );
                    } else if !is_cover
                        && !ctx.landmark_fragments.contains_key(&LandmarkType::StartReading)
                    {
                        ctx.landmark_fragments.insert(
                            LandmarkType::StartReading,
                            LandmarkTarget {
                                fragment_id: fid,
                                offset: 0,
                                label: book.metadata().title.clone(),
                            },
                        );
                    }
                }

                // Stop once we have both
                if ctx.landmark_fragments.contains_key(&LandmarkType::Cover)
                    && ctx.landmark_fragments.contains_key(&LandmarkType::StartReading)
                {
                    break;
                }
            }
        }
    }

    // 1c. Register TOC strings
    register_toc_symbols(book.toc(), &mut ctx);

    // 1d. Register nav container names as symbols
    ctx.nav_container_symbols.toc = ctx.symbols.get_or_intern("toc");
    ctx.nav_container_symbols.headings = ctx.symbols.get_or_intern("headings");
    ctx.nav_container_symbols.landmarks = ctx.symbols.get_or_intern("landmarks");

    // 1e. Register resource paths and create short names
    // IMPORTANT: Short names must be interned during Pass 1 to ensure
    // consistent symbol IDs when they're referenced later in storylines
    let asset_paths: Vec<_> = book.list_assets();
    for asset_path in &asset_paths {
        if is_media_asset(asset_path) {
            let href = asset_path.to_string_lossy().to_string();
            ctx.resource_registry.register(&href, &mut ctx.symbols);
            // Create and intern the short name (e.g., "e0")
            let short_name = ctx.resource_registry.get_or_create_name(&href);
            ctx.symbols.get_or_intern(&short_name);
        }
    }

    // After Pass 1: ctx.symbols is COMPLETE, ctx.position_map has all EIDs

    // ========================================================================
    // PASS 2: SYNTHESIS (Generate Ion)
    // Now ctx.position_map is populated. We can resolve links correctly.
    // ========================================================================

    let mut fragments = Vec::new();

    // Entity order matches reference KFX:
    // 1. content_features ($585)
    // 2. book_metadata ($490)
    // 3. metadata ($258)
    // 4. document_data ($538)
    // 5. book_navigation ($389)
    // 6+. sections ($260) - all together
    // N+. storylines ($259) - all together
    // M+. content ($145) - all together

    // 2a. Content features fragment ($585)
    fragments.push(build_content_features_fragment());

    // 2b. Book metadata fragment ($490) - contains categorised_metadata
    fragments.push(build_book_metadata_fragment(book, &container_id, &ctx));

    // 2c. Metadata fragment ($258) - contains reading_orders
    fragments.push(build_metadata_fragment(&ctx));

    // 2d. Document data fragment ($538) - contains document settings
    fragments.push(build_document_data_fragment(&ctx));

    // 2e. Book navigation fragment (uses ctx.position_map for TOC links)
    fragments.push(build_book_navigation_fragment_with_positions(book, &ctx));

    // 2f. Chapter entities - collect separately for proper grouping
    // Note: This also collects styles during token generation
    let mut section_fragments = Vec::new();
    let mut storyline_fragments = Vec::new();
    let mut content_fragments = Vec::new();

    // Generate standalone cover section if needed (c0)
    // Note: cover_fragment_id was assigned in Pass 1 for landmark resolution
    if let Some(ref cover_path) = standalone_cover_path {
        let section_id = ctx.cover_fragment_id.expect("cover_fragment_id should be set in Pass 1");
        let (section, storyline) = build_cover_section(cover_path, section_id, &mut ctx);
        section_fragments.push(section);
        storyline_fragments.push(storyline);
    }

    for (chapter_id, section_name) in &spine_info {
        if let Ok(chapter) = book.load_chapter(*chapter_id) {
            let (section, storyline, content) = build_chapter_entities_grouped(
                &chapter,
                *chapter_id,
                section_name,
                &mut ctx,
            );
            section_fragments.push(section);
            storyline_fragments.push(storyline);
            if let Some(c) = content {
                content_fragments.push(c);
            }
        }
    }

    // 2g. Style entities ($157) - generated AFTER chapters since styles are collected during token generation
    // This includes the default style plus any unique styles found in the content
    let style_fragments = build_style_fragments(&mut ctx);
    fragments.extend(style_fragments);

    // Add chapter content in grouped order: sections, then storylines, then content
    fragments.extend(section_fragments);
    fragments.extend(storyline_fragments);
    fragments.extend(content_fragments);

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
    // Also collect fragment_id → anchor symbol IDs for position_map
    let (anchor_frags, anchor_ids_by_fragment) = build_anchor_fragments(&mut ctx);
    fragments.extend(anchor_frags);

    // 2h. Navigation maps for reader functionality
    fragments.push(build_position_map_fragment(&ctx, &anchor_ids_by_fragment));
    fragments.push(build_position_id_map_fragment(&ctx));
    fragments.push(build_location_map_fragment(&ctx));

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
fn survey_chapter(chapter: &IRChapter, chapter_id: ChapterId, source_path: &str, ctx: &mut ExportContext) {
    // Begin surveying this chapter (with source path for TOC resolution)
    let _fragment_id = ctx.begin_chapter_survey(chapter_id, source_path);

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

    // Track heading positions for headings navigation
    if let Role::Heading(level) = node.role {
        ctx.record_heading(level);
    }

    // If node has an anchor ID, record it for link resolution
    if let Some(anchor_id) = chapter.semantics.id(node_id) {
        ctx.record_anchor(anchor_id, node_id);

        // Also resolve in AnchorRegistry if this anchor is a link target
        if ctx.anchor_registry.is_anchor_needed(anchor_id) {
            ctx.anchor_registry.resolve_anchor(
                anchor_id,
                ctx.current_fragment_id(),
                ctx.current_text_offset(),
            );
        }
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

/// Collect needed anchors from a chapter's href attributes.
/// Anchors are only needed if they are targets of links.
///
/// Also registers link targets with the AnchorRegistry to generate
/// anchor symbols for use in style_events.
fn collect_needed_anchors_from_chapter(chapter: &IRChapter, node_id: NodeId, ctx: &mut ExportContext) {
    if chapter.node(node_id).is_none() {
        return;
    }

    // Check for href with fragment (internal link target)
    if let Some(href) = chapter.semantics.href(node_id) {
        // Register with AnchorRegistry to generate a symbol for this link target
        ctx.anchor_registry.register_link_target(href);

        // Also register with the old system for backwards compatibility
        if let Some(hash_pos) = href.find('#') {
            let anchor = &href[hash_pos + 1..];
            if !anchor.is_empty() {
                ctx.register_needed_anchor(anchor);
            }
        }
    }

    // Recurse into children
    for child in chapter.children(node_id) {
        collect_needed_anchors_from_chapter(chapter, child, ctx);
    }
}

/// Collect needed anchors from TOC entries.
fn collect_needed_anchors_from_toc(entries: &[crate::book::TocEntry], ctx: &mut ExportContext) {
    for entry in entries {
        // Extract anchor from href (e.g., "chapter1.xhtml#section2" -> "section2")
        if let Some(hash_pos) = entry.href.find('#') {
            let anchor = &entry.href[hash_pos + 1..];
            if !anchor.is_empty() {
                ctx.register_needed_anchor(anchor);
            }
        }
        if !entry.children.is_empty() {
            collect_needed_anchors_from_toc(&entry.children, ctx);
        }
    }
}

/// Build style fragments from the registry.
///
/// KFX requires every storyline element to have a style reference.
/// This generates all collected styles from the registry, including the default.
fn build_style_fragments(ctx: &mut ExportContext) -> Vec<KfxFragment> {
    // Drain all styles from the registry to generate Ion fragments
    let style_pairs = ctx.style_registry.drain_to_ion();

    style_pairs
        .into_iter()
        .map(|(name, ion)| KfxFragment::new(KfxSymbol::Style, &name, ion))
        .collect()
}

/// Build the metadata fragment ($258) - contains reading_orders.
fn build_metadata_fragment(ctx: &ExportContext) -> KfxFragment {
    // Build section list from context's registered sections
    let sections: Vec<IonValue> = ctx.section_ids
        .iter()
        .map(|&id| IonValue::Symbol(id))
        .collect();

    // reading_order_name should be a STRING (not a symbol) per KFX spec
    let reading_order = IonValue::Struct(vec![
        (
            KfxSymbol::ReadingOrderName as u64,
            IonValue::Symbol(KfxSymbol::Default as u64),
        ),
        (KfxSymbol::Sections as u64, IonValue::List(sections)),
    ]);

    let reading_orders = IonValue::List(vec![reading_order]);

    // $258 (metadata) contains reading_orders directly
    let metadata = IonValue::Struct(vec![
        (KfxSymbol::ReadingOrders as u64, reading_orders),
    ]);

    KfxFragment::singleton(KfxSymbol::Metadata, metadata)
}

/// Build the book metadata fragment ($490) - contains categorised_metadata.
///
/// Uses the metadata schema to map IR metadata to KFX categories.
/// To add new metadata fields, update the schema in `kfx/metadata.rs`.
fn build_book_metadata_fragment(book: &Book, container_id: &str, ctx: &ExportContext) -> KfxFragment {
    let meta = book.metadata();

    // Build metadata context with transformed values
    // Cover path in metadata may not match the registered resource path exactly.
    // Try common path variations (with/without epub/ prefix, etc.)
    let cover_resource_name = meta.cover_image.as_ref().and_then(|path| {
        // Try exact path first
        if let Some(name) = ctx.resource_registry.get_name(path) {
            return Some(name);
        }
        // Try with epub/ prefix
        let with_prefix = format!("epub/{}", path);
        if let Some(name) = ctx.resource_registry.get_name(&with_prefix) {
            return Some(name);
        }
        // Try stripping leading path components to match filename
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())?;
        // Search for a resource ending with this filename
        for (href, _) in ctx.resource_registry.iter() {
            if href.ends_with(filename) {
                return ctx.resource_registry.get_name(href);
            }
        }
        None
    });

    // Generate book_id from identifier (deterministic per publication)
    let book_id = if !meta.identifier.is_empty() {
        Some(generate_book_id(&meta.identifier))
    } else {
        None
    };

    let meta_ctx = MetadataContext {
        version: Some(env!("CARGO_PKG_VERSION")),
        cover_resource_name,
        asset_id: Some(container_id),
        book_id,
    };

    // Build each category using the schema
    let categories = [
        MetadataCategory::KindleEbook,
        MetadataCategory::KindleTitle,
        MetadataCategory::KindleAudit,
    ];

    let categorised: Vec<IonValue> = categories
        .iter()
        .map(|&cat| {
            let entries = build_category_entries(cat, meta, &meta_ctx);
            let ion_entries: Vec<IonValue> = entries
                .into_iter()
                .map(|(k, v)| metadata_kv(k, &v))
                .collect();

            IonValue::Struct(vec![
                (
                    KfxSymbol::Category as u64,
                    IonValue::String(cat.as_str().to_string()),
                ),
                (KfxSymbol::Metadata as u64, IonValue::List(ion_entries)),
            ])
        })
        .collect();

    let book_metadata = IonValue::Struct(vec![(
        KfxSymbol::CategorisedMetadata as u64,
        IonValue::List(categorised),
    )]);

    KfxFragment::singleton(KfxSymbol::BookMetadata, book_metadata)
}

/// Helper to create a metadata key-value struct.
fn metadata_kv(key: &str, value: &str) -> IonValue {
    IonValue::Struct(vec![
        (KfxSymbol::Key as u64, IonValue::String(key.to_string())),
        (KfxSymbol::Value as u64, IonValue::String(value.to_string())),
    ])
}

/// Build the content features fragment ($585).
///
/// This describes the content capabilities/features of the book.
fn build_content_features_fragment() -> KfxFragment {
    // Build feature entries matching reference KFX
    let reflow_style = IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String("com.amazon.yjconversion".to_string()),
        ),
        (
            KfxSymbol::Key as u64,
            IonValue::String("reflow-style".to_string()),
        ),
        (
            KfxSymbol::VersionInfo as u64,
            IonValue::Struct(vec![(
                KfxSymbol::Version as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::MajorVersion as u64, IonValue::Int(6)),
                    (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
                ]),
            )]),
        ),
    ]);

    let canonical_format = IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String("SDK.Marker".to_string()),
        ),
        (
            KfxSymbol::Key as u64,
            IonValue::String("CanonicalFormat".to_string()),
        ),
        (
            KfxSymbol::VersionInfo as u64,
            IonValue::Struct(vec![(
                KfxSymbol::Version as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::MajorVersion as u64, IonValue::Int(1)),
                    (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
                ]),
            )]),
        ),
    ]);

    let yj_hdv = IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String("com.amazon.yjconversion".to_string()),
        ),
        (KfxSymbol::Key as u64, IonValue::String("yj_hdv".to_string())),
        (
            KfxSymbol::VersionInfo as u64,
            IonValue::Struct(vec![(
                KfxSymbol::Version as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::MajorVersion as u64, IonValue::Int(1)),
                    (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
                ]),
            )]),
        ),
    ]);

    let content_features = IonValue::Struct(vec![(
        KfxSymbol::Features as u64,
        IonValue::List(vec![reflow_style, canonical_format, yj_hdv]),
    )]);

    KfxFragment::singleton(KfxSymbol::ContentFeatures, content_features)
}

/// Build the document data fragment ($538).
///
/// Contains document-level settings like direction, font size, line height, max_id.
fn build_document_data_fragment(ctx: &ExportContext) -> KfxFragment {
    // Build section list from context's registered sections
    let sections: Vec<IonValue> = ctx
        .section_ids
        .iter()
        .map(|&id| IonValue::Symbol(id))
        .collect();

    let reading_order = IonValue::Struct(vec![
        (
            KfxSymbol::ReadingOrderName as u64,
            IonValue::Symbol(KfxSymbol::Default as u64),
        ),
        (KfxSymbol::Sections as u64, IonValue::List(sections)),
    ]);

    // Calculate max_id from context (highest EID used)
    let max_id = ctx.max_eid();

    let document_data = IonValue::Struct(vec![
        (
            KfxSymbol::Direction as u64,
            IonValue::Symbol(KfxSymbol::Ltr as u64),
        ),
        (
            KfxSymbol::ColumnCount as u64,
            IonValue::Symbol(KfxSymbol::Auto as u64),
        ),
        (
            KfxSymbol::FontSize as u64,
            IonValue::Struct(vec![
                (KfxSymbol::Value as u64, IonValue::Decimal("1".to_string())),
                (KfxSymbol::Unit as u64, IonValue::Symbol(KfxSymbol::Em as u64)),
            ]),
        ),
        (
            KfxSymbol::WritingMode as u64,
            IonValue::Symbol(KfxSymbol::HorizontalTb as u64),
        ),
        (
            KfxSymbol::Selection as u64,
            IonValue::Symbol(KfxSymbol::Enabled as u64),
        ),
        (KfxSymbol::MaxId as u64, IonValue::Int(max_id as i64)),
        (
            KfxSymbol::LineHeight as u64,
            IonValue::Struct(vec![
                (KfxSymbol::Value as u64, IonValue::Decimal("1.2".to_string())),
                (KfxSymbol::Unit as u64, IonValue::Symbol(KfxSymbol::Em as u64)),
            ]),
        ),
        (
            KfxSymbol::SpacingPercentBase as u64,
            IonValue::Symbol(KfxSymbol::Width as u64),
        ),
        (
            KfxSymbol::ReadingOrders as u64,
            IonValue::List(vec![reading_order]),
        ),
    ]);

    KfxFragment::singleton(KfxSymbol::DocumentData, document_data)
}

/// Build the book navigation fragment with resolved positions.
///
/// Uses ctx.position_map to generate correct fid:off positions for TOC entries.
/// Structure: [{reading_order_name: default, nav_containers: [nav_container::{...}, ...]}]
/// Order matches reference KFX: headings, toc, landmarks
fn build_book_navigation_fragment_with_positions(book: &Book, ctx: &ExportContext) -> KfxFragment {
    let mut nav_containers = Vec::new();

    // 1. Add headings nav container (first, per reference KFX order)
    let headings_entries = build_headings_entries(ctx);
    let headings_container = IonValue::Struct(vec![
        (
            KfxSymbol::NavType as u64,
            IonValue::Symbol(KfxSymbol::Headings as u64),
        ),
        (
            KfxSymbol::NavContainerName as u64,
            IonValue::Symbol(ctx.nav_container_symbols.headings),
        ),
        (KfxSymbol::Entries as u64, IonValue::List(headings_entries)),
    ]);
    let annotated = IonValue::Annotated(
        vec![KfxSymbol::NavContainer as u64],
        Box::new(headings_container),
    );
    nav_containers.push(annotated);

    // 2. Add TOC nav container if there are TOC entries
    if !book.toc().is_empty() {
        let toc_entries = build_toc_entries_with_positions(book.toc(), ctx);
        let toc_container = IonValue::Struct(vec![
            (
                KfxSymbol::NavType as u64,
                IonValue::Symbol(KfxSymbol::Toc as u64),
            ),
            (
                KfxSymbol::NavContainerName as u64,
                IonValue::Symbol(ctx.nav_container_symbols.toc),
            ),
            (KfxSymbol::Entries as u64, IonValue::List(toc_entries)),
        ]);
        let annotated = IonValue::Annotated(
            vec![KfxSymbol::NavContainer as u64],
            Box::new(toc_container),
        );
        nav_containers.push(annotated);
    }

    // 3. Add landmarks nav container (cover_page and start reading location)
    let landmarks_entries = build_landmarks_entries(book, ctx);
    if !landmarks_entries.is_empty() {
        let landmarks_container = IonValue::Struct(vec![
            (
                KfxSymbol::NavType as u64,
                IonValue::Symbol(KfxSymbol::Landmarks as u64),
            ),
            (
                KfxSymbol::NavContainerName as u64,
                IonValue::Symbol(ctx.nav_container_symbols.landmarks),
            ),
            (KfxSymbol::Entries as u64, IonValue::List(landmarks_entries)),
        ]);
        let annotated = IonValue::Annotated(
            vec![KfxSymbol::NavContainer as u64],
            Box::new(landmarks_container),
        );
        nav_containers.push(annotated);
    }

    // Wrap in reading order structure: [{reading_order_name, nav_containers}]
    let reading_order = IonValue::Struct(vec![
        (
            KfxSymbol::ReadingOrderName as u64,
            IonValue::Symbol(KfxSymbol::Default as u64),
        ),
        (
            KfxSymbol::NavContainers as u64,
            IonValue::List(nav_containers),
        ),
    ]);

    let book_nav = IonValue::List(vec![reading_order]);

    KfxFragment::singleton(KfxSymbol::BookNavigation, book_nav)
}

/// Build headings navigation entries grouped by heading level.
///
/// Structure: Each heading level (h2, h3, etc.) gets a nav_unit with nested
/// entries for all headings of that level.
fn build_headings_entries(ctx: &ExportContext) -> Vec<IonValue> {
    use std::collections::BTreeMap;

    // Group headings by level
    let mut by_level: BTreeMap<u8, Vec<&crate::kfx::context::HeadingPosition>> = BTreeMap::new();
    for heading in &ctx.heading_positions {
        by_level.entry(heading.level).or_default().push(heading);
    }

    // Convert heading level to KFX symbol
    fn level_to_symbol(level: u8) -> Option<KfxSymbol> {
        match level {
            2 => Some(KfxSymbol::H2),
            3 => Some(KfxSymbol::H3),
            4 => Some(KfxSymbol::H4),
            5 => Some(KfxSymbol::H5),
            6 => Some(KfxSymbol::H6),
            _ => None, // h1 not typically used in body
        }
    }

    let mut entries = Vec::new();

    for (level, headings) in by_level {
        let Some(level_symbol) = level_to_symbol(level) else {
            continue;
        };

        if headings.is_empty() {
            continue;
        }

        // Build nested entries for each heading of this level
        let nested_entries: Vec<IonValue> = headings
            .iter()
            .map(|h| {
                IonValue::Annotated(
                    vec![KfxSymbol::NavUnit as u64],
                    Box::new(IonValue::Struct(vec![
                        (
                            KfxSymbol::Representation as u64,
                            IonValue::Struct(vec![(
                                KfxSymbol::Label as u64,
                                IonValue::String("heading-nav-unit".to_string()),
                            )]),
                        ),
                        (
                            KfxSymbol::TargetPosition as u64,
                            IonValue::Struct(vec![
                                (KfxSymbol::Id as u64, IonValue::Int(h.fragment_id as i64)),
                                (KfxSymbol::Offset as u64, IonValue::Int(h.offset as i64)),
                            ]),
                        ),
                    ])),
                )
            })
            .collect();

        // Use first heading's position for the level entry
        let first = headings[0];

        // Build the level entry with nested headings
        let level_entry = IonValue::Annotated(
            vec![KfxSymbol::NavUnit as u64],
            Box::new(IonValue::Struct(vec![
                (
                    KfxSymbol::LandmarkType as u64,
                    IonValue::Symbol(level_symbol as u64),
                ),
                (
                    KfxSymbol::Representation as u64,
                    IonValue::Struct(vec![(
                        KfxSymbol::Label as u64,
                        IonValue::String("heading-nav-unit".to_string()),
                    )]),
                ),
                (
                    KfxSymbol::TargetPosition as u64,
                    IonValue::Struct(vec![
                        (KfxSymbol::Id as u64, IonValue::Int(first.fragment_id as i64)),
                        (KfxSymbol::Offset as u64, IonValue::Int(first.offset as i64)),
                    ]),
                ),
                (KfxSymbol::Entries as u64, IonValue::List(nested_entries)),
            ])),
        );

        entries.push(level_entry);
    }

    entries
}

/// Build landmarks navigation entries.
///
/// Build landmark entries from resolved landmarks using schema mapping.
///
/// Iterates over all landmarks in ctx.landmark_fragments and converts each
/// to a KFX nav_unit using the schema for type conversion.
fn build_landmarks_entries(_book: &Book, ctx: &ExportContext) -> Vec<IonValue> {
    use crate::kfx::schema::schema;

    let mut entries = Vec::new();

    // Sort landmarks for consistent output (Cover first, then StartReading, then others)
    let mut landmarks: Vec<_> = ctx.landmark_fragments.iter().collect();
    landmarks.sort_by_key(|(lt, _)| match lt {
        LandmarkType::Cover => 0,
        LandmarkType::StartReading => 1,
        _ => 2,
    });

    for (landmark_type, target) in landmarks {
        // Convert IR landmark type to KFX symbol via schema
        let Some(kfx_symbol) = schema().landmark_to_kfx(*landmark_type) else {
            continue; // Skip landmarks with no KFX equivalent
        };

        let entry = IonValue::Annotated(
            vec![KfxSymbol::NavUnit as u64],
            Box::new(IonValue::Struct(vec![
                (
                    KfxSymbol::LandmarkType as u64,
                    IonValue::Symbol(kfx_symbol as u64),
                ),
                (
                    KfxSymbol::Representation as u64,
                    IonValue::Struct(vec![(
                        KfxSymbol::Label as u64,
                        IonValue::String(target.label.clone()),
                    )]),
                ),
                (
                    KfxSymbol::TargetPosition as u64,
                    IonValue::Struct(vec![
                        (KfxSymbol::Id as u64, IonValue::Int(target.fragment_id as i64)),
                        (KfxSymbol::Offset as u64, IonValue::Int(target.offset as i64)),
                    ]),
                ),
            ])),
        );
        entries.push(entry);
    }

    entries
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

            let nav_unit = IonValue::Struct(fields);
            // Annotate with nav_unit::
            IonValue::Annotated(vec![KfxSymbol::NavUnit as u64], Box::new(nav_unit))
        })
        .collect()
}

/// Resolve a TOC href to (fragment_id, offset).
fn resolve_toc_position(href: &str, ctx: &ExportContext) -> (u64, usize) {
    // Extract base path and anchor from href
    let (base_path, anchor) = if let Some(hash_pos) = href.find('#') {
        (&href[..hash_pos], Some(&href[hash_pos + 1..]))
    } else {
        (href, None)
    };

    // Look up the fragment ID for this path
    if let Some(fragment_id) = ctx.get_fragment_for_path(base_path) {
        // If there's an anchor, try to get its offset
        let offset = anchor
            .and_then(|anchor_id| ctx.anchor_map.get(anchor_id))
            .and_then(|(chapter_id, node_id)| ctx.position_map.get(&(*chapter_id, *node_id)))
            .map(|pos| pos.offset)
            .unwrap_or(0);

        return (fragment_id, offset);
    }

    // Fallback: try first chapter fragment
    if let Some(&frag_id) = ctx.chapter_fragments.values().next() {
        (frag_id, 0)
    } else {
        (200, 0) // Default to start if no chapters
    }
}

// ============================================================================
// Entity Assembler: Packages Schema output into KFX Entity Hierarchy
// ============================================================================

/// Build chapter entities returning them separately for grouped emission.
///
/// Returns (section, storyline, Option<content>) so they can be grouped by type.
fn build_chapter_entities_grouped(
    chapter: &IRChapter,
    chapter_id: ChapterId,
    section_name: &str,
    ctx: &mut ExportContext,
) -> (KfxFragment, KfxFragment, Option<KfxFragment>) {
    use crate::kfx::storyline::{ir_to_tokens, tokens_to_ion};

    // Check if this is a cover chapter (image-only)
    // Only treat as cover if there's no standalone cover section (c0)
    // When ctx.cover_fragment_id is set, c0 already handles the cover
    let is_cover = ctx.cover_fragment_id.is_none() && is_image_only_chapter(chapter);

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
    let section_id = ctx
        .get_chapter_fragment(chapter_id)
        .unwrap_or_else(|| ctx.next_fragment_id());

    // =========================================================================
    // 2. GENERATE: Schema-driven token generation + text/structure split
    // =========================================================================
    let (storyline_content_list, content_strings) = if is_cover {
        // For cover chapters, generate flat storyline with direct image
        let content_list = build_cover_storyline(chapter, ctx);
        let text = ctx.drain_text();
        (content_list, text)
    } else {
        // Normal chapter: full token-based generation
        let tokens = ir_to_tokens(chapter, ctx);
        let content_list = tokens_to_ion(&tokens, ctx);
        let text = ctx.drain_text();
        (content_list, text)
    };

    // =========================================================================
    // 3. ASSEMBLE: Package into three KFX Entities
    // =========================================================================

    // Entity A: CONTENT ($145) - Holds the raw text strings
    let content_fragment = if !content_strings.is_empty() {
        let content_ion = IonValue::Struct(vec![
            (KfxSymbol::Name as u64, IonValue::Symbol(content_name_symbol)),
            (
                KfxSymbol::ContentList as u64,
                IonValue::List(content_strings.into_iter().map(IonValue::String).collect()),
            ),
        ]);
        Some(KfxFragment::new(
            KfxSymbol::Content,
            &content_name,
            content_ion,
        ))
    } else {
        None
    };

    // Entity B: STORYLINE ($259) - Holds the structure, references Content by name
    let storyline_ion = IonValue::Struct(vec![
        (
            KfxSymbol::StoryName as u64,
            IonValue::Symbol(story_name_symbol),
        ),
        (KfxSymbol::ContentList as u64, storyline_content_list),
    ]);
    let storyline_fragment = KfxFragment::new(KfxSymbol::Storyline, &story_name, storyline_ion);

    // Entity C: SECTION ($260) - Entry point, references Storyline by story_name
    let page_template = if is_cover {
        // Cover page: container type with fixed dimensions and scale_fit layout
        IonValue::Struct(vec![
            (KfxSymbol::Id as u64, IonValue::Int(section_id as i64)),
            (
                KfxSymbol::StoryName as u64,
                IonValue::Symbol(story_name_symbol),
            ),
            (
                KfxSymbol::Type as u64,
                IonValue::Symbol(KfxSymbol::Container as u64),
            ),
            (KfxSymbol::FixedWidth as u64, IonValue::Int(1400)),
            (KfxSymbol::FixedHeight as u64, IonValue::Int(2100)),
            (
                KfxSymbol::Layout as u64,
                IonValue::Symbol(KfxSymbol::ScaleFit as u64),
            ),
            (
                KfxSymbol::Float as u64,
                IonValue::Symbol(KfxSymbol::Center as u64),
            ),
        ])
    } else {
        // Normal text page
        IonValue::Struct(vec![
            (KfxSymbol::Id as u64, IonValue::Int(section_id as i64)),
            (
                KfxSymbol::StoryName as u64,
                IonValue::Symbol(story_name_symbol),
            ),
            (
                KfxSymbol::Type as u64,
                IonValue::Symbol(KfxSymbol::Text as u64),
            ),
        ])
    };

    let section_ion = IonValue::Struct(vec![
        (
            KfxSymbol::SectionName as u64,
            IonValue::Symbol(section_name_symbol),
        ),
        (
            KfxSymbol::PageTemplates as u64,
            IonValue::List(vec![page_template]),
        ),
    ]);
    let section_fragment = KfxFragment::new_with_id(
        KfxSymbol::Section,
        section_id,
        section_name,
        section_ion,
    );

    (section_fragment, storyline_fragment, content_fragment)
}

/// Build a simplified storyline for cover chapters.
///
/// Cover pages have a flat structure with just the image directly in content_list,
/// no container wrapper. Structure: [{ type: image, resource_name, style }]
fn build_cover_storyline(chapter: &IRChapter, ctx: &mut ExportContext) -> IonValue {
    use crate::ir::Role;

    // Find the image node
    for node_id in chapter.iter_dfs() {
        let node = match chapter.node(node_id) {
            Some(n) => n,
            None => continue,
        };

        if node.role == Role::Image {
            // Get the image source
            if let Some(src) = chapter.semantics.src(node_id) {
                // Look up the resource name (e.g., "e0")
                let resource_name = ctx.resource_registry.get_or_create_name(src);
                let resource_name_symbol = ctx.symbols.get_or_intern(&resource_name);

                // Register style and get symbol
                let style_symbol = ctx.register_style_id(node.style, &chapter.styles);

                // Generate unique container ID
                let container_id = ctx.fragment_ids.next();

                // Build the image struct directly (no container wrapper)
                let image_struct = IonValue::Struct(vec![
                    (KfxSymbol::Id as u64, IonValue::Int(container_id as i64)),
                    (KfxSymbol::Style as u64, IonValue::Symbol(style_symbol)),
                    (
                        KfxSymbol::Type as u64,
                        IonValue::Symbol(KfxSymbol::Image as u64),
                    ),
                    (
                        KfxSymbol::ResourceName as u64,
                        IonValue::Symbol(resource_name_symbol),
                    ),
                ]);

                return IonValue::List(vec![image_struct]);
            }
        }
    }

    // Fallback: empty list if no image found
    IonValue::List(vec![])
}

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
#[allow(dead_code)]
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


/// Build the document symbols section.
///
/// This writes the local symbol table in the format expected by KFX readers:
/// ```ion
/// $ion_symbol_table::{
///   imports: [{ name: "YJ_symbols", version: 10, max_id: 851 }],
///   symbols: ["local_sym1", "local_sym2", ...]
/// }
/// ```
///
/// Ion system symbol IDs:
/// - $3 = $ion_symbol_table
/// - $4 = name
/// - $5 = version
/// - $6 = imports
/// - $7 = symbols
/// - $8 = max_id
///
/// IMPORTANT: The symbols in the list must appear in the exact same order
/// they were interned, so that symbol ID = KFX_SYMBOL_TABLE_SIZE + index.
fn build_symbol_table_ion(local_symbols: &[String]) -> Vec<u8> {
    use crate::kfx::ion::IonWriter;
    use crate::kfx::symbols::KFX_MAX_SYMBOL_ID;

    let mut writer = IonWriter::new();
    writer.write_bvm();

    // Build the import entry for YJ_symbols (Amazon's KFX symbol table)
    // { name: "YJ_symbols", version: 10, max_id: 851 }
    let import_entry = IonValue::Struct(vec![
        (4, IonValue::String("YJ_symbols".to_string())), // $4 = name
        (5, IonValue::Int(10)),                          // $5 = version
        (8, IonValue::Int(KFX_MAX_SYMBOL_ID as i64)),    // $8 = max_id
    ]);

    // Build the symbols list with local symbols
    let symbols_list: Vec<IonValue> = local_symbols
        .iter()
        .map(|s| IonValue::String(s.clone()))
        .collect();

    // Build the $ion_symbol_table struct
    // { imports: [...], symbols: [...] }
    let symbol_table = IonValue::Struct(vec![
        (6, IonValue::List(vec![import_entry])), // $6 = imports
        (7, IonValue::List(symbols_list)),       // $7 = symbols
    ]);

    // Write with $ion_symbol_table annotation ($3)
    writer.write_annotated(&[3], &symbol_table);

    writer.into_bytes()
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
    // Use resource/ prefix to distinguish from external_resource fragment
    // This ensures bcRawMedia gets a different entity ID
    let resource_name = generate_resource_name(href, ctx);
    let raw_name = format!("resource/{}", resource_name);

    // Register the prefixed name as a symbol
    ctx.symbols.get_or_intern(&raw_name);

    // Create raw fragment for binary resources
    KfxFragment::raw(KfxSymbol::Bcrawmedia as u64, &raw_name, data.to_vec())
}

/// Build anchor fragments ($266) for all recorded anchors.
///
/// Returns (fragments, anchor_ids_by_fragment) where anchor_ids_by_fragment
/// maps fragment_id → list of anchor symbol IDs for use in position_map.
fn build_anchor_fragments(
    ctx: &mut ExportContext,
) -> (Vec<KfxFragment>, HashMap<u64, Vec<u64>>) {
    let mut fragments = Vec::new();
    let mut anchor_ids_by_fragment: HashMap<u64, Vec<u64>> = HashMap::new();

    // Get resolved anchors from the AnchorRegistry
    let resolved_anchors = ctx.anchor_registry.drain_anchors();

    for anchor in resolved_anchors {
        // Intern the anchor symbol to get its ID
        let anchor_symbol_id = ctx.symbols.get_or_intern(&anchor.symbol);

        // Track which anchors belong to which fragment for position_map
        anchor_ids_by_fragment
            .entry(anchor.fragment_id)
            .or_default()
            .push(anchor_symbol_id);

        // Build position struct
        let mut pos_fields = Vec::new();
        pos_fields.push((KfxSymbol::Id as u64, IonValue::Int(anchor.fragment_id as i64)));
        if anchor.offset > 0 {
            pos_fields.push((KfxSymbol::Offset as u64, IonValue::Int(anchor.offset as i64)));
        }

        let ion = IonValue::Struct(vec![
            (KfxSymbol::AnchorName as u64, IonValue::Symbol(anchor_symbol_id)),
            (KfxSymbol::Position as u64, IonValue::Struct(pos_fields)),
        ]);

        fragments.push(KfxFragment::new(KfxSymbol::Anchor, &anchor.symbol, ion));
    }

    (fragments, anchor_ids_by_fragment)
}

/// Generate a short resource name for a given href.
fn generate_resource_name(href: &str, ctx: &mut ExportContext) -> String {
    ctx.resource_registry.get_or_create_name(href)
}

// ============================================================================
// Navigation Maps ($264, $265, $550)
// ============================================================================

/// Build position_map fragment ($264).
///
/// Maps each section to the list of EIDs it contains. This enables
/// the Kindle reader to track which section contains a given position.
fn build_position_map_fragment(
    ctx: &ExportContext,
    anchor_ids_by_fragment: &HashMap<u64, Vec<u64>>,
) -> KfxFragment {
    let mut entries = Vec::new();

    // Handle standalone cover section (c0) if present
    // Cover has no text anchors, just contains its own fragment ID
    let section_offset = if let Some(cover_fid) = ctx.cover_fragment_id {
        let entry = IonValue::Struct(vec![
            (KfxSymbol::Contains as u64, IonValue::List(vec![IonValue::Int(cover_fid as i64)])),
            (KfxSymbol::SectionName as u64, IonValue::Symbol(ctx.section_ids[0])),
        ]);
        entries.push(entry);
        1 // Skip c0 when processing spine chapters
    } else {
        0
    };

    // Build entries for spine chapters (skip cover section if present)
    let fragment_ids: Vec<_> = {
        let mut ids: Vec<_> = ctx.chapter_fragments.values().copied().collect();
        ids.sort();
        ids
    };

    for (idx, &section_sym) in ctx.section_ids.iter().skip(section_offset).enumerate() {
        if let Some(&fragment_id) = fragment_ids.get(idx) {
            // Start with the fragment ID itself
            let mut eid_list = vec![IonValue::Int(fragment_id as i64)];

            // Add all anchor IDs that belong to this fragment
            if let Some(anchor_ids) = anchor_ids_by_fragment.get(&fragment_id) {
                for &anchor_id in anchor_ids {
                    eid_list.push(IonValue::Int(anchor_id as i64));
                }
            }

            let entry = IonValue::Struct(vec![
                (KfxSymbol::Contains as u64, IonValue::List(eid_list)),
                (KfxSymbol::SectionName as u64, IonValue::Symbol(section_sym)),
            ]);
            entries.push(entry);
        }
    }

    let ion = IonValue::List(entries);
    KfxFragment::singleton(KfxSymbol::PositionMap, ion)
}

/// Build position_id_map fragment ($265).
///
/// Maps cumulative character positions (PIDs) to EIDs. This enables
/// reading progress tracking and "go to position" functionality.
fn build_position_id_map_fragment(ctx: &ExportContext) -> KfxFragment {
    let mut entries = Vec::new();
    let mut cumulative_offset = 0i64;

    // Collect chapter fragment IDs in order
    let mut chapter_entries: Vec<_> = ctx.chapter_fragments.iter().collect();
    chapter_entries.sort_by_key(|(_, fid)| **fid);

    for (chapter_id, fragment_id) in &chapter_entries {
        // Find max text offset for this chapter from position_map
        let max_offset = ctx
            .position_map
            .iter()
            .filter(|((cid, _), _)| cid == *chapter_id)
            .map(|(_, pos)| pos.offset)
            .max()
            .unwrap_or(0);

        // Entry: at cumulative_offset, content starts at this EID
        let entry = IonValue::Struct(vec![
            (KfxSymbol::Pid as u64, IonValue::Int(cumulative_offset)),
            (KfxSymbol::Eid as u64, IonValue::Int(**fragment_id as i64)),
        ]);
        entries.push(entry);

        cumulative_offset += max_offset as i64;
    }

    // Terminator entry: total character count with EID 0
    let terminator = IonValue::Struct(vec![
        (KfxSymbol::Pid as u64, IonValue::Int(cumulative_offset)),
        (KfxSymbol::Eid as u64, IonValue::Int(0)),
    ]);
    entries.push(terminator);

    let ion = IonValue::List(entries);
    KfxFragment::singleton(KfxSymbol::PositionIdMap, ion)
}

/// Build location_map fragment ($550).
///
/// Maps location numbers to positions. Locations are synthetic page-like
/// markers every ~110 characters (Kindle's standard).
fn build_location_map_fragment(ctx: &ExportContext) -> KfxFragment {
    const CHARS_PER_LOCATION: i64 = 110;

    let mut location_entries = Vec::new();

    // Collect chapter fragment IDs in order
    let mut chapter_entries: Vec<_> = ctx.chapter_fragments.iter().collect();
    chapter_entries.sort_by_key(|(_, fid)| **fid);

    for (chapter_id, fragment_id) in &chapter_entries {
        // Find max text offset for this chapter
        let chapter_length = ctx
            .position_map
            .iter()
            .filter(|((cid, _), _)| cid == *chapter_id)
            .map(|(_, pos)| pos.offset)
            .max()
            .unwrap_or(0) as i64;

        // Generate location entries for this chapter
        let mut pos_in_chapter = 0i64;
        while pos_in_chapter < chapter_length {
            let entry = IonValue::Struct(vec![
                (KfxSymbol::Id as u64, IonValue::Int(**fragment_id as i64)),
                (KfxSymbol::Offset as u64, IonValue::Int(pos_in_chapter)),
            ]);
            location_entries.push(entry);
            pos_in_chapter += CHARS_PER_LOCATION;
        }
    }

    // Wrap in locations list structure
    let ion = IonValue::List(vec![IonValue::Struct(vec![(
        KfxSymbol::Locations as u64,
        IonValue::List(location_entries),
    )])]);

    KfxFragment::singleton(KfxSymbol::LocationMap, ion)
}

/// Detect format symbol from file extension/magic bytes.
///
/// Delegates to the pure `detect_media_format()` utility and maps to KFX symbol.
fn detect_format_symbol(href: &str, data: &[u8]) -> u64 {
    let format = detect_media_format(href, data);
    format_to_kfx_symbol(format)
}

/// Check if a path is a media asset (image, font, etc.)
fn is_media_asset(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "ttf" | "otf" | "woff" | "woff2"
    )
}

/// Resolve landmarks from the Book's IR to fragment IDs.
///
/// This uses the parsed landmarks from the source format (EPUB, KFX, etc.)
/// to populate landmark_fragments in the context.
///
/// Handles both chapter-level targets (e.g., `chapter.xhtml`) and anchor-level
/// targets (e.g., `chapter.xhtml#section1`) by looking up positions via anchor_map.
fn resolve_landmarks_from_ir(
    book: &Book,
    source_to_chapter: &HashMap<String, ChapterId>,
    ctx: &mut ExportContext,
) {
    for landmark in book.landmarks() {
        // Split href into file path and optional anchor
        let (href_path, anchor) = match landmark.href.split_once('#') {
            Some((path, anchor)) => (path, Some(anchor)),
            None => (landmark.href.as_str(), None),
        };

        // Try to find the chapter ID for this href
        let chapter_id = source_to_chapter.get(href_path).copied();

        if let Some(cid) = chapter_id {
            // Try to resolve target position
            let target = if let Some(anchor_id) = anchor {
                // Look up anchor in anchor_map to get (ChapterId, NodeId)
                // Then use position_map to get the exact position
                let full_href = format!("{}#{}", href_path, anchor_id);
                if let Some(&(_, node_id)) = ctx.anchor_map.get(&full_href) {
                    if let Some(pos) = ctx.position_map.get(&(cid, node_id)) {
                        Some(LandmarkTarget {
                            fragment_id: pos.fragment_id,
                            offset: pos.offset as u64,
                            label: landmark.label.clone(),
                        })
                    } else {
                        None
                    }
                } else if let Some(frag_id) = ctx.chapter_fragments.get(&cid).copied() {
                    // Fall back to chapter start if anchor not found
                    Some(LandmarkTarget {
                        fragment_id: frag_id,
                        offset: 0,
                        label: landmark.label.clone(),
                    })
                } else {
                    None
                }
            } else if let Some(frag_id) = ctx.chapter_fragments.get(&cid).copied() {
                // No anchor, use chapter start
                Some(LandmarkTarget {
                    fragment_id: frag_id,
                    offset: 0,
                    label: landmark.label.clone(),
                })
            } else {
                None
            };

            if let Some(target) = target {
                // Only add if not already present (first wins)
                ctx.landmark_fragments
                    .entry(landmark.landmark_type)
                    .or_insert(target.clone());

                // BodyMatter can serve as StartReading if no explicit SRL
                if landmark.landmark_type == LandmarkType::BodyMatter {
                    ctx.landmark_fragments
                        .entry(LandmarkType::StartReading)
                        .or_insert(target);
                }
            }
        }
    }
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
                KfxSymbol::Null as u32 // Singleton marker ($348 = null)
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

    #[test]
    fn test_metadata_fragment_contains_reading_orders() {
        let mut ctx = ExportContext::new();
        // Register some sections
        ctx.register_section("c0");
        ctx.register_section("c1");

        let frag = build_metadata_fragment(&ctx);

        // Should be $258 (metadata) type
        assert_eq!(frag.ftype, KfxSymbol::Metadata as u64);
        assert!(frag.is_singleton());

        // Extract Ion and verify structure
        if let crate::kfx::fragment::FragmentData::Ion(ion) = &frag.data {
            if let IonValue::Struct(fields) = ion {
                // Should have reading_orders field
                let has_reading_orders = fields
                    .iter()
                    .any(|(id, _)| *id == KfxSymbol::ReadingOrders as u64);
                assert!(has_reading_orders, "metadata should contain reading_orders");
            } else {
                panic!("expected Struct");
            }
        } else {
            panic!("expected Ion data");
        }
    }

    #[test]
    fn test_book_metadata_fragment_has_categorised_metadata() {
        // Load a real book from fixtures
        let book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let ctx = ExportContext::new();
        let container_id = generate_container_id();

        let frag = build_book_metadata_fragment(&book, &container_id, &ctx);

        // Should be $490 (book_metadata) type
        assert_eq!(frag.ftype, KfxSymbol::BookMetadata as u64);
        assert!(frag.is_singleton());

        // Extract Ion and verify structure
        if let crate::kfx::fragment::FragmentData::Ion(ion) = &frag.data {
            if let IonValue::Struct(fields) = ion {
                // Should have categorised_metadata field
                let has_categorised = fields
                    .iter()
                    .any(|(id, _)| *id == KfxSymbol::CategorisedMetadata as u64);
                assert!(has_categorised, "book_metadata should contain categorised_metadata");

                // Get the categorised_metadata list
                let categorised = fields
                    .iter()
                    .find(|(id, _)| *id == KfxSymbol::CategorisedMetadata as u64)
                    .map(|(_, v)| v);

                if let Some(IonValue::List(categories)) = categorised {
                    // Should have 3 categories
                    assert_eq!(categories.len(), 3, "should have 3 metadata categories");
                } else {
                    panic!("categorised_metadata should be a list");
                }
            } else {
                panic!("expected Struct");
            }
        } else {
            panic!("expected Ion data");
        }
    }

    #[test]
    fn test_metadata_kv_helper() {
        let kv = metadata_kv("test_key", "test_value");

        if let IonValue::Struct(fields) = kv {
            assert_eq!(fields.len(), 2);

            let key_field = fields.iter().find(|(id, _)| *id == KfxSymbol::Key as u64);
            let value_field = fields.iter().find(|(id, _)| *id == KfxSymbol::Value as u64);

            assert!(key_field.is_some(), "should have key field");
            assert!(value_field.is_some(), "should have value field");

            if let Some((_, IonValue::String(k))) = key_field {
                assert_eq!(k, "test_key");
            }
            if let Some((_, IonValue::String(v))) = value_field {
                assert_eq!(v, "test_value");
            }
        } else {
            panic!("expected Struct");
        }
    }

    #[test]
    fn test_book_navigation_structure() {
        // Test that navigation has correct wrapper structure:
        // [{reading_order_name: default, nav_containers: [nav_container::{}...]}]
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let mut ctx = ExportContext::new();

        // Collect spine info first to avoid borrow issues
        let spine_info: Vec<_> = book
            .spine()
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let section_name = format!("c{}", idx);
                let source_path = book.source_id(entry.id).unwrap_or("").to_string();
                (entry.id, section_name, source_path)
            })
            .collect();

        // Survey chapters to populate path_to_fragment
        for (chapter_id, section_name, source_path) in &spine_info {
            ctx.register_section(section_name);
            if let Ok(chapter) = book.load_chapter(*chapter_id) {
                survey_chapter(&chapter, *chapter_id, source_path, &mut ctx);
            }
        }

        let frag = build_book_navigation_fragment_with_positions(&book, &ctx);

        // Should be $389 (book_navigation) type
        assert_eq!(frag.ftype, KfxSymbol::BookNavigation as u64);

        if let crate::kfx::fragment::FragmentData::Ion(ion) = &frag.data {
            // Should be a list with one reading order entry
            if let IonValue::List(reading_orders) = ion {
                assert_eq!(reading_orders.len(), 1, "should have one reading order");

                // The reading order should have reading_order_name and nav_containers
                if let IonValue::Struct(fields) = &reading_orders[0] {
                    let has_reading_order_name = fields
                        .iter()
                        .any(|(id, _)| *id == KfxSymbol::ReadingOrderName as u64);
                    let has_nav_containers = fields
                        .iter()
                        .any(|(id, _)| *id == KfxSymbol::NavContainers as u64);

                    assert!(has_reading_order_name, "should have reading_order_name");
                    assert!(has_nav_containers, "should have nav_containers");
                } else {
                    panic!("reading order should be a struct");
                }
            } else {
                panic!("book_navigation should be a list");
            }
        } else {
            panic!("expected Ion data");
        }
    }


    #[test]
    fn test_content_features_fragment() {
        let frag = build_content_features_fragment();

        // Should be $585 (content_features) type
        assert_eq!(frag.ftype, KfxSymbol::ContentFeatures as u64);
        assert!(frag.is_singleton());

        // Extract Ion and verify structure
        if let crate::kfx::fragment::FragmentData::Ion(ion) = &frag.data {
            if let IonValue::Struct(fields) = ion {
                // Should have features field
                let features = fields
                    .iter()
                    .find(|(id, _)| *id == KfxSymbol::Features as u64);
                assert!(features.is_some(), "content_features should contain features");

                // Features should be a list with 3 items
                if let Some((_, IonValue::List(items))) = features {
                    assert_eq!(items.len(), 3, "should have 3 feature entries");
                } else {
                    panic!("features should be a list");
                }
            } else {
                panic!("expected Struct");
            }
        } else {
            panic!("expected Ion data");
        }
    }

    #[test]
    fn test_document_data_fragment() {
        let mut ctx = ExportContext::new();
        ctx.register_section("c0");
        ctx.register_section("c1");
        // Simulate some fragment IDs being used
        ctx.next_fragment_id();
        ctx.next_fragment_id();

        let frag = build_document_data_fragment(&ctx);

        // Should be $538 (document_data) type
        assert_eq!(frag.ftype, KfxSymbol::DocumentData as u64);
        assert!(frag.is_singleton());

        // Extract Ion and verify structure
        if let crate::kfx::fragment::FragmentData::Ion(ion) = &frag.data {
            if let IonValue::Struct(fields) = ion {
                // Check for required fields
                let field_ids: Vec<u64> = fields.iter().map(|(id, _)| *id).collect();

                assert!(
                    field_ids.contains(&(KfxSymbol::Direction as u64)),
                    "should have direction"
                );
                assert!(
                    field_ids.contains(&(KfxSymbol::ColumnCount as u64)),
                    "should have column_count"
                );
                assert!(
                    field_ids.contains(&(KfxSymbol::FontSize as u64)),
                    "should have font_size"
                );
                assert!(
                    field_ids.contains(&(KfxSymbol::WritingMode as u64)),
                    "should have writing_mode"
                );
                assert!(
                    field_ids.contains(&(KfxSymbol::Selection as u64)),
                    "should have selection"
                );
                assert!(
                    field_ids.contains(&(KfxSymbol::MaxId as u64)),
                    "should have max_id"
                );
                assert!(
                    field_ids.contains(&(KfxSymbol::LineHeight as u64)),
                    "should have line_height"
                );
                assert!(
                    field_ids.contains(&(KfxSymbol::ReadingOrders as u64)),
                    "should have reading_orders"
                );
            } else {
                panic!("expected Struct");
            }
        } else {
            panic!("expected Ion data");
        }
    }

    #[test]
    fn test_singleton_uses_null_symbol() {
        // Build a singleton fragment and serialize it
        let frag = build_content_features_fragment();
        let local_symbols: Vec<String> = vec![];
        let entities = serialize_fragments(&[frag], &local_symbols);

        // Singleton should use $348 (null) as ID
        assert_eq!(entities[0].id, KfxSymbol::Null as u32);
    }

    #[test]
    fn test_build_headings_entries_empty() {
        let ctx = ExportContext::new();
        let entries = build_headings_entries(&ctx);
        assert!(entries.is_empty(), "No headings should produce empty entries");
    }

    #[test]
    fn test_build_headings_entries_single_level() {
        use crate::kfx::context::HeadingPosition;

        let mut ctx = ExportContext::new();

        // Push h2 headings at different positions
        ctx.heading_positions.push(HeadingPosition {
            level: 2,
            fragment_id: 100,
            offset: 0,
        });
        ctx.heading_positions.push(HeadingPosition {
            level: 2,
            fragment_id: 100,
            offset: 50,
        });
        ctx.heading_positions.push(HeadingPosition {
            level: 2,
            fragment_id: 101,
            offset: 0,
        });

        let entries = build_headings_entries(&ctx);

        // Should have 1 level entry (h2)
        assert_eq!(entries.len(), 1, "Should have one level group for h2");

        // Verify it's a nav_unit with h2 landmark_type
        if let IonValue::Annotated(annotations, inner) = &entries[0] {
            assert_eq!(annotations[0], KfxSymbol::NavUnit as u64);
            if let IonValue::Struct(fields) = inner.as_ref() {
                // Should have landmark_type = h2
                let landmark = fields
                    .iter()
                    .find(|(id, _)| *id == KfxSymbol::LandmarkType as u64);
                assert!(landmark.is_some(), "Should have landmark_type");
                if let Some((_, IonValue::Symbol(sym))) = landmark {
                    assert_eq!(*sym, KfxSymbol::H2 as u64);
                }

                // Should have nested entries
                let nested = fields.iter().find(|(id, _)| *id == KfxSymbol::Entries as u64);
                assert!(nested.is_some(), "Should have nested entries");
                if let Some((_, IonValue::List(list))) = nested {
                    assert_eq!(list.len(), 3, "Should have 3 nested h2 entries");
                }
            }
        } else {
            panic!("Expected annotated nav_unit");
        }
    }

    #[test]
    fn test_build_headings_entries_multiple_levels() {
        use crate::kfx::context::HeadingPosition;

        let mut ctx = ExportContext::new();

        // Push h2, h3, h4 headings
        ctx.heading_positions.push(HeadingPosition {
            level: 2,
            fragment_id: 100,
            offset: 0,
        });
        ctx.heading_positions.push(HeadingPosition {
            level: 3,
            fragment_id: 100,
            offset: 20,
        });
        ctx.heading_positions.push(HeadingPosition {
            level: 4,
            fragment_id: 101,
            offset: 0,
        });
        ctx.heading_positions.push(HeadingPosition {
            level: 3,
            fragment_id: 101,
            offset: 30,
        });

        let entries = build_headings_entries(&ctx);

        // Should have 3 level entries (h2, h3, h4)
        assert_eq!(entries.len(), 3, "Should have three level groups");

        // Verify ordering is by level (BTreeMap ensures h2 < h3 < h4)
        let levels: Vec<u64> = entries
            .iter()
            .filter_map(|e| {
                if let IonValue::Annotated(_, inner) = e {
                    if let IonValue::Struct(fields) = inner.as_ref() {
                        fields
                            .iter()
                            .find(|(id, _)| *id == KfxSymbol::LandmarkType as u64)
                            .and_then(|(_, v)| {
                                if let IonValue::Symbol(sym) = v {
                                    Some(*sym)
                                } else {
                                    None
                                }
                            })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            levels,
            vec![
                KfxSymbol::H2 as u64,
                KfxSymbol::H3 as u64,
                KfxSymbol::H4 as u64
            ]
        );
    }

    #[test]
    fn test_build_headings_entries_ignores_h1() {
        use crate::kfx::context::HeadingPosition;

        let mut ctx = ExportContext::new();

        ctx.heading_positions.push(HeadingPosition {
            level: 1,
            fragment_id: 100,
            offset: 0,
        });

        let entries = build_headings_entries(&ctx);
        assert!(entries.is_empty(), "h1 should be ignored");
    }

    #[test]
    fn test_build_headings_entries_target_position() {
        use crate::kfx::context::HeadingPosition;

        let mut ctx = ExportContext::new();

        ctx.heading_positions.push(HeadingPosition {
            level: 2,
            fragment_id: 12345,
            offset: 99,
        });

        let entries = build_headings_entries(&ctx);
        assert_eq!(entries.len(), 1);

        // Verify target_position has correct id and offset
        if let IonValue::Annotated(_, inner) = &entries[0] {
            if let IonValue::Struct(fields) = inner.as_ref() {
                let target = fields
                    .iter()
                    .find(|(id, _)| *id == KfxSymbol::TargetPosition as u64);
                if let Some((_, IonValue::Struct(pos_fields))) = target {
                    let id_field = pos_fields.iter().find(|(id, _)| *id == KfxSymbol::Id as u64);
                    let offset_field = pos_fields
                        .iter()
                        .find(|(id, _)| *id == KfxSymbol::Offset as u64);

                    if let Some((_, IonValue::Int(id))) = id_field {
                        assert_eq!(*id, 12345);
                    } else {
                        panic!("Expected Int id");
                    }

                    if let Some((_, IonValue::Int(offset))) = offset_field {
                        assert_eq!(*offset, 99);
                    } else {
                        panic!("Expected Int offset");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod entity_structure_tests {
    use super::*;
    use crate::book::Book;
    use crate::kfx::fragment::FragmentData;

    #[test]
    fn test_entity_order_matches_reference() {
        // Build KFX from EPUB and verify entity order matches Amazon reference
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let container_id = generate_container_id();
        let mut ctx = ExportContext::new();

        // Collect spine info
        let spine_info: Vec<_> = book
            .spine()
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let section_name = format!("c{}", idx);
                (entry.id, section_name)
            })
            .collect();

        // Pass 1: Survey
        for (chapter_id, section_name) in &spine_info {
            ctx.register_section(section_name);
            let source_path = book.source_id(*chapter_id).unwrap_or("").to_string();
            if let Ok(chapter) = book.load_chapter(*chapter_id) {
                survey_chapter(&chapter, *chapter_id, &source_path, &mut ctx);
            }
        }

        // Pass 2: Build fragments in correct order
        let mut fragments = Vec::new();

        fragments.push(build_content_features_fragment());
        fragments.push(build_book_metadata_fragment(&book, &container_id, &ctx));
        fragments.push(build_metadata_fragment(&ctx));
        fragments.push(build_document_data_fragment(&ctx));
        fragments.push(build_book_navigation_fragment_with_positions(&book, &ctx));

        let mut section_fragments = Vec::new();
        let mut storyline_fragments = Vec::new();
        let mut content_fragments = Vec::new();

        for (chapter_id, section_name) in &spine_info {
            if let Ok(chapter) = book.load_chapter(*chapter_id) {
                let (section, storyline, content) =
                    build_chapter_entities_grouped(&chapter, *chapter_id, section_name, &mut ctx);
                section_fragments.push(section);
                storyline_fragments.push(storyline);
                if let Some(c) = content {
                    content_fragments.push(c);
                }
            }
        }

        fragments.extend(section_fragments);
        fragments.extend(storyline_fragments);
        fragments.extend(content_fragments);

        // Verify entity type order matches reference pattern:
        // content_features, book_metadata, metadata, document_data, book_navigation,
        // sections (grouped), storylines (grouped), content (grouped)

        let types: Vec<u64> = fragments.iter().map(|f| f.ftype).collect();

        // First 5 should be the header entities in order
        assert_eq!(types[0], KfxSymbol::ContentFeatures as u64);
        assert_eq!(types[1], KfxSymbol::BookMetadata as u64);
        assert_eq!(types[2], KfxSymbol::Metadata as u64);
        assert_eq!(types[3], KfxSymbol::DocumentData as u64);
        assert_eq!(types[4], KfxSymbol::BookNavigation as u64);

        // After header, all sections should come first, then storylines, then content
        let after_header = &types[5..];
        let section_count = after_header
            .iter()
            .take_while(|&&t| t == KfxSymbol::Section as u64)
            .count();
        assert!(section_count > 0, "should have sections after header");

        let after_sections = &after_header[section_count..];
        let storyline_count = after_sections
            .iter()
            .take_while(|&&t| t == KfxSymbol::Storyline as u64)
            .count();
        assert!(storyline_count > 0, "should have storylines after sections");

        let after_storylines = &after_sections[storyline_count..];
        let content_count = after_storylines
            .iter()
            .take_while(|&&t| t == KfxSymbol::Content as u64)
            .count();
        // Content is optional (image-only chapters may not have content)
        // Just verify that after storylines, we only have content entities (if any)
        for t in after_storylines.iter().take(content_count) {
            assert_eq!(*t, KfxSymbol::Content as u64, "content should follow storylines");
        }

        // Verify grouping - no interleaving
        for i in 1..section_count {
            assert_eq!(
                after_header[i],
                KfxSymbol::Section as u64,
                "sections should be grouped"
            );
        }
        for i in 1..storyline_count {
            assert_eq!(
                after_sections[i],
                KfxSymbol::Storyline as u64,
                "storylines should be grouped"
            );
        }
    }

    #[test]
    fn test_chapter_entities_grouped_returns_correct_types() {
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let mut ctx = ExportContext::new();

        // Get first chapter
        let spine_entry = book.spine().first().unwrap();
        let chapter_id = spine_entry.id;
        let section_name = "c0";
        ctx.register_section(section_name);

        // Survey chapter first
        let source_path = book.source_id(chapter_id).unwrap_or("").to_string();
        if let Ok(chapter) = book.load_chapter(chapter_id) {
            survey_chapter(&chapter, chapter_id, &source_path, &mut ctx);
        }

        // Build entities
        let chapter = book.load_chapter(chapter_id).unwrap();
        let (section, storyline, content) =
            build_chapter_entities_grouped(&chapter, chapter_id, section_name, &mut ctx);

        // Verify types
        assert_eq!(section.ftype, KfxSymbol::Section as u64);
        assert_eq!(storyline.ftype, KfxSymbol::Storyline as u64);

        // Verify section has section_name and page_templates
        if let FragmentData::Ion(IonValue::Struct(fields)) = &section.data {
            let has_section_name = fields
                .iter()
                .any(|(id, _)| *id == KfxSymbol::SectionName as u64);
            let has_page_templates = fields
                .iter()
                .any(|(id, _)| *id == KfxSymbol::PageTemplates as u64);
            assert!(has_section_name, "section should have section_name");
            assert!(has_page_templates, "section should have page_templates");
        }

        // Verify storyline has story_name and content_list
        if let FragmentData::Ion(IonValue::Struct(fields)) = &storyline.data {
            let has_story_name = fields
                .iter()
                .any(|(id, _)| *id == KfxSymbol::StoryName as u64);
            let has_content_list = fields
                .iter()
                .any(|(id, _)| *id == KfxSymbol::ContentList as u64);
            assert!(has_story_name, "storyline should have story_name");
            assert!(has_content_list, "storyline should have content_list");
        }

        // Content is optional but if present should have name and content_list
        if let Some(content_frag) = content {
            assert_eq!(content_frag.ftype, KfxSymbol::Content as u64);
            if let FragmentData::Ion(IonValue::Struct(fields)) = &content_frag.data {
                let has_name = fields.iter().any(|(id, _)| *id == KfxSymbol::Name as u64);
                let has_content_list = fields
                    .iter()
                    .any(|(id, _)| *id == KfxSymbol::ContentList as u64);
                assert!(has_name, "content should have name");
                assert!(has_content_list, "content should have content_list");
            }
        }
    }
}

#[cfg(test)]
mod section_type_tests {
    use super::*;
    use crate::book::Book;
    use crate::kfx::cover::{needs_standalone_cover, normalize_cover_path};
    use crate::kfx::fragment::FragmentData;

    /// When a standalone cover (c0) exists, the titlepage chapter (c1) should have
    /// type: text, NOT type: container. The container type is reserved for c0.
    #[test]
    fn test_titlepage_section_has_text_type_when_standalone_cover_exists() {
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let mut ctx = ExportContext::new();

        // Verify this book needs a standalone cover (cover.jpg != titlepage.png)
        let asset_paths: Vec<_> = book.list_assets();
        let cover_image = book.metadata().cover_image.clone().expect("should have cover");
        let normalized = normalize_cover_path(&cover_image, &asset_paths);

        // Get first chapter ID
        let first_chapter_id = book.spine().first().expect("should have spine").id;
        let first_chapter = book.load_chapter(first_chapter_id).unwrap();
        assert!(
            needs_standalone_cover(&normalized, &first_chapter),
            "test requires a book with different cover and titlepage images"
        );

        // Register c0 for standalone cover, c1 for titlepage
        ctx.register_section("c0");
        ctx.register_section("c1");
        ctx.cover_fragment_id = Some(ctx.next_fragment_id()); // Mark that standalone cover exists

        // Survey the titlepage chapter
        let source_path = book.source_id(first_chapter_id).unwrap_or("").to_string();
        let first_chapter = book.load_chapter(first_chapter_id).unwrap();
        survey_chapter(&first_chapter, first_chapter_id, &source_path, &mut ctx);

        // Build the titlepage section (c1)
        let first_chapter = book.load_chapter(first_chapter_id).unwrap();
        let (section, _, _) = build_chapter_entities_grouped(
            &first_chapter,
            first_chapter_id,
            "c1",
            &mut ctx,
        );

        // Extract the page_template type from the section
        if let FragmentData::Ion(IonValue::Struct(fields)) = &section.data {
            let page_templates = fields
                .iter()
                .find(|(id, _)| *id == KfxSymbol::PageTemplates as u64)
                .expect("section should have page_templates");

            if let (_, IonValue::List(templates)) = page_templates {
                let template = &templates[0];
                if let IonValue::Struct(template_fields) = template {
                    let type_field = template_fields
                        .iter()
                        .find(|(id, _)| *id == KfxSymbol::Type as u64)
                        .expect("page_template should have type");

                    if let (_, IonValue::Symbol(type_sym)) = type_field {
                        assert_eq!(
                            *type_sym,
                            KfxSymbol::Text as u64,
                            "titlepage (c1) should have type: text when standalone cover exists, \
                             but got type: container"
                        );
                    } else {
                        panic!("type should be a symbol");
                    }
                }
            }
        } else {
            panic!("section should have Ion struct data");
        }
    }
}

#[cfg(test)]
mod resource_export_tests {
    use super::*;
    use crate::book::Book;

    #[test]
    fn test_kfx_export_includes_images() {
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let data = build_kfx_container(&mut book).unwrap();
        
        // KFX should be > 400KB (images alone are ~401KB)
        assert!(data.len() > 400000, 
            "KFX should include image data, got {} bytes", data.len());
    }

    #[test]
    fn test_kfx_asset_roundtrip() {
        // Export EPUB to KFX
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let kfx_data = build_kfx_container(&mut book).unwrap();
        
        // Write to temp file and re-open
        let temp_path = std::env::temp_dir().join("test_roundtrip.kfx");
        std::fs::write(&temp_path, &kfx_data).unwrap();
        
        let mut reimported = Book::open(&temp_path).unwrap();
        let assets = reimported.list_assets();
        
        // Load all assets and verify total size
        let total_size: usize = assets.iter()
            .filter_map(|a| reimported.load_asset(a).ok())
            .map(|d| d.len())
            .sum();
        
        std::fs::remove_file(&temp_path).ok();
        
        // Should have ~401KB of image data
        assert!(total_size > 100000, 
            "Expected > 100KB of assets from KFX, got {} bytes", total_size);
    }
}
