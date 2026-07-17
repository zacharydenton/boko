# boko

[![CI](https://github.com/zacharydenton/boko/actions/workflows/ci.yml/badge.svg)](https://github.com/zacharydenton/boko/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/boko.svg)](https://crates.io/crates/boko)
[![docs.rs](https://docs.rs/boko/badge.svg)](https://docs.rs/boko)
[![license](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

**Convert EPUB to KFX natively — no Kindle Previewer, no Wine, no Amazon software.**

boko is a fast ebook converter for EPUB, KFX, AZW3, and MOBI, written in Rust.
It is the only KFX writer that doesn't shell out to Amazon's proprietary Kindle
Previewer, so it runs anywhere: natively on Linux, headless on a server, in
Docker, or entirely in your browser.

KFX is the format Kindles actually use — it renders with hyphenation, kerning,
and ligatures. AZW3 doesn't. MOBI (Calibre's default Kindle format) is 25 years
old at this point. It's 2026, use boko to send .kfx files to your Kindle!

**Browser app**: https://zacharydenton.github.io/boko — converts ebooks fully
client-side. No upload, no account; your books never leave your device.

## Why boko

Every other route to KFX is a wrapper around Amazon's Kindle Previewer — a GUI
app with no Linux version, no headless mode, and conversion times measured in
tens of seconds per book. boko is an independent, native implementation.

|                        | boko | Calibre KFX Output plugin | Kindle Previewer |
|------------------------|------|---------------------------|------------------|
| Requires Kindle Previewer | **no** | yes (it's a bridge to it) | — |
| Linux                  | **native** | via Wine | via Wine (no GUI) |
| Headless / Docker / CI | **yes** | painful | no |
| In-browser (WASM)      | **yes** | no | no |
| Library API            | **Rust crate** | no | no |
| Typical book           | **milliseconds** | seconds–minutes | 30 s+ |

Speed, measured on real books: a typical novel converts EPUB→KFX in ~10 ms; an
83 MB image-heavy travel guide in ~0.25 s. Converting a 1,234-book library took
~5 seconds on a 32-core machine (warm cache) — the same library through
Previewer-based pipelines is an overnight job. In a like-for-like EPUB→AZW3
test against Calibre's `ebook-convert`, boko averaged ~70× faster per book.

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

`cargo install boko`, then `boko convert in.epub out.kfx`. That's it — no
Wine, no Previewer, no Amazon account. It works the same in Docker or CI,
and there's a WASM build if you want it in a browser.

### How is this different from the Calibre KFX Output plugin?

The Calibre plugin doesn't write KFX itself — it drives Amazon's Kindle
Previewer under the hood, so you must install Previewer (on Linux: under
Wine, where its GUI doesn't work, and not at all inside Flatpak/Snap
containers). boko is an independent KFX serializer with no external
dependencies.

### Can I read KFX files with it too?

Yes — KFX import is supported, so you can convert your existing (DRM-free)
.kfx books to EPUB, AZW3, Markdown, or plain text.

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

Drop minimized crashers into `tests/fixtures/crashes/` — the crash-corpus
test replays them on every `cargo test`.

## License

[GPL-3.0-or-later](LICENSE).
