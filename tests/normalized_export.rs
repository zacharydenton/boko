//! Integration tests for normalized export pipeline.

use std::io::Cursor;
use std::sync::Arc;

use boko::Book;
use boko::export::{EpubConfig, EpubExporter, Exporter, GlobalStylePool, normalize_book};
use boko::model::Chapter;
use boko::style::{ComputedStyle, FontStyle, FontWeight, StyleId};

// ============================================================================
// Unit Tests for GlobalStylePool
// ============================================================================

#[test]
fn test_global_style_pool_merge_deduplicates() {
    let mut global = GlobalStylePool::new();

    // Create two chapters with identical bold styles
    let mut chapter1 = Chapter::new();
    let bold = ComputedStyle {
        font_weight: FontWeight::BOLD,
        ..Default::default()
    };
    let bold_id1 = chapter1.styles.intern(bold.clone());

    let mut chapter2 = Chapter::new();
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

    let mut chapter1 = Chapter::new();
    let bold = ComputedStyle {
        font_weight: FontWeight::BOLD,
        ..Default::default()
    };
    let bold_id = chapter1.styles.intern(bold);

    let mut chapter2 = Chapter::new();
    let italic = ComputedStyle {
        font_style: FontStyle::Italic,
        ..Default::default()
    };
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
fn test_global_style_pool_used_styles_dedupes_and_sorts() {
    let mut global = GlobalStylePool::new();

    let mut chapter1 = Chapter::new();
    let bold = ComputedStyle {
        font_weight: FontWeight::BOLD,
        ..Default::default()
    };
    let bold_id1 = chapter1.styles.intern(bold.clone());

    let mut chapter2 = Chapter::new();
    let bold_id2 = chapter2.styles.intern(bold);

    global.merge(0, &chapter1);
    global.merge(1, &chapter2);

    let used = global.used_styles();

    // Should include default and a single bold style, sorted by StyleId.
    assert!(used.contains(&StyleId::DEFAULT));
    assert_eq!(used.len(), 2);

    let global_bold1 = global.remap(0, bold_id1);
    let global_bold2 = global.remap(1, bold_id2);
    assert_eq!(global_bold1, global_bold2);
    assert_eq!(used, vec![StyleId::DEFAULT, global_bold1]);
}

// ============================================================================
// Integration Tests
// ============================================================================

fn extract_style_classes(document: &str) -> Vec<String> {
    let mut classes = Vec::new();
    let mut rest = document;

    while let Some(idx) = rest.find("class=\"") {
        rest = &rest[idx + 7..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let class_attr = &rest[..end];
        for token in class_attr.split_whitespace() {
            if token.starts_with('c')
                && token[1..].chars().all(|c| c.is_ascii_digit())
            {
                classes.push(token.to_string());
            }
        }
        rest = &rest[end + 1..];
    }

    classes
}

#[test]
fn test_normalize_book_emits_css_for_used_classes() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let content = normalize_book(&mut book).expect("normalize_book failed");

    // Ensure chapters reference the unified stylesheet and CSS matches class usage.
    let mut all_classes = Vec::new();
    for chapter in &content.chapters {
        assert!(
            chapter.document.contains("style.css"),
            "Chapter should reference style.css"
        );
        all_classes.extend(extract_style_classes(&chapter.document));
    }

    assert!(
        !all_classes.is_empty(),
        "Expected normalized XHTML to reference at least one style class"
    );

    for class_name in all_classes {
        let needle = format!(".{}", class_name);
        assert!(
            content.css.contains(&needle),
            "CSS should contain rule for class {}",
            class_name
        );
    }
}

#[test]
fn test_normalized_export_contains_css_and_numbered_chapters() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let mut output = Cursor::new(Vec::new());

    let exporter = EpubExporter::new().with_config(EpubConfig {
        normalize: true,
        ..Default::default()
    });

    exporter
        .export(&mut book, &mut output)
        .expect("Normalized export failed");

    let data = output.into_inner();
    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).expect("Failed to read ZIP");

    let mut found_style = false;
    let mut found_chapter_0 = false;
    for i in 0..archive.len() {
        let file = archive.by_index(i).expect("Failed to read ZIP entry");
        if file.name().ends_with("style.css") {
            found_style = true;
        }
        if file.name().contains("chapter_0.xhtml") {
            found_chapter_0 = true;
        }
    }

    assert!(found_style, "Normalized EPUB should contain style.css");
    assert!(
        found_chapter_0,
        "Normalized EPUB should have numbered chapter files"
    );
}

#[test]
fn test_load_chapter_cached_returns_same_arc() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").expect("Failed to open test book");

    let spine: Vec<_> = book.spine().to_vec();
    assert!(!spine.is_empty(), "Book should have spine entries");

    let chapter1 = book
        .load_chapter_cached(spine[0].id)
        .expect("First load failed");
    let chapter2 = book
        .load_chapter_cached(spine[0].id)
        .expect("Second load failed");

    assert!(
        Arc::ptr_eq(&chapter1, &chapter2),
        "Expected cached load to return the same Arc"
    );
}
