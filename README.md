# boko

[![CI](https://github.com/zacharydenton/boko/actions/workflows/ci.yml/badge.svg)](https://github.com/zacharydenton/boko/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/boko.svg)](https://crates.io/crates/boko)
[![docs.rs](https://docs.rs/boko/badge.svg)](https://docs.rs/boko)
[![license](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

boko is a fast ebook converter for EPUB, KFX, AZW3, and MOBI, written in Rust.
It is the only KFX writer that doesn't shell out to Amazon's proprietary Kindle
Previewer, so it runs anywhere: natively on Linux, headless on a server, in
Docker, or entirely in your browser.

KFX is the preferred format for Kindle as of 2026 — it renders with hyphenation, kerning,
and ligatures. AZW3 doesn't. MOBI (Calibre's default Kindle format) is 25 years
old at this point.

**Browser app**: https://zacharydenton.github.io/boko — converts ebooks fully
client-side.

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

KFX/KDF/Ion internals can be inspected with the `kfx-dump` subcommand:

    boko kfx-dump book.kfx
    boko kfx-dump -f metadata -f sections book.kfx

## Library

```rust
use boko::{Book, Format};
use std::fs::File;

let book = Book::open("in.epub")?;
let mut out = File::create("out.kfx")?;
book.export(Format::Kfx, &mut out)?;
```

Full API: https://docs.rs/boko

## FAQ

### How do I convert EPUB to KFX on Linux without Kindle Previewer?

`cargo install boko`, then `boko convert in.epub out.kfx`.

### How is this different from the Calibre KFX Output plugin?

The Calibre plugin doesn't write KFX itself — it drives Amazon's Kindle
Previewer under the hood, so you must install Previewer (on Linux: under
Wine, where its GUI doesn't work, and not at all inside Flatpak/Snap
containers). boko is an independent KFX writer with no external
dependencies.

### Can I read KFX files with it too?

Yes — KFX import is supported, so you can convert your existing (DRM-free)
.kfx books to EPUB, AZW3, Markdown, or plain text.

### Why doesn't my sideloaded book show its cover in the Kindle library?

The Kindle never renders library covers from a sideloaded book itself — it
looks up a sidecar image on the device, keyed by the book's metadata. Generate
it (here with [libvips](https://www.libvips.org/)) and copy it alongside the
book:

    id=$(boko kfx-dump -f metadata book.kfx | awk '/content_id/ {print $2}')
    vipsthumbnail cover.jpg -s 330x500 -o thumbnail_${id}_EBOK_portrait.jpg

where `cover.jpg` is the book's cover image. The book goes in `documents/`,
the thumbnail in `system/thumbnails/`.

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

Fuzzing the import parsers (requires nightly and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)):

    cargo +nightly fuzz run from_bytes

## License

[GPL-3.0-or-later](LICENSE).
