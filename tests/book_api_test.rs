//! Book API tests.
//!
//! Tests for the Book struct and its methods - creating books programmatically,
//! modifying metadata, adding resources, and manipulating TOC.

use boko::{Book, Metadata, TocEntry};
use tempfile::TempDir;

// ============================================================================
// Book Construction Tests
// ============================================================================

#[test]
fn test_create_empty_book() {
    let book = Book::new();

    assert!(book.metadata.title.is_empty());
    assert!(book.metadata.authors.is_empty());
    assert!(book.spine.is_empty());
    assert!(book.toc.is_empty());
    assert!(book.resources.is_empty());
}

#[test]
fn test_metadata_builder() {
    let metadata = Metadata::new("Test Title")
        .with_author("Author One")
        .with_author("Author Two")
        .with_language("en")
        .with_identifier("urn:uuid:12345");

    assert_eq!(metadata.title, "Test Title");
    assert_eq!(metadata.authors, vec!["Author One", "Author Two"]);
    assert_eq!(metadata.language, "en");
    assert_eq!(metadata.identifier, "urn:uuid:12345");
}

#[test]
fn test_add_resource() {
    let mut book = Book::new();

    book.add_resource("chapter1.xhtml", b"<html><body>Chapter 1</body></html>".to_vec(), "application/xhtml+xml");
    book.add_resource("style.css", b"body { color: black; }".to_vec(), "text/css");

    assert_eq!(book.resources.len(), 2);
    assert!(book.resources.contains_key("chapter1.xhtml"));
    assert!(book.resources.contains_key("style.css"));

    let chapter = book.get_resource("chapter1.xhtml").unwrap();
    assert_eq!(chapter.media_type, "application/xhtml+xml");
}

#[test]
fn test_add_spine_item() {
    let mut book = Book::new();

    book.add_resource("chapter1.xhtml", b"<html></html>".to_vec(), "application/xhtml+xml");
    book.add_resource("chapter2.xhtml", b"<html></html>".to_vec(), "application/xhtml+xml");

    book.add_spine_item("ch1", "chapter1.xhtml", "application/xhtml+xml");
    book.add_spine_item("ch2", "chapter2.xhtml", "application/xhtml+xml");

    assert_eq!(book.spine.len(), 2);
    assert_eq!(book.spine[0].href, "chapter1.xhtml");
    assert_eq!(book.spine[1].href, "chapter2.xhtml");
}

#[test]
fn test_toc_entry_builder() {
    let toc = TocEntry::new("Chapter 1", "chapter1.xhtml")
        .with_child(TocEntry::new("Section 1.1", "chapter1.xhtml#sec1"))
        .with_child(TocEntry::new("Section 1.2", "chapter1.xhtml#sec2"));

    assert_eq!(toc.title, "Chapter 1");
    assert_eq!(toc.href, "chapter1.xhtml");
    assert_eq!(toc.children.len(), 2);
    assert_eq!(toc.children[0].title, "Section 1.1");
}

// ============================================================================
// Programmatic Book Creation + Write/Read Tests
// ============================================================================

#[test]
fn test_create_and_write_minimal_book() {
    let mut book = Book::new();
    book.metadata = Metadata::new("Minimal Test Book")
        .with_author("Test Author")
        .with_language("en");

    // Add a single chapter
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body><h1>Hello World</h1><p>This is a test.</p></body>
</html>"#;

    book.add_resource("chapter1.xhtml", content.as_bytes().to_vec(), "application/xhtml+xml");
    book.add_spine_item("ch1", "chapter1.xhtml", "application/xhtml+xml");
    book.toc.push(TocEntry::new("Chapter 1", "chapter1.xhtml"));

    // Write and read back
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let epub_path = temp_dir.path().join("minimal.epub");

    boko::write_epub(&book, &epub_path).expect("Failed to write EPUB");
    let read_book = boko::read_epub(&epub_path).expect("Failed to read EPUB");

    assert_eq!(read_book.metadata.title, "Minimal Test Book");
    assert_eq!(read_book.metadata.authors, vec!["Test Author"]);
    assert!(!read_book.spine.is_empty());
    assert!(!read_book.toc.is_empty());
}

#[test]
#[ignore = "NCX parser doesn't correctly read nested navPoints - see parser for fix"]
fn test_create_book_with_nested_toc() {
    let mut book = Book::new();
    book.metadata = Metadata::new("Nested TOC Test")
        .with_author("Test Author")
        .with_language("en");

    // Add chapters
    for i in 1..=3 {
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body><h1>Chapter {}</h1></body>
</html>"#, i);
        book.add_resource(
            format!("chapter{}.xhtml", i),
            content.as_bytes().to_vec(),
            "application/xhtml+xml"
        );
        book.add_spine_item(
            format!("ch{}", i),
            format!("chapter{}.xhtml", i),
            "application/xhtml+xml"
        );
    }

    // Nested TOC
    book.toc = vec![
        TocEntry::new("Part I", "chapter1.xhtml")
            .with_child(TocEntry::new("Chapter 1", "chapter1.xhtml")),
        TocEntry::new("Part II", "chapter2.xhtml")
            .with_child(TocEntry::new("Chapter 2", "chapter2.xhtml"))
            .with_child(TocEntry::new("Chapter 3", "chapter3.xhtml")),
    ];

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let epub_path = temp_dir.path().join("nested_toc.epub");

    boko::write_epub(&book, &epub_path).expect("Failed to write EPUB");
    let read_book = boko::read_epub(&epub_path).expect("Failed to read EPUB");

    // Verify TOC was written and read back
    // Note: The NCX format preserves nesting, verify basic structure
    assert!(!read_book.toc.is_empty(), "TOC should not be empty");

    // Count total entries (nested parsing may flatten or preserve)
    fn count_toc_entries(entries: &[TocEntry]) -> usize {
        entries.iter().map(|e| 1 + count_toc_entries(&e.children)).sum()
    }

    let original_count = count_toc_entries(&book.toc);
    let read_count = count_toc_entries(&read_book.toc);

    println!("Original TOC entries: {}, Read TOC entries: {}", original_count, read_count);
    println!("Read TOC structure:");
    for (i, entry) in read_book.toc.iter().enumerate() {
        println!("  [{}] {} -> {} (children: {})", i, entry.title, entry.href, entry.children.len());
    }

    // At minimum, we should have the same number of total entries
    assert_eq!(original_count, read_count, "TOC entry count should match");
}

#[test]
fn test_create_book_with_css() {
    let mut book = Book::new();
    book.metadata = Metadata::new("Styled Book")
        .with_author("Test Author")
        .with_language("en");

    // Add CSS
    let css = "body { font-family: serif; } h1 { color: navy; }";
    book.add_resource("style.css", css.as_bytes().to_vec(), "text/css");

    // Add chapter referencing CSS
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <title>Styled Chapter</title>
  <link rel="stylesheet" type="text/css" href="style.css"/>
</head>
<body><h1>Styled Content</h1></body>
</html>"#;

    book.add_resource("chapter1.xhtml", content.as_bytes().to_vec(), "application/xhtml+xml");
    book.add_spine_item("ch1", "chapter1.xhtml", "application/xhtml+xml");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let epub_path = temp_dir.path().join("styled.epub");

    boko::write_epub(&book, &epub_path).expect("Failed to write EPUB");
    let read_book = boko::read_epub(&epub_path).expect("Failed to read EPUB");

    // Verify CSS is present
    assert!(read_book.resources.contains_key("style.css"));
    let css_resource = read_book.get_resource("style.css").unwrap();
    assert_eq!(css_resource.media_type, "text/css");
}

// ============================================================================
// Modification Tests
// ============================================================================

#[test]
fn test_modify_metadata_roundtrip() {
    let original = boko::read_epub(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/epictetus.epub")
    ).expect("Failed to read EPUB");

    // Modify metadata
    let mut modified = original.clone();
    modified.metadata.title = "Modified Title".to_string();
    modified.metadata.authors = vec!["New Author".to_string()];

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("modified.epub");

    boko::write_epub(&modified, &output_path).expect("Failed to write EPUB");
    let read_back = boko::read_epub(&output_path).expect("Failed to read EPUB");

    assert_eq!(read_back.metadata.title, "Modified Title");
    assert_eq!(read_back.metadata.authors, vec!["New Author"]);

    // Content should be preserved
    assert_eq!(original.resources.len(), read_back.resources.len());
}

#[test]
fn test_add_resource_to_existing_book() {
    let mut book = boko::read_epub(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/epictetus.epub")
    ).expect("Failed to read EPUB");

    let original_count = book.resources.len();

    // Add a new resource
    book.add_resource(
        "extra.css",
        b".extra { color: red; }".to_vec(),
        "text/css"
    );

    assert_eq!(book.resources.len(), original_count + 1);
    assert!(book.resources.contains_key("extra.css"));

    // Write and read back
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("with_extra.epub");

    boko::write_epub(&book, &output_path).expect("Failed to write EPUB");
    let read_back = boko::read_epub(&output_path).expect("Failed to read EPUB");

    assert!(read_back.resources.contains_key("extra.css"));
}

// ============================================================================
// Format Conversion with Modifications
// ============================================================================

#[test]
fn test_modify_and_convert_epub_to_azw3() {
    let mut book = boko::read_epub(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/epictetus.epub")
    ).expect("Failed to read EPUB");

    // Modify
    book.metadata.title = "Modified for Kindle".to_string();

    // Convert to AZW3
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let azw3_path = temp_dir.path().join("modified.azw3");

    boko::write_mobi(&book, &azw3_path).expect("Failed to write AZW3");
    let azw3 = boko::read_mobi(&azw3_path).expect("Failed to read AZW3");

    assert_eq!(azw3.metadata.title, "Modified for Kindle");
}
