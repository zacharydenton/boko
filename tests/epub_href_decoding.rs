//! Regression tests for percent-encoded hrefs and spine ChapterId assignment.
//!
//! OPF/NCX/nav hrefs are URLs (`my%20chapter.xhtml`) while ZIP entry names are
//! literal bytes (`my chapter.xhtml`): the importer must percent-decode hrefs
//! at the parse boundary before matching them against archive names, or one
//! `%20` in a manifest aborts the whole conversion with `Error::NotFound`.
//!
//! Separately, a dangling spine idref (no matching manifest item) must not
//! shift the ChapterIds of every later chapter.

use std::io::{Cursor, Write};

use boko::Book;
use boko::model::{ChapterId, Format};
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

/// Assemble an EPUB from literal (name, bytes) entries, plus the standard
/// mimetype and container.xml.
fn build_epub(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(CONTAINER_XML.as_bytes()).unwrap();
    for (name, bytes) in entries {
        zip.start_file(*name, deflated).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// An EPUB whose manifest, NCX, and nav hrefs are all percent-encoded while
/// the ZIP entry names contain the literal characters.
fn encoded_href_epub() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:href-decoding-test</dc:identifier>
    <dc:title>Encoded Hrefs</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="cover" href="images/cover%20art.png" media-type="image/png" properties="cover-image"/>
    <item id="ch1" href="text/my%20chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="ch1"/>
  </spine>
</package>
"#;
    let ncx = r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head/>
  <docTitle><text>Encoded Hrefs</text></docTitle>
  <navMap>
    <navPoint id="np1" playOrder="1">
      <navLabel><text>My Chapter</text></navLabel>
      <content src="text/my%20chapter.xhtml"/>
      <navPoint id="np2" playOrder="2">
        <navLabel><text>Part Two</text></navLabel>
        <content src="text/my%20chapter.xhtml#part%20two"/>
      </navPoint>
    </navPoint>
  </navMap>
</ncx>
"#;
    let nav = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Navigation</title></head>
<body>
<nav epub:type="landmarks"><ol>
<li><a epub:type="bodymatter" href="text/my%20chapter.xhtml">Start</a></li>
</ol></nav>
</body>
</html>
"#;
    build_epub(&[
        ("OEBPS/content.opf", opf.as_bytes().to_vec()),
        ("OEBPS/toc.ncx", ncx.as_bytes().to_vec()),
        ("OEBPS/nav.xhtml", nav.as_bytes().to_vec()),
        (
            "OEBPS/text/my chapter.xhtml",
            xhtml("My Chapter", "<p>PERCENT-MARKER prose.</p>").into_bytes(),
        ),
        ("OEBPS/images/cover art.png", vec![0x89, 0x50, 0x4E, 0x47]),
    ])
}

#[test]
fn percent_encoded_manifest_href_resolves_to_literal_zip_name() {
    // Regression: this open failed with Error::NotFound because the encoded
    // manifest href was matched verbatim against the literal ZIP entry name.
    let book = Book::from_bytes(&encoded_href_epub(), Format::Epub)
        .expect("EPUB with percent-encoded manifest hrefs must import");

    assert_eq!(book.spine().len(), 1);
    let id = book.spine()[0].id;
    assert_eq!(
        book.source_id(id),
        Some("OEBPS/text/my chapter.xhtml"),
        "spine path must be the decoded, literal archive name"
    );
    let raw = book
        .load_raw(id)
        .expect("chapter bytes load via decoded path");
    let text = String::from_utf8(raw).unwrap();
    assert!(text.contains("PERCENT-MARKER"));
}

#[test]
fn percent_encoded_ncx_hrefs_decoded_with_fragment_preserved() {
    let book = Book::from_bytes(&encoded_href_epub(), Format::Epub).unwrap();

    let toc = book.toc();
    assert_eq!(toc.len(), 1);
    assert_eq!(toc[0].href, "OEBPS/text/my chapter.xhtml");
    // Path and fragment are decoded separately; the '#' separator survives.
    assert_eq!(toc[0].children.len(), 1);
    assert_eq!(
        toc[0].children[0].href,
        "OEBPS/text/my chapter.xhtml#part two"
    );
}

#[test]
fn percent_encoded_landmark_and_cover_hrefs_decoded() {
    let book = Book::from_bytes(&encoded_href_epub(), Format::Epub).unwrap();

    let landmarks = book.landmarks();
    assert_eq!(landmarks.len(), 1);
    assert_eq!(landmarks[0].href, "OEBPS/text/my chapter.xhtml");

    assert_eq!(
        book.metadata().cover_image.as_deref(),
        Some("OEBPS/images/cover art.png"),
        "cover href must match the literal asset key"
    );
    // And the decoded cover path actually loads.
    assert!(book.load_asset("OEBPS/images/cover art.png").is_ok());
}

#[test]
fn dangling_spine_idref_does_not_shift_chapter_ids() {
    // Regression: SpineEntry ids came from the itemref enumerate index, but
    // spine_paths only grows for idrefs found in the manifest — the "ghost"
    // idref shifted every later ChapterId off its path.
    let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:dangling-idref-test</dc:identifier>
    <dc:title>Dangling Idref</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ghost"/>
    <itemref idref="ch2"/>
  </spine>
</package>
"#;
    let bytes = build_epub(&[
        ("OEBPS/content.opf", opf.as_bytes().to_vec()),
        (
            "OEBPS/text/ch1.xhtml",
            xhtml("One", "<p>FIRST chapter.</p>").into_bytes(),
        ),
        (
            "OEBPS/text/ch2.xhtml",
            xhtml("Two", "<p>SECOND chapter.</p>").into_bytes(),
        ),
    ]);
    let book = Book::from_bytes(&bytes, Format::Epub).unwrap();

    // The ghost idref is skipped; ids must stay contiguous and aligned with
    // their paths.
    assert_eq!(book.spine().len(), 2);
    assert_eq!(book.spine()[0].id, ChapterId(0));
    assert_eq!(book.spine()[1].id, ChapterId(1));
    assert_eq!(book.source_id(ChapterId(0)), Some("OEBPS/text/ch1.xhtml"));
    assert_eq!(book.source_id(ChapterId(1)), Some("OEBPS/text/ch2.xhtml"));

    let second = String::from_utf8(book.load_raw(book.spine()[1].id).unwrap()).unwrap();
    assert!(
        second.contains("SECOND"),
        "second spine entry must load the second chapter, not a shifted one"
    );
}
