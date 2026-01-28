# boko

A fast Rust library and CLI for converting between ebook formats.

## Features

- **Multi-format support**: Read and write EPUB, KFX (Kindle Format 10), and AZW3
- **Intermediate representation**: Content is compiled to a semantic IR for accurate format conversion
- **CSS preservation**: Full CSS parsing and transformation between formats
- **Metadata fidelity**: Extended EPUB3 metadata (contributors, series, refinements) round-trips through KFX
- **Lazy loading**: Efficient random access via `ByteSource` trait

## Supported Formats

| Format | Read | Write | Notes |
|--------|------|-------|-------|
| EPUB 2/3 | ✓ | ✓ | Full EPUB3 metadata support |
| KFX | ✓ | ✓ | Kindle Format 10 with enhanced typography |
| AZW3 | ✓ | ✓ | Kindle Format 8 |
| MOBI | ✓ | - | Legacy format, read-only |
| Text | - | ✓ | Plain text export |
| Markdown | - | ✓ | Markdown export |

## Installation

Requires Rust nightly (edition 2024).

```bash
cargo install boko
```

## CLI Usage

### Show book info

```bash
# Human-readable output
boko info book.epub

# JSON output
boko info --json book.epub
```

### Convert between formats

```bash
# EPUB to KFX (latest Kindle format)
boko convert book.epub book.kfx

# EPUB to AZW3
boko convert book.epub book.azw3

# KFX/AZW3/MOBI to EPUB
boko convert book.kfx book.epub

# Export to text or markdown
boko convert book.epub book.txt
boko convert book.epub book.md
```

### Inspect the IR

```bash
# Dump chapter structure
boko dump book.epub

# Show structure without text content
boko dump -s book.epub

# Dump a specific chapter
boko dump -c 0 book.epub

# Show only the style pool
boko dump --styles-only book.epub
```

## Library Usage

```rust
use boko::Book;

// Open a book (format auto-detected from extension)
let mut book = Book::open("input.epub")?;

// Access metadata
println!("Title: {}", book.metadata().title);
println!("Authors: {:?}", book.metadata().authors);

// Iterate chapters
let spine: Vec<_> = book.spine().to_vec();
for entry in spine {
    let chapter = book.load_chapter(entry.id)?;
    println!("Chapter has {} nodes", chapter.nodes.len());
}

// Export to another format
book.export("output.kfx")?;
```

### Working with the IR

Boko compiles ebook content to an intermediate representation (IR) that captures semantic structure:

```rust
use boko::{Book, compile_html, Stylesheet};

// Compile HTML to IR
let html = r#"<p class="intro">Hello <em>world</em></p>"#;
let css = Stylesheet::parse("p.intro { font-size: 1.2em; }");
let chapter = compile_html(html, &css)?;

// Traverse the node tree
for node in &chapter.nodes {
    match &node.content {
        boko::ir::Content::Text(text) => println!("Text: {}", text),
        boko::ir::Content::Element { tag, .. } => println!("Element: {}", tag),
        _ => {}
    }
}
```

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Import    │     │     IR      │     │   Export    │
├─────────────┤     ├─────────────┤     ├─────────────┤
│ EPUB        │────▶│ Nodes       │────▶│ EPUB        │
│ KFX         │     │ Styles      │     │ KFX         │
│ AZW3        │     │ Metadata    │     │ AZW3        │
│ MOBI        │     │ TOC         │     │ Text/MD     │
└─────────────┘     └─────────────┘     └─────────────┘
```

The IR captures:
- **Nodes**: Semantic tree with elements, text, and structure
- **Styles**: Computed CSS properties per node
- **Roles**: Semantic annotations (heading, paragraph, list, etc.)
- **Metadata**: Title, authors, contributors, series, etc.

## Metadata Support

Extended EPUB3 metadata is preserved during conversion:

- `dcterms:modified` - Modification timestamp
- `dc:contributor` with role refinements (translator, editor, illustrator)
- `file-as` refinements for sort ordering
- `belongs-to-collection` with series position

Example output from `boko info`:

```
Title: The Great Novel
Authors: Jane Author
Language: en
Modified: 2024-01-15T12:00:00Z
Title Sort: Great Novel, The
Author Sort: Author, Jane
Contributors:
  John Translator (trl) [Translator, John]
Collection: Epic Saga (series, #2)
```

## License

MIT
