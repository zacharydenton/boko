//! Crash-corpus regression tests.
//!
//! Feeds malformed/truncated bytes through the real library entry point
//! (`Book::from_bytes` plus the lazy load paths) and asserts the importers never
//! panic — they must return `Err`, per the `Importer` contract. A panic here
//! fails the test by aborting the process.
//!
//! The corpus is derived deterministically from the real fixtures (truncations +
//! byte mutations) so it exercises the binary parsers across many input shapes
//! without a fuzzer. As `cargo fuzz` surfaces new crashers, drop the minimized
//! input under `tests/fixtures/crashes/` and it will be picked up here too.

use std::path::Path;

use boko::{Book, Format};

const EPUB: &[u8] = include_bytes!("fixtures/epictetus.epub");
const AZW3: &[u8] = include_bytes!("fixtures/epictetus.azw3");
const MOBI: &[u8] = include_bytes!("fixtures/epictetus.mobi");
const KFX: &[u8] = include_bytes!("fixtures/epictetus.kfx");

/// Drive a book as hard as the importers allow. Any `Err` is fine; the point is
/// that none of these calls panic on malformed input.
fn drive(data: &[u8], format: Format) {
    let Ok(mut book) = Book::from_bytes(data, format) else {
        return;
    };
    let _ = book.metadata();
    let spine: Vec<_> = book.spine().to_vec();
    for entry in &spine {
        let _ = book.load_raw(entry.id);
        let _ = book.load_chapter(entry.id);
    }
    let _ = book.resolve_links();
    let assets: Vec<_> = book.list_assets().to_vec();
    for asset in assets.iter().take(8) {
        let _ = book.load_asset(asset);
    }
    let _ = book.load_asset(Path::new("does/not/exist"));
}

/// Deterministic malformed inputs derived from a valid fixture: truncations at a
/// spread of lengths, plus single/multi-byte mutations at fixed offsets.
fn mutations(seed: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();

    // Truncations — the largest source of odd-sized record buffers.
    out.push(Vec::new());
    for len in [
        1usize, 2, 4, 8, 12, 16, 17, 20, 24, 31, 32, 48, 64, 96, 128, 192, 256, 512, 1024, 4096,
    ] {
        if len < seed.len() {
            out.push(seed[..len].to_vec());
        }
    }
    // A few truncations relative to the full size.
    for frac in [2usize, 3, 4, 8, 16] {
        out.push(seed[..seed.len() / frac].to_vec());
    }

    // Byte mutations at spread-out offsets (headers, offset tables, lengths).
    for &off in &[
        0usize, 1, 4, 8, 16, 20, 24, 32, 64, 78, 100, 128, 256, 512, 1024,
    ] {
        if off < seed.len() {
            for &xor in &[0xFFu8, 0x01, 0x80, 0x7F] {
                let mut m = seed.to_vec();
                m[off] ^= xor;
                out.push(m);
            }
        }
    }

    // Pure garbage of a few sizes.
    for len in [4usize, 33, 200] {
        out.push(vec![0xABu8; len]);
    }

    out
}

fn fuzz_format(seed: &[u8], format: Format) {
    for input in mutations(seed) {
        drive(&input, format);
    }
}

#[test]
fn epub_malformed_inputs_do_not_panic() {
    fuzz_format(EPUB, Format::Epub);
}

#[test]
fn mobi_malformed_inputs_do_not_panic() {
    fuzz_format(MOBI, Format::Mobi);
}

#[test]
fn azw3_malformed_inputs_do_not_panic() {
    fuzz_format(AZW3, Format::Azw3);
}

#[test]
fn kfx_malformed_inputs_do_not_panic() {
    fuzz_format(KFX, Format::Kfx);
}

/// Every format must tolerate being handed another format's bytes.
#[test]
fn cross_format_bytes_do_not_panic() {
    let seeds = [EPUB, AZW3, MOBI, KFX];
    let formats = [Format::Epub, Format::Azw3, Format::Mobi, Format::Kfx];
    for seed in seeds {
        for format in formats {
            drive(seed, format);
        }
    }
}

/// Any minimized crashers committed under fixtures/crashes/ must parse to `Err`
/// (or `Ok`) without panicking. Named `<format>-<desc>` so the format is known.
#[test]
fn committed_crash_corpus_does_not_panic() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crashes");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return; // directory is optional until a fuzzer populates it
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let format = if name.starts_with("epub") {
            Format::Epub
        } else if name.starts_with("azw3") {
            Format::Azw3
        } else if name.starts_with("mobi") {
            Format::Mobi
        } else if name.starts_with("kfx") {
            Format::Kfx
        } else {
            continue;
        };
        if let Ok(data) = std::fs::read(&path) {
            drive(&data, format);
        }
    }
}
