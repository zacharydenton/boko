//! Simple ebook converter CLI

use std::env;
use std::path::Path;

use boko::{read_epub, read_mobi, write_epub, write_mobi};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        eprintln!("Usage: {} <input> <output>", args[0]);
        eprintln!("Supported formats: .epub, .azw3, .mobi");
        std::process::exit(1);
    }

    let input = &args[1];
    let output = &args[2];

    let input_ext = Path::new(input)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let output_ext = Path::new(output)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Read input
    println!("Reading {}...", input);
    let book = match input_ext.as_str() {
        "epub" => read_epub(input).expect("Failed to read EPUB"),
        "azw3" | "mobi" => read_mobi(input).expect("Failed to read MOBI/AZW3"),
        _ => {
            eprintln!("Unsupported input format: {}", input_ext);
            std::process::exit(1);
        }
    };

    println!("  Title: {}", book.metadata.title);
    println!("  Authors: {}", book.metadata.authors.join(", "));
    println!("  Spine: {} items", book.spine.len());
    println!("  TOC: {} entries", book.toc.len());
    println!("  Resources: {}", book.resources.len());

    // Write output
    println!("Writing {}...", output);
    match output_ext.as_str() {
        "epub" => write_epub(&book, output).expect("Failed to write EPUB"),
        "azw3" | "mobi" => write_mobi(&book, output).expect("Failed to write MOBI/AZW3"),
        _ => {
            eprintln!("Unsupported output format: {}", output_ext);
            std::process::exit(1);
        }
    };

    println!("Done!");
}
