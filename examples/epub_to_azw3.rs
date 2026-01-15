use ebookconvert::{read_epub, write_mobi};

fn main() {
    // Convert the Martin Eden EPUB we created to AZW3
    let input = "/tmp/martin_eden_kf8.epub";
    let output = "/tmp/martin_eden_roundtrip.azw3";

    println!("Reading EPUB: {}", input);
    let book = read_epub(input).expect("Failed to read EPUB");

    println!("\nBook info:");
    println!("  Title: {}", book.metadata.title);
    println!("  Authors: {:?}", book.metadata.authors);
    println!("  Spine items: {}", book.spine.len());
    println!("  Resources: {}", book.resources.len());

    println!("\nWriting AZW3: {}", output);
    write_mobi(&book, output).expect("Failed to write AZW3");

    // Show file size
    let file_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!("Output size: {} bytes", file_size);

    // Try to read it back
    println!("\nReading back AZW3...");
    let book2 = ebookconvert::read_mobi(output).expect("Failed to read AZW3");
    println!("  Title: {}", book2.metadata.title);
    println!("  Authors: {:?}", book2.metadata.authors);
    println!("  Spine items: {}", book2.spine.len());
    println!("  Resources: {}", book2.resources.len());

    println!("\nDone!");
}
