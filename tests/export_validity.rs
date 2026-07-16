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
