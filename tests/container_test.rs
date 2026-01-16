//! Container tests ported from calibre's polish tests.
//!
//! These tests verify EPUB and AZW3 container operations using the
//! Standard Ebooks edition of Epictetus's "Short Works" as test data.
//!
//! Original calibre tests: calibre/src/calibre/ebooks/oeb/polish/tests/

use std::collections::HashSet;

use boko::{read_epub, read_mobi, write_epub, write_mobi, Book};
use tempfile::TempDir;

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture_path(name: &str) -> String {
    format!("{}/{}", FIXTURES_DIR, name)
}

/// Validate that all spine items and TOC entries reference existing resources.
/// Ported from calibre's BaseTest.check_links method.
fn check_links(book: &Book) -> Vec<String> {
    let mut errors = Vec::new();
    let resource_hrefs: HashSet<_> = book.resources.keys().cloned().collect();

    // Check spine references
    for item in &book.spine {
        if !resource_hrefs.contains(&item.href) {
            errors.push(format!("Spine item '{}' not found in resources", item.href));
        }
    }

    // Check TOC references
    fn check_toc_entry(entry: &boko::TocEntry, resources: &HashSet<String>, errors: &mut Vec<String>) {
        let href_base = entry.href.split('#').next().unwrap_or(&entry.href);
        if !href_base.is_empty() {
            let found = resources.iter().any(|r| {
                r == href_base || r.ends_with(href_base) || {
                    let r_base = r.rsplit('/').next().unwrap_or(r);
                    r_base == href_base
                }
            });
            if !found {
                errors.push(format!("TOC entry '{}' ({}) not found in resources", entry.title, entry.href));
            }
        }
        for child in &entry.children {
            check_toc_entry(child, resources, errors);
        }
    }

    for entry in &book.toc {
        check_toc_entry(entry, &resource_hrefs, &mut errors);
    }

    errors
}

// ============================================================================
// EPUB Container Tests
// Ported from calibre's ContainerTests class
// ============================================================================

#[test]
fn test_read_epub() {
    let path = fixture_path("epictetus.epub");
    let book = read_epub(&path).expect("Failed to read EPUB");

    // Verify metadata was extracted
    assert!(!book.metadata.title.is_empty(), "Title should not be empty");
    assert!(!book.metadata.authors.is_empty(), "Authors should not be empty");
    assert!(!book.metadata.language.is_empty(), "Language should not be empty");

    // Verify structure
    assert!(!book.spine.is_empty(), "Spine should not be empty");
    assert!(!book.resources.is_empty(), "Resources should not be empty");
}

#[test]
fn test_read_epub_toc() {
    let path = fixture_path("epictetus.epub");
    let book = read_epub(&path).expect("Failed to read EPUB");

    // Standard Ebooks editions have well-structured TOCs
    assert!(!book.toc.is_empty(), "TOC should not be empty");

    // Print TOC for debugging
    fn print_toc(entries: &[boko::TocEntry], depth: usize) {
        for entry in entries {
            println!("{}{}: {}", "  ".repeat(depth), entry.title, entry.href);
            print_toc(&entry.children, depth + 1);
        }
    }
    println!("TOC structure:");
    print_toc(&book.toc, 0);
}

#[test]
fn test_epub_links_valid() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");
    let errors = check_links(&book);
    assert!(errors.is_empty(), "Link validation errors: {:?}", errors);
}

#[test]
fn test_epub_resources() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Verify resource types
    let has_xhtml = book.resources.values().any(|r| {
        r.media_type == "application/xhtml+xml" || r.media_type == "text/html"
    });
    assert!(has_xhtml, "Should have XHTML content documents");

    let has_css = book.resources.values().any(|r| r.media_type == "text/css");
    assert!(has_css, "Standard Ebooks include CSS stylesheets");

    // Print resource summary
    let types: HashSet<_> = book.resources.values().map(|r| r.media_type.as_str()).collect();
    println!("Resource types: {:?}", types);
    println!("Total resources: {}", book.resources.len());
}

// ============================================================================
// AZW3 Container Tests
// ============================================================================

#[test]
fn test_read_azw3() {
    let path = fixture_path("epictetus.azw3");
    let book = read_mobi(&path).expect("Failed to read AZW3");

    // Verify metadata was extracted
    assert!(!book.metadata.title.is_empty(), "Title should not be empty");
    assert!(!book.metadata.authors.is_empty(), "Authors should not be empty");

    // Verify structure
    assert!(!book.spine.is_empty(), "Spine should not be empty");
    assert!(!book.resources.is_empty(), "Resources should not be empty");
}

#[test]
fn test_read_azw3_toc() {
    let path = fixture_path("epictetus.azw3");
    let book = read_mobi(&path).expect("Failed to read AZW3");

    assert!(!book.toc.is_empty(), "TOC should not be empty");
    println!("AZW3 TOC entries: {}", book.toc.len());
}

#[test]
fn test_azw3_links_valid() {
    let book = read_mobi(&fixture_path("epictetus.azw3")).expect("Failed to read AZW3");
    let errors = check_links(&book);
    assert!(errors.is_empty(), "Link validation errors: {:?}", errors);
}

// ============================================================================
// Clone/Copy Tests
// Ported from calibre's test_clone
// ============================================================================

#[test]
fn test_clone_epub() {
    let original = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Clone by writing and reading back
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let clone_path = temp_dir.path().join("clone.epub");

    write_epub(&original, &clone_path).expect("Failed to write clone");
    let cloned = read_epub(&clone_path).expect("Failed to read clone");

    // Verify metadata preserved
    assert_eq!(original.metadata.title, cloned.metadata.title);
    assert_eq!(original.metadata.authors, cloned.metadata.authors);
    assert_eq!(original.metadata.language, cloned.metadata.language);

    // Verify structure preserved
    assert_eq!(original.spine.len(), cloned.spine.len());
    assert_eq!(original.toc.len(), cloned.toc.len());
    assert_eq!(original.resources.len(), cloned.resources.len());
}

#[test]
fn test_clone_azw3() {
    let original = read_mobi(&fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let clone_path = temp_dir.path().join("clone.azw3");

    write_mobi(&original, &clone_path).expect("Failed to write clone");
    let cloned = read_mobi(&clone_path).expect("Failed to read clone");

    // Verify metadata preserved
    assert_eq!(original.metadata.title, cloned.metadata.title);
    assert_eq!(original.metadata.authors, cloned.metadata.authors);

    // Verify structure preserved
    assert_eq!(original.spine.len(), cloned.spine.len());
    assert_eq!(original.toc.len(), cloned.toc.len());
}

// ============================================================================
// Round-trip Tests
// ============================================================================

#[test]
fn test_epub_roundtrip() {
    let original = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.epub");

    write_epub(&original, &output_path).expect("Failed to write EPUB");
    let roundtrip = read_epub(&output_path).expect("Failed to read roundtrip");

    // Verify metadata preserved
    assert_eq!(original.metadata.title, roundtrip.metadata.title);
    assert_eq!(original.metadata.authors, roundtrip.metadata.authors);

    // Verify structure preserved
    assert_eq!(original.spine.len(), roundtrip.spine.len());
    assert_eq!(original.toc.len(), roundtrip.toc.len());

    // Verify links still valid
    let errors = check_links(&roundtrip);
    assert!(errors.is_empty(), "Roundtrip link errors: {:?}", errors);
}

#[test]
fn test_epub_multiple_roundtrips() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut path = fixture_path("epictetus.epub");

    // Read and write 3 times
    for i in 0..3 {
        let book = read_epub(&path).expect(&format!("Failed to read iteration {}", i));
        let new_path = temp_dir.path().join(format!("iter_{}.epub", i));
        write_epub(&book, &new_path).expect(&format!("Failed to write iteration {}", i));
        path = new_path.to_string_lossy().to_string();
    }

    // Final book should still be valid
    let final_book = read_epub(&path).expect("Failed to read final iteration");
    assert!(!final_book.metadata.title.is_empty());
    let errors = check_links(&final_book);
    assert!(errors.is_empty(), "Final iteration link errors: {:?}", errors);
}

// ============================================================================
// Cross-format Conversion Tests
// ============================================================================

#[test]
fn test_epub_azw3_metadata_equivalence() {
    let epub = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");
    let azw3 = read_mobi(&fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    // Metadata should match (both from same Standard Ebooks source)
    assert_eq!(epub.metadata.title, azw3.metadata.title);
    assert_eq!(epub.metadata.authors, azw3.metadata.authors);
}

#[test]
fn test_epub_to_azw3_conversion() {
    let epub = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let azw3_path = temp_dir.path().join("converted.azw3");

    write_mobi(&epub, &azw3_path).expect("Failed to write AZW3");
    let azw3 = read_mobi(&azw3_path).expect("Failed to read converted AZW3");

    // Verify metadata preserved
    assert_eq!(epub.metadata.title, azw3.metadata.title);
    assert_eq!(epub.metadata.authors, azw3.metadata.authors);

    // Content should be present
    assert!(!azw3.spine.is_empty());
}

#[test]
fn test_azw3_to_epub_conversion() {
    let azw3 = read_mobi(&fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let epub_path = temp_dir.path().join("converted.epub");

    write_epub(&azw3, &epub_path).expect("Failed to write EPUB");
    let epub = read_epub(&epub_path).expect("Failed to read converted EPUB");

    // Verify metadata preserved
    assert_eq!(azw3.metadata.title, epub.metadata.title);
    assert_eq!(azw3.metadata.authors, epub.metadata.authors);

    // Content should be present
    assert!(!epub.spine.is_empty());
    assert!(!epub.resources.is_empty());
}

#[test]
fn test_full_roundtrip_epub_azw3_epub() {
    // EPUB → AZW3 → EPUB
    let original = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read original EPUB");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // EPUB → AZW3
    let azw3_path = temp_dir.path().join("step1.azw3");
    write_mobi(&original, &azw3_path).expect("Failed to write AZW3");
    let azw3 = read_mobi(&azw3_path).expect("Failed to read AZW3");

    // AZW3 → EPUB
    let epub_path = temp_dir.path().join("step2.epub");
    write_epub(&azw3, &epub_path).expect("Failed to write EPUB");
    let final_epub = read_epub(&epub_path).expect("Failed to read final EPUB");

    // Verify metadata survived the journey
    assert_eq!(original.metadata.title, final_epub.metadata.title);
    assert_eq!(original.metadata.authors, final_epub.metadata.authors);
}

// ============================================================================
// Content Verification Tests
// ============================================================================

#[test]
fn test_epub_content_extraction() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Get all text content
    let mut total_text = String::new();
    for resource in book.resources.values() {
        if resource.media_type == "application/xhtml+xml" {
            let content = String::from_utf8_lossy(&resource.data);
            total_text.push_str(&content);
        }
    }

    // Epictetus's works should contain philosophical content
    assert!(
        total_text.contains("Epictetus") || total_text.contains("philosophy") || total_text.contains("Stoic"),
        "Expected Epictetus content not found"
    );
}

#[test]
fn test_azw3_content_extraction() {
    let book = read_mobi(&fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    // Get all text content
    let mut total_text = String::new();
    for resource in book.resources.values() {
        if resource.media_type == "application/xhtml+xml" || resource.media_type == "text/html" {
            let content = String::from_utf8_lossy(&resource.data);
            total_text.push_str(&content);
        }
    }

    // Should have actual content
    assert!(!total_text.is_empty(), "AZW3 should have extractable content");
}
