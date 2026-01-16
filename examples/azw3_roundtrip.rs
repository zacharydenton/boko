use boko::{read_mobi, write_epub, read_epub, write_mobi};

fn main() {
    let original = "/srv/books/Jack London/Martin Eden (448)/Martin Eden - Jack London.azw3";
    let epub_out = "/tmp/roundtrip_test.epub";
    let azw3_out = "/tmp/roundtrip_test.azw3";

    println!("=== AZW3 → EPUB → AZW3 Round-trip Test ===\n");

    // Step 1: Read original AZW3
    println!("1. Reading original AZW3: {}", original);
    let book1 = read_mobi(original).expect("Failed to read original AZW3");
    println!("   Title: {}", book1.metadata.title);
    println!("   Authors: {:?}", book1.metadata.authors);
    println!("   Spine: {} items", book1.spine.len());
    println!("   TOC: {} entries", book1.toc.len());

    // Step 2: Write to EPUB
    println!("\n2. Writing EPUB: {}", epub_out);
    write_epub(&book1, epub_out).expect("Failed to write EPUB");
    let epub_size = std::fs::metadata(epub_out).map(|m| m.len()).unwrap_or(0);
    println!("   Size: {} bytes", epub_size);

    // Step 3: Read back EPUB
    println!("\n3. Reading EPUB back...");
    let book2 = read_epub(epub_out).expect("Failed to read EPUB");
    println!("   Title: {}", book2.metadata.title);
    println!("   Spine: {} items", book2.spine.len());

    // Step 4: Write to AZW3
    println!("\n4. Writing AZW3: {}", azw3_out);
    write_mobi(&book2, azw3_out).expect("Failed to write AZW3");
    let azw3_size = std::fs::metadata(azw3_out).map(|m| m.len()).unwrap_or(0);
    println!("   Size: {} bytes", azw3_size);

    // Step 5: Read back AZW3
    println!("\n5. Reading AZW3 back...");
    let book3 = read_mobi(azw3_out).expect("Failed to read AZW3");
    println!("   Title: {}", book3.metadata.title);
    println!("   Authors: {:?}", book3.metadata.authors);
    println!("   Spine: {} items", book3.spine.len());

    // Step 6: Convert back to EPUB for verification
    let final_epub = "/tmp/roundtrip_final.epub";
    println!("\n6. Writing final EPUB for verification: {}", final_epub);
    write_epub(&book3, final_epub).expect("Failed to write final EPUB");
    let final_size = std::fs::metadata(final_epub).map(|m| m.len()).unwrap_or(0);
    println!("   Size: {} bytes", final_size);

    println!("\n=== Round-trip complete! ===");
    println!("Check {} in iBooks", final_epub);
}
