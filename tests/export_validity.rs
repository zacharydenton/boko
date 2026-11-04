//! Exported-book validity checks.
//!
//! - EPUB outputs are validated with `epubcheck` when it is installed
//!   (skipped otherwise, so CI without Java still passes).
//! - KFX→AZW3 must produce an NCX whose entries point at distinct positions
//!   (a regression here means every TOC entry resolves to byte 0).

mod common;

use std::process::Command;

use boko::model::Format;

fn epubcheck_errors(bytes: &[u8], name: &str) -> Option<usize> {
    let dir = std::env::temp_dir().join("boko-export-validity");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).ok()?;

    let output = Command::new("epubcheck").arg(&path).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // "Messages: 0 fatals / 4 errors / ..."
    let errors = text.lines().find_map(|line| {
        let rest = line.strip_prefix("Messages: ")?;
        let mut parts = rest.split(" / ");
        let fatals: usize = parts.next()?.split_whitespace().next()?.parse().ok()?;
        let errors: usize = parts.next()?.split_whitespace().next()?.parse().ok()?;
        Some(fatals + errors)
    });
    std::fs::remove_file(&path).ok();
    errors
}

#[test]
fn epub_to_epub_export_passes_epubcheck() {
    let mut book = common::open_fixture("epictetus.epub");
    let bytes = common::export_to_bytes(&mut book, Format::Epub);
    match epubcheck_errors(&bytes, "from_epub.epub") {
        Some(errors) => assert_eq!(errors, 0, "epubcheck reported {errors} errors"),
        None => eprintln!("epubcheck not available; skipping validation"),
    }
}

#[test]
fn kfx_to_epub_export_is_nearly_clean_under_epubcheck() {
    let mut book = common::open_fixture("epictetus.kfx");
    let bytes = common::export_to_bytes(&mut book, Format::Epub);
    match epubcheck_errors(&bytes, "from_kfx.epub") {
        // Known remainder: epubcheck flags the extensionless KFX resource
        // names ("e6") as corrupted images even though the bytes are valid
        // JPEGs. Everything else (fragments, nesting, nav) must be clean.
        Some(errors) => assert!(
            errors <= 3,
            "epubcheck reported {errors} errors (expected <= 3)"
        ),
        None => eprintln!("epubcheck not available; skipping validation"),
    }
}

#[test]
fn kfx_to_azw3_toc_targets_are_distinct() {
    let mut book = common::open_fixture("epictetus.kfx");
    let reimported = common::roundtrip(&mut book, Format::Azw3);

    let toc = reimported.toc();
    assert!(
        toc.len() > 3,
        "expected a real TOC, got {} entries",
        toc.len()
    );

    // Before the normalized-AZW3 TOC rewrite, every NCX entry resolved to
    // position 0, so all entries re-imported pointing at the same chapter.
    let mut hrefs = std::collections::HashSet::new();
    fn collect<'a>(
        entries: &'a [boko::model::TocEntry],
        out: &mut std::collections::HashSet<&'a str>,
    ) {
        for e in entries {
            out.insert(e.href.as_str());
            collect(&e.children, out);
        }
    }
    collect(toc, &mut hrefs);
    assert!(
        hrefs.len() > 3,
        "TOC entries collapsed to too few targets: {hrefs:?}"
    );
}

#[test]
fn rtl_direction_survives_kfx_and_azw3_roundtrips() {
    use common::{Doc, EpubBuilder};

    let epub = EpubBuilder::new("RTL Book")
        .language("ar")
        .doc(
            Doc::new(
                "text/ch1.xhtml",
                "الفصل",
                "<h1>عنوان</h1><p>نص عربي.</p><p dir=\"ltr\">English inside.</p>",
            )
            .lang_dir("ar", "rtl"),
        )
        .build();

    // No spine direction in the source: Arabic alone must make it RTL.
    for format in [Format::Kfx, Format::Azw3] {
        let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import rtl epub");
        let mut back = common::roundtrip(&mut book, format);
        assert_eq!(
            back.metadata().page_progression_direction.as_deref(),
            Some("rtl"),
            "{format:?} must carry the RTL reading direction"
        );
        if format == Format::Kfx {
            // KFX → EPUB rebuilds chapters from the IR: the document direction
            // lands on <html>, the LTR paragraph keeps its own.
            let bytes = common::export_to_bytes(&mut back, Format::Epub);
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut text = String::new();
            for i in 0..zip.len() {
                use std::io::Read;
                zip.by_index(i).unwrap().read_to_string(&mut text).ok();
            }
            assert!(text.contains(r#"dir="rtl""#), "EPUB from KFX lost dir=rtl");
            assert!(
                text.contains("direction: ltr"),
                "EPUB from KFX lost the LTR paragraph"
            );
        }
    }
}

#[test]
fn rtl_page_progression_direction_survives_epub_roundtrip() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("RTL Book")
        .language("ar")
        .direction("rtl")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "الفصل",
            "<h1>عنوان</h1><p>نص عربي.</p>",
        ))
        .nav(vec![Nav::new("عنوان", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import rtl epub");
    assert_eq!(
        book.metadata().page_progression_direction.as_deref(),
        Some("rtl"),
        "RTL direction must be read from the source spine"
    );

    let bytes = common::export_to_bytes(&mut book, Format::Epub);
    let reimported = boko::Book::from_bytes(&bytes, Format::Epub).expect("reimport rtl epub");
    assert_eq!(
        reimported.metadata().page_progression_direction.as_deref(),
        Some("rtl"),
        "RTL direction must survive the EPUB round trip"
    );

    // And the export must still validate.
    if let Some(errors) = epubcheck_errors(&bytes, "rtl.epub") {
        assert_eq!(
            errors, 0,
            "epubcheck reported {errors} errors on the RTL export"
        );
    }
}
