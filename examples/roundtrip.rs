use boko::{read_epub, read_mobi, write_epub};

fn main() {
    // Test EPUB
    println!("=== EPUB Test ===");
    let epub_input = "/srv/books/Anne Bronte/Agnes Grey (669)/Agnes Grey - Anne Bronte.epub";
    let epub_output = "/tmp/agnes_grey_roundtrip.epub";

    println!("Reading: {}", epub_input);
    let book = read_epub(epub_input).expect("Failed to read EPUB");
    print_book_info(&book);

    println!("\nWriting: {}", epub_output);
    write_epub(&book, epub_output).expect("Failed to write EPUB");
    println!("Done!");

    // Test MOBI/AZW3
    println!("\n=== AZW3 Test ===");
    let azw3_input = "/srv/books/Vernor Vinge/True Names (34)/True Names - Vernor Vinge.azw3";
    let azw3_output = "/tmp/true_names_converted.epub";

    println!("Reading: {}", azw3_input);
    match read_mobi(azw3_input) {
        Ok(book) => {
            print_book_info(&book);

            println!("\nConverting to EPUB: {}", azw3_output);
            write_epub(&book, azw3_output).expect("Failed to write EPUB");
            println!("Done!");
        }
        Err(e) => {
            println!("Failed to read AZW3: {}", e);
        }
    }
}

fn print_book_info(book: &boko::Book) {
    println!("Title: {}", book.metadata.title);
    println!("Authors: {:?}", book.metadata.authors);
    println!("Resources: {}", book.resources.len());
    println!("Spine items: {}", book.spine.len());
    println!("TOC entries: {}", book.toc.len());
    if let Some(ref cover) = book.metadata.cover_image {
        println!("Cover: {}", cover);
    }
}
