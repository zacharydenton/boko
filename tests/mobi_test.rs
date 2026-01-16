use boko::{read_mobi, write_epub, write_mobi};
use tempfile::NamedTempFile;

const TEST_AZW3: &str = "/srv/books/Vernor Vinge/True Names (34)/True Names - Vernor Vinge.azw3";

#[test]
fn test_read_azw3() {
    let book = read_mobi(TEST_AZW3).expect("Failed to read AZW3");

    assert_eq!(book.metadata.title, "True Names");
    assert!(book.metadata.authors.contains(&"Vernor Vinge".to_string()));
    assert!(!book.resources.is_empty());
    assert!(!book.spine.is_empty());

    println!("Title: {}", book.metadata.title);
    println!("Authors: {:?}", book.metadata.authors);
    println!("Resources: {}", book.resources.len());
}

#[test]
fn test_azw3_to_epub_conversion() {
    // Read AZW3
    let book = read_mobi(TEST_AZW3).expect("Failed to read AZW3");

    // Write to EPUB
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write_epub(&book, temp_file.path()).expect("Failed to write EPUB");

    // Read it back as EPUB
    let book2 = boko::read_epub(temp_file.path()).expect("Failed to read converted EPUB");

    // Verify metadata preserved
    assert_eq!(book2.metadata.title, "True Names");
    assert!(book2.metadata.authors.contains(&"Vernor Vinge".to_string()));
}

#[test]
fn test_read_multiple_formats() {
    // Try reading different AZW3 files
    let files = [
        "/srv/books/Vernor Vinge/True Names (34)/True Names - Vernor Vinge.azw3",
        "/srv/books/Lily Mara/Refactoring to Rust (1826)/Refactoring to Rust - Lily Mara.azw3",
    ];

    for path in files {
        if std::path::Path::new(path).exists() {
            match read_mobi(path) {
                Ok(book) => {
                    println!("Successfully read: {}", book.metadata.title);
                    assert!(!book.metadata.title.is_empty());
                }
                Err(e) => {
                    println!("Failed to read {}: {}", path, e);
                }
            }
        }
    }
}

#[test]
fn test_write_mobi() {
    // Read AZW3
    let book = read_mobi(TEST_AZW3).expect("Failed to read AZW3");

    // Write to MOBI/AZW3
    let temp_file = NamedTempFile::with_suffix(".azw3").expect("Failed to create temp file");
    write_mobi(&book, temp_file.path()).expect("Failed to write MOBI");

    // Read it back
    let book2 = read_mobi(temp_file.path()).expect("Failed to read written MOBI");

    // Verify metadata preserved
    assert_eq!(book2.metadata.title, "True Names");
    assert!(book2.metadata.authors.contains(&"Vernor Vinge".to_string()));
}

#[test]
fn test_epub_to_mobi_roundtrip() {
    // Read AZW3
    let book = read_mobi(TEST_AZW3).expect("Failed to read AZW3");

    // Write to EPUB
    let epub_file = NamedTempFile::with_suffix(".epub").expect("Failed to create temp file");
    write_epub(&book, epub_file.path()).expect("Failed to write EPUB");

    // Read EPUB back
    let book2 = boko::read_epub(epub_file.path()).expect("Failed to read EPUB");

    // Write to MOBI
    let mobi_file = NamedTempFile::with_suffix(".azw3").expect("Failed to create temp file");
    write_mobi(&book2, mobi_file.path()).expect("Failed to write MOBI");

    // Read MOBI back
    let book3 = read_mobi(mobi_file.path()).expect("Failed to read final MOBI");

    // Verify
    assert_eq!(book3.metadata.title, "True Names");
}
