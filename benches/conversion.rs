use std::io::Cursor;

use criterion::{Criterion, criterion_group, criterion_main};

use boko::epub::{read_epub_from_reader, write_epub_to_writer};
use boko::mobi::{read_mobi_from_reader, write_mobi_to_writer};

const EPUB_BYTES: &[u8] = include_bytes!("../tests/fixtures/epictetus.epub");
const AZW3_BYTES: &[u8] = include_bytes!("../tests/fixtures/epictetus.azw3");

fn bench_write_azw3(c: &mut Criterion) {
    let book = read_epub_from_reader(Cursor::new(EPUB_BYTES)).unwrap();

    c.bench_function("write_azw3", |b| {
        b.iter(|| {
            let mut output = Cursor::new(Vec::new());
            write_mobi_to_writer(&book, &mut output).unwrap();
        });
    });
}

fn bench_write_epub(c: &mut Criterion) {
    let book = read_mobi_from_reader(Cursor::new(AZW3_BYTES)).unwrap();

    c.bench_function("write_epub", |b| {
        b.iter(|| {
            let mut output = Vec::new();
            write_epub_to_writer(&book, Cursor::new(&mut output)).unwrap();
        });
    });
}

fn bench_read_epub(c: &mut Criterion) {
    c.bench_function("read_epub", |b| {
        b.iter(|| {
            read_epub_from_reader(Cursor::new(EPUB_BYTES)).unwrap();
        });
    });
}

fn bench_read_azw3(c: &mut Criterion) {
    c.bench_function("read_azw3", |b| {
        b.iter(|| {
            read_mobi_from_reader(Cursor::new(AZW3_BYTES)).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_write_azw3,
    bench_write_epub,
    bench_read_epub,
    bench_read_azw3,
);
criterion_main!(benches);
