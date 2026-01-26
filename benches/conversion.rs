//! Benchmarks for ebook conversion pipeline.
//!
//! Run with: cargo bench

use std::io::Cursor;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};

use boko::export::{Azw3Exporter, EpubExporter, Exporter, TextExporter};
use boko::{Book, Format, compile_html, Origin, Stylesheet};

const EPUB_BYTES: &[u8] = include_bytes!("../tests/fixtures/epictetus.epub");
const AZW3_BYTES: &[u8] = include_bytes!("../tests/fixtures/epictetus.azw3");

/// Load sample HTML and CSS from the epub fixture for IR benchmarks.
fn load_sample_content() -> (String, Stylesheet) {
    let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();

    // Find the enchiridion chapter (largest content chapter)
    let spine: Vec<_> = book.spine().to_vec();
    let chapter_id = spine
        .iter()
        .find(|e| {
            book.source_id(e.id)
                .map(|s| s.contains("enchiridion"))
                .unwrap_or(false)
        })
        .map(|e| e.id)
        .unwrap_or(spine[0].id);

    let html_bytes = book.load_raw(chapter_id).unwrap();
    let html = String::from_utf8_lossy(&html_bytes).into_owned();

    // Load CSS
    let css_bytes = book.load_asset(Path::new("epub/css/core.css")).unwrap();
    let css = String::from_utf8_lossy(&css_bytes);
    let stylesheet = Stylesheet::parse(&css);

    (html, stylesheet)
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

// ============================================================================
// Text Export Benchmarks
// ============================================================================

fn bench_write_markdown(c: &mut Criterion) {
    let mut book = Book::from_bytes(EPUB_BYTES, Format::Epub).unwrap();

    c.bench_function("write_markdown", |b| {
        b.iter(|| {
            let mut output = Vec::new();
            TextExporter::new()
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
    bench_write_epub,
    bench_write_azw3,
    // IR pipeline
    bench_compile_html,
    bench_compile_html_no_css,
    // Text export
    bench_write_markdown,
);
criterion_main!(benches);
