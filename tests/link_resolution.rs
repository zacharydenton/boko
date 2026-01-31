//! Test link resolution for different formats.

use boko::model::AnchorTarget;
use boko::Book;

#[test]
fn test_azw3_link_resolution() {
    let path = "tests/fixtures/epictetus.azw3";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {}", path);
        return;
    }

    let mut book = Book::open(path).expect("Should open AZW3");

    // Load all chapters
    let spine: Vec<_> = book.spine().to_vec();
    for entry in &spine {
        let _ = book.load_chapter_cached(entry.id);
    }

    // Resolve links
    let resolved = book.resolve_links().expect("Should resolve links");

    let broken = resolved.broken_links();
    let internal: usize = resolved
        .iter()
        .filter(|(_, t)| matches!(t, AnchorTarget::Internal(_)))
        .count();
    let chapter: usize = resolved
        .iter()
        .filter(|(_, t)| matches!(t, AnchorTarget::Chapter(_)))
        .count();
    let external: usize = resolved
        .iter()
        .filter(|(_, t)| matches!(t, AnchorTarget::External(_)))
        .count();

    println!(
        "AZW3: Total: {}, Internal: {}, Chapter: {}, External: {}, Broken: {}",
        resolved.len(),
        internal,
        chapter,
        external,
        broken.len()
    );

    for (source, href) in broken.iter().take(5) {
        println!("  Broken: {:?} -> {}", source, href);
    }

    // Should have mostly internal links, not just chapter-level
    // The old code had 0 internal, 197 chapter - we expect more internal now
    assert!(
        internal > chapter,
        "Should have more internal links than chapter-level: internal={}, chapter={}",
        internal,
        chapter
    );
}

#[test]
fn test_mobi_link_resolution() {
    let path = "tests/fixtures/epictetus.mobi";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {}", path);
        return;
    }

    let mut book = Book::open(path).expect("Should open MOBI");

    // Load all chapters
    let spine: Vec<_> = book.spine().to_vec();
    for entry in &spine {
        let _ = book.load_chapter_cached(entry.id);
    }

    // Resolve links
    let resolved = book.resolve_links().expect("Should resolve links");

    let broken = resolved.broken_links();
    let internal: usize = resolved
        .iter()
        .filter(|(_, t)| matches!(t, AnchorTarget::Internal(_)))
        .count();
    let chapter: usize = resolved
        .iter()
        .filter(|(_, t)| matches!(t, AnchorTarget::Chapter(_)))
        .count();
    let external: usize = resolved
        .iter()
        .filter(|(_, t)| matches!(t, AnchorTarget::External(_)))
        .count();

    println!(
        "MOBI: Total: {}, Internal: {}, Chapter: {}, External: {}, Broken: {}",
        resolved.len(),
        internal,
        chapter,
        external,
        broken.len()
    );

    for (source, href) in broken.iter().take(5) {
        println!("  Broken: {:?} -> {}", source, href);
    }
}
