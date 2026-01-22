//! Position and EID calculation for KFX content.
//!
//! This module handles:
//! - EID (Entity ID) calculation and assignment
//! - Position maps ($264, $265, $550)
//! - Page templates ($266)
//! - Section and anchor EID mapping

use std::collections::HashMap;

use crate::kfx::ion::IonValue;

use super::content::{ChapterData, ContentItem, count_content_items};
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
        let total_items = count_content_items(&chapter.content);
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
                ContentItem::Image { .. } => {
                    *content_eid += 1;
                }
                ContentItem::Container {
                    children,
                    element_id,
                    ..
                } => {
                    for child in children {
                        collect_anchor_eids_recursive(child, content_eid, source_path, anchor_eids);
                    }
                    if let Some(id) = element_id {
                        let key = format!("{}#{}", source_path, id);
                        anchor_eids.insert(key, (*content_eid, 0));
                    }
                    *content_eid += 1;
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

        let total_items = count_content_items(&chapter.content);
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

    if has_cover {
        eid_base += 2;
    }

    for chapter in chapters {
        let section_id = format!("section-{}", chapter.id);
        let section_sym = symtab.get_or_intern(&section_id);

        let total_items = count_content_items(&chapter.content);
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
pub fn build_position_id_map(chapters: &[ChapterData], has_cover: bool) -> IonValue {
    let mut entries = Vec::new();
    let mut char_offset = 0i64;
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

    if has_cover {
        eid_base += 2;
    }

    fn add_entries_recursive(
        item: &ContentItem,
        eid: &mut i64,
        char_offset: &mut i64,
        entries: &mut Vec<IonValue>,
    ) {
        match item {
            ContentItem::Text { text, .. } => {
                let mut entry = HashMap::new();
                entry.insert(sym::EID_INDEX, IonValue::Int(*char_offset));
                entry.insert(sym::EID_VALUE, IonValue::Int(*eid));
                entries.push(IonValue::Struct(entry));
                *char_offset += text.len() as i64;
                *eid += 1;
            }
            ContentItem::Image { .. } => {
                let mut entry = HashMap::new();
                entry.insert(sym::EID_INDEX, IonValue::Int(*char_offset));
                entry.insert(sym::EID_VALUE, IonValue::Int(*eid));
                entries.push(IonValue::Struct(entry));
                *char_offset += 1;
                *eid += 1;
            }
            ContentItem::Container { children, .. } => {
                for child in children {
                    add_entries_recursive(child, eid, char_offset, entries);
                }
                let mut entry = HashMap::new();
                entry.insert(sym::EID_INDEX, IonValue::Int(*char_offset));
                entry.insert(sym::EID_VALUE, IonValue::Int(*eid));
                entries.push(IonValue::Struct(entry));
                *char_offset += 1;
                *eid += 1;
            }
        }
    }

    for chapter in chapters {
        let section_eid = eid_base;
        let mut section_entry = HashMap::new();
        section_entry.insert(sym::EID_INDEX, IonValue::Int(char_offset));
        section_entry.insert(sym::EID_VALUE, IonValue::Int(section_eid));
        entries.push(IonValue::Struct(section_entry));
        char_offset += 1;

        let mut content_eid = eid_base + 1;
        for content_item in &chapter.content {
            add_entries_recursive(
                content_item,
                &mut content_eid,
                &mut char_offset,
                &mut entries,
            );
        }

        let total_items = count_content_items(&chapter.content);
        eid_base += 1 + total_items as i64;
    }

    // End marker
    let mut end_entry = HashMap::new();
    end_entry.insert(sym::EID_INDEX, IonValue::Int(char_offset));
    end_entry.insert(sym::EID_VALUE, IonValue::Int(0));
    entries.push(IonValue::Struct(end_entry));

    IonValue::List(entries)
}

/// Build location map fragment ($550)
pub fn build_location_map(chapters: &[ChapterData], has_cover: bool) -> IonValue {
    const CHARS_PER_LOCATION: usize = 110;

    #[derive(Debug)]
    struct ContentRange {
        eid: i64,
        char_start: usize,
        char_end: usize,
    }

    let mut content_ranges: Vec<ContentRange> = Vec::new();
    let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

    if has_cover {
        eid_base += 2;
    }

    let mut total_chars: usize = 0;

    for chapter in chapters {
        let mut content_eid = eid_base + 1;

        fn collect_ranges_recursive(
            item: &ContentItem,
            content_eid: &mut i64,
            char_pos: &mut usize,
            ranges: &mut Vec<ContentRange>,
        ) {
            match item {
                ContentItem::Text { text, .. } => {
                    let start = *char_pos;
                    let end = start + text.len();
                    ranges.push(ContentRange {
                        eid: *content_eid,
                        char_start: start,
                        char_end: end,
                    });
                    *content_eid += 1;
                    *char_pos = end;
                }
                ContentItem::Image { .. } => {
                    ranges.push(ContentRange {
                        eid: *content_eid,
                        char_start: *char_pos,
                        char_end: *char_pos,
                    });
                    *content_eid += 1;
                }
                ContentItem::Container { children, .. } => {
                    for child in children {
                        collect_ranges_recursive(child, content_eid, char_pos, ranges);
                    }
                    ranges.push(ContentRange {
                        eid: *content_eid,
                        char_start: *char_pos,
                        char_end: *char_pos,
                    });
                    *content_eid += 1;
                }
            }
        }

        for content_item in &chapter.content {
            collect_ranges_recursive(
                content_item,
                &mut content_eid,
                &mut total_chars,
                &mut content_ranges,
            );
        }

        let total_items = count_content_items(&chapter.content);
        eid_base += 1 + total_items as i64;
    }

    let mut location_entries = Vec::new();
    let mut added_eids: std::collections::HashSet<i64> = std::collections::HashSet::new();

    // Add an entry for every content item at offset 0
    for range in &content_ranges {
        if !added_eids.contains(&range.eid) {
            location_entries.push(IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(range.eid)),
                (sym::OFFSET, IonValue::Int(0)),
            ]));
            added_eids.insert(range.eid);
        }
    }

    // Add additional entries at character position boundaries
    let num_locations = (total_chars / CHARS_PER_LOCATION).max(1);
    for loc_idx in 1..num_locations {
        let char_pos = loc_idx * CHARS_PER_LOCATION;

        let range = content_ranges
            .iter()
            .find(|r| char_pos >= r.char_start && char_pos < r.char_end)
            .or_else(|| content_ranges.last());

        if let Some(range) = range {
            let offset_within_item = if char_pos >= range.char_start {
                (char_pos - range.char_start) as i64
            } else {
                0
            };

            if offset_within_item > 0 {
                location_entries.push(IonValue::OrderedStruct(vec![
                    (sym::POSITION, IonValue::Int(range.eid)),
                    (sym::OFFSET, IonValue::Int(offset_within_item)),
                ]));
            }
        }
    }

    let mut wrapper = HashMap::new();
    wrapper.insert(sym::LOCATION_ENTRIES, IonValue::List(location_entries));

    IonValue::List(vec![IonValue::Struct(wrapper)])
}
