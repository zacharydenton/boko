use boko::{read_epub, read_mobi, write_epub, write_mobi};
use std::time::Instant;

fn main() {
    let epubs: Vec<_> = walkdir::WalkDir::new("/srv/books")
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "epub").unwrap_or(false))
        .take(100)
        .map(|e| e.path().to_path_buf())
        .collect();
    
    let azw3s: Vec<_> = walkdir::WalkDir::new("/srv/books")
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "azw3").unwrap_or(false))
        .take(100)
        .map(|e| e.path().to_path_buf())
        .collect();

    println!("=== Benchmark (with STORED for images) ===\n");

    let start = Instant::now();
    let mut c = 0;
    for p in &epubs { if read_epub(p).and_then(|b| write_mobi(&b, "/tmp/b.azw3")).is_ok() { c += 1; } }
    let e = start.elapsed();
    println!("EPUB→AZW3: {} in {:?} ({:.1}/sec, {:.1}ms)", c, e, c as f64/e.as_secs_f64(), e.as_millis() as f64/c as f64);

    let start = Instant::now();
    c = 0;
    for p in &epubs { if read_epub(p).and_then(|b| write_epub(&b, "/tmp/b.epub")).is_ok() { c += 1; } }
    let e = start.elapsed();
    println!("EPUB→EPUB: {} in {:?} ({:.1}/sec, {:.1}ms)", c, e, c as f64/e.as_secs_f64(), e.as_millis() as f64/c as f64);

    let start = Instant::now();
    c = 0;
    for p in &azw3s { if read_mobi(p).and_then(|b| write_epub(&b, "/tmp/b.epub")).is_ok() { c += 1; } }
    let e = start.elapsed();
    println!("AZW3→EPUB: {} in {:?} ({:.1}/sec, {:.1}ms)", c, e, c as f64/e.as_secs_f64(), e.as_millis() as f64/c as f64);

    let start = Instant::now();
    c = 0;
    for p in &azw3s { if read_mobi(p).and_then(|b| write_mobi(&b, "/tmp/b.azw3")).is_ok() { c += 1; } }
    let e = start.elapsed();
    println!("AZW3→AZW3: {} in {:?} ({:.1}/sec, {:.1}ms)", c, e, c as f64/e.as_secs_f64(), e.as_millis() as f64/c as f64);
}
