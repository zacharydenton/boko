//! Profiling script for EPUB <-> AZW3 conversion
use boko::{read_epub, read_mobi, write_epub, write_mobi};
use std::time::Instant;

fn main() {
    // Use larger test files if available
    let epub_path = if std::path::Path::new("/tmp/bangkok_retail.epub").exists() {
        "/tmp/bangkok_retail.epub"
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/epictetus.epub")
    };
    let azw3_path = if std::path::Path::new("/tmp/bangkok.azw3").exists() {
        "/tmp/bangkok.azw3"
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/epictetus.azw3")
    };

    // Warmup
    let _ = read_epub(epub_path);
    let _ = read_mobi(azw3_path);

    const ITERATIONS: u32 = 3;

    // Profile EPUB -> AZW3
    println!("=== EPUB -> AZW3 ===");
    let mut total_read = 0u128;
    let mut total_write = 0u128;

    for i in 0..ITERATIONS {
        let start = Instant::now();
        let book = read_epub(epub_path).unwrap();
        let read_time = start.elapsed();
        total_read += read_time.as_micros();

        let out_path = format!("/tmp/profile_out_{}.azw3", i);
        let start = Instant::now();
        write_mobi(&book, &out_path).unwrap();
        let write_time = start.elapsed();
        total_write += write_time.as_micros();

        if i == 0 {
            println!("  Read EPUB:  {:>8} µs", read_time.as_micros());
            println!("  Write AZW3: {:>8} µs", write_time.as_micros());
        }
    }
    println!("  Avg Read:   {:>8} µs", total_read / ITERATIONS as u128);
    println!("  Avg Write:  {:>8} µs", total_write / ITERATIONS as u128);

    // Profile AZW3 -> EPUB
    println!("\n=== AZW3 -> EPUB ===");
    total_read = 0;
    total_write = 0;

    for i in 0..ITERATIONS {
        let start = Instant::now();
        let book = read_mobi(azw3_path).unwrap();
        let read_time = start.elapsed();
        total_read += read_time.as_micros();

        let out_path = format!("/tmp/profile_out_{}.epub", i);
        let start = Instant::now();
        write_epub(&book, &out_path).unwrap();
        let write_time = start.elapsed();
        total_write += write_time.as_micros();

        if i == 0 {
            println!("  Read AZW3:  {:>8} µs", read_time.as_micros());
            println!("  Write EPUB: {:>8} µs", write_time.as_micros());
        }
    }
    println!("  Avg Read:   {:>8} µs", total_read / ITERATIONS as u128);
    println!("  Avg Write:  {:>8} µs", total_write / ITERATIONS as u128);

    // File sizes
    println!("\n=== File Sizes ===");
    println!(
        "  EPUB input:  {:>8} bytes",
        std::fs::metadata(epub_path).unwrap().len()
    );
    println!(
        "  AZW3 input:  {:>8} bytes",
        std::fs::metadata(azw3_path).unwrap().len()
    );
    println!(
        "  AZW3 output: {:>8} bytes",
        std::fs::metadata("/tmp/profile_out_0.azw3").unwrap().len()
    );
    println!(
        "  EPUB output: {:>8} bytes",
        std::fs::metadata("/tmp/profile_out_0.epub").unwrap().len()
    );
}
