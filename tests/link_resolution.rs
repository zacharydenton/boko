//! Test link resolution for different formats.

use boko::Book;
#[test]
fn test_azw3_toc_resolution() {
    let path = "tests/fixtures/epictetus.azw3";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {}", path);
        return;
    }

    let mut book = Book::open(path).expect("Should open AZW3");

    // Before resolve_links: TOC hrefs don't have fragments
    // Resolve links (also resolves TOC)
    let _ = book.resolve_links().expect("Should resolve links");

    // After resolve_links: TOC hrefs should have fragments
    let toc = book.toc();

    fn count_with_fragments(entries: &[boko::model::TocEntry]) -> (usize, usize, usize) {
        let mut total = 0;
        let mut with_fragment = 0;
        let mut with_target = 0;
        for entry in entries {
            total += 1;
            if entry.href.contains('#') {
                with_fragment += 1;
            }
            if entry.target.is_some() {
                with_target += 1;
            }
            let (t, f, tgt) = count_with_fragments(&entry.children);
            total += t;
            with_fragment += f;
            with_target += tgt;
        }
        (total, with_fragment, with_target)
    }

    let (_total, with_fragment, with_target) = count_with_fragments(toc);
    // At least some TOC entries should have fragments
    assert!(
        with_fragment > 0,
        "Expected some TOC entries to have fragments, got {}",
        with_fragment
    );

    // TOC entries should have targets
    assert!(
        with_target > 0,
        "Expected some TOC entries to have targets, got {}",
        with_target
    );

    // Every TOC entry should have a unique href (catches insert_pos vs start_pos bug)
    assert_unique_toc_hrefs(toc, "AZW3");
}

/// Helper to collect all TOC hrefs recursively.
fn collect_toc_hrefs(entries: &[boko::model::TocEntry], hrefs: &mut Vec<String>) {
    for entry in entries {
        hrefs.push(entry.href.clone());
        collect_toc_hrefs(&entry.children, hrefs);
    }
}

/// Helper to assert all TOC entries have unique hrefs.
fn assert_unique_toc_hrefs(toc: &[boko::model::TocEntry], format_name: &str) {
    use std::collections::HashMap;

    let mut all_hrefs = Vec::new();
    collect_toc_hrefs(toc, &mut all_hrefs);

    let mut href_counts: HashMap<&String, usize> = HashMap::new();
    for href in &all_hrefs {
        *href_counts.entry(href).or_default() += 1;
    }
    let unique_count = href_counts.len();
    assert_eq!(
        all_hrefs.len(),
        unique_count,
        "{}: Every TOC entry should have a unique href",
        format_name
    );
}

#[test]
fn test_epub_toc_resolution() {
    let path = "tests/fixtures/epictetus.epub";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {}", path);
        return;
    }

    let mut book = Book::open(path).expect("Should open EPUB");
    let _ = book.resolve_links().expect("Should resolve links");

    assert_unique_toc_hrefs(book.toc(), "EPUB");
}

#[test]
fn test_mobi_toc_resolution() {
    let path = "tests/fixtures/epictetus.mobi";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {}", path);
        return;
    }

    let mut book = Book::open(path).expect("Should open MOBI");
    let _ = book.resolve_links().expect("Should resolve links");

    assert_unique_toc_hrefs(book.toc(), "MOBI");
}

#[test]
fn test_kfx_toc_resolution() {
    let path = "tests/fixtures/epictetus.kfx";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {}", path);
        return;
    }

    let mut book = Book::open(path).expect("Should open KFX");
    let _ = book.resolve_links().expect("Should resolve links");

    assert_unique_toc_hrefs(book.toc(), "KFX");
}
