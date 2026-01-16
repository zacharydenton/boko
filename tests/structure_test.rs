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
