# boko

[![CI](https://github.com/zacharydenton/boko/actions/workflows/ci.yml/badge.svg)](https://github.com/zacharydenton/boko/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/boko.svg)](https://crates.io/crates/boko)
[![docs.rs](https://docs.rs/boko/badge.svg)](https://docs.rs/boko)
[![license](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

Boko is a fast ebook converter for EPUB, KFX, AZW3, and MOBI, written in Rust.

KFX renders with hyphenation, kerning, and ligatures. AZW3 doesn't. MOBI (Calibre's default Kindle format) is 25 years old at this point. It's 2026, use boko to send .kfx files to your Kindle!

Browser app: https://zacharydenton.github.io/boko Converts ebooks in your browser, fully client-side (even on your phone).

## Formats

| Format | Read | Write |
|--------|------|-------|
| KFX | yes | yes |
| AZW3 | yes | yes |
| EPUB 2/3 | yes | yes |
| MOBI | yes | no |
| Markdown | no | yes |
| Plain text | no | yes |

## Install

Requires Rust 1.85+.

    cargo install boko        # CLI
    cargo add boko            # library

## CLI

    boko convert in.epub out.kfx
    boko convert in.epub out.azw3
    boko convert in.kfx  out.epub

    boko info in.epub
    boko info --json in.epub

    boko dump in.epub
    boko dump -c 0 in.epub

`kfx-dump` is installed alongside `boko`.

## Library

```rust
use boko::{Book, Format};
use std::fs::File;

let mut book = Book::open("in.epub")?;
let mut out = File::create("out.kfx")?;
book.export(Format::Kfx, &mut out)?;
```

Full API: https://docs.rs/boko

## Architecture

Format → semantic IR → format. Imports compile to an intermediate representation: nodes, computed styles, semantic roles, metadata, TOC. Exporters render IR back out.

```
EPUB ─┐                    ┌─ EPUB
KFX  ─┼─→  semantic IR  ─→─┼─ KFX
AZW3 ─┤                    ├─ AZW3
MOBI ─┘                    └─ Markdown / text
```

## Contributing

Bug reports with sample files welcome, especially KFX and AZW3 edge cases.

    cargo test
    cargo clippy -- -D warnings
    cargo fmt --check

## License

[GPL-3.0-or-later](LICENSE).
