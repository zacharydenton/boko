//! Integration tests for the AZW3 writer.
//!
//! True bit-for-bit EPUB→AZW3→EPUB roundtrip isn't achievable — AZW3 is a
//! compressed binary format that renames spine files, rewrites HTML to add
//! `aid` attributes, and hashes images into `image_NNNN.ext` resource names.
//! These tests instead assert *structural* preservation: anything that ends up
//! mismatched between the source EPUB and the AZW3 that comes back through
//! boko's importer points at a writer bug.
//!
//! Each assertion below corresponds to a specific bug class previously fixed
//! in the writer; if one regresses it should fail here, not on a Kindle.

use std::io::Cursor;

use boko::model::{Format, TocEntry};
use boko::{Book, export::Exporter};

fn export_epub_to_azw3_bytes(epub_path: &str) -> Vec<u8> {
    let book = Book::open(epub_path).expect("opening source epub");
    let mut buf = Cursor::new(Vec::new());
    boko::export::Azw3Exporter::new()
        .export(&book, &mut buf)
        .expect("azw3 export");
    buf.into_inner()
}

fn count_toc(entries: &[TocEntry]) -> usize {
    entries.iter().map(|e| 1 + count_toc(&e.children)).sum()
}

/// The EXTH `length` field must equal the actual block length
/// (magic + length + count + records + pad). It was overstated by 4 bytes
/// because `content` already includes the record count.
#[test]
fn exth_length_field_matches_actual_block() {
    let bytes = export_epub_to_azw3_bytes("tests/fixtures/epictetus.epub");
    let pos = bytes
        .windows(4)
        .position(|w| w == b"EXTH")
        .expect("EXTH block present");
    let hdr_len = u32::from_be_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
    let count = u32::from_be_bytes(bytes[pos + 8..pos + 12].try_into().unwrap()) as usize;
    let mut p = pos + 12;
    for _ in 0..count {
        let rlen = u32::from_be_bytes(bytes[p + 4..p + 8].try_into().unwrap()) as usize;
        p += rlen;
    }
    // Content padding to a 4-byte boundary (content starts at pos + 8).
    while (p - (pos + 8)) % 4 != 0 {
        p += 1;
    }
    assert_eq!(
        hdr_len,
        p - pos,
        "EXTH length field must equal the actual block length"
    );
}

fn max_depth(entries: &[TocEntry], current: usize) -> usize {
    entries
        .iter()
        .map(|e| max_depth(&e.children, current + 1))
        .max()
        .unwrap_or(current)
}

#[test]
fn azw3_roundtrip_preserves_structure() {
    let src_path = "tests/fixtures/epictetus.epub";
    let source = Book::open(src_path).expect("open source epub");
    let src_spine_len = source.spine().len();
    let src_toc_count = count_toc(source.toc());
    let src_toc_depth = max_depth(source.toc(), 0);
    let src_title = source.metadata().title.clone();
    assert!(src_spine_len > 1, "fixture must have multi-file spine");
    assert!(src_toc_depth >= 2, "fixture must have nested TOC");

    let bytes = export_epub_to_azw3_bytes(src_path);
    let book = Book::from_bytes(&bytes, Format::Azw3).expect("reopen azw3");

    // Metadata survives the EXTH round-trip.
    assert_eq!(book.metadata().title, src_title);
    assert!(
        book.metadata().cover_image.is_some(),
        "cover_image must be present in re-imported AZW3 (EXTH 201 regression)"
    );

    // Spine count is preserved. If `SkelEntry.chunk_count` ever drops back to
    // zero, KindleUnpack/our importer collapses every spine file into a single
    // part and this assertion catches it.
    assert_eq!(
        book.spine().len(),
        src_spine_len,
        "spine length must match source (chunk_count regression)"
    );

    // TOC count and hierarchy survive flatten/rebuild.
    assert_eq!(
        count_toc(book.toc()),
        src_toc_count,
        "TOC entry count must match source"
    );
    assert_eq!(
        max_depth(book.toc(), 0),
        src_toc_depth,
        "TOC depth must match source"
    );

    // Nested entries must resolve to distinct positions. The source fixture's
    // "Enchiridion" section has dozens of children that all live in the same
    // spine file but at different anchor offsets. If NCX `pos_fid` (tag 6) is
    // missing or always zero, every child collapses to the same href and the
    // unique-href count is 1 instead of N.
    let _ = book.resolve_links().expect("resolve_links");
    let mut nested_hrefs = std::collections::HashSet::new();
    for top in book.toc() {
        for child in &top.children {
            nested_hrefs.insert(child.href.clone());
        }
    }
    if nested_hrefs.len() <= 1 {
        // Only fail if the source actually had nested children to begin with.
        let nested_in_source: usize = source.toc().iter().map(|t| t.children.len()).sum();
        assert!(
            nested_in_source == 0,
            "nested TOC entries collapsed to a single href (pos_fid regression): {nested_hrefs:?}"
        );
    }

    // Nested entries must resolve to *unique* hrefs (one per chapter section).
    // The number of distinct hrefs should be close to the number of nested
    // entries — if NCX pos_fid is broken, they all collapse to one href.
    let total_nested: usize = book.toc().iter().map(|t| t.children.len()).sum();
    let unique_nested: std::collections::HashSet<_> = book
        .toc()
        .iter()
        .flat_map(|t| &t.children)
        .map(|c| c.href.clone())
        .collect();
    if total_nested >= 4 {
        // Each nested entry should land on a distinct anchor; allow a little
        // slack for duplicate-anchor edge cases but require most to be unique.
        assert!(
            unique_nested.len() * 4 >= total_nested * 3,
            "nested TOC entries should map to mostly-distinct anchors \
             (got {unique} unique out of {total})",
            unique = unique_nested.len(),
            total = total_nested,
        );
    }
}

#[test]
fn azw3_roundtrip_resource_records_are_recognisable() {
    // Regression guard for the record-ordering bug. When INDX records were
    // written between resources and FDST/FLIS/FCIS, our importer (and Kindle)
    // saw the index records sitting in the "resource" range and the AZW3
    // importer either failed to surface images or surfaced records that aren't
    // images. Asserting that assets list contains real image extensions catches
    // the regression.
    let bytes = export_epub_to_azw3_bytes("tests/fixtures/epictetus.epub");
    let book = Book::from_bytes(&bytes, Format::Azw3).expect("reopen azw3");

    let assets = book.list_assets();
    assert!(
        !assets.is_empty(),
        "AZW3 must surface at least one image asset"
    );

    let has_real_image = assets.iter().any(|p| {
        let ext = std::path::Path::new(p)
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase);
        matches!(ext.as_deref(), Some("jpg" | "jpeg" | "png" | "gif"))
    });
    assert!(
        has_real_image,
        "expected at least one JPEG/PNG/GIF asset; got {assets:?}"
    );
}

/// CSS flows must resolve as assets. The writer stores stylesheets as FDST
/// flows 1+ and rewrites `<link href>` to `kindle:flow:XXXX`; on import,
/// `transform_kindle_refs` rewrites those references to
/// `styles/styleNNNN.css`. Without the importer discovering and serving
/// those paths, every stylesheet link in the imported book dangles — the
/// HTML references CSS files that no asset provides.
#[test]
fn azw3_roundtrip_serves_css_flows_as_assets() {
    let bytes = export_epub_to_azw3_bytes("tests/fixtures/epictetus.epub");
    let book = Book::from_bytes(&bytes, Format::Azw3).expect("reopen azw3");

    let css_assets: Vec<String> = book
        .list_assets()
        .iter()
        .filter(|a| a.starts_with("styles/style") && a.ends_with(".css"))
        .cloned()
        .collect();
    assert!(
        !css_assets.is_empty(),
        "AZW3 built from an EPUB with stylesheets must expose styles/styleNNNN.css assets"
    );

    // The imported chapter HTML links those exact paths...
    let spine: Vec<_> = book.spine().to_vec();
    let referenced = spine.iter().any(|entry| {
        let html = book.load_raw(entry.id).expect("load chapter");
        let html = String::from_utf8_lossy(&html);
        css_assets.iter().any(|a| html.contains(a.as_str()))
    });
    assert!(
        referenced,
        "chapter HTML must reference the styles/styleNNNN.css paths"
    );

    // ...and each path must load as plausible CSS with kindle: refs resolved.
    for asset in &css_assets {
        let css = book.load_asset(asset).expect("CSS flow asset must load");
        let css_str = String::from_utf8_lossy(&css);
        assert!(
            css_str.contains('{'),
            "{asset} should contain CSS rules, got {} bytes",
            css.len()
        );
        assert!(
            !css_str.contains("kindle:embed:"),
            "kindle:embed references must be rewritten to asset paths in {asset}"
        );
    }
}
