//! Regression tests: EPUB 3 books whose only table of contents is the nav
//! document (`<nav epub:type="toc">`) must not end up with an empty TOC.
//!
//! EPUB 3 makes the nav document the canonical TOC and the NCX optional. The
//! importer keeps its existing NCX-first behavior when a usable NCX exists
//! (avoids churn for dual-TOC books) and falls back to the nav TOC otherwise.

use std::io::{Cursor, Write};

use boko::Book;
use boko::model::Format;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

fn xhtml(title: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>{title}</title></head>
<body>{body}</body>
</html>
"#
    )
}

fn build_epub(entries: &[(&str, String)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(CONTAINER_XML.as_bytes()).unwrap();
    for (name, content) in entries {
        zip.start_file(*name, deflated).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// OPF for a two-chapter book. `with_ncx` controls whether an NCX is declared
/// (manifest item + spine@toc).
fn opf(with_ncx: bool) -> String {
    let ncx_item = if with_ncx {
        r#"    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
"#
    } else {
        ""
    };
    let spine_toc = if with_ncx { r#" toc="ncx""# } else { "" };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:nav-toc-test</dc:identifier>
    <dc:title>Nav TOC</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
{ncx_item}    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine{spine_toc}>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>
"#
    )
}

const NAV_DOC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Navigation</title></head>
<body>
<nav epub:type="toc"><ol>
<li><a href="text/ch1.xhtml">Nav Chapter One</a>
  <ol>
    <li><a href="text/ch1.xhtml#sec1">Nav Section 1.1</a></li>
  </ol>
</li>
<li><a href="text/ch2.xhtml">Nav Chapter Two</a></li>
</ol></nav>
<nav epub:type="landmarks"><ol>
<li><a epub:type="bodymatter" href="text/ch1.xhtml">Start</a></li>
</ol></nav>
</body>
</html>
"#;

const NCX_DOC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head/>
  <docTitle><text>Nav TOC</text></docTitle>
  <navMap>
    <navPoint id="np1" playOrder="1">
      <navLabel><text>NCX Chapter One</text></navLabel>
      <content src="text/ch1.xhtml"/>
    </navPoint>
    <navPoint id="np2" playOrder="2">
      <navLabel><text>NCX Chapter Two</text></navLabel>
      <content src="text/ch2.xhtml"/>
    </navPoint>
  </navMap>
</ncx>
"#;

fn chapters() -> Vec<(&'static str, String)> {
    vec![
        (
            "OEBPS/text/ch1.xhtml",
            xhtml("One", r#"<h1>One</h1><p id="sec1">Section text.</p>"#),
        ),
        ("OEBPS/text/ch2.xhtml", xhtml("Two", "<h1>Two</h1>")),
    ]
}

#[test]
fn nav_only_epub3_gets_toc_from_nav_document() {
    // Regression: no NCX at all -> the TOC was built solely from NCX and came
    // out empty even though the nav document declares a full TOC.
    let mut entries = vec![
        ("OEBPS/content.opf", opf(false)),
        ("OEBPS/nav.xhtml", NAV_DOC.to_string()),
    ];
    entries.extend(chapters());
    let book = Book::from_bytes(&build_epub(&entries), Format::Epub).unwrap();

    let toc = book.toc();
    assert_eq!(toc.len(), 2, "nav-only book must not have an empty TOC");
    assert_eq!(toc[0].title, "Nav Chapter One");
    assert_eq!(toc[0].href, "OEBPS/text/ch1.xhtml");
    // Nested <ol> becomes children, with fragment and base path intact.
    assert_eq!(toc[0].children.len(), 1);
    assert_eq!(toc[0].children[0].title, "Nav Section 1.1");
    assert_eq!(toc[0].children[0].href, "OEBPS/text/ch1.xhtml#sec1");
    assert_eq!(toc[1].title, "Nav Chapter Two");

    // The landmarks nav must not leak into the TOC (no "Start" entry) and
    // still parses as landmarks.
    assert!(toc.iter().all(|e| e.title != "Start"));
    assert_eq!(book.landmarks().len(), 1);
}

#[test]
fn ncx_still_wins_when_both_ncx_and_nav_exist() {
    // Documented choice: when both TOC sources exist, keep the existing
    // NCX-derived TOC to avoid churn; the nav TOC is only a fallback.
    let mut entries = vec![
        ("OEBPS/content.opf", opf(true)),
        ("OEBPS/toc.ncx", NCX_DOC.to_string()),
        ("OEBPS/nav.xhtml", NAV_DOC.to_string()),
    ];
    entries.extend(chapters());
    let book = Book::from_bytes(&build_epub(&entries), Format::Epub).unwrap();

    let toc = book.toc();
    assert_eq!(toc.len(), 2);
    assert_eq!(toc[0].title, "NCX Chapter One");
    assert_eq!(toc[1].title, "NCX Chapter Two");
}

#[test]
fn missing_ncx_entry_falls_back_to_nav_toc() {
    // The OPF declares an NCX that is absent from the archive: the importer
    // used to silently produce an empty TOC; now it falls back to the nav.
    let mut entries = vec![
        ("OEBPS/content.opf", opf(true)),
        // note: no OEBPS/toc.ncx entry in the zip
        ("OEBPS/nav.xhtml", NAV_DOC.to_string()),
    ];
    entries.extend(chapters());
    let book = Book::from_bytes(&build_epub(&entries), Format::Epub).unwrap();

    let toc = book.toc();
    assert_eq!(toc.len(), 2);
    assert_eq!(toc[0].title, "Nav Chapter One");
    assert_eq!(toc[1].title, "Nav Chapter Two");
}
