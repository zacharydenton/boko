//! Benchmarks for ebook conversion pipeline.
//!
//! Run with: cargo bench

use std::io::Cursor;

use criterion::{Criterion, criterion_group, criterion_main};

use boko::export::{Azw3Exporter, EpubExporter, Exporter, KfxExporter, MarkdownExporter};
use boko::{Book, Format, Origin, Stylesheet, compile_html};

const EPUB_BYTES: &[u8] = include_bytes!("../tests/fixtures/epictetus.epub");
const AZW3_BYTES: &[u8] = include_bytes!("../tests/fixtures/epictetus.azw3");
const KFX_BYTES: &[u8] = include_bytes!("../tests/fixtures/epictetus.kfx");

/// Load sample HTML and CSS from the epub fixture for IR benchmarks.
fn load_sample_content() -> (String, Stylesheet) {
    let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();

    // Find the enchiridion chapter (largest content chapter)
    let spine: Vec<_> = book.spine().to_vec();
    let chapter_id = spine
        .iter()
        .find(|e| {
            book.source_id(e.id)
                .is_some_and(|s| s.contains("enchiridion"))
        })
        .map_or(spine[0].id, |e| e.id);

    let html_bytes = book.load_raw(chapter_id).unwrap();
    let html = String::from_utf8_lossy(&html_bytes).into_owned();

    // Load CSS
    let css_bytes = book.load_asset("epub/css/core.css").unwrap();
    let css = String::from_utf8_lossy(&css_bytes);
    let stylesheet = Stylesheet::parse(&css);

    (html, stylesheet)
}

/// Build a synthetic EPUB with `chapters` content documents in memory.
///
/// Each chapter mixes paragraphs, inline spans, lists, and a table so
/// per-node costs (cascade, IR transform, style interning) dominate like
/// they do in real books. Used by the large-book and cold-convert benches.
fn build_synthetic_epub(chapters: usize) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();

    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
    )
    .unwrap();

    zip.start_file("OEBPS/style.css", deflated).unwrap();
    zip.write_all(
        b"body { margin: 1em; font-family: serif; }\n\
          p { text-indent: 1em; margin: 0.2em 0; }\n\
          p.first { text-indent: 0; }\n\
          em { font-style: italic; }\n\
          li strong { color: #333; }\n\
          table td { padding: 0.2em; border: 1px solid #999; }\n",
    )
    .unwrap();

    let mut manifest = String::new();
    let mut spine = String::new();
    for i in 0..chapters {
        let mut body = String::with_capacity(8 * 1024);
        body.push_str(&format!("<h1>Chapter {}</h1>", i + 1));
        for p in 0..20 {
            body.push_str(&format!(
                "<p class=\"{}\">Paragraph {} of chapter {} with <em>emphasis</em>, \
                 a <a href=\"chapter_{}.xhtml\">link</a>, and <span class=\"note\">spans</span> \
                 to exercise inline handling in the cascade and IR transform.</p>",
                if p == 0 { "first" } else { "body" },
                p,
                i,
                (i + 1) % chapters,
            ));
        }
        body.push_str("<ul><li><strong>alpha</strong></li><li>beta</li><li>gamma</li></ul>");
        body.push_str("<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>");

        let doc = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <html xmlns=\"http://www.w3.org/1999/xhtml\"><head>\
             <title>Chapter {}</title>\
             <link rel=\"stylesheet\" href=\"style.css\"/>\
             </head><body>{}</body></html>",
            i + 1,
            body
        );
        zip.start_file(format!("OEBPS/chapter_{i}.xhtml"), deflated)
            .unwrap();
        zip.write_all(doc.as_bytes()).unwrap();

        manifest.push_str(&format!(
            "<item id=\"ch{i}\" href=\"chapter_{i}.xhtml\" media-type=\"application/xhtml+xml\"/>"
        ));
        spine.push_str(&format!("<itemref idref=\"ch{i}\"/>"));
    }

    zip.start_file("OEBPS/content.opf", deflated).unwrap();
    zip.write_all(
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:bench-large-book</dc:identifier>
    <dc:title>Synthetic Large Book</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="css" href="style.css" media-type="text/css"/>
    {manifest}
  </manifest>
  <spine>{spine}</spine>
</package>"#
        )
        .as_bytes(),
    )
    .unwrap();

    zip.finish().unwrap().into_inner()
}

// ============================================================================
// Book I/O Benchmarks
// ============================================================================

fn bench_read_epub(c: &mut Criterion) {
    c.bench_function("read_epub", |b| {
        b.iter(|| Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap());
    });
}

fn bench_read_azw3(c: &mut Criterion) {
    c.bench_function("read_azw3", |b| {
        b.iter(|| Book::from_bytes(AZW3_BYTES, Format::Azw3).unwrap());
    });
}

fn bench_read_kfx(c: &mut Criterion) {
    c.bench_function("read_kfx", |b| {
        b.iter(|| Book::from_bytes(KFX_BYTES, Format::Kfx).unwrap());
    });
}

fn bench_write_epub(c: &mut Criterion) {
    let mut book = Book::from_bytes(AZW3_BYTES, Format::Azw3).unwrap();

    c.bench_function("write_epub", |b| {
        b.iter(|| {
            let mut output = Cursor::new(Vec::new());
            EpubExporter::new().export(&mut book, &mut output).unwrap();
        });
    });
}

fn bench_write_azw3(c: &mut Criterion) {
    let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();

    c.bench_function("write_azw3", |b| {
        b.iter(|| {
            let mut output = Cursor::new(Vec::new());
            Azw3Exporter::new().export(&mut book, &mut output).unwrap();
        });
    });
}

fn bench_write_kfx(c: &mut Criterion) {
    let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();

    c.bench_function("write_kfx", |b| {
        b.iter(|| {
            let mut output = Cursor::new(Vec::new());
            KfxExporter::new().export(&mut book, &mut output).unwrap();
        });
    });
}

// ============================================================================
// Cold End-to-End Benchmarks
//
// The write_* benches above reuse one Book, so the IR cache hides all
// import/compile cost after the first iteration. These run the full
// pipeline (parse container + compile every chapter + export) per
// iteration, which is what the CLI actually does.
// ============================================================================

fn bench_convert_epub_to_kfx_cold(c: &mut Criterion) {
    c.bench_function("convert_epub_to_kfx_cold", |b| {
        b.iter(|| {
            let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();
            let mut output = Cursor::new(Vec::new());
            KfxExporter::new().export(&mut book, &mut output).unwrap();
        });
    });
}

fn bench_convert_epub_to_azw3_cold(c: &mut Criterion) {
    c.bench_function("convert_epub_to_azw3_cold", |b| {
        b.iter(|| {
            let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();
            let mut output = Cursor::new(Vec::new());
            Azw3Exporter::new().export(&mut book, &mut output).unwrap();
        });
    });
}

// ============================================================================
// Large-Book Benchmarks (100 synthetic chapters)
// ============================================================================

fn bench_large_book_to_kfx_cold(c: &mut Criterion) {
    let epub = build_synthetic_epub(100);
    let mut group = c.benchmark_group("large_book");
    group.sample_size(10);
    group.bench_function("to_kfx_cold", |b| {
        b.iter(|| {
            let mut book = Book::from_bytes(&epub, Format::Epub).unwrap();
            let mut output = Cursor::new(Vec::new());
            KfxExporter::new().export(&mut book, &mut output).unwrap();
        });
    });
    group.finish();
}

fn bench_large_book_to_azw3_cold(c: &mut Criterion) {
    let epub = build_synthetic_epub(100);
    let mut group = c.benchmark_group("large_book");
    group.sample_size(10);
    group.bench_function("to_azw3_cold", |b| {
        b.iter(|| {
            let mut book = Book::from_bytes(&epub, Format::Epub).unwrap();
            let mut output = Cursor::new(Vec::new());
            Azw3Exporter::new().export(&mut book, &mut output).unwrap();
        });
    });
    group.finish();
}

// ============================================================================
// IR Pipeline Benchmarks
// ============================================================================

fn bench_compile_html(c: &mut Criterion) {
    let (html, stylesheet) = load_sample_content();

    c.bench_function("compile_html", |b| {
        b.iter(|| compile_html(&html, &[(stylesheet.clone(), Origin::Author)]));
    });
}

fn bench_compile_html_no_css(c: &mut Criterion) {
    let (html, _) = load_sample_content();

    c.bench_function("compile_html_no_css", |b| {
        b.iter(|| compile_html(&html, &[]));
    });
}

fn bench_compile_html_heavy_css(c: &mut Criterion) {
    let (html, base) = load_sample_content();

    // A rule-heavy stylesheet: hundreds of class/descendant/tag rules so the
    // per-element candidate collection and selector matching dominate.
    let mut css = String::new();
    for i in 0..300 {
        css.push_str(&format!(
            ".c{i} {{ margin-top: {}px; color: #0{}0; }}\n\
             div .c{i} em {{ font-style: italic; }}\n\
             p.c{i} > span {{ letter-spacing: 0.0{}em; }}\n",
            i % 17,
            i % 9,
            i % 7,
        ));
    }
    for tag in ["p", "em", "span", "a", "li", "td", "h1", "h2", "blockquote"] {
        css.push_str(&format!("section {tag} {{ line-height: 1.{}; }}\n", 4));
    }
    let heavy = Stylesheet::parse(&css);

    c.bench_function("compile_html_heavy_css", |b| {
        b.iter(|| {
            compile_html(
                &html,
                &[
                    (base.clone(), Origin::Author),
                    (heavy.clone(), Origin::Author),
                ],
            )
        });
    });
}

// ============================================================================
// Text Export Benchmarks
// ============================================================================

fn bench_write_markdown(c: &mut Criterion) {
    let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();

    c.bench_function("write_markdown", |b| {
        b.iter(|| {
            let mut output = Vec::new();
            MarkdownExporter::new()
                .export(&mut book, &mut Cursor::new(&mut output))
                .unwrap();
        });
    });
}

criterion_group!(
    benches,
    // Book I/O
    bench_read_epub,
    bench_read_azw3,
    bench_read_kfx,
    bench_write_epub,
    bench_write_azw3,
    bench_write_kfx,
    // Cold end-to-end
    bench_convert_epub_to_kfx_cold,
    bench_convert_epub_to_azw3_cold,
    // Large synthetic book
    bench_large_book_to_kfx_cold,
    bench_large_book_to_azw3_cold,
    // IR pipeline
    bench_compile_html,
    bench_compile_html_no_css,
    bench_compile_html_heavy_css,
    // Text export
    bench_write_markdown,
);
criterion_main!(benches);
