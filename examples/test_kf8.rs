use ebookconvert::{read_mobi, write_epub};

fn main() {
    let input = "/srv/books/Jack London/Martin Eden (448)/Martin Eden - Jack London.azw3";
    let output = "/tmp/martin_eden_kf8.epub";

    println!("Reading: {}", input);
    let book = read_mobi(input).expect("Failed to read AZW3");

    println!("\nTitle: {}", book.metadata.title);
    println!("Authors: {:?}", book.metadata.authors);
    println!("Resources: {}", book.resources.len());
    println!("Spine items: {}", book.spine.len());
    println!("TOC entries: {}", book.toc.len());

    println!("\n--- Spine ---");
    for (i, item) in book.spine.iter().enumerate() {
        let size = book.resources.get(&item.href).map(|r| r.data.len()).unwrap_or(0);
        println!("  {:3}. {} ({} bytes)", i, item.href, size);
    }

    println!("\n--- TOC ---");
    for (i, entry) in book.toc.iter().enumerate() {
        println!("  {:3}. {} -> {}", i, entry.title, entry.href);
    }

    println!("\nWriting: {}", output);
    write_epub(&book, output).expect("Failed to write EPUB");

    // Show file size
    let file_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!("Output size: {} bytes", file_size);
    println!("Done!");
}
