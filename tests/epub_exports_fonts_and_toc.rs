//! Regression tests for the EPUB exporter's font writing and TOC resolution.
//!
//! Covers two distinct improvements to the EPUB exporter:
//!
//! - Embedded fonts surfaced by the importer (`fonts/font_NNNN.*` asset paths)
//!   are now written into the exported EPUB even when the normalized-content
//!   pipeline did not pull them into its asset list. Without this, books with
//!   custom typography exported a `@font-face` stylesheet whose `src:` URLs
//!   pointed at files the exporter never wrote into the ZIP.
//! - `book.resolve_toc()` is now called by the exporter, so TOC entries that
//!   importers leave with bare chapter hrefs (AZW3 / MOBI) get their fragment
//!   suffix populated before the NCX is generated.

use std::io::{Cursor, Read};
use std::path::Path;

use boko::Book;
use boko::export::{EpubExporter, Exporter};
use flate2::read::GzDecoder;
use zip::ZipArchive;

/// Decompress the `fonts_only.kfx.gz` fixture to a temp file.
///
/// The fixture is a stripped KFX containing only `bcRawFont` and `Font`
/// entities — exactly the shape that exercises the importer's font discovery
/// pipeline added in #13.
fn decompress_fonts_only_fixture() -> Option<tempfile::NamedTempFile> {
    let gz_path = "tests/fixtures/fonts_only.kfx.gz";
    if !Path::new(gz_path).exists() {
        eprintln!("Skipping test - fixture not found: {gz_path}");
        return None;
    }

    let gz_data = std::fs::read(gz_path).expect("read fixture");
    let mut decoder = GzDecoder::new(&gz_data[..]);
    let mut kfx_data = Vec::new();
    decoder.read_to_end(&mut kfx_data).expect("decompress");

    let mut tmp = tempfile::Builder::new()
        .suffix(".kfx")
        .tempfile()
        .expect("temp file");
    std::io::Write::write_all(&mut tmp, &kfx_data).expect("write temp");
    Some(tmp)
}

#[test]
fn epub_export_writes_font_assets_from_kfx() {
    let Some(tmp) = decompress_fonts_only_fixture() else {
        return;
    };

    let mut book = Book::open(tmp.path()).expect("open KFX fonts_only fixture");

    // Sanity: the importer surfaces the fonts via list_assets.
    let surfaced_fonts: Vec<_> = book
        .list_assets()
        .iter()
        .filter(|p| p.to_string_lossy().starts_with("fonts/"))
        .cloned()
        .collect();
    assert_eq!(
        surfaced_fonts.len(),
        3,
        "importer should expose 3 font assets, got {surfaced_fonts:?}",
    );

    // Export to an in-memory EPUB.
    let mut buf = Cursor::new(Vec::<u8>::new());
    EpubExporter::new()
        .export(&mut book, &mut buf)
        .expect("epub export");

    let epub_bytes = buf.into_inner();
    let mut zip = ZipArchive::new(Cursor::new(epub_bytes)).expect("open epub zip");

    // The exporter must write every font asset the importer surfaced.
    for asset in &surfaced_fonts {
        let zip_path = format!("OEBPS/{}", asset.to_string_lossy());
        let mut entry = zip
            .by_name(&zip_path)
            .unwrap_or_else(|_| panic!("missing {zip_path} in exported EPUB"));
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read font entry");
        assert!(
            data.len() > 1000,
            "{zip_path} truncated: only {} bytes",
            data.len()
        );
    }

    // The OPF manifest must reference each font asset as well. The OPF emits
    // hrefs relative to its own location (OEBPS/), so we look for the bare
    // `fonts/font_NNNN.*` path, not the `OEBPS/...` ZIP path.
    let mut opf = String::new();
    zip.by_name("OEBPS/content.opf")
        .expect("OEBPS/content.opf present")
        .read_to_string(&mut opf)
        .expect("read opf");
    for asset in &surfaced_fonts {
        let needle = asset.to_string_lossy().to_string();
        assert!(
            opf.contains(&needle),
            "content.opf manifest missing href \"{needle}\""
        );
    }
}

#[test]
fn epub_export_resolves_azw3_toc_fragments() {
    let azw3_path = "tests/fixtures/epictetus.azw3";
    if !Path::new(azw3_path).exists() {
        eprintln!("Skipping test - fixture not found: {azw3_path}");
        return;
    }

    // Sanity check: the AZW3 importer leaves TOC entries with bare chapter
    // hrefs at open time — every Enchiridion section initially points at
    // part0002.html with no fragment.
    {
        let book = Book::open(azw3_path).expect("open epictetus.azw3");
        let mut enchiridion_section_hrefs = Vec::new();
        for entry in book.toc() {
            if entry.title == "The Enchiridion" {
                for child in &entry.children {
                    enchiridion_section_hrefs.push(child.href.clone());
                }
                break;
            }
        }
        assert!(
            !enchiridion_section_hrefs.is_empty(),
            "expected nested Enchiridion sections"
        );
        for href in &enchiridion_section_hrefs {
            assert!(
                !href.contains('#'),
                "expected bare chapter href before resolve_toc, got {href}"
            );
        }
    }

    // Export to in-memory EPUB. The exporter must call `resolve_toc()`
    // internally so the generated NCX has fragments populated.
    let mut book = Book::open(azw3_path).expect("re-open epictetus.azw3");
    let mut buf = Cursor::new(Vec::<u8>::new());
    EpubExporter::new()
        .export(&mut book, &mut buf)
        .expect("epub export");

    let epub_bytes = buf.into_inner();
    let mut zip = ZipArchive::new(Cursor::new(epub_bytes)).expect("open epub zip");

    let mut ncx = String::new();
    zip.by_name("OEBPS/toc.ncx")
        .expect("OEBPS/toc.ncx present")
        .read_to_string(&mut ncx)
        .expect("read ncx");

    // Resolved entries get an `#aid-XXXX` fragment populated from the NCX
    // index. The pre-fix exporter produced bare `partNNNN.html` hrefs with
    // no fragment, so any non-trivial count here proves resolve_toc ran.
    let aid_refs = ncx.matches("#aid-").count();
    assert!(
        aid_refs > 10,
        "expected resolved #aid- fragments in NCX, got only {aid_refs}; \
         resolve_toc likely didn't run during export. NCX:\n{ncx}",
    );
}
