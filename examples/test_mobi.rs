use boko::{read_mobi, write_epub};
use std::io;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).map(|s| s.as_str()).unwrap_or("/tmp/ash-framework_P1.0.mobi");

    println!("Reading {}...", path);
    let book = read_mobi(path)?;

    println!("Title: {}", book.metadata.title);
    println!("Authors: {:?}", book.metadata.authors);
    println!("Resources: {}", book.resources.len());
    println!("Spine: {}", book.spine.len());
    println!("TOC: {}", book.toc.len());

    for entry in book.toc.iter().take(5) {
        println!("  {} -> {}", entry.title, entry.href);
    }

    // Write to EPUB for verification
    let out_path = path.replace(".mobi", "_converted.epub").replace(".azw3", "_converted.epub");
    println!("\nWriting {}...", out_path);
    write_epub(&book, &out_path)?;
    println!("Done!");

    Ok(())
}
