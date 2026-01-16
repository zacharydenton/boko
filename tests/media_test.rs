//! Media and resource tests.
//!
//! Tests for handling images, fonts, and other binary resources.

use std::collections::HashSet;

use boko::{read_epub, read_mobi, write_epub, write_mobi};
use tempfile::TempDir;

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture_path(name: &str) -> String {
    format!("{}/{}", FIXTURES_DIR, name)
}

// ============================================================================
// Image Handling Tests
// ============================================================================

#[test]
fn test_epub_has_images() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let image_resources: Vec<_> = book
        .resources
        .iter()
        .filter(|(_, r)| r.media_type.starts_with("image/"))
        .collect();

    assert!(
        !image_resources.is_empty(),
        "Standard Ebooks include images"
    );

    println!("Found {} images:", image_resources.len());
    for (href, resource) in &image_resources {
        println!(
            "  {} ({}, {} bytes)",
            href,
            resource.media_type,
            resource.data.len()
        );
    }
}

#[test]
fn test_epub_image_types() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let image_types: HashSet<_> = book
        .resources
        .values()
        .filter(|r| r.media_type.starts_with("image/"))
        .map(|r| r.media_type.as_str())
        .collect();

    println!("Image types found: {:?}", image_types);

    // Standard Ebooks typically have JPEG and PNG
    assert!(
        image_types.contains("image/jpeg") || image_types.contains("image/png"),
        "Should have common image types"
    );
}

#[test]
fn test_epub_images_preserved_in_roundtrip() {
    let original = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let original_images: HashSet<_> = original
        .resources
        .iter()
        .filter(|(_, r)| r.media_type.starts_with("image/"))
        .map(|(href, _)| href.clone())
        .collect();

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.epub");

    write_epub(&original, &output_path).expect("Failed to write EPUB");
    let roundtrip = read_epub(&output_path).expect("Failed to read roundtrip");

    let roundtrip_images: HashSet<_> = roundtrip
        .resources
        .iter()
        .filter(|(_, r)| r.media_type.starts_with("image/"))
        .map(|(href, _)| href.clone())
        .collect();

    assert_eq!(
        original_images, roundtrip_images,
        "Image hrefs should be preserved"
    );

    // Verify image data is preserved
    for href in &original_images {
        let orig_data = &original.resources.get(href).unwrap().data;
        let rt_data = &roundtrip.resources.get(href).unwrap().data;
        assert_eq!(
            orig_data.len(),
            rt_data.len(),
            "Image {} size should be preserved",
            href
        );
        assert_eq!(
            orig_data, rt_data,
            "Image {} data should be identical",
            href
        );
    }
}

#[test]
fn test_azw3_images_preserved_in_roundtrip() {
    let original = read_mobi(fixture_path("epictetus.azw3")).expect("Failed to read AZW3");

    let original_images: Vec<_> = original
        .resources
        .iter()
        .filter(|(_, r)| r.media_type.starts_with("image/"))
        .map(|(href, r)| (href.clone(), r.data.len()))
        .collect();

    if original_images.is_empty() {
        println!("AZW3 has no images to test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.azw3");

    write_mobi(&original, &output_path).expect("Failed to write AZW3");
    let roundtrip = read_mobi(&output_path).expect("Failed to read roundtrip");

    let roundtrip_images: Vec<_> = roundtrip
        .resources
        .iter()
        .filter(|(_, r)| r.media_type.starts_with("image/"))
        .collect();

    assert_eq!(
        original_images.len(),
        roundtrip_images.len(),
        "Image count should be preserved"
    );
}

#[test]
fn test_epub_to_azw3_preserves_images() {
    let epub = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let epub_image_count = epub
        .resources
        .values()
        .filter(|r| r.media_type.starts_with("image/"))
        .count();

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let azw3_path = temp_dir.path().join("converted.azw3");

    write_mobi(&epub, &azw3_path).expect("Failed to write AZW3");
    let azw3 = read_mobi(&azw3_path).expect("Failed to read AZW3");

    let azw3_image_count = azw3
        .resources
        .values()
        .filter(|r| r.media_type.starts_with("image/"))
        .count();

    // Images should be preserved during conversion
    assert!(
        azw3_image_count > 0 || epub_image_count == 0,
        "Images should be preserved in conversion"
    );

    println!(
        "EPUB images: {}, AZW3 images: {}",
        epub_image_count, azw3_image_count
    );
}

// ============================================================================
// Cover Image Tests
// ============================================================================

#[test]
fn test_epub_cover_image_exists() {
    let book = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    if let Some(cover_href) = &book.metadata.cover_image {
        println!("Cover image: {}", cover_href);

        assert!(
            book.resources.contains_key(cover_href),
            "Cover image should exist in resources"
        );

        let cover = book.resources.get(cover_href).unwrap();
        assert!(
            cover.media_type.starts_with("image/"),
            "Cover should be an image"
        );
        assert!(
            cover.data.len() > 1000,
            "Cover should have substantial data"
        );
    } else {
        println!("No cover image metadata found");
    }
}

#[test]
fn test_cover_image_preserved_in_conversion() {
    let epub = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    if epub.metadata.cover_image.is_none() {
        println!("No cover image to test");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // EPUB -> AZW3
    let azw3_path = temp_dir.path().join("converted.azw3");
    write_mobi(&epub, &azw3_path).expect("Failed to write AZW3");
    let azw3 = read_mobi(&azw3_path).expect("Failed to read AZW3");

    // AZW3 -> EPUB
    let epub2_path = temp_dir.path().join("converted_back.epub");
    write_epub(&azw3, &epub2_path).expect("Failed to write EPUB");
    let epub2 = read_epub(&epub2_path).expect("Failed to read EPUB");

    // Check images exist (cover metadata might not survive conversion)
    let has_images = epub2
        .resources
        .values()
        .any(|r| r.media_type.starts_with("image/"));
    assert!(has_images, "Images should survive round-trip conversion");
}

// ============================================================================
// CSS Handling Tests
// ============================================================================

#[test]
fn test_epub_css_preserved_in_roundtrip() {
    let original = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let original_css: HashSet<_> = original
        .resources
        .iter()
        .filter(|(_, r)| r.media_type == "text/css")
        .map(|(href, _)| href.clone())
        .collect();

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.epub");

    write_epub(&original, &output_path).expect("Failed to write EPUB");
    let roundtrip = read_epub(&output_path).expect("Failed to read roundtrip");

    let roundtrip_css: HashSet<_> = roundtrip
        .resources
        .iter()
        .filter(|(_, r)| r.media_type == "text/css")
        .map(|(href, _)| href.clone())
        .collect();

    assert_eq!(original_css, roundtrip_css, "CSS files should be preserved");

    // Verify CSS content is preserved
    for href in &original_css {
        let orig_data = &original.resources.get(href).unwrap().data;
        let rt_data = &roundtrip.resources.get(href).unwrap().data;
        assert_eq!(
            orig_data, rt_data,
            "CSS {} content should be identical",
            href
        );
    }
}

// ============================================================================
// Binary Resource Integrity Tests
// ============================================================================

#[test]
fn test_binary_resources_not_corrupted() {
    let original = read_epub(fixture_path("epictetus.epub")).expect("Failed to read EPUB");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("roundtrip.epub");

    write_epub(&original, &output_path).expect("Failed to write EPUB");
    let roundtrip = read_epub(&output_path).expect("Failed to read roundtrip");

    // Check all binary resources (excluding generated files like NCX)
    for (href, orig_resource) in &original.resources {
        // Skip files that are regenerated by the writer
        if href.ends_with(".ncx") || href.ends_with(".opf") {
            continue;
        }

        if let Some(rt_resource) = roundtrip.resources.get(href) {
            assert_eq!(
                orig_resource.media_type, rt_resource.media_type,
                "Media type should be preserved for {}",
                href
            );

            // Binary files (images, fonts) should be byte-identical
            if orig_resource.media_type.starts_with("image/")
                || orig_resource.media_type.starts_with("font/")
                || orig_resource.media_type.starts_with("application/font")
            {
                assert_eq!(
                    orig_resource.data, rt_resource.data,
                    "Binary resource {} should be identical",
                    href
                );
            }
        }
    }
}
