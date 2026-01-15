use ebookconvert::{read_epub, write_epub, Book, Metadata, TocEntry};
use tempfile::NamedTempFile;

const TEST_EPUB: &str = "/srv/books/Anne Bronte/Agnes Grey (669)/Agnes Grey - Anne Bronte.epub";

#[test]
fn test_read_epub() {
    let book = read_epub(TEST_EPUB).expect("Failed to read EPUB");

    // Check that we got metadata
    assert!(!book.metadata.title.is_empty(), "Title should not be empty");
    println!("Title: {}", book.metadata.title);
    println!("Authors: {:?}", book.metadata.authors);
    println!("Language: {}", book.metadata.language);

    // Check that we got resources
    assert!(!book.resources.is_empty(), "Should have resources");
    println!("Resources: {}", book.resources.len());

    // Check that we got spine items
    assert!(!book.spine.is_empty(), "Should have spine items");
    println!("Spine items: {}", book.spine.len());

    // Print TOC
    println!("TOC entries: {}", book.toc.len());
    for entry in &book.toc {
        println!("  - {} -> {}", entry.title, entry.href);
    }
}

#[test]
fn test_write_epub() {
    // Create a simple book
    let mut book = Book::new();
    book.metadata = Metadata::new("Test Book")
        .with_author("Test Author")
        .with_language("en")
        .with_identifier("test-id-12345");

    // Add a simple chapter
    let chapter_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
<h1>Chapter 1</h1>
<p>This is the first chapter.</p>
</body>
</html>"#;

    book.add_resource(
        "chapter1.xhtml",
        chapter_content.as_bytes().to_vec(),
        "application/xhtml+xml",
    );
    book.add_spine_item("chapter1", "chapter1.xhtml", "application/xhtml+xml");
    book.toc.push(TocEntry::new("Chapter 1", "chapter1.xhtml"));

    // Write to temp file
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write_epub(&book, temp_file.path()).expect("Failed to write EPUB");

    // Read it back
    let book2 = read_epub(temp_file.path()).expect("Failed to read written EPUB");

    assert_eq!(book2.metadata.title, "Test Book");
    assert_eq!(book2.metadata.authors, vec!["Test Author"]);
    assert_eq!(book2.metadata.language, "en");
    assert!(!book2.resources.is_empty());
}

#[test]
fn test_roundtrip_epub() {
    // Read an existing EPUB
    let book = read_epub(TEST_EPUB).expect("Failed to read EPUB");
    let original_title = book.metadata.title.clone();
    let original_resource_count = book.resources.len();

    // Write to temp file
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write_epub(&book, temp_file.path()).expect("Failed to write EPUB");

    // Read it back
    let book2 = read_epub(temp_file.path()).expect("Failed to read round-tripped EPUB");

    // Verify key properties are preserved
    assert_eq!(book2.metadata.title, original_title);
    assert_eq!(book2.resources.len(), original_resource_count);
}
