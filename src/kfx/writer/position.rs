//! Position and EID calculation for KFX content.
//!
//! This module handles:
//! - EID (Entity ID) calculation and assignment
//! - Position maps ($264, $265, $550)
//! - Page templates ($266)
//! - Section and anchor EID mapping

use std::collections::HashMap;

use crate::kfx::ion::IonValue;

use super::content::{ChapterData, ContentItem, count_content_eids};
use super::symbols::{SymbolTable, sym};

/// Build section EID mapping for internal link targets
/// Maps XHTML paths to their FIRST CONTENT ITEM EID (not section EID)
pub fn build_section_eids(chapters: &[ChapterData], has_cover: bool) -> HashMap<String, i64> {
    let mut section_eids = HashMap::new();
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

    if has_cover {
        eid_base += 2;
    }

    for chapter in chapters {
        section_eids.insert(chapter.source_path.clone(), eid_base + 1);
        let total_items = count_content_eids(&chapter.content);
        eid_base += 1 + total_items as i64;
    }

    section_eids
}

/// Build anchor EID mapping for TOC navigation with fragment IDs
/// Maps "source_path#element_id" → (EID, offset)
pub fn build_anchor_eids(chapters: &[ChapterData], has_cover: bool) -> HashMap<String, (i64, i64)> {
    let mut anchor_eids = HashMap::new();
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

    if has_cover {
        eid_base += 2;
    }

    for chapter in chapters {
        let mut content_eid = eid_base + 1;

        fn collect_anchor_eids_recursive(
            item: &ContentItem,
            content_eid: &mut i64,
            source_path: &str,
            anchor_eids: &mut HashMap<String, (i64, i64)>,
        ) {
            match item {
                ContentItem::Text {
                    element_id,
                    inline_runs,
                    ..
                } => {
                    if let Some(id) = element_id {
                        let key = format!("{}#{}", source_path, id);
                        anchor_eids.insert(key, (*content_eid, 0));
                    }
                    for run in inline_runs {
                        if let Some(ref id) = run.element_id {
                            let key = format!("{}#{}", source_path, id);
                            anchor_eids
                                .entry(key)
                                .or_insert((*content_eid, run.offset as i64));
                        }
                    }
                    *content_eid += 1;
                }
                ContentItem::Image { .. } | ContentItem::Svg { .. } => {
                    *content_eid += 1;
                }
                ContentItem::Container {
                    children,
                    element_id,
                    tag,
                    ..
                } => {
                    // Handle list items specially - complex list items flatten nested containers
                    if tag == "li" && item.has_nested_containers() {
                        // Complex list item: nested containers are flattened (no EIDs)
                        // Only the outer list item and leaf text/image items get EIDs
                        let flattened_count: usize =
                            children.iter().map(|c| c.count_flattened_items()).sum();
                        *content_eid += flattened_count as i64;
                        // List item container gets its EID after all flattened children
                        let container_eid = *content_eid;
                        *content_eid += 1;
                        if let Some(id) = element_id {
                            let key = format!("{}#{}", source_path, id);
                            anchor_eids.insert(key, (container_eid, 0));
                        }
                    } else {
                        // Normal container: process children recursively, then assign EID
                        for child in children {
                            collect_anchor_eids_recursive(
                                child,
                                content_eid,
                                source_path,
                                anchor_eids,
                            );
                        }
                        let container_eid = *content_eid;
                        *content_eid += 1;
                        if let Some(id) = element_id {
                            let key = format!("{}#{}", source_path, id);
                            anchor_eids.insert(key, (container_eid, 0));
                        }
                    }
                }
            }
        }

        for content_item in &chapter.content {
            collect_anchor_eids_recursive(
                content_item,
                &mut content_eid,
                &chapter.source_path,
                &mut anchor_eids,
            );
        }

        let total_items = count_content_eids(&chapter.content);
        eid_base += 1 + total_items as i64;
    }

    anchor_eids
}

/// Build position map fragment ($264)
/// Maps each section to the list of EIDs it contains
pub fn build_position_map(
    chapters: &[ChapterData],
    symtab: &mut SymbolTable,
    has_cover: bool,
) -> IonValue {
    let mut entries = Vec::new();
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

    // Add cover section entry if present
    if has_cover {
        let cover_section_sym = symtab.get_or_intern("cover-section");
        // Cover has 2 EIDs: section (eid_base) and content item (eid_base + 1)
        let cover_eids = vec![
            IonValue::Int(eid_base),
            IonValue::Int(eid_base + 1),
        ];
        let mut cover_entry = HashMap::new();
        cover_entry.insert(sym::ENTITY_ID_LIST, IonValue::List(cover_eids));
        cover_entry.insert(sym::SECTION_NAME, IonValue::Symbol(cover_section_sym));
        entries.push(IonValue::Struct(cover_entry));
        eid_base += 2;
    }

    for chapter in chapters {
        let section_id = format!("section-{}", chapter.id);
        let section_sym = symtab.get_or_intern(&section_id);

        let total_items = count_content_eids(&chapter.content);
        let mut eids = Vec::new();
        eids.push(IonValue::Int(eid_base));
        for i in 0..total_items {
            eids.push(IonValue::Int(eid_base + 1 + i as i64));
        }

        let mut entry = HashMap::new();
        entry.insert(sym::ENTITY_ID_LIST, IonValue::List(eids));
        entry.insert(sym::SECTION_NAME, IonValue::Symbol(section_sym));
        entries.push(IonValue::Struct(entry));

        eid_base += 1 + total_items as i64;
    }

    IonValue::List(entries)
}

/// Build position ID map fragment ($265)
/// Maps character offsets to EIDs
///
/// The position ID map tracks character positions in the book and which EID
/// contains each position. This is used for reading progress tracking.
/// Only TEXT content items are included - containers are skipped since they
/// don't contribute readable content.
pub fn build_position_id_map(chapters: &[ChapterData], has_cover: bool) -> IonValue {
    let mut entries = Vec::new();
    let mut char_offset = 0i64;
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

    /// Add an entry to the position ID map
    fn add_entry(entries: &mut Vec<IonValue>, char_offset: i64, eid: i64) {
        let mut entry = HashMap::new();
        entry.insert(sym::EID_INDEX, IonValue::Int(char_offset));
        entry.insert(sym::EID_VALUE, IonValue::Int(eid));
        entries.push(IonValue::Struct(entry));
    }

    // Add cover section entries if present
    // Cover gets 2 EIDs: section (eid_base) and image (eid_base + 1)
    // Both get entries in position_id_map for consistency
    if has_cover {
        add_entry(&mut entries, char_offset, eid_base); // Cover section
        char_offset += 1;
        add_entry(&mut entries, char_offset, eid_base + 1); // Cover image
        char_offset += 1;
        eid_base += 2;
    }

    /// Process items and add text entries to position ID map.
    /// Also advances eid counter for all items (including containers).
    fn process_items_recursive(
        item: &ContentItem,
        eid: &mut i64,
        char_offset: &mut i64,
        entries: &mut Vec<IonValue>,
    ) {
        match item {
            ContentItem::Text { text, is_verse, .. } => {
                // Match normalize_text_for_kfx behavior
                if *is_verse {
                    for line in text.split('\n').filter(|s| !s.trim().is_empty()) {
                        add_entry(entries, *char_offset, *eid);
                        *char_offset += line.trim().len() as i64;
                        *eid += 1;
                    }
                } else if !text.trim().is_empty() {
                    add_entry(entries, *char_offset, *eid);
                    *char_offset += text.len() as i64;
                    *eid += 1;
                }
                // Empty text: no entry, no EID
            }
            ContentItem::Image { .. } | ContentItem::Svg { .. } => {
                // Images/SVGs advance position but get entry in map
                add_entry(entries, *char_offset, *eid);
                *char_offset += 1;
                *eid += 1;
            }
            ContentItem::Container {
                children, tag, ..
            } => {
                if tag == "li" && item.has_nested_containers() {
                    // Complex list item: flattened children + container
                    fn process_flattened(
                        item: &ContentItem,
                        eid: &mut i64,
                        char_offset: &mut i64,
                        entries: &mut Vec<IonValue>,
                    ) {
                        match item {
                            ContentItem::Text { text, is_verse, .. } => {
                                if *is_verse {
                                    for line in text.split('\n').filter(|s| !s.trim().is_empty()) {
                                        add_entry(entries, *char_offset, *eid);
                                        *char_offset += line.trim().len() as i64;
                                        *eid += 1;
                                    }
                                } else if !text.trim().is_empty() {
                                    add_entry(entries, *char_offset, *eid);
                                    *char_offset += text.len() as i64;
                                    *eid += 1;
                                }
                            }
                            ContentItem::Image { .. } | ContentItem::Svg { .. } => {
                                add_entry(entries, *char_offset, *eid);
                                *char_offset += 1;
                                *eid += 1;
                            }
                            ContentItem::Container { children, .. } => {
                                // Nested container is flattened - no EID for it
                                for child in children {
                                    process_flattened(child, eid, char_offset, entries);
                                }
                            }
                        }
                    }

                    for child in children {
                        process_flattened(child, eid, char_offset, entries);
                    }
                    // Complex list item container gets EID but NO entry in position map
                    // (containers don't contribute readable content)
                    *char_offset += 1;
                    *eid += 1;
                } else if tag == "li" {
                    // Simple list item: children get EIDs, no container EID
                    for child in children {
                        process_items_recursive(child, eid, char_offset, entries);
                    }
                } else {
                    // Regular container: children then container
                    for child in children {
                        process_items_recursive(child, eid, char_offset, entries);
                    }
                    // Container gets EID but NO entry in position ID map
                    *char_offset += 1;
                    *eid += 1;
                }
            }
        }
    }

    for chapter in chapters {
        let section_eid = eid_base;
        add_entry(&mut entries, char_offset, section_eid);
        char_offset += 1;

        let mut content_eid = eid_base + 1;
        for content_item in &chapter.content {
            process_items_recursive(
                content_item,
                &mut content_eid,
                &mut char_offset,
                &mut entries,
            );
        }

        let total_items = count_content_eids(&chapter.content);
        eid_base += 1 + total_items as i64;
    }

    // End marker
    add_entry(&mut entries, char_offset, 0);

    IonValue::List(entries)
}

/// Build location map fragment ($550)
///
/// The location map provides "locations" (similar to page numbers) throughout the book.
/// Each location represents approximately 110 characters of content.
///
/// Key behavior from kfxinput:
/// - Location boundaries reset at each section start
/// - Entries are created at 110-character intervals
/// - Each entry has an EID and offset within that EID
pub fn build_location_map(chapters: &[ChapterData], has_cover: bool) -> IonValue {
    const CHARS_PER_LOCATION: usize = 110;

    /// A content chunk with position info for location mapping
    struct ContentChunk {
        eid: i64,
        length: usize,
        is_section_start: bool,
    }

    let mut chunks: Vec<ContentChunk> = Vec::new();
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

    // Add cover as first chunk if present
    // Cover image counts as 1 position (like images in content)
    if has_cover {
        chunks.push(ContentChunk {
            eid: eid_base + 1, // Cover image EID (section is eid_base, image is eid_base+1)
            length: 1,
            is_section_start: true,
        });
        eid_base += 2;
    }

    for chapter in chapters {
        let mut content_eid = eid_base + 1;
        let mut is_first_in_section = true;

        fn collect_chunks_recursive(
            item: &ContentItem,
            content_eid: &mut i64,
            chunks: &mut Vec<ContentChunk>,
            is_first: &mut bool,
        ) {
            match item {
                ContentItem::Text { text, is_verse, .. } => {
                    // Match normalize_text_for_kfx behavior for verse text
                    if *is_verse {
                        for line in text.split('\n').filter(|s| !s.trim().is_empty()) {
                            let length = line.trim().len();
                            chunks.push(ContentChunk {
                                eid: *content_eid,
                                length,
                                is_section_start: *is_first,
                            });
                            *is_first = false;
                            *content_eid += 1;
                        }
                    } else if !text.trim().is_empty() {
                        let length = text.len();
                        chunks.push(ContentChunk {
                            eid: *content_eid,
                            length,
                            is_section_start: *is_first,
                        });
                        *is_first = false;
                        *content_eid += 1;
                    }
                    // Empty text: no EID, no chunk
                }
                ContentItem::Image { .. } | ContentItem::Svg { .. } => {
                    // Images/SVGs count as 1 character position
                    chunks.push(ContentChunk {
                        eid: *content_eid,
                        length: 1,
                        is_section_start: *is_first,
                    });
                    *is_first = false;
                    *content_eid += 1;
                }
                ContentItem::Container { children, tag, .. } => {
                    if tag == "li" && item.has_nested_containers() {
                        // Complex list item: flattened children + container
                        fn collect_flattened(
                            item: &ContentItem,
                            content_eid: &mut i64,
                            chunks: &mut Vec<ContentChunk>,
                            is_first: &mut bool,
                        ) {
                            match item {
                                ContentItem::Text { text, is_verse, .. } => {
                                    if *is_verse {
                                        for line in
                                            text.split('\n').filter(|s| !s.trim().is_empty())
                                        {
                                            let length = line.trim().len();
                                            chunks.push(ContentChunk {
                                                eid: *content_eid,
                                                length,
                                                is_section_start: *is_first,
                                            });
                                            *is_first = false;
                                            *content_eid += 1;
                                        }
                                    } else if !text.trim().is_empty() {
                                        let length = text.len();
                                        chunks.push(ContentChunk {
                                            eid: *content_eid,
                                            length,
                                            is_section_start: *is_first,
                                        });
                                        *is_first = false;
                                        *content_eid += 1;
                                    }
                                }
                                ContentItem::Image { .. } | ContentItem::Svg { .. } => {
                                    chunks.push(ContentChunk {
                                        eid: *content_eid,
                                        length: 1,
                                        is_section_start: *is_first,
                                    });
                                    *is_first = false;
                                    *content_eid += 1;
                                }
                                ContentItem::Container { children, .. } => {
                                    // Recursively flatten - container doesn't get EID
                                    for child in children {
                                        collect_flattened(child, content_eid, chunks, is_first);
                                    }
                                }
                            }
                        }

                        for child in children {
                            collect_flattened(child, content_eid, chunks, is_first);
                        }
                        // Complex list item container gets EID after children
                        chunks.push(ContentChunk {
                            eid: *content_eid,
                            length: 1,
                            is_section_start: false, // Container is never first
                        });
                        *content_eid += 1;
                    } else if tag == "li" {
                        // Simple list item: children get EIDs, no container EID
                        for child in children {
                            collect_chunks_recursive(child, content_eid, chunks, is_first);
                        }
                    } else {
                        // Regular container: children then container
                        for child in children {
                            collect_chunks_recursive(child, content_eid, chunks, is_first);
                        }
                        // Container gets EID but minimal length for position purposes
                        chunks.push(ContentChunk {
                            eid: *content_eid,
                            length: 1,
                            is_section_start: false, // Container is never first
                        });
                        *content_eid += 1;
                    }
                }
            }
        }

        for content_item in &chapter.content {
            collect_chunks_recursive(
                content_item,
                &mut content_eid,
                &mut chunks,
                &mut is_first_in_section,
            );
        }

        let total_items = count_content_eids(&chapter.content);
        eid_base += 1 + total_items as i64;
    }

    // Generate location entries following kfxinput's algorithm:
    // - Track cumulative position (pid) across all chunks
    // - Reset next_loc_position at each section boundary
    // - Emit location entries at CHARS_PER_LOCATION intervals
    let mut location_entries = Vec::new();
    let mut pid: usize = 0; // Cumulative position
    let mut next_loc_position: usize = 0;
    let mut in_section = false;

    for chunk in &chunks {
        let mut eid_loc_offset: usize = 0;
        let mut loc_pid = pid;

        // Reset location boundary at section start (key insight from kfxinput)
        if chunk.is_section_start {
            next_loc_position = loc_pid;
            in_section = true;
        }

        // Process this chunk, potentially emitting multiple location entries
        loop {
            // Emit location entry if we're at a boundary
            if loc_pid == next_loc_position && in_section {
                location_entries.push(IonValue::OrderedStruct(vec![
                    (sym::POSITION, IonValue::Int(chunk.eid)),
                    (sym::OFFSET, IonValue::Int(eid_loc_offset as i64)),
                ]));
                next_loc_position += CHARS_PER_LOCATION;
            }

            let eid_remaining = chunk.length - eid_loc_offset;
            let loc_remaining = next_loc_position.saturating_sub(loc_pid);

            // If remaining content in this EID fits within current location span, move on
            if eid_remaining <= loc_remaining {
                break;
            }

            // Otherwise, advance within this EID to the next location boundary
            eid_loc_offset += loc_remaining;
            loc_pid = next_loc_position;
        }

        pid += chunk.length;
    }

    let mut wrapper = HashMap::new();
    wrapper.insert(sym::LOCATION_ENTRIES, IonValue::List(location_entries));

    IonValue::List(vec![IonValue::Struct(wrapper)])
}
