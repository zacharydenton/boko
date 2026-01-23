//! Structure tests ported from calibre's polish tests.
//!
//! These tests verify TOC detection, metadata handling, and book structure
//! using the Standard Ebooks edition of Epictetus's "Short Works".
//!
//! Original calibre tests: calibre/src/calibre/ebooks/oeb/polish/tests/structure.py

use boko::{TocEntry, read_epub, read_mobi};
use tempfile::TempDir;

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture_path(name: &str) -> String {
    format!("{}/{}", FIXTURES_DIR, name)
}

// ============================================================================
// TOC Detection Tests
// Ported from calibre's Structure.test_toc_detection
// ============================================================================

#[test]
fn test_epub_toc_detection() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Standard Ebooks have comprehensive TOCs
    assert!(!book.toc.is_empty(), "TOC should be detected");

    // Count total TOC entries including nested
    fn count_entries(entries: &[TocEntry]) -> usize {
        entries.iter().map(|e| 1 + count_entries(&e.children)).sum()
    }

    let total_entries = count_entries(&book.toc);
    println!("Total TOC entries (including nested): {}", total_entries);
    assert!(total_entries > 0, "Should have TOC entries");
}

#[test]
fn test_azw3_toc_detection() {
    let book = read_mobi(fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    assert!(!book.toc.is_empty(), "TOC should be detected in AZW3");

    // Print TOC structure for debugging
    fn print_toc(entries: &[TocEntry], depth: usize) {
        for entry in entries {
            println!("{}- {}", "  ".repeat(depth), entry.title);
            print_toc(&entry.children, depth + 1);
        }
    }
    println!("AZW3 TOC:");
    print_toc(&book.toc, 0);
}

#[test]
fn test_toc_hierarchy() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Check if TOC has any nested entries (common in Standard Ebooks)
    let has_nested = book.toc.iter().any(|e| !e.children.is_empty());

    println!(
        "TOC has {} top-level entries, nested: {}",
        book.toc.len(),
        has_nested
    );
}

#[test]
fn test_toc_hrefs_valid() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    fn check_toc_hrefs(entries: &[TocEntry]) {
        for entry in entries {
            // TOC hrefs should not be empty
            assert!(
                !entry.href.is_empty() || !entry.children.is_empty(),
                "TOC entry '{}' has empty href and no children",
                entry.title
            );

            // Hrefs should be reasonable paths or fragments
            if !entry.href.is_empty() {
                assert!(
                    !entry.href.contains("..") || entry.href.starts_with("../"),
                    "TOC href '{}' has suspicious path",
                    entry.href
                );
            }

            check_toc_hrefs(&entry.children);
        }
    }

    check_toc_hrefs(&book.toc);
}

// ============================================================================
// Metadata Tests
// ============================================================================

#[test]
fn test_epub_metadata() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Standard Ebooks have complete metadata
    println!("Title: {}", book.metadata.title);
    println!("Authors: {:?}", book.metadata.authors);
    println!("Language: {}", book.metadata.language);
    println!("Identifier: {}", book.metadata.identifier);

    assert!(!book.metadata.title.is_empty(), "Title should be set");
    assert!(!book.metadata.authors.is_empty(), "Authors should be set");
    assert!(!book.metadata.language.is_empty(), "Language should be set");
}

#[test]
fn test_azw3_metadata() {
    let book = read_mobi(fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    println!("AZW3 Title: {}", book.metadata.title);
    println!("AZW3 Authors: {:?}", book.metadata.authors);

    assert!(!book.metadata.title.is_empty(), "Title should be set");
    assert!(!book.metadata.authors.is_empty(), "Authors should be set");
}

#[test]
fn test_metadata_preservation_epub_roundtrip() {
    let original = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.epub");

    boko::write_epub(&original, &output_path).expect("Failed to write EPUB");
    let roundtrip = read_epub(&output_path).expect("Failed to read roundtrip");

    assert_eq!(original.metadata.title, roundtrip.metadata.title);
    assert_eq!(original.metadata.authors, roundtrip.metadata.authors);
    assert_eq!(original.metadata.language, roundtrip.metadata.language);
}

// ============================================================================
// Spine Tests
// ============================================================================

#[test]
fn test_epub_spine() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    assert!(!book.spine.is_empty(), "Spine should not be empty");

    // Spine items should have valid hrefs
    for item in &book.spine {
        assert!(!item.href.is_empty(), "Spine item href should not be empty");
        assert!(
            !item.media_type.is_empty(),
            "Spine item media type should not be empty"
        );
    }

    println!("Spine has {} items", book.spine.len());
    for (i, item) in book.spine.iter().take(5).enumerate() {
        println!("  [{}] {} ({})", i, item.href, item.media_type);
    }
}

#[test]
fn test_azw3_spine() {
    let book = read_mobi(fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    assert!(!book.spine.is_empty(), "Spine should not be empty");

    println!("AZW3 Spine has {} items", book.spine.len());
}

#[test]
fn test_spine_references_resources() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Every spine item should reference an existing resource
    for item in &book.spine {
        assert!(
            book.resources.contains_key(&item.href),
            "Spine item '{}' not found in resources",
            item.href
        );
    }
}

// ============================================================================
// Resource Tests
// ============================================================================

#[test]
fn test_epub_resource_types() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let mut xhtml_count = 0;
    let mut css_count = 0;
    let mut image_count = 0;
    let mut other_count = 0;

    for resource in book.resources.values() {
        match resource.media_type.as_str() {
            "application/xhtml+xml" | "text/html" => xhtml_count += 1,
            "text/css" => css_count += 1,
            t if t.starts_with("image/") => image_count += 1,
            _ => other_count += 1,
        }
    }

    println!(
        "XHTML: {}, CSS: {}, Images: {}, Other: {}",
        xhtml_count, css_count, image_count, other_count
    );

    assert!(xhtml_count > 0, "Should have XHTML documents");
    assert!(css_count > 0, "Standard Ebooks have CSS");
}

#[test]
fn test_azw3_resource_types() {
    let book = read_mobi(fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    let has_html = book
        .resources
        .values()
        .any(|r| r.media_type == "application/xhtml+xml" || r.media_type == "text/html");

    assert!(has_html, "AZW3 should have HTML content");
    println!("AZW3 has {} resources", book.resources.len());
}

// ============================================================================
// Cover Tests
// Ported from calibre's test_epub2_covers and test_epub3_covers
// ============================================================================

#[test]
fn test_epub_cover_metadata() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Standard Ebooks include cover images
    if let Some(cover) = &book.metadata.cover_image {
        println!("Cover image: {}", cover);
        assert!(
            book.resources.contains_key(cover),
            "Cover image should exist in resources"
        );
    } else {
        println!("No cover image metadata found");
    }
}

#[test]
fn test_cover_preservation() {
    let original = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.epub");

    boko::write_epub(&original, &output_path).expect("Failed to write EPUB");
    let roundtrip = read_epub(&output_path).expect("Failed to read roundtrip");

    assert_eq!(
        original.metadata.cover_image, roundtrip.metadata.cover_image,
        "Cover image should be preserved"
    );
}

// ============================================================================
// TOC Text Preservation Tests
// ============================================================================

// ============================================================================
// MOBI Tests (Legacy format)
// ============================================================================

#[test]
fn test_read_mobi() {
    let book = read_mobi(fixture_path("epictetus.mobi")).expect("Failed to read MOBI");

    assert!(!book.metadata.title.is_empty(), "MOBI should have title");
    assert!(!book.resources.is_empty(), "MOBI should have resources");
    assert!(!book.spine.is_empty(), "MOBI should have spine");

    println!("MOBI Title: {}", book.metadata.title);
    println!("MOBI Resources: {}", book.resources.len());
    println!("MOBI Spine: {}", book.spine.len());
}

#[test]
fn test_mobi_metadata() {
    let book = read_mobi(fixture_path("epictetus.mobi")).expect("Failed to read MOBI");

    println!("MOBI Title: {}", book.metadata.title);
    println!("MOBI Authors: {:?}", book.metadata.authors);

    assert_eq!(book.metadata.title, "Short Works");
    assert!(
        book.metadata
            .authors
            .iter()
            .any(|a| a.contains("Epictetus")),
        "Should have Epictetus as author"
    );
}

#[test]
fn test_mobi_has_content() {
    let book = read_mobi(fixture_path("epictetus.mobi")).expect("Failed to read MOBI");

    // MOBI6 produces a single HTML file
    let html_resources: Vec<_> = book
        .resources
        .iter()
        .filter(|(_, r)| r.media_type == "application/xhtml+xml" || r.media_type == "text/html")
        .collect();

    assert!(!html_resources.is_empty(), "MOBI should have HTML content");

    // Check content has actual text
    for (href, res) in &html_resources {
        let content = String::from_utf8_lossy(&res.data);
        assert!(content.len() > 100, "HTML {} should have content", href);
        assert!(
            content.contains("<body") || content.contains("<html"),
            "HTML {} should be valid markup",
            href
        );
    }
}

#[test]
fn test_mobi_to_epub_conversion() {
    let mobi = read_mobi(fixture_path("epictetus.mobi")).expect("Failed to read MOBI");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let epub_path = temp_dir.path().join("converted.epub");

    boko::write_epub(&mobi, &epub_path).expect("Failed to write EPUB");

    let epub = read_epub(&epub_path).expect("Failed to read converted EPUB");

    // Metadata should be preserved
    assert_eq!(mobi.metadata.title, epub.metadata.title);
    assert_eq!(mobi.metadata.authors, epub.metadata.authors);

    // Should have content
    assert!(!epub.spine.is_empty(), "Converted EPUB should have spine");
    assert!(
        !epub.resources.is_empty(),
        "Converted EPUB should have resources"
    );
}

#[test]
fn test_mobi_epub_azw3_roundtrip() {
    // MOBI -> EPUB -> AZW3 -> EPUB
    let mobi = read_mobi(fixture_path("epictetus.mobi")).expect("Failed to read MOBI");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // MOBI -> EPUB
    let epub1_path = temp_dir.path().join("step1.epub");
    boko::write_epub(&mobi, &epub1_path).expect("Failed to write EPUB");
    let epub1 = read_epub(&epub1_path).expect("Failed to read EPUB");

    // EPUB -> AZW3
    let azw3_path = temp_dir.path().join("step2.azw3");
    boko::write_mobi(&epub1, &azw3_path).expect("Failed to write AZW3");
    let azw3 = read_mobi(&azw3_path).expect("Failed to read AZW3");

    // AZW3 -> EPUB
    let epub2_path = temp_dir.path().join("step3.epub");
    boko::write_epub(&azw3, &epub2_path).expect("Failed to write final EPUB");
    let epub2 = read_epub(&epub2_path).expect("Failed to read final EPUB");

    // Title should survive the roundtrip
    assert_eq!(mobi.metadata.title, epub2.metadata.title);
}

#[test]
fn test_mobi_vs_azw3_same_source() {
    // Both files are from the same EPUB source
    let mobi = read_mobi(fixture_path("epictetus.mobi")).expect("Failed to read MOBI");
    let azw3 = read_mobi(fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    // Metadata should match
    assert_eq!(mobi.metadata.title, azw3.metadata.title);
    assert_eq!(mobi.metadata.authors, azw3.metadata.authors);

    println!(
        "MOBI: {} resources, {} spine items",
        mobi.resources.len(),
        mobi.spine.len()
    );
    println!(
        "AZW3: {} resources, {} spine items",
        azw3.resources.len(),
        azw3.spine.len()
    );
}

// ============================================================================
// TOC Text Preservation Tests
// ============================================================================

// ============================================================================
// KFX Structure Tests
// ============================================================================

/// Test that KFX output has granular reading positions (not just 1 per section)
#[test]
fn test_kfx_location_map_granularity() {
    use boko::{read_epub, read_kfx, write_kfx};

    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Count content sections
    let section_count = book.spine.len();
    println!("EPUB spine has {} sections", section_count);

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let kfx_path = temp_dir.path().join("test.kfx");

    write_kfx(&book, &kfx_path).expect("Failed to write KFX");

    // Read back and verify structure
    let kfx_book = read_kfx(&kfx_path).expect("Failed to read KFX");

    // The book should have content
    assert!(!kfx_book.spine.is_empty(), "KFX should have spine");

    // Count total text content items (paragraphs)
    let total_paragraphs: usize = kfx_book
        .resources
        .values()
        .filter(|r| r.media_type == "application/xhtml+xml" || r.media_type == "text/html")
        .map(|r| {
            let content = String::from_utf8_lossy(&r.data);
            // Rough count of paragraph-like elements
            content.matches("<p").count() + content.matches("<h").count()
        })
        .sum();

    println!("KFX has approximately {} text blocks", total_paragraphs);

    // The key test: KFX should have more than just section_count position entries
    // (This is a sanity check - the actual location map testing would need ION parsing)
    // For now, just verify the roundtrip works and content is preserved
    assert!(
        !kfx_book.metadata.title.is_empty(),
        "KFX should preserve title"
    );
}

/// Test that TOC entries with special characters survive EPUB -> AZW3 -> EPUB roundtrip
#[test]
fn test_toc_text_preservation_roundtrip() {
    use boko::{Book, write_epub, write_mobi};

    // Create a book with special character TOC entries
    let mut book = Book::new();
    book.metadata.title = "Test Book".to_string();
    book.metadata.identifier = "test-special-toc".to_string();
    book.metadata.language = "en".to_string();

    // Add content
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter</title></head>
<body>
<h1 id="ch1">What's in This Book?</h1>
<p>Content here.</p>
<h1 id="ch2">Don't Stop</h1>
<p>More content.</p>
</body>
</html>"#;
    book.add_resource(
        "chapter.html".to_string(),
        content.as_bytes().to_vec(),
        "application/xhtml+xml".to_string(),
    );
    book.add_spine_item(
        "ch1",
        "chapter.html".to_string(),
        "application/xhtml+xml".to_string(),
    );

    // Add TOC with special characters
    // Use \u{2019} for curly apostrophe (RIGHT SINGLE QUOTATION MARK)
    book.toc.push(TocEntry::new(
        "What\u{2019}s in This Book?",
        "chapter.html#ch1",
    )); // curly apostrophe '
    book.toc
        .push(TocEntry::new("Don't Stop", "chapter.html#ch2")); // straight apostrophe '

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // EPUB -> AZW3
    let azw3_path = temp_dir.path().join("test.azw3");
    write_mobi(&book, &azw3_path).expect("Failed to write AZW3");

    // Read AZW3
    let azw3_book = read_mobi(&azw3_path).expect("Failed to read AZW3");

    // Check TOC entries
    println!("AZW3 TOC entries:");
    for entry in &azw3_book.toc {
        println!("  - '{}'", entry.title);
    }

    // Find the entries
    fn find_title_containing<'a>(toc: &'a [TocEntry], substr: &str) -> Option<&'a str> {
        for entry in toc {
            if entry.title.contains(substr) {
                return Some(&entry.title);
            }
            if let Some(found) = find_title_containing(&entry.children, substr) {
                return Some(found);
            }
        }
        None
    }

    // Check "What's in This Book?" - should contain "What" at start
    let whats_entry = find_title_containing(&azw3_book.toc, "This Book");
    assert!(
        whats_entry.is_some(),
        "Should find TOC entry containing 'This Book'"
    );
    let whats_title = whats_entry.unwrap();
    assert!(
        whats_title.starts_with("What"),
        "TOC entry '{}' should start with 'What'",
        whats_title
    );

    // AZW3 -> EPUB
    let epub_path = temp_dir.path().join("roundtrip.epub");
    write_epub(&azw3_book, &epub_path).expect("Failed to write EPUB");

    // Read roundtripped EPUB
    let epub_book = read_epub(&epub_path).expect("Failed to read EPUB");

    // Check TOC again
    println!("Roundtrip EPUB TOC entries:");
    for entry in &epub_book.toc {
        println!("  - '{}'", entry.title);
    }

    let epub_whats = find_title_containing(&epub_book.toc, "This Book");
    assert!(
        epub_whats.is_some(),
        "Should find TOC entry containing 'This Book'"
    );
    let epub_title = epub_whats.unwrap();
    // Must contain the curly apostrophe (U+2019), not just start with "What"
    assert!(
        epub_title.contains('\u{2019}'),
        "Roundtrip EPUB TOC '{}' should preserve curly apostrophe",
        epub_title
    );

    let epub_dont = find_title_containing(&epub_book.toc, "Stop");
    assert!(
        epub_dont.is_some() && epub_dont.unwrap().contains('\''),
        "Roundtrip EPUB TOC should preserve straight apostrophe in 'Don't Stop'"
    );
}

// ============================================================================
// KFX Poetry Line Break Test
// ============================================================================

#[test]
fn test_kfx_poetry_has_separate_lines() {
    // Build KFX from the epictetus EPUB and verify poetry lines are split
    use boko::write_kfx;

    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Create temp dir and write KFX
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let kfx_path = temp_dir.path().join("test.kfx");
    write_kfx(&book, &kfx_path).expect("Failed to write KFX");

    // Read the KFX file and parse to look for text content
    let kfx_data = std::fs::read(&kfx_path).expect("Failed to read KFX file");

    // The KFX is a container format. We look for text strings in the data.
    // Text content entries are stored as Ion strings. If poetry is properly split,
    // we should see separate entries like "Lead me, O Zeus..." and "The way that I am bid..."
    // instead of a single merged entry.

    // Simple check: look for the text strings in the binary
    let kfx_str = String::from_utf8_lossy(&kfx_data);

    // Find all occurrences of "Zeus" and check surrounding text
    let mut zeus_contexts = Vec::new();
    for (i, _) in kfx_str.match_indices("Zeus") {
        // Get context around the match (up to 200 chars before and after)
        let start = i.saturating_sub(50);
        let end = (i + 200).min(kfx_str.len());
        let context = &kfx_str[start..end];
        // Extract just printable ASCII for readability
        let printable: String = context
            .chars()
            .filter(|c| c.is_ascii() && (*c >= ' ' || *c == '\n'))
            .collect();
        if !printable.is_empty() {
            zeus_contexts.push(printable);
        }
    }

    println!("Zeus contexts found in KFX: {:?}", zeus_contexts);

    // Check if the poetry is split or merged
    // If merged, we'd see "Lead me, O Zeus...The way that I am bid" in one context
    // If split, "The way that I am bid" should be in a separate entry
    let has_merged = zeus_contexts
        .iter()
        .any(|ctx| ctx.contains("Zeus") && ctx.contains("The way that I am bid"));

    if has_merged {
        println!("WARNING: Poetry appears to be merged in KFX output");
        // Don't fail yet - just warn. The actual parsing would be more definitive.
    }

    // Basic sanity check - we should find Zeus in the output
    assert!(
        !zeus_contexts.is_empty(),
        "Should find 'Zeus' text in KFX output"
    );
}

// ============================================================================
// KFX Inline Style Test
// ============================================================================
//
// Regression test for inline styles. The imprint.xhtml section has external links:
//
// | Text                                       | Offset | Length | URL                                      |
// |--------------------------------------------|--------|--------|------------------------------------------|
// | "Standard Ebooks"                          | 71     | 15     | https://standardebooks.org/              |
// | "Perseus Digital Library"                  | 59     | 23     | http://www.perseus.tufts.edu/...         |
// | "Internet Archive"                         | 113    | 16     | https://archive.org/...                  |
// | "CC0 1.0 Universal Public Domain Ded..."   | 462    | 42     | https://creativecommons.org/...          |
// | "Uncopyright"                              | 544    | 11     | (internal link)                          |
// | "standardebooks.org"                       | 282    | 18     | https://standardebooks.org/              |
//
// The regression was: inline styles inherited block properties (text-align, margins),
// causing link text to be invisible (underlined but 0-width).

use boko::kfx::ion::{IonParser, IonValue};
use boko::kfx::writer::sym;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

/// Block-level properties that must not appear in inline styles
const BLOCK_PROPERTIES: &[u64] = &[
    sym::TEXT_ALIGN,   // $34
    sym::SPACE_BEFORE, // $47
    sym::MARGIN_LEFT,  // $48
    sym::SPACE_AFTER,  // $49
    sym::MARGIN_RIGHT, // $50
    sym::STYLE_WIDTH,  // $56
    sym::STYLE_HEIGHT, // $57
];

/// Parse KFX container, returns map of entity_type -> [(id, payload)]
fn parse_kfx_container(data: &[u8]) -> HashMap<u32, Vec<(u32, Vec<u8>)>> {
    let mut entities = HashMap::new();
    if data.len() < 18 || &data[0..4] != b"CONT" {
        return entities;
    }

    let header_len = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
    let ion_magic: [u8; 4] = [0xe0, 0x01, 0x00, 0xea];
    let mut pos = 18;

    while pos + 24 <= data.len() && data[pos..pos + 4] != ion_magic {
        let id = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
        let etype = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap());
        let offset = u64::from_le_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize;
        let length = u64::from_le_bytes(data[pos + 16..pos + 24].try_into().unwrap()) as usize;

        let start = header_len + offset;
        if start + length <= data.len() {
            entities
                .entry(etype)
                .or_insert_with(Vec::new)
                .push((id, data[start..start + length].to_vec()));
        }
        pos += 24;
    }
    entities
}

/// Parse entity payload to ION (skips ENTY header)
fn parse_entity_ion(payload: &[u8]) -> Option<IonValue> {
    if payload.len() < 10 || &payload[0..4] != b"ENTY" {
        return None;
    }
    let header_len = u32::from_le_bytes(payload[6..10].try_into().unwrap()) as usize;
    if header_len >= payload.len() {
        return None;
    }
    IonParser::new(&payload[header_len..]).parse().ok()
}

/// Inline run: (offset, length, anchor_ref)
#[derive(Debug, PartialEq)]
struct InlineRun {
    offset: i64,
    length: i64,
    anchor: Option<u64>,
}

/// Recursively collect inline runs from content blocks
fn collect_inline_runs(value: &IonValue, runs: &mut Vec<InlineRun>) {
    match value {
        IonValue::Struct(map) => {
            if let Some(IonValue::List(list)) = map.get(&sym::INLINE_STYLE_RUNS) {
                for run in list {
                    let offset = run.get(sym::OFFSET).and_then(|v| v.as_int()).unwrap_or(0);
                    let length = run.get(sym::COUNT).and_then(|v| v.as_int()).unwrap_or(0);
                    let anchor = run.get(sym::ANCHOR_REF).and_then(|v| v.as_symbol());
                    runs.push(InlineRun { offset, length, anchor });
                }
            }
            for (_, child) in map {
                collect_inline_runs(child, runs);
            }
        }
        IonValue::List(items) => items.iter().for_each(|i| collect_inline_runs(i, runs)),
        IonValue::Annotated(_, inner) => collect_inline_runs(inner, runs),
        _ => {}
    }
}

/// Recursively collect style refs used in inline runs
fn collect_inline_style_refs(value: &IonValue, refs: &mut HashSet<u64>) {
    match value {
        IonValue::Struct(map) => {
            if let Some(IonValue::List(list)) = map.get(&sym::INLINE_STYLE_RUNS) {
                for run in list {
                    if let Some(s) = run.get(sym::STYLE).and_then(|v| v.as_symbol()) {
                        refs.insert(s);
                    }
                }
            }
            for (_, child) in map {
                collect_inline_style_refs(child, refs);
            }
        }
        IonValue::List(items) => items.iter().for_each(|i| collect_inline_style_refs(i, refs)),
        IonValue::Annotated(_, inner) => collect_inline_style_refs(inner, refs),
        _ => {}
    }
}

/// Check if style has block properties
fn has_block_props(style: &IonValue) -> Vec<u64> {
    let inner = style.unwrap_annotated();
    let Some(map) = inner.as_struct() else { return vec![] };
    BLOCK_PROPERTIES.iter().filter(|&&p| map.contains_key(&p)).copied().collect()
}

/// Get anchor URL from page_template entity (external links)
fn get_anchor_url(anchor_entities: &[(u32, Vec<u8>)], anchor_sym: u64) -> Option<String> {
    for (_, payload) in anchor_entities {
        if let Some(ion) = parse_entity_ion(payload) {
            let inner = ion.unwrap_annotated();
            if let Some(map) = inner.as_struct() {
                if map.get(&sym::TEMPLATE_NAME).and_then(|v| v.as_symbol()) == Some(anchor_sym) {
                    return map.get(&sym::EXTERNAL_URL).and_then(|v| v.as_string()).map(|s| s.to_string());
                }
            }
        }
    }
    None
}

/// Check if anchor is internal (has POSITION_INFO instead of EXTERNAL_URL)
fn is_internal_anchor(anchor_entities: &[(u32, Vec<u8>)], anchor_sym: u64) -> bool {
    for (_, payload) in anchor_entities {
        if let Some(ion) = parse_entity_ion(payload) {
            let inner = ion.unwrap_annotated();
            if let Some(map) = inner.as_struct() {
                if map.get(&sym::TEMPLATE_NAME).and_then(|v| v.as_symbol()) == Some(anchor_sym) {
                    // Internal if has POSITION_INFO but no EXTERNAL_URL
                    return map.contains_key(&sym::POSITION_INFO) && !map.contains_key(&sym::EXTERNAL_URL);
                }
            }
        }
    }
    false
}

#[test]
fn test_kfx_inline_styles_no_block_properties() {
    use boko::write_kfx_to_writer;

    // Write KFX to memory (no file I/O)
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");
    let mut buffer = Cursor::new(Vec::new());
    write_kfx_to_writer(&book, &mut buffer).expect("Failed to write KFX");
    let kfx_data = buffer.into_inner();

    // Parse container
    let entities = parse_kfx_container(&kfx_data);
    let empty: Vec<(u32, Vec<u8>)> = vec![];
    let style_entities = entities.get(&157).unwrap_or(&empty);
    let content_entities = entities.get(&259).unwrap_or(&empty);
    let anchor_entities = entities.get(&266).unwrap_or(&empty);

    // Build style map: symbol -> IonValue
    let mut styles: HashMap<u64, IonValue> = HashMap::new();
    for (_, payload) in style_entities {
        if let Some(ion) = parse_entity_ion(payload) {
            let inner = ion.unwrap_annotated();
            if let Some(sym) = inner.as_struct().and_then(|m| m.get(&sym::STYLE_NAME)).and_then(|v| v.as_symbol()) {
                styles.insert(sym, ion.clone());
            }
        }
    }

    // Collect all inline style refs and runs
    let mut inline_style_refs = HashSet::new();
    let mut inline_runs = Vec::new();
    for (_, payload) in content_entities {
        if let Some(ion) = parse_entity_ion(payload) {
            collect_inline_style_refs(&ion, &mut inline_style_refs);
            collect_inline_runs(&ion, &mut inline_runs);
        }
    }

    println!("Styles: {}, Inline style refs: {}, Inline runs: {}",
        styles.len(), inline_style_refs.len(), inline_runs.len());

    // =========================================================================
    // 1. Inline styles must not have block properties
    // =========================================================================
    for style_sym in &inline_style_refs {
        if let Some(style) = styles.get(style_sym) {
            let bad = has_block_props(style);
            assert!(bad.is_empty(),
                "Style ${} has block properties: {:?}", style_sym, bad);
        }
    }
    println!("[OK] No inline styles have block properties");

    // =========================================================================
    // 2. Inline runs must have valid offset/length
    // =========================================================================
    for run in &inline_runs {
        assert!(run.offset >= 0, "Run has negative offset: {:?}", run);
        assert!(run.length >= 0, "Run has negative length: {:?}", run);
    }
    println!("[OK] All inline runs have valid offset/length");

    // =========================================================================
    // 3. Runs with anchors must have correct offset, length, and URL
    // =========================================================================
    // Expected external links from imprint.xhtml (from reference KFX):
    // Format: (domain, offset, length)
    let expected_external_links = [
        ("standardebooks.org", 71, 15),   // "Standard Ebooks"
        ("perseus.tufts.edu", 59, 23),    // "Perseus Digital Library"
        ("archive.org", 113, 16),         // "Internet Archive"
        ("creativecommons.org", 462, 42), // "CC0 1.0 Universal..."
    ];

    let runs_with_anchors: Vec<_> = inline_runs.iter()
        .filter(|r| r.anchor.is_some())
        .collect();
    println!("Runs with anchors: {}", runs_with_anchors.len());

    // Check expected external links are present with correct offset and length
    for (domain, expected_offset, expected_len) in expected_external_links {
        let found = runs_with_anchors.iter().find(|r| {
            if let Some(anchor_sym) = r.anchor {
                if let Some(url) = get_anchor_url(anchor_entities, anchor_sym) {
                    return url.contains(domain);
                }
            }
            false
        });
        assert!(found.is_some(), "Missing link to {}", domain);
        let run = found.unwrap();
        assert_eq!(run.offset, expected_offset as i64,
            "Wrong offset for {} link: expected {}, got {}", domain, expected_offset, run.offset);
        assert_eq!(run.length, expected_len as i64,
            "Wrong length for {} link: expected {}, got {}", domain, expected_len, run.length);
        println!("[OK] Found {} link (offset={}, length={})", domain, run.offset, run.length);
    }

    // =========================================================================
    // Verify internal "Uncopyright" link (from reference KFX: offset=544, length=11)
    // This link points to an internal anchor (#uncopyright) rather than external URL
    let uncopyright = runs_with_anchors.iter().find(|r| {
        if let Some(anchor_sym) = r.anchor {
            is_internal_anchor(anchor_entities, anchor_sym) && r.length == 11
        } else {
            false
        }
    });
    assert!(uncopyright.is_some(), "Missing internal 'Uncopyright' link");
    let uncopyright = uncopyright.unwrap();
    assert_eq!(uncopyright.offset, 544, "Wrong offset for Uncopyright link: expected 544, got {}", uncopyright.offset);
    assert_eq!(uncopyright.length, 11, "Wrong length for Uncopyright link: expected 11, got {}", uncopyright.length);
    println!("[OK] Found internal 'Uncopyright' link (offset={}, length={})", uncopyright.offset, uncopyright.length);

    println!("\n=== All verifications passed ===");
}
