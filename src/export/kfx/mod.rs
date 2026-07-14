//! KFX format exporter.
//!
//! This module provides the `KfxExporter` which implements the `Exporter` trait
//! for writing books in Amazon's KFX format.

mod content;
mod metadata_frags;
mod navigation;
mod positions;
mod resources;
mod survey;

use content::*;
use metadata_frags::*;
use navigation::*;
use positions::*;
use resources::*;
use survey::*;

use std::collections::{BTreeSet, HashMap};
use std::io::{self, Seek, Write};

use crate::export::Exporter;
use crate::import::ChapterId;
use crate::kfx::auxiliary::build_auxiliary_data_fragment;
use crate::kfx::context::{ExportContext, LandmarkTarget};
use crate::kfx::cover::{
    COVER_SECTION_NAME, build_cover_section, is_image_only_chapter, needs_standalone_cover,
    normalize_cover_path,
};
use crate::kfx::fragment::KfxFragment;
use crate::kfx::ion::IonValue;
use crate::kfx::metadata::{
    MetadataCategory, MetadataContext, build_category_entries, generate_book_id,
};
use crate::kfx::serialization::{
    SerializedEntity, create_entity_data, generate_container_id, serialize_annotated_ion,
    serialize_container,
};
use crate::kfx::symbols::KfxSymbol;
use crate::kfx::transforms::format_to_kfx_symbol;
use crate::model::{
    AnchorTarget, Book, Chapter, GlobalNodeId, LandmarkType, NodeId, ResolvedLinks, Role,
};
use crate::util::detect_media_format;

/// KFX format exporter.
///
/// Converts books to Amazon's KFX format for Kindle devices.
#[derive(Default)]
pub struct KfxExporter;

impl KfxExporter {
    /// Create a new KfxExporter.
    pub fn new() -> Self {
        Self
    }
}

impl Exporter for KfxExporter {
    fn export<W: Write + Seek>(&self, book: &mut Book, writer: &mut W) -> crate::Result<()> {
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
fn build_kfx_container(book: &mut Book) -> crate::Result<Vec<u8>> {
    let container_id = generate_container_id();
    let mut ctx = ExportContext::new();

    // ========================================================================
    // PASS 1: SURVEY (Read-Only / State Accumulation)
    // Goal: Populate ctx.symbols, ctx.position_map, ctx.chapter_fragments
    // NO ION GENERATION HERE!
    // ========================================================================

    // Check if we need a standalone cover section
    // This happens when the EPUB cover image differs from the first spine chapter's image
    let asset_paths: Vec<_> = book.list_assets().to_vec();
    let cover_image = book.metadata().cover_image.clone();
    let first_chapter_id = book.spine().first().map(|e| e.id);

    let standalone_cover_path: Option<String> = match (cover_image, first_chapter_id) {
        (Some(cover_img), Some(first_id)) => {
            let normalized = normalize_cover_path(&cover_img, &asset_paths);
            book.load_chapter_cached(first_id)
                .ok()
                .and_then(|first_chapter| {
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
    let section_offset = if standalone_cover_path.is_some() {
        1
    } else {
        0
    };

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

    // Register cover section in Pass 1 if standalone cover is needed
    // This ensures it appears in reading_orders.sections and landmarks point to it
    if standalone_cover_path.is_some() {
        ctx.register_section(COVER_SECTION_NAME);
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

    // 1a. Resolve all links using the centralized resolver
    // This builds the forward/reverse link maps and resolves TOC targets.
    let resolved = book.resolve_links()?;

    // 1b. Register link targets with the anchor registry
    // This maps hrefs to targets for storyline link_to generation.
    register_link_targets(book, &spine_info, &resolved, &mut ctx)?;

    // 1c. Survey each chapter: assign fragment IDs, build position map
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
        if let Ok(chapter) = book.load_chapter_cached(*chapter_id) {
            survey_chapter(&chapter, *chapter_id, &source_path, &mut ctx);
        }
    }

    // 1d. Resolve landmarks to fragment IDs
    // First try IR landmarks, then fall back to heuristics for Cover/StartReading
    resolve_landmarks_from_ir(book, &source_to_chapter, &resolved, &mut ctx);

    // Fall back to heuristics if IR didn't provide Cover or StartReading
    let has_cover = ctx.landmark_fragments.contains_key(&LandmarkType::Cover);
    let has_srl = ctx
        .landmark_fragments
        .contains_key(&LandmarkType::StartReading);

    if !has_cover || !has_srl {
        for (chapter_id, _section_name) in &spine_info {
            if let Ok(chapter) = book.load_chapter_cached(*chapter_id) {
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
                        && !ctx
                            .landmark_fragments
                            .contains_key(&LandmarkType::StartReading)
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
                    && ctx
                        .landmark_fragments
                        .contains_key(&LandmarkType::StartReading)
                {
                    break;
                }
            }
        }
    }

    // 1c. TOC strings are used directly in Ion output, no symbol interning needed

    // 1d. Register nav container names as symbols
    ctx.nav_container_symbols.toc = ctx.symbols.get_or_intern("toc");
    ctx.nav_container_symbols.headings = ctx.symbols.get_or_intern("headings");
    ctx.nav_container_symbols.landmarks = ctx.symbols.get_or_intern("landmarks");

    // 1e. Register resource paths and create short names
    // IMPORTANT: Short names must be interned during Pass 1 to ensure
    // consistent symbol IDs when they're referenced later in storylines
    let asset_paths: Vec<_> = book.list_assets().to_vec();
    for asset_path in &asset_paths {
        if is_media_asset(asset_path) {
            ctx.resource_registry.register(asset_path, &mut ctx.symbols);
            // Create and intern the short name (e.g., "e0")
            let short_name = ctx.resource_registry.get_or_create_name(asset_path);
            ctx.symbols.get_or_intern(&short_name);
        }
    }

    // After Pass 1: ctx.symbols is COMPLETE, ctx.position_map has all EIDs
    // Note: TOC anchor entity IDs are computed AFTER Pass 2 chapter processing
    // since anchors are created during content generation.

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

    // NOTE: document_data ($538) is built AFTER chapters so max_id includes all content IDs.
    // We'll insert it at this position (index 3) later.
    let document_data_index = fragments.len();

    // 2g. Chapter entities - collect separately for proper grouping
    // Note: This also collects styles during token generation
    let mut section_fragments = Vec::new();
    let mut storyline_fragments = Vec::new();
    let mut content_fragments = Vec::new();

    // Generate standalone cover section if needed (c0)
    // Note: cover_fragment_id was assigned in Pass 1 for landmark resolution
    if let Some(ref cover_path) = standalone_cover_path {
        let section_id = ctx
            .cover_fragment_id
            .expect("cover_fragment_id should be set in Pass 1");
        // Get the next fragment ID which will be the cover's content ID
        let cover_content_id = ctx.fragment_ids.peek();
        // Store cover content ID for position_map (so c0 contains both section and content IDs)
        ctx.cover_content_id = Some(cover_content_id);
        let (section, storyline) = build_cover_section(cover_path, section_id, &mut ctx);
        section_fragments.push(section);
        storyline_fragments.push(storyline);

        // Update cover landmark to use the content ID instead of section ID
        if let Some(target) = ctx.landmark_fragments.get_mut(&LandmarkType::Cover) {
            target.fragment_id = cover_content_id;
        }
    }

    for (chapter_id, section_name) in &spine_info {
        if let Ok(chapter) = book.load_chapter_cached(*chapter_id) {
            // Set up chapter-start anchor before generating content
            ctx.begin_chapter_export(*chapter_id);

            let (section, storyline, content) =
                build_chapter_entities_grouped(&chapter, *chapter_id, section_name, &mut ctx);
            section_fragments.push(section);
            storyline_fragments.push(storyline);
            if let Some(c) = content {
                content_fragments.push(c);
            }

            // Record which image resources this section depends on, so the
            // container_entity_map can declare the dependency graph that
            // Kindle uses to locate images.
            for node_id in chapter.iter_dfs() {
                if let Some(node) = chapter.node(node_id)
                    && node.role == crate::model::Role::Image
                    && let Some(src) = chapter.semantics.src(node_id)
                {
                    let short_name = ctx.resource_registry.get_or_create_name(src);
                    ctx.record_section_image_ref(section_name, &short_name);
                }
            }
        }
    }

    // Fix landmark IDs to use storyline content IDs instead of section IDs
    ctx.fix_landmark_content_ids();

    // 2e. Book navigation fragment - built AFTER chapters so heading/anchor positions are available
    fragments.push(build_book_navigation_fragment_with_positions(book, &ctx));

    // Add chapter content in reference order: sections, then storylines, then content
    fragments.extend(section_fragments);
    fragments.extend(storyline_fragments);
    fragments.extend(content_fragments);

    // 2g. Style entities ($157) - generated AFTER chapters since styles are collected during token generation
    // This includes the default style plus any unique styles found in the content
    let style_fragments = build_style_fragments(&mut ctx);
    fragments.extend(style_fragments);

    // 2h. Anchor fragments - must come after sections/storylines/content/styles
    // This matches the reference KFX entity ordering
    let (anchor_frags, anchor_ids_by_fragment) = build_anchor_fragments(&mut ctx);
    fragments.extend(anchor_frags);

    // 2i. Auxiliary data fragments - mark sections as navigation targets
    // Generate one auxiliary_data entity per section
    if standalone_cover_path.is_some() {
        fragments.push(build_auxiliary_data_fragment(COVER_SECTION_NAME, &mut ctx));
    }
    for (_, section_name) in &spine_info {
        fragments.push(build_auxiliary_data_fragment(section_name, &mut ctx));
    }

    // 2j. Resource fragments (images, fonts, etc.)
    // Each resource gets two entities: external_resource (metadata) + bcRawMedia (bytes)
    for asset_path in &asset_paths {
        if is_media_asset(asset_path)
            && let Ok(data) = book.load_asset(asset_path)
        {
            // external_resource ($164) - metadata about the resource
            fragments.push(build_external_resource_fragment(asset_path, &data, &mut ctx));
            // bcRawMedia ($417) - the actual bytes (moved, not copied)
            fragments.push(build_resource_fragment(asset_path, data, &mut ctx));
        }
    }

    // 2j-2. Font entity fragments ($262)
    // These link font_family names to resource locations (from @font-face rules)
    let font_frags = build_font_fragments(book, &mut ctx);
    fragments.extend(font_frags);

    // 2k. Navigation maps for reader functionality
    fragments.push(build_position_map_fragment(&ctx, &anchor_ids_by_fragment));
    fragments.push(build_position_id_map_fragment(&ctx));
    fragments.push(build_location_map_fragment(&ctx));

    // 2l. Container metadata entities
    fragments.push(build_resource_path_fragment());
    fragments.push(build_container_entity_map_fragment(
        &container_id,
        &fragments,
        &ctx,
    ));

    // 2d. Document data fragment ($538) - built AFTER all IDs are assigned so max_id is correct
    // Insert at position 3 (after content_features, book_metadata, metadata)
    fragments.insert(document_data_index, build_document_data_fragment(&ctx));

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

/// Serialize fragments to entities.
fn serialize_fragments<'a>(
    fragments: &'a [KfxFragment],
    local_symbols: &[String],
) -> Vec<SerializedEntity<'a>> {
    // Index the local symbol table once; a per-fragment linear `position`
    // scan is O(fragments × symbols), which is quadratic in book size.
    let symbol_index: rustc_hash::FxHashMap<&str, usize> = local_symbols
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    fragments
        .iter()
        .map(|frag| {
            let id = if frag.is_singleton() {
                KfxSymbol::Null as u32 // Singleton marker ($348 = null)
            } else {
                // Look up local symbol ID
                symbol_index
                    .get(frag.fid.as_str())
                    .map(|&i| (crate::kfx::symbols::KFX_SYMBOL_TABLE_SIZE + i) as u32)
                    .unwrap_or(0)
            };

            let (data, raw) = match &frag.data {
                crate::kfx::fragment::FragmentData::Ion(value) => (create_entity_data(value), None),
                // Raw media bodies are borrowed, not copied: the container
                // writer emits them straight from the fragment.
                crate::kfx::fragment::FragmentData::Raw(bytes) => (
                    crate::kfx::serialization::create_raw_media_header(),
                    Some(bytes.as_slice()),
                ),
            };

            SerializedEntity {
                id,
                entity_type: frag.ftype as u32,
                data,
                raw,
            }
        })
        .collect()
}
