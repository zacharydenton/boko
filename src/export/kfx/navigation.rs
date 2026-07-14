use super::*;

/// Build the book navigation fragment with resolved positions.
///
/// Uses ctx.position_map to generate correct fid:off positions for TOC entries.
/// Structure: [{reading_order_name: default, nav_containers: [nav_container::{...}, ...]}]
/// Order matches reference KFX: headings, toc, landmarks
pub(super) fn build_book_navigation_fragment_with_positions(
    book: &Book,
    ctx: &ExportContext,
) -> KfxFragment {
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
pub(super) fn build_headings_entries(ctx: &ExportContext) -> Vec<IonValue> {
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
                        (
                            KfxSymbol::Id as u64,
                            IonValue::Int(first.fragment_id as i64),
                        ),
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
pub(super) fn build_landmarks_entries(_book: &Book, ctx: &ExportContext) -> Vec<IonValue> {
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
                        (
                            KfxSymbol::Id as u64,
                            IonValue::Int(target.fragment_id as i64),
                        ),
                        (
                            KfxSymbol::Offset as u64,
                            IonValue::Int(target.offset as i64),
                        ),
                    ]),
                ),
            ])),
        );
        entries.push(entry);
    }

    entries
}

/// Build TOC entries recursively with anchor entity IDs.
///
/// TOC entries point to content fragment IDs (with offset 0) rather than
/// anchor entities. The `entry.target` field is pre-resolved by `resolve_links()`.
pub(super) fn build_toc_entries_with_positions(
    entries: &[crate::model::TocEntry],
    ctx: &ExportContext,
) -> Vec<IonValue> {
    entries
        .iter()
        .filter_map(|entry| {
            // Use pre-resolved target to look up position
            let (fragment_id, offset) = resolve_toc_target(&entry.target, &entry.href, ctx)?;

            let mut fields = Vec::new();

            // Add representation with label
            let representation = IonValue::Struct(vec![(
                KfxSymbol::Label as u64,
                IonValue::String(entry.title.clone()),
            )]);
            fields.push((KfxSymbol::Representation as u64, representation));

            // Target position points directly to content fragment
            let target = IonValue::Struct(vec![
                (KfxSymbol::Id as u64, IonValue::Int(fragment_id as i64)),
                (KfxSymbol::Offset as u64, IonValue::Int(offset as i64)),
            ]);
            fields.push((KfxSymbol::TargetPosition as u64, target));

            // Add children if present
            if !entry.children.is_empty() {
                let child_entries = build_toc_entries_with_positions(&entry.children, ctx);
                if !child_entries.is_empty() {
                    fields.push((KfxSymbol::Entries as u64, IonValue::List(child_entries)));
                }
            }

            let nav_unit = IonValue::Struct(fields);
            // Annotate with nav_unit::
            Some(IonValue::Annotated(
                vec![KfxSymbol::NavUnit as u64],
                Box::new(nav_unit),
            ))
        })
        .collect()
}

/// Resolve a TOC entry's pre-resolved target to (fragment_id, offset).
///
/// Uses the target from `resolve_links()` to look up the content position.
pub(super) fn resolve_toc_target(
    target: &Option<AnchorTarget>,
    href: &str,
    ctx: &ExportContext,
) -> Option<(u64, usize)> {
    match target {
        Some(AnchorTarget::Internal(gid)) => {
            // Look up node position - TOC always uses offset 0 (Kindle requirement)
            if let Some((fragment_id, _offset)) = ctx.anchor_registry.get_node_position(*gid) {
                return Some((fragment_id, 0));
            }
        }
        Some(AnchorTarget::Chapter(chapter_id)) => {
            // Look up chapter position
            if let Some(fragment_id) = ctx.anchor_registry.get_chapter_position(*chapter_id) {
                return Some((fragment_id, 0));
            }
        }
        Some(AnchorTarget::External(_)) => {
            // External links in TOC - shouldn't happen but handle gracefully
            return None;
        }
        None => {}
    }

    eprintln!("Warning: TOC href not resolved: {}", href);
    None
}

// ============================================================================
// Entity Assembler: Packages Schema output into KFX Entity Hierarchy
// ============================================================================

/// Resolve landmarks from the Book's IR to fragment IDs.
///
/// This uses the parsed landmarks from the source format (EPUB, KFX, etc.)
/// to populate landmark_fragments in the context.
///
/// Handles both chapter-level targets (e.g., `chapter.xhtml`) and anchor-level
/// targets (e.g., `chapter.xhtml#section1`) by using ResolvedLinks.
pub(super) fn resolve_landmarks_from_ir(
    book: &Book,
    source_to_chapter: &HashMap<String, ChapterId>,
    resolved: &ResolvedLinks,
    ctx: &mut ExportContext,
) {
    for landmark in book.landmarks() {
        // Split href into file path and optional anchor
        let (href_path, _anchor) = match landmark.href.split_once('#') {
            Some((path, anchor)) => (path, Some(anchor)),
            None => (landmark.href.as_str(), None),
        };

        // Try to find the chapter ID for this href
        let chapter_id = source_to_chapter.get(href_path).copied();

        if let Some(cid) = chapter_id {
            // Resolve the landmark's href using the book's resolver
            let resolved_target = book.resolve_href(cid, &landmark.href);

            let target = match resolved_target {
                Some(AnchorTarget::Internal(gid)) => {
                    // Look up position for the internal target
                    ctx.position_map
                        .get(&(gid.chapter, gid.node))
                        .map(|pos| LandmarkTarget {
                            fragment_id: pos.fragment_id,
                            offset: 0,
                            label: landmark.label.clone(),
                        })
                }
                Some(AnchorTarget::Chapter(target_chapter)) => {
                    // Use chapter's fragment ID
                    ctx.chapter_fragments
                        .get(&target_chapter)
                        .copied()
                        .map(|frag_id| LandmarkTarget {
                            fragment_id: frag_id,
                            offset: 0,
                            label: landmark.label.clone(),
                        })
                }
                _ => {
                    // Fall back to chapter's fragment ID
                    ctx.chapter_fragments
                        .get(&cid)
                        .copied()
                        .map(|frag_id| LandmarkTarget {
                            fragment_id: frag_id,
                            offset: 0,
                            label: landmark.label.clone(),
                        })
                }
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

    // Suppress unused variable warning - resolved is used for consistency
    let _ = resolved;
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
                assert!(
                    has_categorised,
                    "book_metadata should contain categorised_metadata"
                );

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
                assert!(
                    features.is_some(),
                    "content_features should contain features"
                );

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
    fn test_document_data_max_id_reflects_all_fragment_ids() {
        let mut ctx = ExportContext::new();
        ctx.register_section("c0");

        // Simulate generating many fragment IDs (like content generation does)
        for _ in 0..100 {
            ctx.next_fragment_id();
        }

        let frag = build_document_data_fragment(&ctx);

        // Extract max_id from the fragment
        if let crate::kfx::fragment::FragmentData::Ion(IonValue::Struct(fields)) = &frag.data {
            let max_id_field = fields.iter().find(|(id, _)| *id == KfxSymbol::MaxId as u64);

            if let Some((_, IonValue::Int(max_id))) = max_id_field {
                // max_id should be at least 100 (the IDs we generated)
                // Context starts at 866, so after 100 IDs we should be at 965
                assert!(
                    *max_id >= 100,
                    "max_id ({}) should reflect all generated fragment IDs",
                    max_id
                );
            } else {
                panic!("max_id should be an integer");
            }
        } else {
            panic!("expected Ion struct data");
        }
    }

    #[test]
    fn test_singleton_uses_null_symbol() {
        // Build a singleton fragment and serialize it
        let frags = [build_content_features_fragment()];
        let local_symbols: Vec<String> = vec![];
        let entities = serialize_fragments(&frags, &local_symbols);

        // Singleton should use $348 (null) as ID
        assert_eq!(entities[0].id, KfxSymbol::Null as u32);
    }

    #[test]
    fn test_build_headings_entries_empty() {
        let ctx = ExportContext::new();
        let entries = build_headings_entries(&ctx);
        assert!(
            entries.is_empty(),
            "No headings should produce empty entries"
        );
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
                let nested = fields
                    .iter()
                    .find(|(id, _)| *id == KfxSymbol::Entries as u64);
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
        if let IonValue::Annotated(_, inner) = &entries[0]
            && let IonValue::Struct(fields) = inner.as_ref()
        {
            let target = fields
                .iter()
                .find(|(id, _)| *id == KfxSymbol::TargetPosition as u64);
            if let Some((_, IonValue::Struct(pos_fields))) = target {
                let id_field = pos_fields
                    .iter()
                    .find(|(id, _)| *id == KfxSymbol::Id as u64);
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

    #[test]
    fn test_position_id_map_includes_all_content_ids() {
        use crate::ChapterId;

        let mut ctx = ExportContext::new();
        ctx.register_section("c0");
        ctx.register_section("c1");

        // Simulate two chapters with multiple content IDs each
        let chapter1 = ChapterId(1);
        let chapter2 = ChapterId(2);

        // Add content IDs for each chapter
        ctx.content_ids_by_chapter
            .entry(chapter1)
            .or_default()
            .extend(vec![100, 101, 102]);
        ctx.content_ids_by_chapter
            .entry(chapter2)
            .or_default()
            .extend(vec![200, 201]);

        // Set up chapter_fragments for ordering
        ctx.chapter_fragments.insert(chapter1, 90);
        ctx.chapter_fragments.insert(chapter2, 95);

        let frag = build_position_id_map_fragment(&ctx);

        // Extract and verify the position_id_map entries
        if let crate::kfx::fragment::FragmentData::Ion(IonValue::List(entries)) = &frag.data {
            // Should have 6 entries (100, 101, 102, 200, 201) + 1 terminator (eid=0)
            assert_eq!(
                entries.len(),
                6,
                "position_id_map should have one entry per content ID plus terminator"
            );

            // Extract all eids
            let eids: Vec<i64> = entries
                .iter()
                .filter_map(|entry| {
                    if let IonValue::Struct(fields) = entry {
                        fields
                            .iter()
                            .find(|(id, _)| *id == KfxSymbol::Eid as u64)
                            .and_then(|(_, v)| {
                                if let IonValue::Int(eid) = v {
                                    Some(*eid)
                                } else {
                                    None
                                }
                            })
                    } else {
                        None
                    }
                })
                .collect();

            // Should contain all content IDs
            assert!(eids.contains(&100), "should contain content ID 100");
            assert!(eids.contains(&101), "should contain content ID 101");
            assert!(eids.contains(&102), "should contain content ID 102");
            assert!(eids.contains(&200), "should contain content ID 200");
            assert!(eids.contains(&201), "should contain content ID 201");
        } else {
            panic!("expected List data");
        }
    }
}
