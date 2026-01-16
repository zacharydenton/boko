use boko::{read_mobi, write_epub};

fn main() {
    let input = "/srv/books/Jack London/Martin Eden (448)/Martin Eden - Jack London.azw3";
    let output = "/tmp/martin_eden_converted.epub";

    println!("Reading: {}", input);
    let book = read_mobi(input).expect("Failed to read AZW3");

    println!("Title: {}", book.metadata.title);
    println!("Authors: {:?}", book.metadata.authors);
    println!("Resources: {}", book.resources.len());
    println!("Spine items: {}", book.spine.len());
    println!("TOC entries: {}", book.toc.len());

    for entry in &book.toc {
        println!("  - {}", entry.title);
    }

    println!("\nWriting: {}", output);
    write_epub(&book, output).expect("Failed to write EPUB");
    println!("Done!");
}
