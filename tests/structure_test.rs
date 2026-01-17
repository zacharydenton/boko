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
