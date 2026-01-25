//! Integration tests for normalized export pipeline.

use std::io::Cursor;

use boko::export::{normalize_book, EpubConfig, EpubExporter, Exporter, GlobalStylePool};
use boko::ir::{ComputedStyle, FontWeight, IRChapter, StyleId};
use boko::Book;

// ============================================================================
// Unit Tests for GlobalStylePool
// ============================================================================

#[test]
fn test_global_style_pool_merge_deduplicates() {
    let mut global = GlobalStylePool::new();

    // Create two chapters with identical bold styles
    let mut chapter1 = IRChapter::new();
    let mut bold = ComputedStyle::default();
    bold.font_weight = FontWeight::BOLD;
    let bold_id1 = chapter1.styles.intern(bold.clone());

    let mut chapter2 = IRChapter::new();
    let bold_id2 = chapter2.styles.intern(bold);

    // Merge both chapters
    global.merge(0, &chapter1);
    global.merge(1, &chapter2);

    // Both should map to the same global ID
    let global_id1 = global.remap(0, bold_id1);
    let global_id2 = global.remap(1, bold_id2);
    assert_eq!(global_id1, global_id2);

    // Global pool should have exactly 2 styles (default + bold)
    assert_eq!(global.pool().len(), 2);
}

#[test]
fn test_global_style_pool_different_styles_get_different_ids() {
    let mut global = GlobalStylePool::new();

    let mut chapter1 = IRChapter::new();
    let mut bold = ComputedStyle::default();
    bold.font_weight = FontWeight::BOLD;
    let bold_id = chapter1.styles.intern(bold);

    let mut chapter2 = IRChapter::new();
    let mut italic = ComputedStyle::default();
    italic.font_style = boko::ir::FontStyle::Italic;
    let italic_id = chapter2.styles.intern(italic);

    global.merge(0, &chapter1);
    global.merge(1, &chapter2);

    let global_bold = global.remap(0, bold_id);
    let global_italic = global.remap(1, italic_id);

    // Different styles should get different global IDs
    assert_ne!(global_bold, global_italic);

    // Global pool should have 3 styles (default + bold + italic)
    assert_eq!(global.pool().len(), 3);
}

#[test]
fn test_global_style_pool_remap_unknown_returns_default() {
    let global = GlobalStylePool::new();

    // Unknown chapter/style should return default
    let result = global.remap(999, StyleId(999));
    assert_eq!(result, StyleId::DEFAULT);
}

#[test]
fn test_global_style_pool_used_styles() {
    let mut global = GlobalStylePool::new();

    let mut chapter = IRChapter::new();
    let mut bold = ComputedStyle::default();
    bold.font_weight = FontWeight::BOLD;
    chapter.styles.intern(bold);

    global.merge(0, &chapter);

    let used = global.used_styles();
    assert!(!used.is_empty());
    // Should contain at least default and bold
    assert!(used.len() >= 2);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_normalized_epub_export_produces_valid_output() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let mut output = Cursor::new(Vec::new());

    let exporter = EpubExporter::new().with_config(EpubConfig {
        normalize: true,
        ..Default::default()
    });

    exporter
        .export(&mut book, &mut output)
        .expect("Normalized export failed");

    // Verify output is not empty
    let data = output.into_inner();
    assert!(!data.is_empty(), "Exported EPUB should not be empty");

    // Verify it's a valid ZIP (starts with PK)
    assert!(
        data.starts_with(b"PK"),
        "Exported EPUB should be a valid ZIP file"
    );
}

#[test]
fn test_normalize_book_produces_content() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let content = normalize_book(&mut book).expect("normalize_book failed");

    // Should have chapters
    assert!(!content.chapters.is_empty(), "Should have normalized chapters");

    // Each chapter should have content
    for chapter in &content.chapters {
        assert!(!chapter.document.is_empty(), "Chapter document should not be empty");
        assert!(
            chapter.document.contains("<!DOCTYPE"),
            "Chapter should be valid XHTML"
        );
        assert!(
            chapter.document.contains("<html"),
            "Chapter should contain html element"
        );
    }
}

#[test]
fn test_normalized_export_includes_stylesheet_reference() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let content = normalize_book(&mut book).expect("normalize_book failed");

    // Check that chapters reference the stylesheet
    for chapter in &content.chapters {
        assert!(
            chapter.document.contains("style.css"),
            "Chapter should reference style.css"
        );
    }
}

#[test]
fn test_normalized_epub_contains_style_css() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let mut output = Cursor::new(Vec::new());

    let exporter = EpubExporter::new().with_config(EpubConfig {
        normalize: true,
        ..Default::default()
    });

    exporter
        .export(&mut book, &mut output)
        .expect("Normalized export failed");

    // Read the ZIP and verify style.css exists
    let data = output.into_inner();
    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).expect("Failed to read ZIP");

    let mut found_style = false;
    for i in 0..archive.len() {
        let file = archive.by_index(i).expect("Failed to read ZIP entry");
        if file.name().ends_with("style.css") {
            found_style = true;
            break;
        }
    }

    assert!(found_style, "Normalized EPUB should contain style.css");
}

#[test]
fn test_normalized_export_has_numbered_chapters() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    // Export normalized
    let mut norm_output = Cursor::new(Vec::new());
    let norm_exporter = EpubExporter::new().with_config(EpubConfig {
        normalize: true,
        ..Default::default()
    });
    norm_exporter
        .export(&mut book, &mut norm_output)
        .expect("Normalized export failed");

    let norm_data = norm_output.into_inner();
    assert!(!norm_data.is_empty());

    // Normalized should have chapter_0.xhtml, chapter_1.xhtml, etc.
    let norm_reader = Cursor::new(norm_data);
    let mut norm_archive = zip::ZipArchive::new(norm_reader).expect("Failed to read norm ZIP");

    let mut has_chapter_0 = false;
    for i in 0..norm_archive.len() {
        let file = norm_archive.by_index(i).expect("Failed to read ZIP entry");
        if file.name().contains("chapter_0.xhtml") {
            has_chapter_0 = true;
            break;
        }
    }

    assert!(
        has_chapter_0,
        "Normalized EPUB should have numbered chapter files"
    );
}

#[test]
fn test_azw3_to_normalized_epub() {
    let mut book = Book::open("tests/fixtures/epictetus.azw3").expect("Failed to open AZW3 book");

    let mut output = Cursor::new(Vec::new());

    let exporter = EpubExporter::new().with_config(EpubConfig {
        normalize: true,
        ..Default::default()
    });

    exporter
        .export(&mut book, &mut output)
        .expect("AZW3 to normalized EPUB export failed");

    let data = output.into_inner();
    assert!(!data.is_empty(), "Exported EPUB should not be empty");
    assert!(
        data.starts_with(b"PK"),
        "Exported EPUB should be a valid ZIP file"
    );
}

#[test]
fn test_book_cache_works() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let spine: Vec<_> = book.spine().to_vec();
    assert!(!spine.is_empty(), "Book should have spine entries");

    // Load same chapter twice with cache
    let chapter1 = book
        .load_chapter_cached(spine[0].id)
        .expect("First load failed");
    let chapter2 = book
        .load_chapter_cached(spine[0].id)
        .expect("Second load failed");

    // Both should have the same structure
    assert_eq!(chapter1.node_count(), chapter2.node_count());

    // Clear cache should work
    book.clear_cache();
}
