//! Shared test helpers: fixture loading, an in-memory EPUB builder for the
//! diverse corpus, roundtrip helpers, and a semantic book comparator.
//!
//! Integration test files opt in with `mod common;`. Not every helper is used by
//! every test binary, so individual items may be dead in some — that's expected.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::io::{Cursor, Write};

use boko::Book;
use boko::export::Exporter;
use boko::model::{Format, TocEntry};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

/// Absolute-ish path to a file under `tests/fixtures/`.
pub fn fixture_path(name: &str) -> String {
    format!("tests/fixtures/{name}")
}

/// Open a fixture by name (e.g. `"epictetus.epub"`).
pub fn open_fixture(name: &str) -> Book {
    Book::open(fixture_path(name)).unwrap_or_else(|e| panic!("open fixture {name}: {e}"))
}

// ---------------------------------------------------------------------------
// Roundtrip helpers
// ---------------------------------------------------------------------------

/// Export a book to the given format and return the bytes.
pub fn export_to_bytes(book: &mut Book, format: Format) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    match format {
        Format::Kfx => boko::export::KfxExporter::new()
            .export(book, &mut buf)
            .expect("kfx export"),
        Format::Azw3 => boko::export::Azw3Exporter::new()
            .export(book, &mut buf)
            .expect("azw3 export"),
        Format::Epub => boko::export::EpubExporter::new()
            .export(book, &mut buf)
            .expect("epub export"),
        Format::Markdown => boko::export::MarkdownExporter::new()
            .export(book, &mut buf)
            .expect("markdown export"),
        other => panic!("unsupported export format {other:?}"),
    }
    buf.into_inner()
}

/// Export `book` to `format`, then re-import it. The core roundtrip primitive.
pub fn roundtrip(book: &mut Book, format: Format) -> Book {
    let bytes = export_to_bytes(book, format);
    Book::from_bytes(&bytes, format)
        .unwrap_or_else(|e| panic!("re-import {format:?} after export: {e}"))
}

// ---------------------------------------------------------------------------
// TOC helpers
// ---------------------------------------------------------------------------

pub fn count_toc(entries: &[TocEntry]) -> usize {
    entries.iter().map(|e| 1 + count_toc(&e.children)).sum()
}

pub fn max_toc_depth(entries: &[TocEntry], current: usize) -> usize {
    entries
        .iter()
        .map(|e| max_toc_depth(&e.children, current + 1))
        .max()
        .unwrap_or(current)
}

// ---------------------------------------------------------------------------
// Semantic comparator
// ---------------------------------------------------------------------------

/// A format-agnostic summary of a book's content, used to assert that a
/// conversion preserved meaning even though the bytes differ.
#[derive(Debug, Clone)]
pub struct BookSummary {
    pub title: String,
    pub spine_len: usize,
    pub toc_count: usize,
    pub toc_depth: usize,
    /// Lowercased alphanumeric content words (length >= 4) in document order.
    pub words: Vec<String>,
    /// Distinct lowercased asset file extensions (jpg, png, css, ...).
    pub asset_exts: BTreeSet<String>,
}

impl BookSummary {
    /// The set of distinct content words, for overlap comparisons.
    pub fn word_set(&self) -> BTreeSet<&str> {
        self.words.iter().map(std::string::String::as_str).collect()
    }
}

/// Summarize a book by exporting its text to Markdown (the canonical text
/// surface) and pulling structural facts from the model.
pub fn summarize(book: &mut Book) -> BookSummary {
    let title = book.metadata().title.clone();
    let spine_len = book.spine().len();
    let toc_count = count_toc(book.toc());
    let toc_depth = max_toc_depth(book.toc(), 0);

    let asset_exts = book
        .list_assets()
        .iter()
        .filter_map(|p| std::path::Path::new(p).extension().and_then(|s| s.to_str()))
        .map(str::to_ascii_lowercase)
        .collect();

    let markdown = String::from_utf8(export_to_bytes(book, Format::Markdown))
        .expect("markdown output is utf-8");
    let words = content_words(&strip_link_targets(&markdown));

    BookSummary {
        title,
        spine_len,
        toc_count,
        toc_depth,
        words,
        asset_exts,
    }
}

/// Remove Markdown link/image URL targets (`](url)`) so that path/URL tokens
/// (which legally change across formats — images get renamed to `image_NNNN`)
/// aren't counted as content words. Visible link/alt text is retained.
pub fn strip_link_targets(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut chars = md.chars();
    let mut prev = '\0';
    while let Some(c) = chars.next() {
        if prev == ']' && c == '(' {
            // Consume through the matching ')'.
            for u in chars.by_ref() {
                if u == ')' {
                    break;
                }
            }
            out.push(' ');
            prev = ' ';
            continue;
        }
        out.push(c);
        prev = c;
    }
    out
}

/// Extract lowercased alphanumeric content words of length >= 4. Short words and
/// Markdown punctuation are dropped so the comparison is robust to formatting
/// differences (heading markers, list bullets, footnote references) that legally
/// differ across formats.
pub fn content_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_lowercase)
        .collect()
}

/// Fraction of `source`'s distinct content words that also appear in `other`.
/// 1.0 means every source word survived the conversion.
pub fn word_retention(source: &BookSummary, other: &BookSummary) -> f64 {
    let src = source.word_set();
    if src.is_empty() {
        return 1.0;
    }
    let other = other.word_set();
    let kept = src.iter().filter(|w| other.contains(*w)).count();
    kept as f64 / src.len() as f64
}

// ---------------------------------------------------------------------------
// EPUB builder (in-memory corpus fixtures)
// ---------------------------------------------------------------------------

/// A single navigation point (NCX navPoint), possibly nested.
#[derive(Debug, Clone)]
pub struct Nav {
    pub label: String,
    /// Content src relative to the OPF base, e.g. `text/ch1.xhtml` or
    /// `text/ch1.xhtml#sec2`.
    pub src: String,
    pub children: Vec<Nav>,
}

impl Nav {
    pub fn new(label: &str, src: &str) -> Self {
        Self {
            label: label.into(),
            src: src.into(),
            children: Vec::new(),
        }
    }
    pub fn with_children(mut self, children: Vec<Nav>) -> Self {
        self.children = children;
        self
    }
}

/// A chapter document. `file` is relative to the OPF base (e.g. `text/ch1.xhtml`)
/// and `body` is the inner XHTML of `<body>`.
#[derive(Debug, Clone)]
pub struct Doc {
    pub file: String,
    pub title: String,
    pub body: String,
    pub lang: Option<String>,
    pub dir: Option<String>,
}

impl Doc {
    pub fn new(file: &str, title: &str, body: &str) -> Self {
        Self {
            file: file.into(),
            title: title.into(),
            body: body.into(),
            lang: None,
            dir: None,
        }
    }
    /// Set the document language and text direction (e.g. `"ar"`, `"rtl"`).
    pub fn lang_dir(mut self, lang: &str, dir: &str) -> Self {
        self.lang = Some(lang.into());
        self.dir = Some(dir.into());
        self
    }
}

/// Builds a valid EPUB 3 (with NCX) as an in-memory zip. Layout mirrors what the
/// importer expects: everything under `OEBPS/`, OPF at `OEBPS/content.opf`.
pub struct EpubBuilder {
    title: String,
    language: String,
    identifier: String,
    author: String,
    docs: Vec<Doc>,
    nav: Vec<Nav>,
    css: Option<String>,
    images: Vec<(String, Vec<u8>)>,
    cover: Option<String>,
}

impl EpubBuilder {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.into(),
            language: "en".into(),
            identifier: format!("urn:uuid:test-{}", title.replace(' ', "-").to_lowercase()),
            author: "Test Author".into(),
            docs: Vec::new(),
            nav: Vec::new(),
            css: None,
            images: Vec::new(),
            cover: None,
        }
    }

    pub fn language(mut self, lang: &str) -> Self {
        self.language = lang.into();
        self
    }

    pub fn doc(mut self, doc: Doc) -> Self {
        self.docs.push(doc);
        self
    }

    pub fn nav(mut self, nav: Vec<Nav>) -> Self {
        self.nav = nav;
        self
    }

    pub fn css(mut self, css: &str) -> Self {
        self.css = Some(css.into());
        self
    }

    /// Add an image. `file` is relative to the OPF base (e.g. `images/fig.png`).
    pub fn image(mut self, file: &str, bytes: Vec<u8>) -> Self {
        self.images.push((file.into(), bytes));
        self
    }

    /// Add a 1x1 PNG cover at `images/cover.png`.
    pub fn cover_png(mut self) -> Self {
        self.cover = Some("images/cover.png".into());
        self.images.push(("images/cover.png".into(), tiny_png()));
        self
    }

    /// Assemble the EPUB bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        // mimetype must be first and stored.
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        zip.start_file("META-INF/container.xml", deflated).unwrap();
        zip.write_all(CONTAINER_XML.as_bytes()).unwrap();

        zip.start_file("OEBPS/content.opf", deflated).unwrap();
        zip.write_all(self.opf().as_bytes()).unwrap();

        zip.start_file("OEBPS/toc.ncx", deflated).unwrap();
        zip.write_all(self.ncx().as_bytes()).unwrap();

        zip.start_file("OEBPS/nav.xhtml", deflated).unwrap();
        zip.write_all(self.nav_doc().as_bytes()).unwrap();

        if let Some(css) = &self.css {
            zip.start_file("OEBPS/css/style.css", deflated).unwrap();
            zip.write_all(css.as_bytes()).unwrap();
        }

        for doc in &self.docs {
            zip.start_file(format!("OEBPS/{}", doc.file), deflated)
                .unwrap();
            zip.write_all(self.chapter_xhtml(doc).as_bytes()).unwrap();
        }

        for (file, bytes) in &self.images {
            zip.start_file(format!("OEBPS/{file}"), deflated).unwrap();
            zip.write_all(bytes).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    /// Build and open in one step.
    pub fn book(&self) -> Book {
        Book::from_bytes(&self.build(), Format::Epub).expect("import built epub")
    }

    fn opf(&self) -> String {
        let mut manifest = String::new();
        manifest.push_str(
            r#"    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
"#,
        );
        manifest.push_str(
            r#"    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
"#,
        );
        if self.css.is_some() {
            manifest.push_str(
                r#"    <item id="css" href="css/style.css" media-type="text/css"/>
"#,
            );
        }
        for (i, (file, _)) in self.images.iter().enumerate() {
            let media = media_type_for(file);
            let is_cover = self.cover.as_deref() == Some(file.as_str());
            let props = if is_cover {
                r#" properties="cover-image""#
            } else {
                ""
            };
            manifest.push_str(&format!(
                "    <item id=\"img{i}\" href=\"{file}\" media-type=\"{media}\"{props}/>\n"
            ));
        }
        let mut spine = String::new();
        for (i, doc) in self.docs.iter().enumerate() {
            manifest.push_str(&format!(
                "    <item id=\"doc{i}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>\n",
                doc.file
            ));
            spine.push_str(&format!("    <itemref idref=\"doc{i}\"/>\n"));
        }

        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">{identifier}</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:language>{language}</dc:language>
    <dc:creator>{author}</dc:creator>
  </metadata>
  <manifest>
{manifest}  </manifest>
  <spine toc="ncx">
{spine}  </spine>
</package>
"#,
            identifier = xml_escape(&self.identifier),
            title = xml_escape(&self.title),
            language = xml_escape(&self.language),
            author = xml_escape(&self.author),
        )
    }

    fn ncx(&self) -> String {
        let mut order = 0u32;
        let nav_points = self
            .nav
            .iter()
            .map(|n| render_navpoint(n, &mut order))
            .collect::<String>();
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head/>
  <docTitle><text>{title}</text></docTitle>
  <navMap>
{nav_points}  </navMap>
</ncx>
"#,
            title = xml_escape(&self.title),
        )
    }

    fn nav_doc(&self) -> String {
        let items = self.nav.iter().map(render_nav_li).collect::<String>();
        let first = self
            .docs
            .first()
            .map(|d| d.file.clone())
            .unwrap_or_default();
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Navigation</title></head>
<body>
<nav epub:type="toc"><ol>
{items}</ol></nav>
<nav epub:type="landmarks"><ol>
<li><a epub:type="bodymatter" href="{first}">Start</a></li>
</ol></nav>
</body>
</html>
"#
        )
    }

    fn chapter_xhtml(&self, doc: &Doc) -> String {
        let lang = doc
            .lang
            .as_ref()
            .map(|l| format!(" lang=\"{}\" xml:lang=\"{}\"", xml_escape(l), xml_escape(l)))
            .unwrap_or_default();
        let dir = doc
            .dir
            .as_ref()
            .map(|d| format!(" dir=\"{}\"", xml_escape(d)))
            .unwrap_or_default();
        let css_link = if self.css.is_some() {
            "<link rel=\"stylesheet\" type=\"text/css\" href=\"../css/style.css\"/>"
        } else {
            ""
        };
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml"{lang}{dir}>
<head><title>{title}</title>{css_link}</head>
<body>
{body}
</body>
</html>
"#,
            title = xml_escape(&doc.title),
            body = doc.body,
        )
    }
}

fn render_navpoint(nav: &Nav, order: &mut u32) -> String {
    *order += 1;
    let id = format!("np{order}");
    let this_order = *order;
    let children = nav
        .children
        .iter()
        .map(|c| render_navpoint(c, order))
        .collect::<String>();
    format!(
        "    <navPoint id=\"{id}\" playOrder=\"{this_order}\"><navLabel><text>{label}</text></navLabel><content src=\"{src}\"/>\n{children}    </navPoint>\n",
        label = xml_escape(&nav.label),
        src = xml_escape(&nav.src),
    )
}

fn render_nav_li(nav: &Nav) -> String {
    let children = if nav.children.is_empty() {
        String::new()
    } else {
        let inner = nav.children.iter().map(render_nav_li).collect::<String>();
        format!("<ol>\n{inner}</ol>")
    };
    format!(
        "<li><a href=\"{src}\">{label}</a>{children}</li>\n",
        src = xml_escape(&nav.src),
        label = xml_escape(&nav.label),
    )
}

fn media_type_for(file: &str) -> &'static str {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

/// A minimal valid 1x1 transparent PNG.
pub fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}
