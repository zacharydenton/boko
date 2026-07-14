//! Test link resolution for different formats.

use boko::Book;
use sha1_smol::Sha1;
use std::path::Path;

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::from(bytes).hexdigest()
}
#[test]
fn test_azw3_toc_resolution() {
    let path = "tests/fixtures/epictetus.azw3";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {path}");
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
        "Expected some TOC entries to have fragments, got {with_fragment}"
    );

    // TOC entries should have targets
    assert!(
        with_target > 0,
        "Expected some TOC entries to have targets, got {with_target}"
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
        "{format_name}: Every TOC entry should have a unique href"
    );
}

#[test]
fn test_epub_toc_resolution() {
    let path = "tests/fixtures/epictetus.epub";
    if !std::path::Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {path}");
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
        eprintln!("Skipping test - fixture not found: {path}");
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
        eprintln!("Skipping test - fixture not found: {path}");
        return;
    }

    let mut book = Book::open(path).expect("Should open KFX");
    let _ = book.resolve_links().expect("Should resolve links");

    assert_unique_toc_hrefs(book.toc(), "KFX");
}

#[test]
fn test_kfx_named_resource_returns_binary_asset() {
    let path = "tests/fixtures/epictetus.kfx";
    if !Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {path}");
        return;
    }

    let mut book = Book::open(path).expect("Should open KFX");

    let expected = [
        ("resource/rsrc7", "dae4335aa095d1109e81a413cea05e1a3225e4ed"),
        (
            "resource/rsrc1DT",
            "5d4c6c7573d11baff232c9ee8381f2012a4c9be2",
        ),
        (
            "resource/rsrc1DU",
            "4a5dc446b5a102bbb11e9439ee586bcc5a4e0811",
        ),
    ];

    for (resource_name, expected_sha1) in expected {
        let bytes = book
            .load_asset(resource_name)
            .expect("Expected named resource to exist in epictetus.kfx");

        assert!(
            !bytes.starts_with(&[0xE0, 0x01, 0x00, 0xEA]),
            "Expected binary media bytes for {}, got Ion metadata payload ({} bytes)",
            resource_name,
            bytes.len()
        );
        assert!(
            bytes.len() > 256,
            "Expected substantial media payload for {}, got {} bytes",
            resource_name,
            bytes.len()
        );
        assert_eq!(
            sha1_hex(bytes.as_slice()),
            expected_sha1,
            "Unexpected SHA-1 for {resource_name} in epictetus fixture"
        );
    }
}

#[test]
fn test_kfx_direct_asset_id_1102_matches_named_resource_and_hash() {
    let path = "tests/fixtures/epictetus.kfx";
    if !Path::new(path).exists() {
        eprintln!("Skipping test - fixture not found: {path}");
        return;
    }

    let mut book = Book::open(path).expect("Should open KFX");

    assert!(
        book.list_assets().iter().any(|p| p == "#1102"),
        "Expected #1102 to be listed as a KFX asset"
    );

    let id_bytes = book
        .load_asset("#1102")
        .expect("Should load #1102 asset bytes");
    let id_sha1 = sha1_hex(id_bytes.as_slice());

    assert_eq!(
        id_sha1, "5d4c6c7573d11baff232c9ee8381f2012a4c9be2",
        "Unexpected SHA-1 for fixture asset #1102"
    );

    let named_sha1 = book
        .load_asset("resource/rsrc1DT")
        .map(|bytes| sha1_hex(bytes.as_slice()))
        .expect("Should load resource/rsrc1DT by name");
    assert_eq!(
        named_sha1, id_sha1,
        "Expected resource/rsrc1DT to match #1102 payload hash"
    );
}
