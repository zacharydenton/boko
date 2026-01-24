//! Navigation structures for KFX (TOC, landmarks, anchors).

use std::collections::HashMap;

use crate::book::TocEntry;
use crate::kfx::ion::IonValue;

use super::symbols::{SymbolTable, sym};

use super::content::{ChapterData, ContentItem};

/// Positions per page for approximate page list (matches kfxinput default)
const POSITIONS_PER_PAGE: usize = 1850;

/// Build the book navigation fragment ($389)
///
/// Creates a book navigation structure with INLINE $391:: nav containers.
/// Each nav container has INLINE $393:: nav entries.
/// This matches the reference KFX format.
pub fn build_book_navigation(
    toc: &[TocEntry],
    chapters: &[ChapterData],
    section_eids: &HashMap<String, i64>,
    anchor_eids: &HashMap<String, (i64, i64)>,
    symtab: &mut SymbolTable,
    has_cover: bool,
    first_content_eid: Option<i64>,
) -> IonValue {
    let mut nav_containers = Vec::new();

    // === Headings Nav Container ($798) ===
    // Required for Kindle to display TOC properly
    // Contains entries with "heading-nav-unit" labels pointing to each section
    let nav_headings_id = "nav-headings";
    let nav_headings_sym = symtab.get_or_intern(nav_headings_id);

    let headings_entries = build_headings_entries(toc, section_eids, anchor_eids);

    let headings_container = IonValue::OrderedStruct(vec![
        (sym::NAV_TYPE, IonValue::Symbol(sym::HEADINGS_NAV_TYPE)),
        (sym::NAV_ID, IonValue::Symbol(nav_headings_sym)),
        (sym::NAV_ENTRIES, IonValue::List(headings_entries)),
    ]);

    nav_containers.push(IonValue::Annotated(
        vec![sym::NAV_CONTAINER_TYPE],
        Box::new(headings_container),
    ));

    // === TOC Nav Container ===
    // Put TOC right after HEADINGS to match reference KFX structure
    let nav_toc_id = "nav-toc";
    let nav_toc_sym = symtab.get_or_intern(nav_toc_id);

    let nav_entry_values = build_nav_entries_recursive(toc, section_eids, anchor_eids);

    let toc_container = IonValue::OrderedStruct(vec![
        (sym::NAV_TYPE, IonValue::Symbol(sym::TOC)),
        (sym::NAV_ID, IonValue::Symbol(nav_toc_sym)),
        (sym::NAV_ENTRIES, IonValue::List(nav_entry_values)),
    ]);

    // Wrap in $391:: annotation for inline nav container
    nav_containers.push(IonValue::Annotated(
        vec![sym::NAV_CONTAINER_TYPE],
        Box::new(toc_container),
    ));

    // === Landmarks Nav Container ===
    let nav_landmarks_id = "nav-landmarks";
    let nav_landmarks_sym = symtab.get_or_intern(nav_landmarks_id);

    let mut landmark_entries = Vec::new();

    // Cover landmark (if cover exists)
    if has_cover {
        let cover_eid = SymbolTable::LOCAL_MIN_ID as i64 + 1;
        landmark_entries.push(build_landmark_entry(
            "cover-nav-unit",
            cover_eid,
            Some(sym::LANDMARK_COVER),
        ));
    }

    // Bodymatter landmark (first content section)
    if let Some(eid) = first_content_eid {
        let bodymatter_title = toc.first().map(|e| e.title.as_str()).unwrap_or("Content");
        landmark_entries.push(build_landmark_entry(
            bodymatter_title,
            eid,
            Some(sym::LANDMARK_BODYMATTER),
        ));
    }

    let landmarks_container = IonValue::OrderedStruct(vec![
        (sym::NAV_TYPE, IonValue::Symbol(sym::LANDMARKS_NAV_TYPE)),
        (sym::NAV_ID, IonValue::Symbol(nav_landmarks_sym)),
        (sym::NAV_ENTRIES, IonValue::List(landmark_entries)),
    ]);

    // Wrap in $391:: annotation for inline nav container
    nav_containers.push(IonValue::Annotated(
        vec![sym::NAV_CONTAINER_TYPE],
        Box::new(landmarks_container),
    ));

    // === Page List Nav Container ===
    // Generate approximate page numbers based on character positions
    // Placed after TOC and Landmarks to match reference structure
    let page_entries = build_page_list_entries(chapters, has_cover);
    if !page_entries.is_empty() {
        let nav_page_list_id = "nav-page-list";
        let nav_page_list_sym = symtab.get_or_intern(nav_page_list_id);

        let page_list_container = IonValue::OrderedStruct(vec![
            (sym::NAV_TYPE, IonValue::Symbol(sym::PAGE_LIST_NAV_TYPE)),
            (sym::NAV_ID, IonValue::Symbol(nav_page_list_sym)),
            (sym::NAV_ENTRIES, IonValue::List(page_entries)),
        ]);

        nav_containers.push(IonValue::Annotated(
            vec![sym::NAV_CONTAINER_TYPE],
            Box::new(page_list_container),
        ));
    }

    // Book navigation root - contains inline nav containers
    let nav = IonValue::OrderedStruct(vec![
        (sym::READING_ORDER_NAME, IonValue::Symbol(sym::DEFAULT_READING_ORDER)),
        (sym::NAV_CONTAINER_REF, IonValue::List(nav_containers)),
    ]);

    IonValue::List(vec![nav])
}

/// Build headings navigation entries for $798 (HEADINGS) container
/// These entries use "heading-nav-unit" as the label and point to section headings
fn build_headings_entries(
    toc: &[TocEntry],
    section_eids: &HashMap<String, i64>,
    anchor_eids: &HashMap<String, (i64, i64)>,
) -> Vec<IonValue> {
    let mut entries = Vec::new();

    fn collect_headings(
        toc_entries: &[TocEntry],
        section_eids: &HashMap<String, i64>,
        anchor_eids: &HashMap<String, (i64, i64)>,
        entries: &mut Vec<IonValue>,
    ) {
        for entry in toc_entries {
            // Parse the href to extract path and fragment
            let (path, fragment) = if let Some(hash_pos) = entry.href.find('#') {
                (&entry.href[..hash_pos], Some(&entry.href[hash_pos + 1..]))
            } else {
                (entry.href.as_str(), None)
            };

            // Look up the EID for this entry
            let eid_offset = if fragment.is_some() {
                anchor_eids
                    .get(&entry.href)
                    .copied()
                    .or_else(|| section_eids.get(path).map(|&eid| (eid, 0)))
            } else {
                section_eids.get(path).map(|&eid| (eid, 0))
            };

            if let Some((eid, _)) = eid_offset {
                // Create heading entry with "heading-nav-unit" label
                let nav_title = IonValue::OrderedStruct(vec![
                    (sym::TEXT, IonValue::String("heading-nav-unit".to_string())),
                ]);

                let nav_target = IonValue::OrderedStruct(vec![
                    (sym::POSITION, IonValue::Int(eid)),
                    (sym::OFFSET, IonValue::Int(0)),
                ]);

                let nav_entry = IonValue::OrderedStruct(vec![
                    (sym::NAV_TITLE, nav_title),
                    (sym::NAV_TARGET, nav_target),
                ]);

                entries.push(IonValue::Annotated(
                    vec![sym::NAV_DEFINITION],
                    Box::new(nav_entry),
                ));
            }

            // Recurse into children
            if !entry.children.is_empty() {
                collect_headings(&entry.children, section_eids, anchor_eids, entries);
            }
        }
    }

    collect_headings(toc, section_eids, anchor_eids, &mut entries);
    entries
}

/// Build a landmark navigation entry with $393:: annotation
fn build_landmark_entry(title: &str, eid: i64, landmark_type: Option<u64>) -> IonValue {
    let nav_title = IonValue::OrderedStruct(vec![
        (sym::TEXT, IonValue::String(title.to_string())),
    ]);

    // Nav target: { $155: eid, $143: 0 }
    let nav_target = IonValue::OrderedStruct(vec![
        (sym::POSITION, IonValue::Int(eid)),
        (sym::OFFSET, IonValue::Int(0)),
    ]);

    let mut nav_entry_fields = vec![
        (sym::NAV_TITLE, nav_title),
        (sym::NAV_TARGET, nav_target),
    ];

    if let Some(lt) = landmark_type {
        nav_entry_fields.push((sym::LANDMARK_TYPE, IonValue::Symbol(lt)));
    }

    // Wrap in $393:: annotation for inline nav entry
    IonValue::Annotated(
        vec![sym::NAV_DEFINITION],
        Box::new(IonValue::OrderedStruct(nav_entry_fields)),
    )
}

/// Recursively build nav entries with $393:: annotations, preserving TOC hierarchy
fn build_nav_entries_recursive(
    entries: &[TocEntry],
    section_eids: &HashMap<String, i64>,
    anchor_eids: &HashMap<String, (i64, i64)>,
) -> Vec<IonValue> {
    let mut nav_entries = Vec::new();

    for entry in entries {
        // Parse the href to extract path and fragment
        let (path, fragment) = if let Some(hash_pos) = entry.href.find('#') {
            (&entry.href[..hash_pos], Some(&entry.href[hash_pos + 1..]))
        } else {
            (entry.href.as_str(), None)
        };

        // Look up the (EID, offset) for this entry
        let eid_offset = if fragment.is_some() {
            anchor_eids
                .get(&entry.href)
                .copied()
                .or_else(|| section_eids.get(path).map(|&eid| (eid, 0)))
        } else {
            section_eids.get(path).map(|&eid| (eid, 0))
        };

        if let Some((eid, offset)) = eid_offset {
            // Use OrderedStruct for nav_title to ensure consistent field ordering
            let nav_title = IonValue::OrderedStruct(vec![
                (sym::TEXT, IonValue::String(entry.title.clone())),
            ]);

            // nav_target already uses OrderedStruct
            let nav_target = IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(eid)),
                (sym::OFFSET, IonValue::Int(offset)),
            ]);

            // Build nav_entry with OrderedStruct - field order matters for Kindle!
            // Order: NAV_TITLE ($241), NAV_TARGET ($246), NAV_ENTRIES ($247)
            let mut nav_entry_fields = vec![
                (sym::NAV_TITLE, nav_title),
                (sym::NAV_TARGET, nav_target),
            ];

            // Recursively build children
            if !entry.children.is_empty() {
                let nested_entries =
                    build_nav_entries_recursive(&entry.children, section_eids, anchor_eids);
                if !nested_entries.is_empty() {
                    nav_entry_fields.push((sym::NAV_ENTRIES, IonValue::List(nested_entries)));
                }
            }

            // Wrap in $393:: annotation for inline nav entry
            nav_entries.push(IonValue::Annotated(
                vec![sym::NAV_DEFINITION],
                Box::new(IonValue::OrderedStruct(nav_entry_fields)),
            ));
        } else if !entry.children.is_empty() {
            // Entry itself doesn't map to a section, but children might
            let nested_entries =
                build_nav_entries_recursive(&entry.children, section_eids, anchor_eids);
            nav_entries.extend(nested_entries);
        }
    }

    nav_entries
}

/// Build empty nav entries list (placeholder for navigation structure)
pub fn build_nav_unit_list() -> IonValue {
    IonValue::OrderedStruct(vec![
        (sym::NAV_ENTRIES, IonValue::List(Vec::new())),
    ])
}

/// Build anchor symbols mapping for URLs found in content
pub fn build_anchor_symbols(
    chapters: &[crate::kfx::writer::content::ChapterData],
    symtab: &mut SymbolTable,
) -> HashMap<String, u64> {
    use crate::kfx::writer::content::ContentItem;
    use std::collections::BTreeSet;

    fn collect_anchor_hrefs(item: &ContentItem, hrefs: &mut BTreeSet<String>) {
        match item {
            ContentItem::Text { inline_runs, .. } => {
                for run in inline_runs {
                    if let Some(ref href) = run.anchor_href
                        && !href.starts_with('#')
                    {
                        hrefs.insert(href.clone());
                    }
                }
            }
            ContentItem::Container { children, .. } => {
                for child in children {
                    collect_anchor_hrefs(child, hrefs);
                }
            }
            _ => {}
        }
    }

    // Use BTreeSet for deterministic iteration order
    // This ensures anchor symbols are assigned consistently
    let mut unique_hrefs: BTreeSet<String> = BTreeSet::new();
    for chapter in chapters {
        for item in &chapter.content {
            collect_anchor_hrefs(item, &mut unique_hrefs);
        }
    }

    let mut anchor_symbols = HashMap::new();
    for (anchor_index, href) in unique_hrefs.into_iter().enumerate() {
        let anchor_id = format!("anchor{anchor_index}");
        let anchor_sym = symtab.get_or_intern(&anchor_id);
        anchor_symbols.insert(href, anchor_sym);
    }

    anchor_symbols
}

/// Build page list navigation entries for approximate page numbers
///
/// Generates page entries at approximately POSITIONS_PER_PAGE character intervals.
/// Each page entry has a label ("1", "2", etc.) and a target position (EID + offset).
fn build_page_list_entries(chapters: &[ChapterData], has_cover: bool) -> Vec<IonValue> {
    let mut pages = Vec::new();
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
    let mut cumulative_pos: usize = 0;
    let mut next_page_pos: usize = 0;
    let mut page_num = 1;

    // Track content chunks with their EIDs and lengths
    struct ContentChunk {
        eid: i64,
        length: usize,
        is_section_start: bool,
    }

    let mut chunks: Vec<ContentChunk> = Vec::new();

    // Add cover if present (cover image counts as 1 position)
    if has_cover {
        chunks.push(ContentChunk {
            eid: eid_base + 1, // Cover image EID
            length: 1,
            is_section_start: true,
        });
        eid_base += 2;
    }

    // Collect content chunks from chapters
    for chapter in chapters {
        let mut content_eid = eid_base + 1;
        let mut is_first = true;

        fn collect_chunks(
            item: &ContentItem,
            eid: &mut i64,
            chunks: &mut Vec<ContentChunk>,
            is_first: &mut bool,
        ) {
            match item {
                ContentItem::Text { text, is_verse, .. } => {
                    if *is_verse {
                        for line in text.split('\n').filter(|s| !s.trim().is_empty()) {
                            chunks.push(ContentChunk {
                                eid: *eid,
                                length: line.trim().chars().count(),
                                is_section_start: *is_first,
                            });
                            *is_first = false;
                            *eid += 1;
                        }
                    } else if !text.trim().is_empty() {
                        chunks.push(ContentChunk {
                            eid: *eid,
                            length: text.chars().count(),
                            is_section_start: *is_first,
                        });
                        *is_first = false;
                        *eid += 1;
                    }
                }
                ContentItem::Image { .. } | ContentItem::Svg { .. } => {
                    chunks.push(ContentChunk {
                        eid: *eid,
                        length: 1,
                        is_section_start: *is_first,
                    });
                    *is_first = false;
                    *eid += 1;
                }
                ContentItem::Container { children, tag, .. } => {
                    if tag == "li" && item.has_nested_containers() {
                        // Complex list: flatten
                        fn collect_flattened(
                            item: &ContentItem,
                            eid: &mut i64,
                            chunks: &mut Vec<ContentChunk>,
                            is_first: &mut bool,
                        ) {
                            match item {
                                ContentItem::Text { text, is_verse, .. } => {
                                    if *is_verse {
                                        for line in text.split('\n').filter(|s| !s.trim().is_empty())
                                        {
                                            chunks.push(ContentChunk {
                                                eid: *eid,
                                                length: line.trim().chars().count(),
                                                is_section_start: *is_first,
                                            });
                                            *is_first = false;
                                            *eid += 1;
                                        }
                                    } else if !text.trim().is_empty() {
                                        chunks.push(ContentChunk {
                                            eid: *eid,
                                            length: text.chars().count(),
                                            is_section_start: *is_first,
                                        });
                                        *is_first = false;
                                        *eid += 1;
                                    }
                                }
                                ContentItem::Image { .. } | ContentItem::Svg { .. } => {
                                    chunks.push(ContentChunk {
                                        eid: *eid,
                                        length: 1,
                                        is_section_start: *is_first,
                                    });
                                    *is_first = false;
                                    *eid += 1;
                                }
                                ContentItem::Container { children, .. } => {
                                    for child in children {
                                        collect_flattened(child, eid, chunks, is_first);
                                    }
                                }
                            }
                        }
                        for child in children {
                            collect_flattened(child, eid, chunks, is_first);
                        }
                        // Container EID
                        chunks.push(ContentChunk {
                            eid: *eid,
                            length: 1,
                            is_section_start: false,
                        });
                        *eid += 1;
                    } else if tag == "li" {
                        // Simple list item
                        for child in children {
                            collect_chunks(child, eid, chunks, is_first);
                        }
                    } else {
                        // Regular container
                        for child in children {
                            collect_chunks(child, eid, chunks, is_first);
                        }
                        chunks.push(ContentChunk {
                            eid: *eid,
                            length: 1,
                            is_section_start: false,
                        });
                        *eid += 1;
                    }
                }
            }
        }

        for item in &chapter.content {
            collect_chunks(item, &mut content_eid, &mut chunks, &mut is_first);
        }

        let total_eids = super::content::count_content_eids(&chapter.content);
        eid_base += 1 + total_eids as i64;
    }

    // Generate page entries at POSITIONS_PER_PAGE intervals
    for chunk in &chunks {
        // Reset page boundary at section starts
        if chunk.is_section_start {
            next_page_pos = cumulative_pos;
        }

        let mut offset_in_chunk: usize = 0;
        while offset_in_chunk < chunk.length {
            let pos_in_book = cumulative_pos + offset_in_chunk;

            if pos_in_book >= next_page_pos {
                // Emit a page entry
                let nav_title = IonValue::OrderedStruct(vec![
                    (sym::TEXT, IonValue::String(page_num.to_string())),
                ]);

                let nav_target = IonValue::OrderedStruct(vec![
                    (sym::POSITION, IonValue::Int(chunk.eid)),
                    (sym::OFFSET, IonValue::Int(offset_in_chunk as i64)),
                ]);

                let nav_entry = IonValue::OrderedStruct(vec![
                    (sym::NAV_TITLE, nav_title),
                    (sym::NAV_TARGET, nav_target),
                ]);

                pages.push(IonValue::Annotated(
                    vec![sym::NAV_DEFINITION],
                    Box::new(nav_entry),
                ));

                page_num += 1;
                next_page_pos += POSITIONS_PER_PAGE;
            }

            // Move to next potential page boundary
            let remaining_in_chunk = chunk.length - offset_in_chunk;
            let remaining_to_next_page = next_page_pos.saturating_sub(pos_in_book);

            if remaining_in_chunk <= remaining_to_next_page {
                break;
            }
            offset_in_chunk += remaining_to_next_page;
        }

        cumulative_pos += chunk.length;
    }

    pages
}
