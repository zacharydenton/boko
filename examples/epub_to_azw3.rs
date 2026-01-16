use boko::{read_epub, write_mobi};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    let (input, output) = if args.len() >= 3 {
        (args[1].clone(), args[2].clone())
    } else {
        // Default paths for testing
        ("/tmp/martin_eden_kf8.epub".to_string(), "/tmp/martin_eden_roundtrip.azw3".to_string())
    };

    println!("Reading EPUB: {}", input);
    let book = read_epub(&input).expect("Failed to read EPUB");

    println!("\nBook info:");
    println!("  Title: {}", book.metadata.title);
    println!("  Authors: {:?}", book.metadata.authors);
    println!("  Spine items: {}", book.spine.len());
    println!("  Resources: {}", book.resources.len());

    // Print first few spine hrefs
    println!("\nFirst 5 spine items:");
    for (i, item) in book.spine.iter().take(5).enumerate() {
        println!("  {}: {}", i, item.href);
    }

    println!("\nWriting AZW3: {}", output);
    write_mobi(&book, &output).expect("Failed to write AZW3");

    // Show file size
    let file_size = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
    println!("Output size: {} bytes", file_size);

    // Try to read it back
    println!("\nReading back AZW3...");
    let book2 = boko::read_mobi(&output).expect("Failed to read AZW3");
    println!("  Title: {}", book2.metadata.title);
    println!("  Authors: {:?}", book2.metadata.authors);
    println!("  Spine items: {}", book2.spine.len());
    println!("  Resources: {}", book2.resources.len());

    println!("\nDone!");
}
