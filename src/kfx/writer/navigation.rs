//! Navigation structures for KFX (TOC, landmarks, anchors).

use std::collections::HashMap;

use crate::book::TocEntry;
use crate::kfx::ion::IonValue;

use super::symbols::{SymbolTable, sym};

/// Build the book navigation fragment ($389)
///
/// Creates a complete TOC navigation structure with:
/// - Reading order reference
/// - Nav container with type=toc ($212)
/// - Nav container with type=landmarks ($236)
pub fn build_book_navigation(
    toc: &[TocEntry],
    section_eids: &HashMap<String, i64>,
    anchor_eids: &HashMap<String, (i64, i64)>,
    symtab: &mut SymbolTable,
    has_cover: bool,
    first_content_eid: Option<i64>,
) -> IonValue {
    let mut nav_containers = Vec::new();

    // === TOC Nav Container ===
    let nav_toc_id = "nav-toc";
    let nav_toc_sym = symtab.get_or_intern(nav_toc_id);

    let nav_entry_values = build_nav_entries_recursive(toc, section_eids, anchor_eids);

    let mut toc_container = HashMap::new();
    toc_container.insert(sym::NAV_TYPE, IonValue::Symbol(sym::TOC));
    toc_container.insert(sym::NAV_ID, IonValue::Symbol(nav_toc_sym));
    toc_container.insert(sym::NAV_ENTRIES, IonValue::List(nav_entry_values));

    nav_containers.push(IonValue::Annotated(
        vec![sym::NAV_CONTAINER_TYPE],
        Box::new(IonValue::Struct(toc_container)),
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

    let mut landmarks_container = HashMap::new();
    landmarks_container.insert(sym::NAV_TYPE, IonValue::Symbol(sym::LANDMARKS_NAV_TYPE));
    landmarks_container.insert(sym::NAV_ID, IonValue::Symbol(nav_landmarks_sym));
    landmarks_container.insert(sym::NAV_ENTRIES, IonValue::List(landmark_entries));

    nav_containers.push(IonValue::Annotated(
        vec![sym::NAV_CONTAINER_TYPE],
        Box::new(IonValue::Struct(landmarks_container)),
    ));

    // Book navigation root
    let mut nav = HashMap::new();
    nav.insert(
        sym::READING_ORDER_NAME,
        IonValue::Symbol(sym::DEFAULT_READING_ORDER),
    );
    nav.insert(sym::NAV_CONTAINER_REF, IonValue::List(nav_containers));

    IonValue::List(vec![IonValue::Struct(nav)])
}

/// Build a landmark navigation entry
fn build_landmark_entry(title: &str, eid: i64, landmark_type: Option<u64>) -> IonValue {
    let mut nav_title = HashMap::new();
    nav_title.insert(sym::TEXT, IonValue::String(title.to_string()));

    // Nav target: { $155: eid, $143: 0 }
    let nav_target = IonValue::OrderedStruct(vec![
        (sym::POSITION, IonValue::Int(eid)),
        (sym::OFFSET, IonValue::Int(0)),
    ]);

    let mut nav_entry = HashMap::new();
    nav_entry.insert(sym::NAV_TITLE, IonValue::Struct(nav_title));
    nav_entry.insert(sym::NAV_TARGET, nav_target);

    if let Some(lt) = landmark_type {
        nav_entry.insert(sym::LANDMARK_TYPE, IonValue::Symbol(lt));
    }

    IonValue::Annotated(
        vec![sym::NAV_DEFINITION],
        Box::new(IonValue::Struct(nav_entry)),
    )
}

/// Recursively build nav entries, preserving TOC hierarchy via nested $247 entries
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
            let mut nav_title = HashMap::new();
            nav_title.insert(sym::TEXT, IonValue::String(entry.title.clone()));

            let nav_target = IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(eid)),
                (sym::OFFSET, IonValue::Int(offset)),
            ]);

            let mut nav_entry = HashMap::new();
            nav_entry.insert(sym::NAV_TITLE, IonValue::Struct(nav_title));
            nav_entry.insert(sym::NAV_TARGET, nav_target);

            // Recursively build children
            if !entry.children.is_empty() {
                let nested_entries =
                    build_nav_entries_recursive(&entry.children, section_eids, anchor_eids);
                if !nested_entries.is_empty() {
                    nav_entry.insert(sym::NAV_ENTRIES, IonValue::List(nested_entries));
                }
            }

            nav_entries.push(IonValue::Annotated(
                vec![sym::NAV_DEFINITION],
                Box::new(IonValue::Struct(nav_entry)),
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
    let mut nav_unit_list = HashMap::new();
    nav_unit_list.insert(sym::NAV_ENTRIES, IonValue::List(Vec::new()));
    IonValue::Struct(nav_unit_list)
}

/// Build anchor symbols mapping for URLs found in content
pub fn build_anchor_symbols(
    chapters: &[crate::kfx::writer::content::ChapterData],
    symtab: &mut SymbolTable,
) -> HashMap<String, u64> {
    use crate::kfx::writer::content::ContentItem;
    use std::collections::HashSet;

    fn collect_anchor_hrefs(item: &ContentItem, hrefs: &mut HashSet<String>) {
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

    let mut unique_hrefs: HashSet<String> = HashSet::new();
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
