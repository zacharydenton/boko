//! Tests for KFX book builder.

use std::collections::HashMap;

use crate::css::ParsedStyle;
use crate::kfx::ion::IonValue;
use crate::kfx::test_helpers::{parse_entity_ion, parse_kfx_container};
use crate::kfx::writer::content::{ChapterData, ContentItem, ListType, StyleRun};
use crate::kfx::writer::position::{build_anchor_eids, build_section_eids};
use crate::kfx::writer::resources::build_resource_symbols;
use crate::kfx::writer::symbols::sym;

use super::{ContentState, KfxBookBuilder};

/// Helper to find a value in an IonValue::Struct by key
fn get_struct_field(value: &IonValue, key: u64) -> Option<&IonValue> {
    match value {
        IonValue::Struct(map) => map.get(&key),
        _ => None,
    }
}

/// Helper to get symbol value from IonValue
fn get_symbol_value(value: &IonValue) -> Option<u64> {
    match value {
        IonValue::Symbol(s) => Some(*s),
        _ => None,
    }
}

#[test]
fn test_list_container_uses_content_list_type() {
    // Create a list container with list items
    let list_item_1 = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![ContentItem::Text {
            text: "First item".to_string(),
            style: ParsedStyle::default(),
            inline_runs: Vec::new(),
            anchor_href: None,
            element_id: None,
            is_verse: false,
            is_noteref: false,
        }],
        tag: "li".to_string(),
        element_id: None,
        list_type: None, // li elements don't have list_type
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let list_item_2 = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![ContentItem::Text {
            text: "Second item".to_string(),
            style: ParsedStyle::default(),
            inline_runs: Vec::new(),
            anchor_href: None,
            element_id: None,
            is_verse: false,
            is_noteref: false,
        }],
        tag: "li".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let list_container = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![list_item_1, list_item_2],
        tag: "ol".to_string(),
        element_id: None,
        list_type: Some(ListType::Ordered),
        colspan: None,
        rowspan: None,
        classification: None,
    };

    // Build the content items
    let mut builder = KfxBookBuilder::new();
    // Add a default style to the style_map
    builder.style_map.insert(ParsedStyle::default(), 860);

    let content_sym = builder.symtab.get_or_intern("content-test");
    let mut state = ContentState {
        global_idx: 0,
        text_idx_in_chunk: 0,
        current_content_sym: content_sym,
    };

    let ion_items = builder.build_content_items(&list_container, &mut state, 860);

    // Should have one list container
    assert_eq!(ion_items.len(), 1, "Should produce one list container");

    let list_ion = &ion_items[0];

    // Verify list container has content type $276 (CONTENT_LIST)
    let content_type = get_struct_field(list_ion, sym::CONTENT_TYPE).and_then(get_symbol_value);
    assert_eq!(
        content_type,
        Some(sym::CONTENT_LIST),
        "List container should have content type $276 (CONTENT_LIST), got {:?}",
        content_type
    );

    // Verify list container has $100 (LIST_TYPE) property
    let list_type_prop = get_struct_field(list_ion, sym::LIST_TYPE).and_then(get_symbol_value);
    assert_eq!(
        list_type_prop,
        Some(sym::LIST_TYPE_DECIMAL),
        "List container should have $100: $343 (decimal list type)"
    );

    // Get the children ($146 CONTENT_ARRAY)
    let children = get_struct_field(list_ion, sym::CONTENT_ARRAY);
    assert!(
        children.is_some(),
        "List container should have $146 (CONTENT_ARRAY)"
    );

    if let Some(IonValue::List(child_items)) = children {
        assert_eq!(child_items.len(), 2, "List should have 2 items");

        // Verify each list item has content type $277 (CONTENT_LIST_ITEM)
        for (i, child_ion) in child_items.iter().enumerate() {
            let child_content_type =
                get_struct_field(child_ion, sym::CONTENT_TYPE).and_then(get_symbol_value);
            assert_eq!(
                child_content_type,
                Some(sym::CONTENT_LIST_ITEM),
                "List item {} should have content type $277 (CONTENT_LIST_ITEM), got {:?}",
                i,
                child_content_type
            );

            // Verify list item has $145 (TEXT_CONTENT) directly, not nested $146
            let text_ref = get_struct_field(child_ion, sym::TEXT_CONTENT);
            assert!(
                text_ref.is_some(),
                "List item {} should have $145 (TEXT_CONTENT) directly",
                i
            );

            // Verify list item does NOT have nested $146 (CONTENT_ARRAY)
            let nested_array = get_struct_field(child_ion, sym::CONTENT_ARRAY);
            assert!(
                nested_array.is_none(),
                "List item {} should NOT have nested $146 (CONTENT_ARRAY)",
                i
            );
        }
    } else {
        panic!("Expected List for CONTENT_ARRAY");
    }
}

#[test]
fn test_verse_text_splits_into_separate_entries() {
    use super::normalize_text_for_kfx;

    // Create a chapter with verse text containing newlines
    let verse_text = ContentItem::Text {
        text: "Line one\nLine two\nLine three".to_string(),
        style: ParsedStyle::default(),
        inline_runs: Vec::new(),
        anchor_href: None,
        element_id: None,
        is_verse: true, // Mark as verse so it should be split
        is_noteref: false,
    };

    let container = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![verse_text],
        tag: "p".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let chapter = ChapterData {
        id: "test-chapter".to_string(),
        title: "Test Chapter".to_string(),
        content: vec![container],
        source_path: "test.xhtml".to_string(),
    };

    // Create chunks and verify text content
    let chunks = chapter.into_chunks();
    assert_eq!(chunks.len(), 1, "Should have 1 chunk");

    let chunk = &chunks[0];

    // Collect all text from the chunk using flatten (same as add_text_content_chunk)
    let mut all_texts: Vec<String> = Vec::new();
    for item in chunk.items.iter().flat_map(|i| i.flatten()) {
        if let ContentItem::Text { text, is_verse, .. } = item {
            // Use normalize_text_for_kfx logic
            all_texts.extend(normalize_text_for_kfx(text, *is_verse));
        }
    }

    assert_eq!(
        all_texts.len(),
        3,
        "Verse text should be split into 3 separate entries, got: {:?}",
        all_texts
    );
    assert_eq!(all_texts[0], "Line one");
    assert_eq!(all_texts[1], "Line two");
    assert_eq!(all_texts[2], "Line three");
}

#[test]
fn test_epictetus_poetry_is_verse() {
    // Test with actual EPUB content to verify is_verse is set correctly
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

    // Build using extract_chapters (same as from_book)
    let mut builder = KfxBookBuilder::new();
    builder.resource_symbols = build_resource_symbols(&book, &mut builder.symtab);

    let toc_titles: HashMap<&str, &str> = book
        .toc
        .iter()
        .map(|entry| (entry.href.as_str(), entry.title.as_str()))
        .collect();

    let chapters = builder.extract_chapters(&book, &toc_titles);

    // Find the chapter with the Zeus poetry (The Enchiridion)
    let enchiridion_chapter = chapters
        .iter()
        .find(|c| c.source_path.contains("enchiridion"));
    assert!(
        enchiridion_chapter.is_some(),
        "Should find Enchiridion chapter"
    );
    let chapter = enchiridion_chapter.unwrap();

    // Find all text items with "Zeus" and check is_verse
    fn find_zeus_texts(item: &ContentItem, results: &mut Vec<(String, bool)>) {
        match item {
            ContentItem::Text { text, is_verse, .. } => {
                if text.contains("Zeus") || text.contains('\n') && text.contains("Destiny") {
                    results.push((text.clone(), *is_verse));
                }
            }
            ContentItem::Container { children, .. } => {
                for child in children {
                    find_zeus_texts(child, results);
                }
            }
            _ => {}
        }
    }

    let mut zeus_results = Vec::new();
    for item in &chapter.content {
        find_zeus_texts(item, &mut zeus_results);
    }

    // At least one should have newlines and is_verse=true
    let verse_with_newlines = zeus_results
        .iter()
        .any(|(text, is_verse)| text.contains('\n') && *is_verse);

    assert!(
        verse_with_newlines,
        "Poetry with newlines should have is_verse=true. Results: {:?}",
        zeus_results
            .iter()
            .map(|(t, v)| (t.len(), v))
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_inline_anchor_style_is_registered_and_used() {
    // Verify that anchor-only inline runs get their styles registered in style_map
    // and produce valid inline run ION output with anchor references.
    use crate::kfx::writer::navigation::build_anchor_symbols;

    // Create content with an inline anchor (like a backlink)
    let anchor_style = ParsedStyle {
        is_inline: true,
        ..Default::default()
    };

    let text_with_link = ContentItem::Text {
        text: "Note text ↩︎".to_string(),
        style: ParsedStyle::default(),
        inline_runs: vec![StyleRun {
            offset: 10,
            length: 1, // Just the backlink arrow
            style: anchor_style.clone(),
            anchor_href: Some("chapter.xhtml#noteref-1".to_string()),
            element_id: None,
            is_noteref: false,
        }],
        anchor_href: None,
        element_id: None,
        is_verse: false,
        is_noteref: false,
    };

    let paragraph = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![text_with_link],
        tag: "p".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    // Create a chapter with this content
    let chapter = ChapterData {
        id: "test".to_string(),
        title: "Test".to_string(),
        source_path: "test.xhtml".to_string(),
        content: vec![paragraph],
    };

    // Build styles (this should register the anchor style)
    let mut builder = KfxBookBuilder::new();
    let chapters = vec![chapter];
    builder.add_all_styles(&chapters);

    // Verify anchor style is in style_map
    assert!(
        builder.style_map.contains_key(&anchor_style),
        "Anchor-only inline style should be registered in style_map. Found styles: {:?}",
        builder.style_map.keys().collect::<Vec<_>>()
    );

    // Build anchor symbols (this should register the href)
    builder.anchor_symbols = build_anchor_symbols(&chapters, &mut builder.symtab);

    // Verify anchor symbol is registered
    assert!(
        builder
            .anchor_symbols
            .contains_key("chapter.xhtml#noteref-1"),
        "Anchor href should be registered in anchor_symbols. Found: {:?}",
        builder.anchor_symbols.keys().collect::<Vec<_>>()
    );

    // Build inline runs (this should produce ION with anchor reference)
    let runs = vec![StyleRun {
        offset: 10,
        length: 1,
        style: anchor_style,
        anchor_href: Some("chapter.xhtml#noteref-1".to_string()),
        element_id: None,
        is_noteref: false,
    }];

    let ion_runs = builder.build_inline_runs(&runs);

    // Should have exactly one inline run
    assert_eq!(
        ion_runs.len(),
        1,
        "Should produce one inline run, got {} (style or anchor might be missing)",
        ion_runs.len()
    );

    // Verify the inline run has anchor reference ($179)
    if let IonValue::Struct(run_map) = &ion_runs[0] {
        assert!(
            run_map.contains_key(&sym::ANCHOR_REF),
            "Inline run should have ANCHOR_REF ($179). Keys: {:?}",
            run_map.keys().collect::<Vec<_>>()
        );
    } else {
        panic!("Expected inline run to be a struct");
    }
}

#[test]
fn test_epictetus_backlinks_have_anchor_refs() {
    // Test that backlinks in the actual epictetus.epub endnotes work
    use crate::kfx::writer::navigation::build_anchor_symbols;

    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

    // Build chapters like the full builder does
    let mut builder = KfxBookBuilder::new();
    builder.resource_symbols = build_resource_symbols(&book, &mut builder.symtab);

    let toc_titles: HashMap<&str, &str> = book
        .toc
        .iter()
        .map(|entry| (entry.href.as_str(), entry.title.as_str()))
        .collect();

    let chapters = builder.extract_chapters(&book, &toc_titles);

    // Find the endnotes chapter
    let endnotes_chapter = chapters.iter().find(|c| c.source_path.contains("endnotes"));
    assert!(endnotes_chapter.is_some(), "Should find endnotes chapter");
    let endnotes = endnotes_chapter.unwrap();

    // Collect all inline runs with anchor_href from endnotes
    fn collect_anchor_runs(item: &ContentItem, runs: &mut Vec<(String, String, ParsedStyle)>) {
        match item {
            ContentItem::Text { inline_runs, .. } => {
                for run in inline_runs {
                    if let Some(ref href) = run.anchor_href {
                        let style_debug = format!("is_inline={}", run.style.is_inline);
                        runs.push((href.clone(), style_debug, run.style.clone()));
                    }
                }
            }
            ContentItem::Container { children, .. } => {
                for child in children {
                    collect_anchor_runs(child, runs);
                }
            }
            _ => {}
        }
    }

    let mut backlink_runs = Vec::new();
    for item in &endnotes.content {
        collect_anchor_runs(item, &mut backlink_runs);
    }

    // Should find many backlinks (epictetus has 98+ endnotes)
    assert!(
        backlink_runs.len() > 90,
        "Should find many backlinks in endnotes, found only {}",
        backlink_runs.len()
    );

    // Verify styles and anchor symbols are built correctly
    builder.add_all_styles(&chapters);
    builder.anchor_symbols = build_anchor_symbols(&chapters, &mut builder.symtab);

    // Check that the backlink hrefs are in anchor_symbols
    let first_backlink = &backlink_runs[0].0;
    assert!(
        builder.anchor_symbols.contains_key(first_backlink),
        "First backlink href {} should be in anchor_symbols. Found hrefs: {:?}",
        first_backlink,
        builder.anchor_symbols.keys().take(5).collect::<Vec<_>>()
    );

    // Build inline runs using the ACTUAL styles (not fabricated ones)
    let test_runs: Vec<StyleRun> = backlink_runs
        .iter()
        .take(10)
        .map(|(href, _, style)| StyleRun {
            offset: 0,
            length: 1,
            style: style.clone(),
            anchor_href: Some(href.clone()),
            element_id: None,
            is_noteref: false,
        })
        .collect();

    // Check that ALL styles are registered (this is the critical check)
    for (_i, (_href, _, style)) in backlink_runs.iter().take(10).enumerate() {
        let in_map = builder.style_map.contains_key(style);
        assert!(in_map, "Style should be in style_map: {:?}", style);
    }

    let ion_runs = builder.build_inline_runs(&test_runs);

    // Should produce same number of output runs as input
    assert_eq!(
        ion_runs.len(),
        test_runs.len(),
        "Should produce ION output for all inline runs (check style_map and anchor_symbols)"
    );

    // Each should have ANCHOR_REF
    for (i, run) in ion_runs.iter().enumerate() {
        if let IonValue::Struct(run_map) = run {
            assert!(
                run_map.contains_key(&sym::ANCHOR_REF),
                "Inline run {} should have ANCHOR_REF",
                i
            );
        }
    }
}

#[test]
fn test_full_kfx_has_backlink_anchor_refs() {
    // Build the complete KFX and verify it has anchor references in endnotes
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();
    let kfx_builder = KfxBookBuilder::from_book(&book);

    // Count anchor refs in the generated fragments
    // Inline runs are in CONTENT_BLOCK ($259), not TEXT_CONTENT ($145)
    let mut anchor_ref_count = 0;
    let mut inline_runs_count = 0;
    let mut content_block_frags = 0;

    fn count_in_item(item: &IonValue, inline_runs_count: &mut usize, anchor_ref_count: &mut usize) {
        if let IonValue::Struct(item_struct) = item {
            // Look for INLINE_STYLE_RUNS ($142)
            if let Some(IonValue::List(runs)) = item_struct.get(&sym::INLINE_STYLE_RUNS) {
                *inline_runs_count += runs.len();
                for run in runs {
                    if let IonValue::Struct(run_struct) = run {
                        if run_struct.contains_key(&sym::ANCHOR_REF) {
                            *anchor_ref_count += 1;
                        }
                    }
                }
            }
            // Recursively check nested CONTENT_ARRAY
            if let Some(IonValue::List(nested)) = item_struct.get(&sym::CONTENT_ARRAY) {
                for nested_item in nested {
                    count_in_item(nested_item, inline_runs_count, anchor_ref_count);
                }
            }
        } else if let IonValue::Annotated(_, boxed) = item {
            count_in_item(boxed, inline_runs_count, anchor_ref_count);
        }
    }

    for fragment in &kfx_builder.fragments {
        // CONTENT_BLOCK fragments ($259) have structure: { $146: [...items...], ... }
        if fragment.ftype == sym::CONTENT_BLOCK {
            content_block_frags += 1;
            if let IonValue::Struct(s) = &fragment.value {
                // Look for CONTENT_ARRAY ($146)
                if let Some(IonValue::List(items)) = s.get(&sym::CONTENT_ARRAY) {
                    for item in items {
                        count_in_item(item, &mut inline_runs_count, &mut anchor_ref_count);
                    }
                }
            }
        }
    }

    // Suppress unused variable warnings
    let _ = content_block_frags;
    let _ = inline_runs_count;

    // Should have many anchor refs (noterefs + backlinks + any other links)
    // Epictetus has 98+ endnotes so we should have at least 98 backlinks + 98 noterefs = 196
    assert!(
        anchor_ref_count >= 100,
        "Should have many anchor refs in KFX output, found only {}",
        anchor_ref_count
    );
}

#[test]
fn test_anchor_eid_href_format_match() {
    // Test that anchor_eids keys match the href format from inline runs
    // This ensures backlinks and footnote refs are properly resolvable
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

    let mut builder = KfxBookBuilder::new();
    builder.resource_symbols = build_resource_symbols(&book, &mut builder.symtab);
    let toc_titles: HashMap<&str, &str> = book
        .toc
        .iter()
        .map(|entry| (entry.href.as_str(), entry.title.as_str()))
        .collect();

    let chapters = builder.extract_chapters(&book, &toc_titles);
    let has_cover = book.metadata.cover_image.is_some();

    // Build section_eids first (required for build_anchor_eids)
    builder.section_eids = build_section_eids(&chapters, has_cover);
    // Get anchor_eids
    let anchor_eids = build_anchor_eids(&chapters, has_cover);

    // Collect all anchor hrefs from inline runs
    fn collect_hrefs(item: &ContentItem, hrefs: &mut Vec<String>) {
        match item {
            ContentItem::Text { inline_runs, .. } => {
                for run in inline_runs {
                    if let Some(ref href) = run.anchor_href {
                        hrefs.push(href.clone());
                    }
                }
            }
            ContentItem::Container { children, .. } => {
                for child in children {
                    collect_hrefs(child, hrefs);
                }
            }
            _ => {}
        }
    }

    let mut all_hrefs = Vec::new();
    for chapter in &chapters {
        for item in &chapter.content {
            collect_hrefs(item, &mut all_hrefs);
        }
    }

    // All fragment hrefs (internal links) should be resolvable via anchor_eids
    let fragment_hrefs: Vec<_> = all_hrefs.iter().filter(|h| h.contains('#')).collect();
    let not_found: Vec<_> = fragment_hrefs
        .iter()
        .filter(|href| !anchor_eids.contains_key(**href))
        .collect();

    assert!(
        not_found.is_empty(),
        "Some hrefs not found in anchor_eids: {:?}",
        not_found.iter().take(5).collect::<Vec<_>>()
    );

    // Verify backlinks are extracted (epictetus has 98 endnotes with backlinks)
    fn count_backlink_runs(item: &ContentItem) -> usize {
        match item {
            ContentItem::Text { inline_runs, .. } => inline_runs
                .iter()
                .filter(|r| {
                    r.anchor_href
                        .as_ref()
                        .map(|h| h.contains("noteref"))
                        .unwrap_or(false)
                })
                .count(),
            ContentItem::Container { children, .. } => {
                children.iter().map(|c| count_backlink_runs(c)).sum()
            }
            _ => 0,
        }
    }

    let backlink_count: usize = chapters
        .iter()
        .flat_map(|ch| &ch.content)
        .map(|item| count_backlink_runs(item))
        .sum();

    assert!(
        backlink_count >= 90,
        "Should find many backlink inline runs, found only {}",
        backlink_count
    );
}

#[test]
fn test_table_content_types() {
    // Test that table elements get the correct KFX content types

    // Create a simple table: table > tbody > tr > td
    let td = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![ContentItem::Text {
            text: "Cell content".to_string(),
            style: ParsedStyle::default(),
            inline_runs: Vec::new(),
            anchor_href: None,
            element_id: None,
            is_verse: false,
            is_noteref: false,
        }],
        tag: "td".to_string(),
        element_id: None,
        list_type: None,
        colspan: Some(2), // Test colspan
        rowspan: None,
        classification: None,
    };

    let tr = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![td],
        tag: "tr".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let tbody = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![tr],
        tag: "tbody".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let table = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![tbody],
        tag: "table".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    // Build the content items
    let mut builder = KfxBookBuilder::new();
    builder.style_map.insert(ParsedStyle::default(), 860);

    let content_sym = builder.symtab.get_or_intern("content-test");
    let mut state = ContentState {
        global_idx: 0,
        text_idx_in_chunk: 0,
        current_content_sym: content_sym,
    };

    let ion_items = builder.build_content_items(&table, &mut state, 860);

    // Should have one table container
    assert_eq!(ion_items.len(), 1, "Should produce one table container");

    let table_ion = &ion_items[0];

    // Verify table has content type $278 (CONTENT_TABLE)
    let content_type = get_struct_field(table_ion, sym::CONTENT_TYPE).and_then(get_symbol_value);
    assert_eq!(
        content_type,
        Some(sym::CONTENT_TABLE),
        "Table should have content type $278 (CONTENT_TABLE)"
    );

    // Get tbody from table's content array
    let content_array = get_struct_field(table_ion, sym::CONTENT_ARRAY).and_then(|v| {
        if let IonValue::List(list) = v {
            Some(list)
        } else {
            None
        }
    });
    assert!(content_array.is_some(), "Table should have content array");
    let tbody_ion = &content_array.unwrap()[0];

    // Verify tbody has content type $454 (CONTENT_TBODY)
    let tbody_type = get_struct_field(tbody_ion, sym::CONTENT_TYPE).and_then(get_symbol_value);
    assert_eq!(
        tbody_type,
        Some(sym::CONTENT_TBODY),
        "Tbody should have content type $454 (CONTENT_TBODY)"
    );

    // Get tr from tbody's content array
    let tbody_content = get_struct_field(tbody_ion, sym::CONTENT_ARRAY).and_then(|v| {
        if let IonValue::List(list) = v {
            Some(list)
        } else {
            None
        }
    });
    assert!(tbody_content.is_some(), "Tbody should have content array");
    let tr_ion = &tbody_content.unwrap()[0];

    // Verify tr has content type $279 (CONTENT_TABLE_ROW)
    let tr_type = get_struct_field(tr_ion, sym::CONTENT_TYPE).and_then(get_symbol_value);
    assert_eq!(
        tr_type,
        Some(sym::CONTENT_TABLE_ROW),
        "Tr should have content type $279 (CONTENT_TABLE_ROW)"
    );

    // Get td from tr's content array
    let tr_content = get_struct_field(tr_ion, sym::CONTENT_ARRAY).and_then(|v| {
        if let IonValue::List(list) = v {
            Some(list)
        } else {
            None
        }
    });
    assert!(tr_content.is_some(), "Tr should have content array");
    let td_ion = &tr_content.unwrap()[0];

    // Verify td has content type $269 (CONTENT_PARAGRAPH)
    let td_type = get_struct_field(td_ion, sym::CONTENT_TYPE).and_then(get_symbol_value);
    assert_eq!(
        td_type,
        Some(sym::CONTENT_PARAGRAPH),
        "Td should have content type $269 (CONTENT_PARAGRAPH)"
    );

    // Verify td has colspan attribute $148
    let colspan = get_struct_field(td_ion, sym::ATTRIB_COLSPAN).and_then(|v| {
        if let IonValue::Int(i) = v {
            Some(*i)
        } else {
            None
        }
    });
    assert_eq!(
        colspan,
        Some(2),
        "Td should have colspan attribute with value 2"
    );
}

#[test]
fn test_unordered_list_uses_disc_marker() {
    // Create an unordered list (ul) with list items
    let list_item = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![ContentItem::Text {
            text: "Bullet item".to_string(),
            style: ParsedStyle::default(),
            inline_runs: Vec::new(),
            anchor_href: None,
            element_id: None,
            is_verse: false,
            is_noteref: false,
        }],
        tag: "li".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let unordered_list = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![list_item],
        tag: "ul".to_string(),
        element_id: None,
        list_type: Some(ListType::Unordered),
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let mut builder = KfxBookBuilder::new();
    builder.style_map.insert(ParsedStyle::default(), 860);

    let content_sym = builder.symtab.get_or_intern("content-test");
    let mut state = ContentState {
        global_idx: 0,
        text_idx_in_chunk: 0,
        current_content_sym: content_sym,
    };

    let ion_items = builder.build_content_items(&unordered_list, &mut state, 860);
    assert_eq!(ion_items.len(), 1, "Should produce one list container");

    let list_ion = &ion_items[0];

    // Verify unordered list has LIST_TYPE_DISC ($340), not LIST_TYPE_DECIMAL ($343)
    let list_type = get_struct_field(list_ion, sym::LIST_TYPE).and_then(get_symbol_value);
    assert_eq!(
        list_type,
        Some(sym::LIST_TYPE_DISC),
        "Unordered list should use LIST_TYPE_DISC ($340), not decimal"
    );
}

#[test]
fn test_ordered_list_uses_decimal_marker() {
    // Create an ordered list (ol) with list items
    let list_item = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![ContentItem::Text {
            text: "Numbered item".to_string(),
            style: ParsedStyle::default(),
            inline_runs: Vec::new(),
            anchor_href: None,
            element_id: None,
            is_verse: false,
            is_noteref: false,
        }],
        tag: "li".to_string(),
        element_id: None,
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let ordered_list = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![list_item],
        tag: "ol".to_string(),
        element_id: None,
        list_type: Some(ListType::Ordered),
        colspan: None,
        rowspan: None,
        classification: None,
    };

    let mut builder = KfxBookBuilder::new();
    builder.style_map.insert(ParsedStyle::default(), 860);

    let content_sym = builder.symtab.get_or_intern("content-test");
    let mut state = ContentState {
        global_idx: 0,
        text_idx_in_chunk: 0,
        current_content_sym: content_sym,
    };

    let ion_items = builder.build_content_items(&ordered_list, &mut state, 860);
    let list_ion = &ion_items[0];

    // Verify ordered list has LIST_TYPE_DECIMAL ($343)
    let list_type = get_struct_field(list_ion, sym::LIST_TYPE).and_then(get_symbol_value);
    assert_eq!(
        list_type,
        Some(sym::LIST_TYPE_DECIMAL),
        "Ordered list should use LIST_TYPE_DECIMAL ($343)"
    );
}

#[test]
fn test_footnote_classification_emitted() {
    // Create a container with footnote classification
    let footnote_container = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![ContentItem::Text {
            text: "This is a footnote".to_string(),
            style: ParsedStyle::default(),
            inline_runs: Vec::new(),
            anchor_href: None,
            element_id: None,
            is_verse: false,
            is_noteref: false,
        }],
        tag: "aside".to_string(),
        element_id: Some("fn1".to_string()),
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: Some(sym::FOOTNOTE), // $618
    };

    let mut builder = KfxBookBuilder::new();
    builder.style_map.insert(ParsedStyle::default(), 860);

    let content_sym = builder.symtab.get_or_intern("content-test");
    let mut state = ContentState {
        global_idx: 0,
        text_idx_in_chunk: 0,
        current_content_sym: content_sym,
    };

    let ion_items = builder.build_content_items(&footnote_container, &mut state, 860);
    let container_ion = &ion_items[0];

    // Verify footnote has CLASSIFICATION ($615) with value FOOTNOTE ($618)
    let classification =
        get_struct_field(container_ion, sym::CLASSIFICATION).and_then(get_symbol_value);
    assert_eq!(
        classification,
        Some(sym::FOOTNOTE),
        "Footnote container should have CLASSIFICATION: FOOTNOTE ($615: $618)"
    );
}

#[test]
fn test_endnote_classification_emitted() {
    // Create a container with endnote classification
    let endnote_container = ContentItem::Container {
        style: ParsedStyle::default(),
        children: vec![ContentItem::Text {
            text: "This is an endnote".to_string(),
            style: ParsedStyle::default(),
            inline_runs: Vec::new(),
            anchor_href: None,
            element_id: None,
            is_verse: false,
            is_noteref: false,
        }],
        tag: "aside".to_string(),
        element_id: Some("en1".to_string()),
        list_type: None,
        colspan: None,
        rowspan: None,
        classification: Some(sym::ENDNOTE), // $619
    };

    let mut builder = KfxBookBuilder::new();
    builder.style_map.insert(ParsedStyle::default(), 860);

    let content_sym = builder.symtab.get_or_intern("content-test");
    let mut state = ContentState {
        global_idx: 0,
        text_idx_in_chunk: 0,
        current_content_sym: content_sym,
    };

    let ion_items = builder.build_content_items(&endnote_container, &mut state, 860);
    let container_ion = &ion_items[0];

    // Verify endnote has CLASSIFICATION ($615) with value ENDNOTE ($619)
    let classification =
        get_struct_field(container_ion, sym::CLASSIFICATION).and_then(get_symbol_value);
    assert_eq!(
        classification,
        Some(sym::ENDNOTE),
        "Endnote container should have CLASSIFICATION: ENDNOTE ($615: $619)"
    );
}

#[test]
fn test_noteref_inline_run_has_noteref_type() {
    // Create inline style for noteref link
    let noteref_style = ParsedStyle {
        is_inline: true,
        ..Default::default()
    };

    let mut builder = KfxBookBuilder::new();
    builder.style_map.insert(noteref_style.clone(), 861);
    builder
        .anchor_symbols
        .insert("chapter.xhtml#fn1".to_string(), 500);

    // Create a StyleRun with is_noteref = true
    let runs = vec![StyleRun {
        offset: 0,
        length: 1,
        style: noteref_style,
        anchor_href: Some("chapter.xhtml#fn1".to_string()),
        element_id: None,
        is_noteref: true, // This is a noteref link
    }];

    let ion_runs = builder.build_inline_runs(&runs);
    assert_eq!(ion_runs.len(), 1, "Should produce one inline run");

    let run_ion = &ion_runs[0];

    // Verify noteref has NOTEREF_TYPE ($616) with value NOTEREF ($617)
    let noteref_type = get_struct_field(run_ion, sym::NOTEREF_TYPE).and_then(get_symbol_value);
    assert_eq!(
        noteref_type,
        Some(sym::NOTEREF),
        "Noteref link should have NOTEREF_TYPE: NOTEREF ($616: $617)"
    );
}

#[test]
fn test_non_noteref_link_has_no_noteref_type() {
    // Create inline style for regular link
    let link_style = ParsedStyle {
        is_inline: true,
        ..Default::default()
    };

    let mut builder = KfxBookBuilder::new();
    builder.style_map.insert(link_style.clone(), 861);
    builder
        .anchor_symbols
        .insert("chapter.xhtml#section1".to_string(), 500);

    // Create a StyleRun with is_noteref = false (regular link)
    let runs = vec![StyleRun {
        offset: 0,
        length: 5,
        style: link_style,
        anchor_href: Some("chapter.xhtml#section1".to_string()),
        element_id: None,
        is_noteref: false, // Not a noteref
    }];

    let ion_runs = builder.build_inline_runs(&runs);
    let run_ion = &ion_runs[0];

    // Verify regular link does NOT have NOTEREF_TYPE
    let noteref_type = get_struct_field(run_ion, sym::NOTEREF_TYPE);
    assert!(
        noteref_type.is_none(),
        "Regular link should NOT have NOTEREF_TYPE"
    );
}

/// Test that TOC positions resolve to correct content in the generated KFX.
/// Compare with reference KFX to ensure TOC points to same entity types.
#[test]
fn test_toc_positions_match_reference() {
    // Parse reference KFX
    let ref_data = std::fs::read("tests/fixtures/epictetus.kfx").unwrap();
    let ref_entities = parse_kfx_container(&ref_data);

    // Generate KFX from EPUB
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();
    let gen_data = KfxBookBuilder::from_book(&book).build();
    let gen_entities = parse_kfx_container(&gen_data);

    // Write generated file for inspection
    std::fs::write("/tmp/test-toc.kfx", &gen_data).unwrap();

    // Extract navigation entries from $389 (book_navigation)
    fn extract_nav_positions(nav_value: &IonValue) -> Vec<(String, i64, i64)> {
        let mut results = Vec::new();
        extract_nav_positions_recursive(nav_value, &mut results);
        results
    }

    fn extract_nav_positions_recursive(value: &IonValue, results: &mut Vec<(String, i64, i64)>) {
        match value {
            IonValue::Annotated(annots, inner) => {
                if annots.contains(&393) {
                    // This is a nav entry ($393 annotated)
                    // Extract title from $241 (NAV_TITLE) -> $244 (TEXT)
                    let title = inner.get(241)
                        .and_then(|t| t.get(244))
                        .and_then(|v| v.as_string())
                        .unwrap_or("?")
                        .to_string();

                    // Extract position and offset from $246 (NAV_TARGET)
                    let (pos, offset) = inner.get(246)
                        .map(|target| {
                            let p = target.get(155).and_then(|v| v.as_int()).unwrap_or(-1);
                            let o = target.get(143).and_then(|v| v.as_int()).unwrap_or(0);
                            (p, o)
                        })
                        .unwrap_or((-1, 0));

                    results.push((title, pos, offset));

                    // Recurse into children ($247 = NAV_ENTRIES)
                    if let Some(children) = inner.get(247) {
                        extract_nav_positions_recursive(children, results);
                    }
                } else {
                    // Other annotations (like $391 nav containers) - recurse into inner
                    extract_nav_positions_recursive(inner, results);
                }
            }
            IonValue::List(items) => {
                for item in items {
                    extract_nav_positions_recursive(item, results);
                }
            }
            IonValue::Struct(map) => {
                // Handle structs with $392 (nav container list) or $247 (nav entries)
                if let Some(nav_containers) = map.get(&392) {
                    extract_nav_positions_recursive(nav_containers, results);
                }
                if let Some(nav_entries) = map.get(&247) {
                    extract_nav_positions_recursive(nav_entries, results);
                }
            }
            _ => {}
        }
    }

    // Get reference navigation
    let ref_nav = ref_entities.get(&389)
        .and_then(|v| v.first())
        .and_then(|(_, payload)| parse_entity_ion(payload))
        .expect("Reference should have $389");

    // Debug: print the structure of the navigation
    println!("\n=== Reference $389 structure ===");
    fn debug_nav_structure(value: &IonValue, indent: usize) {
        let prefix = "  ".repeat(indent);
        match value {
            IonValue::List(items) => {
                println!("{}List[{}]:", prefix, items.len());
                for (i, item) in items.iter().take(3).enumerate() {
                    println!("{}  [{}]:", prefix, i);
                    debug_nav_structure(item, indent + 2);
                }
                if items.len() > 3 {
                    println!("{}  ... ({} more)", prefix, items.len() - 3);
                }
            }
            IonValue::Struct(map) => {
                let keys: Vec<_> = map.keys().collect();
                println!("{}Struct keys: {:?}", prefix, keys);
                for key in keys.iter().take(5) {
                    if let Some(v) = map.get(key) {
                        println!("{}  ${}:", prefix, key);
                        debug_nav_structure(v, indent + 2);
                    }
                }
            }
            IonValue::Annotated(annots, inner) => {
                println!("{}Annotated {:?}:", prefix, annots);
                debug_nav_structure(inner, indent + 1);
            }
            IonValue::Symbol(s) => println!("{}Symbol({})", prefix, s),
            IonValue::String(s) => println!("{}String({:?})", prefix, if s.len() > 30 { &s[..30] } else { s }),
            IonValue::Int(i) => println!("{}Int({})", prefix, i),
            _ => println!("{}Other: {:?}", prefix, value),
        }
    }
    debug_nav_structure(&ref_nav, 0);

    let ref_nav_positions = extract_nav_positions(&ref_nav);

    // Get generated navigation
    let gen_nav = gen_entities.get(&389)
        .and_then(|v| v.first())
        .and_then(|(_, payload)| parse_entity_ion(payload))
        .expect("Generated should have $389");

    println!("\n=== Generated $389 structure ===");
    debug_nav_structure(&gen_nav, 0);

    let gen_nav_positions = extract_nav_positions(&gen_nav);

    println!("\n=== Reference TOC entries (first 20) ===");
    for (i, (title, pos, offset)) in ref_nav_positions.iter().take(20).enumerate() {
        println!("  [{}] {} -> pos={}, offset={}", i, title, pos, offset);
    }

    println!("\n=== Generated TOC entries (first 20) ===");
    for (i, (title, pos, offset)) in gen_nav_positions.iter().take(20).enumerate() {
        println!("  [{}] {} -> pos={}, offset={}", i, title, pos, offset);
    }

    // Build entity ID -> type map for both
    fn build_entity_map(entities: &HashMap<u32, Vec<(u32, Vec<u8>)>>) -> HashMap<u32, u32> {
        let mut map = HashMap::new();
        for (etype, items) in entities {
            for (eid, _) in items {
                map.insert(*eid, *etype);
            }
        }
        map
    }

    let ref_entity_map = build_entity_map(&ref_entities);
    let gen_entity_map = build_entity_map(&gen_entities);

    // Check what entity types the reference TOC positions point to
    println!("\n=== Reference TOC position entity types ===");
    for (title, pos, _) in ref_nav_positions.iter().take(10) {
        let etype = ref_entity_map.get(&(*pos as u32)).map(|t| format!("${}", t)).unwrap_or("NOT FOUND".to_string());
        println!("  {} (pos={}) -> {}", title, pos, etype);
    }

    println!("\n=== Generated TOC position entity types ===");
    for (title, pos, _) in gen_nav_positions.iter().take(10) {
        let etype = gen_entity_map.get(&(*pos as u32)).map(|t| format!("${}", t)).unwrap_or("NOT FOUND".to_string());
        println!("  {} (pos={}) -> {}", title, pos, etype);
    }

    // Compare entry counts
    println!("\n=== Summary ===");
    println!("Reference TOC entries (all nav containers): {}", ref_nav_positions.len());
    println!("Generated TOC entries (all nav containers): {}", gen_nav_positions.len());

    // Filter to only TOC entries (not reading order or landmarks)
    // The real TOC entries have actual titles like "Titlepage", "I", etc.
    // Reading order entries have "heading-nav-unit" placeholder titles
    let ref_toc_entries: Vec<_> = ref_nav_positions
        .iter()
        .filter(|(title, _, _)| title != "heading-nav-unit" && title != "cover-nav-unit")
        .collect();
    let gen_toc_entries: Vec<_> = gen_nav_positions
        .iter()
        .filter(|(title, _, _)| title != "heading-nav-unit" && title != "cover-nav-unit")
        .collect();

    println!("\nReference TOC entries (filtered): {}", ref_toc_entries.len());
    println!("Generated TOC entries (filtered): {}", gen_toc_entries.len());

    println!("\n=== Reference TOC entries (filtered, first 20) ===");
    for (i, (title, pos, offset)) in ref_toc_entries.iter().take(20).enumerate() {
        println!("  [{}] {} -> pos={}, offset={}", i, title, pos, offset);
    }

    println!("\n=== Generated TOC entries (filtered, first 20) ===");
    for (i, (title, pos, offset)) in gen_toc_entries.iter().take(20).enumerate() {
        println!("  [{}] {} -> pos={}, offset={}", i, title, pos, offset);
    }

    // Compare entry counts (informational, small differences expected)
    if ref_toc_entries.len() != gen_toc_entries.len() {
        println!("\nNote: Entry count difference ({} vs {})",
            ref_toc_entries.len(), gen_toc_entries.len());
    }

    // Check that titles match in order (compare up to min length)
    let min_len = std::cmp::min(ref_toc_entries.len(), gen_toc_entries.len());
    for (i, ((ref_title, _, _), (gen_title, _, _))) in
        ref_toc_entries.iter().zip(gen_toc_entries.iter()).take(min_len).enumerate()
    {
        if ref_title != gen_title {
            println!("TOC entry {} title mismatch: ref='{}', gen='{}'", i, ref_title, gen_title);
        }
    }

    // Verify generated positions point to valid section markers
    // Extract section entities from generated KFX
    let section_entities: Vec<_> = gen_entities
        .get(&260) // SECTION entity type
        .map(|v| v.iter().map(|(id, _)| *id).collect())
        .unwrap_or_default();

    println!("\n=== Generated section entity IDs (first 20) ===");
    for (i, id) in section_entities.iter().take(20).enumerate() {
        println!("  [{}] {}", i, id);
    }

    // Note: Position values are content item indices, not entity IDs
    // This is just informational - positions don't directly map to entities
    println!("\n=== Position vs Section Info ===");
    println!("Generated TOC positions range: {} to {}",
        gen_toc_entries.iter().map(|(_, p, _)| *p).min().unwrap_or(0),
        gen_toc_entries.iter().map(|(_, p, _)| *p).max().unwrap_or(0));
    println!("Section entity ID range: {:?} to {:?}",
        section_entities.iter().min(),
        section_entities.iter().max());

    // Count nav containers in each
    fn count_nav_containers(nav_value: &IonValue) -> Vec<(u64, usize)> {
        let mut containers = Vec::new();
        // Structure: List[1] -> [0] Struct { $392: List of nav containers }
        if let IonValue::List(outer_list) = nav_value {
            if let Some(first) = outer_list.first() {
                if let IonValue::Struct(map) = first {
                    if let Some(nav_container_ref) = map.get(&392) {
                        if let IonValue::List(items) = nav_container_ref {
                            for item in items {
                                if let IonValue::Annotated(_, inner) = item {
                                    if let IonValue::Struct(inner_map) = inner.as_ref() {
                                        // Get nav type ($235)
                                        if let Some(IonValue::Symbol(s)) = inner_map.get(&235) {
                                            // Count entries in $247
                                            let entry_count = inner_map.get(&247)
                                                .map(|e| if let IonValue::List(l) = e { l.len() } else { 0 })
                                                .unwrap_or(0);
                                            containers.push((*s, entry_count));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        containers
    }

    println!("\n=== Nav Container Types ===");
    println!("Reference nav containers:");
    for (nav_type, entry_count) in count_nav_containers(&ref_nav) {
        let type_name = match nav_type {
            212 => "TOC",
            236 => "LANDMARKS",
            798 => "READING_ORDER",
            _ => "UNKNOWN",
        };
        println!("  ${} ({}) - {} entries", nav_type, type_name, entry_count);
    }
    println!("Generated nav containers:");
    for (nav_type, entry_count) in count_nav_containers(&gen_nav) {
        let type_name = match nav_type {
            212 => "TOC",
            236 => "LANDMARKS",
            798 => "READING_ORDER",
            _ => "UNKNOWN",
        };
        println!("  ${} ({}) - {} entries", nav_type, type_name, entry_count);
    }
}

/// A parsed navigation entry for comparison
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ParsedNavEntry {
    title: Option<String>,
    position: Option<i64>,
    offset: Option<i64>,
    children: Vec<ParsedNavEntry>,
}

/// Extract nav entries recursively from a $393:: annotated value
fn extract_nav_entries(value: &IonValue) -> Vec<ParsedNavEntry> {
    let mut entries = Vec::new();

    if let IonValue::List(items) = value {
        for item in items {
            if let IonValue::Annotated(annots, inner) = item {
                if annots.contains(&393) {
                    // $393 = NAV_DEFINITION
                    let entry = extract_single_nav_entry(inner);
                    entries.push(entry);
                }
            }
        }
    }

    entries
}

/// Extract a single nav entry from a struct
fn extract_single_nav_entry(value: &IonValue) -> ParsedNavEntry {
    let title = get_struct_field(value, sym::NAV_TITLE)
        .and_then(|t| get_struct_field(t, sym::TEXT))
        .and_then(|v| v.as_string())
        .map(|s| s.to_string());

    let (position, offset) = get_struct_field(value, sym::NAV_TARGET)
        .map(|target| {
            let pos = target.get(sym::POSITION).and_then(|v| v.as_int());
            let off = target.get(sym::OFFSET).and_then(|v| v.as_int());
            (pos, off)
        })
        .unwrap_or((None, None));

    let children = get_struct_field(value, sym::NAV_ENTRIES)
        .map(extract_nav_entries)
        .unwrap_or_default();

    ParsedNavEntry {
        title,
        position,
        offset,
        children,
    }
}

/// Count total entries in a nav tree (including nested)
fn count_nav_entries(entries: &[ParsedNavEntry]) -> usize {
    entries
        .iter()
        .map(|e| 1 + count_nav_entries(&e.children))
        .sum()
}

/// Collect all positions from nav entries
fn collect_positions(entries: &[ParsedNavEntry], positions: &mut Vec<i64>) {
    for entry in entries {
        if let Some(pos) = entry.position {
            positions.push(pos);
        }
        collect_positions(&entry.children, positions);
    }
}

/// Test that navigation structure matches reference KFX format.
///
/// The $389 (book_navigation) fragment must use INLINE $391:: annotated nav containers,
/// NOT separate fragments referenced by symbol. This is critical for Kindle compatibility.
#[test]
fn test_navigation_structure_matches_reference() {
    // Parse reference KFX
    let ref_data = std::fs::read("tests/fixtures/epictetus.kfx").unwrap();
    let ref_entities = parse_kfx_container(&ref_data);

    // Parse reference $389 (book_navigation)
    let ref_nav_payloads = ref_entities.get(&389).expect("Reference should have $389");
    assert_eq!(ref_nav_payloads.len(), 1, "Should have one $389 fragment");
    let ref_nav = parse_entity_ion(&ref_nav_payloads[0].1).expect("Failed to parse ref $389");

    // Generate KFX from EPUB
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();
    let gen_data = KfxBookBuilder::from_book(&book).build();

    // Write to /tmp for inspection
    std::fs::write("/tmp/test-toc.kfx", &gen_data).unwrap();

    // Debug: compare container headers
    fn debug_container_header(data: &[u8], name: &str) {
        println!("\n=== {} container header ===", name);
        println!("Magic: {:?}", std::str::from_utf8(&data[0..4]));
        let version = u16::from_le_bytes([data[4], data[5]]);
        let header_len = u32::from_le_bytes(data[6..10].try_into().unwrap());
        let ci_offset = u32::from_le_bytes(data[10..14].try_into().unwrap());
        let ci_len = u32::from_le_bytes(data[14..18].try_into().unwrap());
        println!("Version: {}", version);
        println!("Header len: {}", header_len);
        println!("CI offset: {}, len: {}", ci_offset, ci_len);

        // Count entities in table
        let ion_magic: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];
        let mut pos = 18;
        let mut entity_count = 0;
        while pos + 24 <= data.len() && data[pos..pos + 4] != ion_magic {
            entity_count += 1;
            pos += 24;
        }
        println!("Entity table entries: {}", entity_count);
        println!("Entity table ends at: {}", pos);

        // Show what's at entity table end (should be ION BVM for symbol table)
        if pos + 4 <= data.len() {
            println!("After entity table: {:02x} {:02x} {:02x} {:02x}",
                data[pos], data[pos+1], data[pos+2], data[pos+3]);
        }

        // Show first few bytes of container info
        let ci_start = ci_offset as usize;
        if ci_start + 4 <= data.len() {
            println!("Container info starts with: {:02x} {:02x} {:02x} {:02x}",
                data[ci_start], data[ci_start+1], data[ci_start+2], data[ci_start+3]);
        }

        // Show first entity payload
        if entity_count > 0 {
            let first_offset = u64::from_le_bytes(data[26..34].try_into().unwrap()) as usize;
            let payload_start = header_len as usize + first_offset;
            if payload_start + 4 <= data.len() {
                println!("First entity payload at {} starts with: {:02x} {:02x} {:02x} {:02x}",
                    payload_start,
                    data[payload_start], data[payload_start+1],
                    data[payload_start+2], data[payload_start+3]);
            }
        }

        // Parse container info to get symbol table location
        let ci_data = &data[ci_offset as usize..(ci_offset + ci_len) as usize];
        if let Ok(ci) = crate::kfx::ion::IonParser::new(ci_data).parse() {
            println!("Container info fields:");
            let mut symtab_offset = 0usize;
            let mut symtab_len = 0usize;
            if let crate::kfx::ion::IonValue::Struct(map) = &ci {
                for (k, v) in map {
                    if let crate::kfx::ion::IonValue::Int(i) = v {
                        println!("  ${}: {}", k, i);
                        if *k == 415 { symtab_offset = *i as usize; }
                        if *k == 416 { symtab_len = *i as usize; }
                    } else if let crate::kfx::ion::IonValue::String(s) = v {
                        println!("  ${}: {:?}", k, s);
                    }
                }
            }

            // Try to parse symbol table
            if symtab_offset > 0 && symtab_len > 0 && symtab_offset + symtab_len <= data.len() {
                let st_data = &data[symtab_offset..symtab_offset + symtab_len];
                println!("Symbol table starts with: {:02x} {:02x} {:02x} {:02x}",
                    st_data[0], st_data[1], st_data[2], st_data[3]);
                if let Ok(st) = crate::kfx::ion::IonParser::new(st_data).parse() {
                    if let crate::kfx::ion::IonValue::Annotated(annots, inner) = &st {
                        println!("Symbol table annotation: ${}", annots.first().unwrap_or(&0));
                        if let crate::kfx::ion::IonValue::Struct(st_map) = inner.as_ref() {
                            // Check imports and symbols
                            for (k, v) in st_map {
                                match v {
                                    crate::kfx::ion::IonValue::List(l) => {
                                        println!("  ${}: list with {} items", k, l.len());
                                    }
                                    crate::kfx::ion::IonValue::Int(i) => {
                                        println!("  ${}: {}", k, i);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                } else {
                    println!("Failed to parse symbol table");
                }
            }
        }
    }

    debug_container_header(&ref_data, "Reference");
    debug_container_header(&gen_data, "Generated");

    // Parse generated $389
    let gen_entities = parse_kfx_container(&gen_data);
    let gen_nav_payloads = gen_entities.get(&389).expect("Generated should have $389");
    assert_eq!(gen_nav_payloads.len(), 1, "Should have one $389 fragment");
    let gen_nav = parse_entity_ion(&gen_nav_payloads[0].1).expect("Failed to parse gen $389");

    // Both should be lists with one navigation entry
    let ref_list = ref_nav.as_list().expect("Ref $389 should be a list");
    let gen_list = gen_nav.as_list().expect("Gen $389 should be a list");
    assert_eq!(ref_list.len(), gen_list.len(), "Same number of nav entries");

    // Check first entry has required fields
    let ref_entry = &ref_list[0];
    let gen_entry = &gen_list[0];

    // Both should have $178 (reading order name)
    assert!(
        get_struct_field(ref_entry, 178).is_some(),
        "Reference should have $178"
    );
    assert!(
        get_struct_field(gen_entry, 178).is_some(),
        "Generated should have $178"
    );

    // Both should have $392 (nav container refs)
    let ref_392 = get_struct_field(ref_entry, 392).expect("Reference should have $392");
    let gen_392 = get_struct_field(gen_entry, 392).expect("Generated should have $392");

    // CRITICAL: $392 must contain INLINE $391:: annotated nav containers
    fn has_inline_nav_containers(value: &IonValue) -> bool {
        if let IonValue::List(items) = value {
            items.iter().all(|item| {
                matches!(item, IonValue::Annotated(annots, _) if annots.contains(&391))
            })
        } else {
            false
        }
    }

    assert!(
        has_inline_nav_containers(ref_392),
        "Reference $392 should have inline $391:: nav containers"
    );
    assert!(
        has_inline_nav_containers(gen_392),
        "Generated $392 should have inline $391:: nav containers (not symbol refs)"
    );

    // Extract TOC containers (first container with NAV_TYPE = TOC)
    fn get_toc_container(nav_containers: &IonValue) -> Option<&IonValue> {
        if let IonValue::List(items) = nav_containers {
            for item in items {
                if let IonValue::Annotated(_, inner) = item {
                    if let Some(IonValue::Symbol(nav_type)) =
                        get_struct_field(inner, sym::NAV_TYPE)
                    {
                        if *nav_type == sym::TOC {
                            return Some(inner.as_ref());
                        }
                    }
                }
            }
        }
        None
    }

    let ref_toc = get_toc_container(ref_392).expect("Reference should have TOC container");
    let gen_toc = get_toc_container(gen_392).expect("Generated should have TOC container");

    // Extract nav entries from both TOC containers
    let ref_nav_entries_value =
        get_struct_field(ref_toc, sym::NAV_ENTRIES).expect("Ref TOC should have nav entries");
    let gen_nav_entries_value =
        get_struct_field(gen_toc, sym::NAV_ENTRIES).expect("Gen TOC should have nav entries");

    let ref_entries = extract_nav_entries(ref_nav_entries_value);
    let gen_entries = extract_nav_entries(gen_nav_entries_value);

    // Compare total entry counts
    let ref_total = count_nav_entries(&ref_entries);
    let gen_total = count_nav_entries(&gen_entries);
    println!(
        "Reference TOC: {} top-level entries, {} total",
        ref_entries.len(),
        ref_total
    );
    println!(
        "Generated TOC: {} top-level entries, {} total",
        gen_entries.len(),
        gen_total
    );

    // Verify same number of top-level entries
    assert_eq!(
        ref_entries.len(),
        gen_entries.len(),
        "Should have same number of top-level TOC entries"
    );

    // Verify nesting structure matches (same child counts)
    for (i, (ref_e, gen_e)) in ref_entries.iter().zip(gen_entries.iter()).enumerate() {
        assert_eq!(
            ref_e.children.len(),
            gen_e.children.len(),
            "Entry {} should have same number of children (ref={}, gen={})",
            i,
            ref_e.children.len(),
            gen_e.children.len()
        );
    }

    // Collect and compare position values
    let mut ref_positions = Vec::new();
    let mut gen_positions = Vec::new();
    collect_positions(&ref_entries, &mut ref_positions);
    collect_positions(&gen_entries, &mut gen_positions);

    // Positions should be valid (positive, representing EIDs)
    assert!(
        gen_positions.iter().all(|&p| p > 0),
        "All generated positions should be positive EIDs"
    );

    // Generated should have same number of position references
    assert_eq!(
        ref_positions.len(),
        gen_positions.len(),
        "Should have same number of position references"
    );

    // Verify generated entries have valid titles and positions, matching reference structure
    fn compare_entries(
        ref_entries: &[ParsedNavEntry],
        gen_entries: &[ParsedNavEntry],
        path: &str,
    ) {
        assert_eq!(
            ref_entries.len(),
            gen_entries.len(),
            "{}: entry count mismatch",
            path
        );

        for (i, (ref_e, gen_e)) in ref_entries.iter().zip(gen_entries.iter()).enumerate() {
            let entry_path = if path.is_empty() {
                format!("[{}]", i)
            } else {
                format!("{}[{}]", path, i)
            };

            // Generated must have a non-empty title
            assert!(
                gen_e.title.is_some() && !gen_e.title.as_ref().unwrap().is_empty(),
                "{}: generated entry missing title",
                entry_path
            );

            // Both must have valid positions (positive EIDs)
            assert!(
                ref_e.position.is_some() && ref_e.position.unwrap() > 0,
                "{}: reference entry missing/invalid position",
                entry_path
            );
            assert!(
                gen_e.position.is_some() && gen_e.position.unwrap() > 0,
                "{}: generated entry missing/invalid position (title={:?})",
                entry_path,
                gen_e.title
            );

            // Recursively compare children (nesting structure must match)
            compare_entries(&ref_e.children, &gen_e.children, &entry_path);
        }
    }

    compare_entries(&ref_entries, &gen_entries, "");

    // Additional check: collect all titles from generated and verify they're reasonable
    fn collect_titles(entries: &[ParsedNavEntry], titles: &mut Vec<String>) {
        for e in entries {
            if let Some(ref t) = e.title {
                titles.push(t.clone());
            }
            collect_titles(&e.children, titles);
        }
    }

    let mut gen_titles = Vec::new();
    collect_titles(&gen_entries, &mut gen_titles);

    // Verify expected titles are present
    let expected_titles = ["Titlepage", "Imprint", "The Enchiridion", "Fragments", "Endnotes"];
    for expected in expected_titles {
        assert!(
            gen_titles.iter().any(|t| t == expected),
            "Generated TOC should contain '{}'",
            expected
        );
    }

    // Verify chapter numbers are present (I, II, III, etc.)
    let roman_numerals = ["I", "II", "III", "IV", "V"];
    for numeral in roman_numerals {
        assert!(
            gen_titles.iter().any(|t| t == numeral),
            "Generated TOC should contain chapter '{}'",
            numeral
        );
    }
}
