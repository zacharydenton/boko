//! Parsing tests ported from calibre's polish tests.
//!
//! These tests verify HTML/XHTML parsing and content handling
//! using the Standard Ebooks edition of Epictetus's "Short Works".
//!
//! Original calibre tests: calibre/src/calibre/ebooks/oeb/polish/tests/parsing.py

use boko::{read_epub, read_mobi};

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture_path(name: &str) -> String {
    format!("{}/{}", FIXTURES_DIR, name)
}

// ============================================================================
// XHTML Parsing Tests
// Ported from calibre's ParsingTests
// ============================================================================

#[test]
fn test_epub_xhtml_wellformed() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // All XHTML resources should be parseable
    for (href, resource) in &book.resources {
        if resource.media_type == "application/xhtml+xml" {
            let content = String::from_utf8_lossy(&resource.data);

            // Basic well-formedness checks
            assert!(
                content.contains("<html") || content.contains("<HTML"),
                "XHTML {} should have html element",
                href
            );
            assert!(
                content.contains("<body") || content.contains("<BODY"),
                "XHTML {} should have body element",
                href
            );
        }
    }
}

#[test]
fn test_epub_xhtml_encoding() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Content should be valid UTF-8 (we already read it successfully)
    for (href, resource) in &book.resources {
        if resource.media_type == "application/xhtml+xml" {
            // Should be valid UTF-8
            let result = String::from_utf8(resource.data.clone());
            assert!(
                result.is_ok(),
                "XHTML {} should be valid UTF-8",
                href
            );
        }
    }
}

#[test]
fn test_epub_namespaces() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Standard Ebooks use proper XHTML namespaces
    for (href, resource) in &book.resources {
        if resource.media_type == "application/xhtml+xml" {
            let content = String::from_utf8_lossy(&resource.data);

            // XHTML namespace should be present
            if content.contains("xmlns") {
                assert!(
                    content.contains("http://www.w3.org/1999/xhtml") ||
                    content.contains("w3.org/1999/xhtml"),
                    "XHTML {} should have XHTML namespace",
                    href
                );
            }
        }
    }
}

// ============================================================================
// Entity Handling Tests
// Ported from calibre's entities test
// ============================================================================

#[test]
fn test_epub_entities() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Get all text content
    let mut all_content = String::new();
    for resource in book.resources.values() {
        if resource.media_type == "application/xhtml+xml" {
            let content = String::from_utf8_lossy(&resource.data);
            all_content.push_str(&content);
        }
    }

    // Common entities should be handled (either encoded or as characters)
    // Non-breaking space can appear as &nbsp; or \u{00a0}
    // The content should not have broken entity references
    assert!(
        !all_content.contains("&amp;nbsp;"),
        "Double-encoded entities found"
    );
}

// ============================================================================
// Content Integrity Tests
// ============================================================================

#[test]
fn test_epub_content_integrity() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Each XHTML document should have proper structure
    for (href, resource) in &book.resources {
        if resource.media_type == "application/xhtml+xml" {
            let content = String::from_utf8_lossy(&resource.data);

            // Should have matching tags (basic check)
            let open_html = content.matches("<html").count() + content.matches("<HTML").count();
            let close_html = content.matches("</html").count() + content.matches("</HTML").count();
            assert_eq!(
                open_html, close_html,
                "Mismatched html tags in {}",
                href
            );

            let open_body = content.matches("<body").count() + content.matches("<BODY").count();
            let close_body = content.matches("</body").count() + content.matches("</BODY").count();
            assert_eq!(
                open_body, close_body,
                "Mismatched body tags in {}",
                href
            );
        }
    }
}

#[test]
fn test_azw3_content_integrity() {
    let book = read_mobi(&fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    // AZW3 extracted content should be valid HTML
    for (href, resource) in &book.resources {
        if resource.media_type == "application/xhtml+xml" || resource.media_type == "text/html" {
            let content = String::from_utf8_lossy(&resource.data);

            // Should have body content
            assert!(
                content.contains("<body") || content.contains("<BODY") ||
                content.contains("<div") || content.contains("<p"),
                "AZW3 content {} should have body elements",
                href
            );
        }
    }
}

// ============================================================================
// CSS Parsing Tests
// ============================================================================

#[test]
fn test_epub_css_present() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let css_resources: Vec<_> = book
        .resources
        .iter()
        .filter(|(_, r)| r.media_type == "text/css")
        .collect();

    assert!(!css_resources.is_empty(), "Standard Ebooks include CSS");

    for (href, resource) in css_resources {
        let content = String::from_utf8_lossy(&resource.data);
        println!("CSS file: {} ({} bytes)", href, resource.data.len());

        // CSS should have some rules
        assert!(
            content.contains('{') && content.contains('}'),
            "CSS {} should have rule blocks",
            href
        );
    }
}

#[test]
fn test_epub_css_references() {
    let book = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    // Find CSS references in XHTML files
    let mut css_refs = Vec::new();
    for (href, resource) in &book.resources {
        if resource.media_type == "application/xhtml+xml" {
            let content = String::from_utf8_lossy(&resource.data);

            // Look for stylesheet links
            if content.contains("stylesheet") {
                css_refs.push(href.clone());
            }
        }
    }

    println!("Files with CSS references: {:?}", css_refs);
}

// ============================================================================
// Roundtrip Parsing Tests
// ============================================================================

#[test]
fn test_epub_roundtrip_preserves_content() {
    use tempfile::TempDir;

    let original = read_epub(&fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.epub");

    boko::write_epub(&original, &output_path).expect("Failed to write EPUB");
    let roundtrip = read_epub(&output_path).expect("Failed to read roundtrip");

    // Compare XHTML content
    for (href, orig_resource) in &original.resources {
        if orig_resource.media_type == "application/xhtml+xml" {
            if let Some(rt_resource) = roundtrip.resources.get(href) {
                // Content should be similar (might have minor serialization differences)
                let orig_len = orig_resource.data.len();
                let rt_len = rt_resource.data.len();

                // Allow some variance in size due to serialization
                let size_ratio = (rt_len as f64) / (orig_len as f64);
                assert!(
                    size_ratio > 0.8 && size_ratio < 1.2,
                    "Content size changed significantly for {}: {} -> {}",
                    href,
                    orig_len,
                    rt_len
                );
            }
        }
    }
}

#[test]
fn test_azw3_roundtrip_preserves_content() {
    use tempfile::TempDir;

    let original = read_mobi(&fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.azw3");

    boko::write_mobi(&original, &output_path).expect("Failed to write AZW3");
    let roundtrip = read_mobi(&output_path).expect("Failed to read roundtrip");

    // Verify content count matches
    let orig_html_count = original
        .resources
        .values()
        .filter(|r| r.media_type == "application/xhtml+xml" || r.media_type == "text/html")
        .count();

    let rt_html_count = roundtrip
        .resources
        .values()
        .filter(|r| r.media_type == "application/xhtml+xml" || r.media_type == "text/html")
        .count();

    assert_eq!(
        orig_html_count, rt_html_count,
        "HTML document count should be preserved"
    );
}
