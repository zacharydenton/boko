# boko

[![CI](https://github.com/zacharydenton/boko/actions/workflows/ci.yml/badge.svg)](https://github.com/zacharydenton/boko/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/boko.svg)](https://crates.io/crates/boko)
[![docs.rs](https://docs.rs/boko/badge.svg)](https://docs.rs/boko)

A fast, lightweight Rust library for converting between EPUB and Kindle (AZW3/MOBI) ebook formats.

## Features

- **Fast**: 100-200x faster than calibre's ebook-convert
- **Lightweight**: Pure Rust with minimal dependencies
- **Complete**: Preserves metadata, table of contents, images, fonts, and CSS
- **Flexible**: Use as a library or command-line tool
- **Cross-platform**: Native binaries for all platforms, plus WebAssembly for browsers

## Performance

Benchmarks converting a 450KB ebook on an AMD Ryzen 9 7950X:

| Conversion | boko | calibre | Speedup |
|------------|------|---------|---------|
| EPUB → AZW3 | 4.5 ms | 1,039 ms | **230x** |
| AZW3 → EPUB | 3.2 ms | 564 ms | **176x** |

## Installation

Requires Rust nightly (uses edition 2024 features):

```bash
rustup default nightly
cargo install boko
```

Or add to your `Cargo.toml`:

```toml
[dependencies]
boko = "0.1"
```

## Usage

### Command Line

```bash
# Convert EPUB to AZW3
boko book.epub book.azw3

# Convert AZW3/MOBI to EPUB
boko book.azw3 book.epub

# Show book metadata
boko -i book.epub
```

### Library

```rust
use boko::Book;

// Open and convert with automatic format detection
let book = Book::open("input.epub")?;
book.save("output.azw3")?;
```

For explicit format control (Format: Epub, Azw3, Mobi):

```rust
use boko::{Book, Format};

let book = Book::open_format("input.bin", Format::Mobi)?;
book.save_format("output.bin", Format::Azw3)?;
```

Free functions are also available:

```rust
use boko::{read_epub, write_mobi};

let book = read_epub("input.epub")?;
write_mobi(&book, "output.azw3")?;
```

### Creating Books Programmatically

```rust
use boko::{Book, Metadata, TocEntry};

let mut book = Book::new();
book.metadata = Metadata::new("My Book")
    .with_author("Author Name")
    .with_language("en");

// Add content
book.add_resource(
    "chapter1.xhtml",
    b"<?xml version=\"1.0\"?>
      <html><body><h1>Chapter 1</h1><p>Hello, world!</p></body></html>".to_vec(),
    "application/xhtml+xml"
);
book.add_spine_item("ch1", "chapter1.xhtml", "application/xhtml+xml");
book.toc.push(TocEntry::new("Chapter 1", "chapter1.xhtml"));

// Save as EPUB or AZW3
book.save("my-book.epub")?;
book.save("my-book.azw3")?;
```

## Web App

A browser-based converter is available at [zacharydenton.github.io/boko](https://zacharydenton.github.io/boko). All conversions happen locally in your browser using WebAssembly.

## Supported Formats

| Format | Read | Write |
|--------|------|-------|
| EPUB 2/3 | ✓ | ✓ |
| AZW3 (KF8) | ✓ | ✓ |
| MOBI | ✓ | ✓* |

*MOBI output uses the modern KF8 format (same as AZW3).

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## License

MIT
