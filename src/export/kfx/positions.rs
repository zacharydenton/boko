use super::*;

/// Build position_map fragment ($264).
///
/// Maps each section to the list of EIDs it contains. This enables
/// the Kindle reader to track which section contains a given position.
pub(super) fn build_position_map_fragment(
    ctx: &ExportContext,
    _anchor_ids_by_fragment: &HashMap<u64, Vec<u64>>,
) -> KfxFragment {
    let mut entries = Vec::new();

    // Handle standalone cover section (c0) if present
    // Cover contains both the page_template ID and the storyline content ID
    if let Some(cover_fid) = ctx.cover_fragment_id {
        // Build contains list: [section_id, content_id]
        let mut contains_list = vec![IonValue::Int(cover_fid as i64)];
        if let Some(content_id) = ctx.cover_content_id {
            contains_list.push(IonValue::Int(content_id as i64));
        }
        let entry = IonValue::Struct(vec![
            (KfxSymbol::Contains as u64, IonValue::List(contains_list)),
            (
                KfxSymbol::SectionName as u64,
                IonValue::Symbol(ctx.section_ids[0]),
            ),
        ]);
        entries.push(entry);
    }

    // Build entries for spine chapters, pairing each section with its chapter
    // by key rather than by positional index: a chapter that failed to load
    // has no entry in `chapter_fragments`, and index-based pairing would
    // shift every later section onto the wrong chapter's EIDs.
    for &(section_sym, chapter_id) in &ctx.spine_section_chapters {
        // Skipped/unloadable chapter: emit no entry rather than a wrong one.
        let Some(&fragment_id) = ctx.chapter_fragments.get(&chapter_id) else {
            continue;
        };

        let mut eid_list = Vec::new();

        // Include page_template ID first (required for section start images)
        eid_list.push(IonValue::Int(fragment_id as i64));

        // Add all content fragment IDs for this chapter
        if let Some(content_ids) = ctx.content_ids_by_chapter.get(&chapter_id) {
            for &content_id in content_ids {
                eid_list.push(IonValue::Int(content_id as i64));
            }
        }

        let entry = IonValue::Struct(vec![
            (KfxSymbol::Contains as u64, IonValue::List(eid_list)),
            (KfxSymbol::SectionName as u64, IonValue::Symbol(section_sym)),
        ]);
        entries.push(entry);
    }

    let ion = IonValue::List(entries);
    KfxFragment::singleton(KfxSymbol::PositionMap, ion)
}

/// Build position_id_map fragment ($265).
///
/// Maps cumulative character positions (PIDs) to EIDs. This enables
/// reading progress tracking and "go to position" functionality.
///
/// Reference format: Sequential PIDs (0, 1, 2...) for initial entries,
/// then character position offsets for content fragments.
pub(super) fn build_position_id_map_fragment(ctx: &ExportContext) -> KfxFragment {
    let mut entries = Vec::new();
    let mut pid = 0i64;

    // Process cover content ID first if present
    if let Some(cover_id) = ctx.cover_content_id {
        let content_len = ctx
            .content_id_lengths
            .get(&cover_id)
            .copied()
            .unwrap_or(1)
            .max(1) as i64;

        // Note: eid comes first, then pid - matching Amazon's format
        // Note: offset field is omitted when zero (Amazon's format doesn't include it)
        let entry = IonValue::Struct(vec![
            (KfxSymbol::Eid as u64, IonValue::Int(cover_id as i64)),
            (KfxSymbol::Pid as u64, IonValue::Int(pid)),
        ]);
        entries.push(entry);
        pid += content_len;
    }

    // Process chapter content in order (sorted by fragment ID)
    let mut chapter_entries: Vec<_> = ctx.chapter_fragments.iter().collect();
    chapter_entries.sort_by_key(|(_, fid)| **fid);

    for (chapter_id, _) in &chapter_entries {
        if let Some(content_ids) = ctx.content_ids_by_chapter.get(chapter_id) {
            for &eid in content_ids {
                let content_len = ctx
                    .content_id_lengths
                    .get(&eid)
                    .copied()
                    .unwrap_or(1)
                    .max(1) as i64;

                // Note: eid comes first, then pid - matching Amazon's format
                // Note: offset field is omitted when zero
                let entry = IonValue::Struct(vec![
                    (KfxSymbol::Eid as u64, IonValue::Int(eid as i64)),
                    (KfxSymbol::Pid as u64, IonValue::Int(pid)),
                ]);
                entries.push(entry);
                pid += content_len;
            }
        }
    }

    // Add terminator entry with eid=0 and pid=max_pid
    // This is required by Amazon's format to indicate the end of content
    // and provides the max position ID for location count calculation
    let terminator = IonValue::Struct(vec![
        (KfxSymbol::Eid as u64, IonValue::Int(0)),
        (KfxSymbol::Pid as u64, IonValue::Int(pid)),
    ]);
    entries.push(terminator);

    let ion = IonValue::List(entries);
    KfxFragment::singleton(KfxSymbol::PositionIdMap, ion)
}

/// Build location_map fragment ($550).
///
/// Maps location numbers to positions. Each content block gets one entry
/// at offset 0 (matching Amazon's format for this entity).
pub(super) fn build_location_map_fragment(ctx: &ExportContext) -> KfxFragment {
    let mut location_entries = Vec::new();

    // Helper closure to process a single content ID - always offset 0
    let mut process_content_id = |content_id: u64| {
        let entry = IonValue::Struct(vec![
            (KfxSymbol::Id as u64, IonValue::Int(content_id as i64)),
            (KfxSymbol::Offset as u64, IonValue::Int(0)),
        ]);
        location_entries.push(entry);
    };

    // Process cover content ID first if present
    if let Some(cover_id) = ctx.cover_content_id {
        process_content_id(cover_id);
    }

    // Process chapter content in order (sorted by fragment ID)
    let mut chapter_entries: Vec<_> = ctx.chapter_fragments.iter().collect();
    chapter_entries.sort_by_key(|(_, fid)| **fid);

    for (chapter_id, _) in &chapter_entries {
        if let Some(content_ids) = ctx.content_ids_by_chapter.get(chapter_id) {
            for &content_id in content_ids {
                process_content_id(content_id);
            }
        }
    }

    // Wrap in locations list structure
    let ion = IonValue::List(vec![IonValue::Struct(vec![(
        KfxSymbol::Locations as u64,
        IonValue::List(location_entries),
    )])]);

    KfxFragment::singleton(KfxSymbol::LocationMap, ion)
}

/// Build container_entity_map fragment ($419).
///
/// Lists all entities in the container for the reader to enumerate, plus an
/// `entity_dependencies` graph that tells Kindle how sections reach their
/// image data: section → external_resource → bcRawMedia location.
pub(super) fn build_container_entity_map_fragment(
    container_id: &str,
    fragments: &[KfxFragment],
    ctx: &ExportContext,
) -> KfxFragment {
    // Collect all non-singleton entity name symbols (including bcRawMedia
    // location strings — Kindle requires these so it can resolve resource
    // dependencies).
    let mut entity_names: Vec<IonValue> = Vec::new();

    for frag in fragments {
        if frag.fid.starts_with('$') {
            continue;
        }
        if let Some(symbol_id) = ctx.symbols.get(&frag.fid) {
            entity_names.push(IonValue::Symbol(symbol_id));
        }
    }

    let container_entry = IonValue::Struct(vec![
        (
            KfxSymbol::Id as u64,
            IonValue::String(container_id.to_string()),
        ),
        (KfxSymbol::Contains as u64, IonValue::List(entity_names)),
    ]);

    // Build entity_dependencies: section → [resource short names], and
    // external_resource → ['resource/<name>'] (the bcRawMedia symbol).
    let mut dependencies: Vec<IonValue> = Vec::new();

    for (section_name, short_names) in &ctx.section_resource_deps {
        if short_names.is_empty() {
            continue;
        }
        let Some(section_sym) = ctx.symbols.get(section_name) else {
            continue;
        };
        let deps: Vec<IonValue> = short_names
            .iter()
            .filter_map(|n| ctx.symbols.get(n).map(IonValue::Symbol))
            .collect();
        if deps.is_empty() {
            continue;
        }
        dependencies.push(IonValue::Struct(vec![
            (KfxSymbol::Id as u64, IonValue::Symbol(section_sym)),
            (
                KfxSymbol::MandatoryDependencies as u64,
                IonValue::List(deps),
            ),
        ]));
    }

    // Collect every distinct resource short name actually used and emit its
    // bcRawMedia location as a dependency.
    let mut all_short_names: BTreeSet<&String> = BTreeSet::new();
    for short_names in ctx.section_resource_deps.values() {
        for n in short_names {
            all_short_names.insert(n);
        }
    }
    for short_name in all_short_names {
        let Some(resource_sym) = ctx.symbols.get(short_name) else {
            continue;
        };
        let raw_name = format!("resource/{short_name}");
        let Some(raw_sym) = ctx.symbols.get(&raw_name) else {
            continue;
        };
        dependencies.push(IonValue::Struct(vec![
            (KfxSymbol::Id as u64, IonValue::Symbol(resource_sym)),
            (
                KfxSymbol::MandatoryDependencies as u64,
                IonValue::List(vec![IonValue::Symbol(raw_sym)]),
            ),
        ]));
    }

    let mut ion_fields = vec![(
        KfxSymbol::ContainerList as u64,
        IonValue::List(vec![container_entry]),
    )];
    if !dependencies.is_empty() {
        ion_fields.push((
            KfxSymbol::EntityDependencies as u64,
            IonValue::List(dependencies),
        ));
    }
    let ion = IonValue::Struct(ion_fields);

    KfxFragment::singleton(KfxSymbol::ContainerEntityMap, ion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChapterId;
    use crate::kfx::fragment::FragmentData;

    /// Extract (section_symbol, contains_eids) pairs from a position_map fragment.
    fn extract_entries(frag: &KfxFragment) -> Vec<(u64, Vec<i64>)> {
        let FragmentData::Ion(IonValue::List(entries)) = &frag.data else {
            panic!("expected Ion list fragment data");
        };
        entries
            .iter()
            .map(|entry| {
                let IonValue::Struct(fields) = entry else {
                    panic!("expected struct entry");
                };
                let mut section = None;
                let mut eids = Vec::new();
                for (id, value) in fields {
                    if *id == KfxSymbol::SectionName as u64 {
                        if let IonValue::Symbol(s) = value {
                            section = Some(*s);
                        }
                    } else if *id == KfxSymbol::Contains as u64
                        && let IonValue::List(list) = value
                    {
                        for item in list {
                            if let IonValue::Int(i) = item {
                                eids.push(*i);
                            }
                        }
                    }
                }
                (section.expect("entry must have section_name"), eids)
            })
            .collect()
    }

    #[test]
    fn position_map_pairs_sections_with_chapters_by_key() {
        let mut ctx = ExportContext::new();
        let (ch1, ch2, ch3) = (ChapterId(1), ChapterId(2), ChapterId(3));
        let c0 = ctx.register_spine_section("c0", ch1);
        let c1 = ctx.register_spine_section("c1", ch2);
        let c2 = ctx.register_spine_section("c2", ch3);

        ctx.chapter_fragments.insert(ch1, 90);
        ctx.chapter_fragments.insert(ch2, 95);
        ctx.chapter_fragments.insert(ch3, 100);
        ctx.content_ids_by_chapter.insert(ch1, vec![91]);
        ctx.content_ids_by_chapter.insert(ch2, vec![96, 97]);
        ctx.content_ids_by_chapter.insert(ch3, vec![101]);

        let frag = build_position_map_fragment(&ctx, &HashMap::new());
        let entries = extract_entries(&frag);
        assert_eq!(
            entries,
            vec![
                (c0, vec![90, 91]),
                (c1, vec![95, 96, 97]),
                (c2, vec![100, 101]),
            ]
        );
    }

    #[test]
    fn position_map_skips_failed_chapter_without_shifting_sections() {
        // Simulate chapter 2 failing to load: its spine section was
        // registered, but the chapter was never surveyed, so it has no
        // fragment ID and no content IDs.
        let mut ctx = ExportContext::new();
        let (ch1, ch2, ch3) = (ChapterId(1), ChapterId(2), ChapterId(3));
        let c0 = ctx.register_spine_section("c0", ch1);
        let _c1 = ctx.register_spine_section("c1", ch2);
        let c2 = ctx.register_spine_section("c2", ch3);

        // Only chapters 1 and 3 were surveyed successfully.
        ctx.chapter_fragments.insert(ch1, 90);
        ctx.chapter_fragments.insert(ch3, 100);
        ctx.content_ids_by_chapter.insert(ch1, vec![91]);
        ctx.content_ids_by_chapter.insert(ch3, vec![101]);

        let frag = build_position_map_fragment(&ctx, &HashMap::new());
        let entries = extract_entries(&frag);

        // The failed chapter's section produces no entry, and — critically —
        // section c2 still maps to chapter 3's EIDs. (The old positional
        // pairing would have placed chapter 3's EIDs under section c1.)
        assert_eq!(entries, vec![(c0, vec![90, 91]), (c2, vec![100, 101])]);
    }
}
