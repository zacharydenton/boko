# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-07-19

This release is about trustworthy KFX at library scale: the writer now follows
the reference content model and validates against the reference toolchain
(jhowell's kfxlib) — 98.3% of a 1,433-book real-world library passes the
deepest checks, including a trial conversion back to EPUB — and a new
optimization pipeline shrinks books for the device. No breaking API changes.

### Added

- **`Book::optimize()` and `boko convert -O/--optimize`** — a pass-based
  asset optimizer. The `images` pass downscales raster images to the 11th-gen
  Kindle Paperwhite content width (1236px long edge, Lanczos3) and re-encodes
  them as JPEG (quality 80, alpha flattened onto white), keeping the original
  whenever the result isn't meaningfully smaller — line art and flat-color
  PNGs survive untouched, as does anything referenced from CSS. Renamed
  assets are rewritten transparently (chapter `src` references, the cover
  path), so the saving applies to every output format; a typical image-heavy
  book shrinks by half or more. Gated behind the new `optimize-images`
  feature (pulls in the `image` crate; on for the CLI, off for wasm).
- **`tools/kfxcheck.py`** — an epubcheck-style KFX validator built on
  jhowell's kfxlib. Runs the full structural battery plus position/location
  map verification and a trial in-memory EPUB conversion; self-contained
  (auto-downloads the KFX Input plugin on first use), with `--json` reports
  and epubcheck-style exit codes. The test suite gates a real conversion on
  it end to end.
- **EPUB font deobfuscation.** Fonts listed in `META-INF/encryption.xml`
  under the IDPF or Adobe scheme are deobfuscated at import — previously they
  shipped into KFX still XOR-scrambled and never rendered. Every
  `dc:identifier` is tried as a key candidate and results are validated by
  font magic; undecodable fonts pass through unchanged.
- **Cover thumbnails for identifier-less books.** `content_id`/`book_id` are
  now always emitted, seeding from title+author when the source has no
  identifier — the Kindle keys sideloaded cover thumbnails by `content_id`,
  so such books could never show a cover.
- Approximate page list generation ($237), element-aligned locations, and
  WOFF/WOFF2 font recognition in KFX output.
- Declared MSRV: Rust 1.91 (`rust-version` in Cargo.toml).

### Changed

- **KFX output follows the reference content model.** Elements never mix
  their own text with element children (inline runs are wrapped per
  Previewer's encoding); tables and sidebars take their reference shape;
  styles and anchors are emitted from a reference walk in deterministic
  order, only when referenced; metadata defaults, conditional feature
  declarations, and font layout match reference output.
- Conformance encodings validated against Previewer gold masters:
  `yj.semantics.*` markers ride only `$269` text elements; scale-fit page
  template images reference an empty style; `visibility` encodes as an Ion
  boolean; percentage font sizes fold to `em` (readers prune inherited
  percentage values, breaking font-size resolution for descendants);
  `box_align` never rides inline-run styles.
- Import performance: SIMD scans for filepos targets and pagebreaks, hot
  maps on `FxHashMap`, pre-compressed EPUB asset passthrough, O(1) child
  append in the IR arena.

### Fixed

- Bordered images were swallowed entirely by the border container-wrapper,
  which assumes text content — tech books lost every bordered screenshot.
  Images are no longer wrapped; border styling stays on the image element.
- Anchor offsets into dropped marker-only text produced positions readers
  cannot locate — broken in-book navigation, worst in link-dense books.
  Offsets now clamp to the element start.
- A misplaced self-closing solidus in real AZW3 markup
  (`<img src="x.gif"/ alt="">`) glued the next attribute into the src value,
  silently breaking image references; it is now relocated to the tag end.
- Empty chapters emitted `content_list: null`, which KFX consumers reject;
  they now emit an empty list.
- CSS cascade and value-parsing correctness bugs; dangling links and EPUB 3
  validity in normalized export; AZW3 normalization, XML entity, and cp1252
  handling; markdown table and footnote rendering; parsers hardened against
  malformed and hostile input.

## [0.4.0] - 2026-07-14

This release introduces a typed error API, a large performance improvement to
KFX and AZW3 export, embedded-font support, and a substantial internal
refactor. It contains breaking changes to the public API.

### Breaking

- **Typed errors.** The `Importer` and `Exporter` traits and every fallible
  `Book` method now return `boko::Result<T>` (`Result<T, boko::Error>`) instead
  of `std::io::Result<T>`. `boko::Error` is a new enum with `Io`,
  `UnsupportedFormat`, `Malformed { format, context }`, `DrmProtected(format)`,
  and `NotFound { what }` variants. `From<std::io::Error>` and
  `From<boko::Error> for std::io::Error` are provided, so code that kept an
  `io::Result` boundary continues to compile via `?` and `.into()`.
  - Two `io::ErrorKind` mappings changed for callers using the compat shim:
    a DRM-protected file now maps to `PermissionDenied` (was `InvalidData`),
    and `Book::open` on an unknown extension maps to `Unsupported` (was
    `InvalidInput`).
- **`Importer::load_stylesheet`** now returns `Option<Arc<Stylesheet>>` instead
  of `Option<Stylesheet>`, so cached stylesheets are shared rather than cloned.
  Custom `Importer` implementations must update the return type.
- **`KfxConfig` and `KfxExporter::with_config` were removed.** They configured
  nothing. Use `KfxExporter::new()` / `KfxExporter::default()`. (`Azw3Config`
  and `EpubConfig`, which carry real options, are unchanged.)
- **`ion-rs` is now gated behind the `cli` feature.** The library uses its own
  Ion codec; `ion-rs` was only needed by the KFX dump tool. Consumers using
  `default-features = false` who relied on `ion-rs` being pulled in transitively
  must enable the `cli` feature.
- **`Book` and the `Importer`/`Exporter` traits operate on shared references.**
  Every data method (`load_raw`, `load_asset`, `load_chapter`, `load_stylesheet`,
  `font_faces`, …) takes `&self`; `Exporter::export` and `Book::export` take
  `&Book`; `Book::resolve_links` is `&self`, memoized, and returns
  `Arc<ResolvedLinks>`. `Importer::toc_mut` is gone — `resolve_toc` returns a
  fixed-up copy and `Book` caches the resolved views. Custom importers convert
  interior caches to `OnceLock`/`RwLock`; in exchange, chapter compilation
  parallelizes for every format and books can be exported concurrently.
- **The standalone `kfx-dump` binary is now the `boko kfx-dump` subcommand.**
  Same flags (`-r`, `-s`, `-f <field>`), byte-identical output; `cargo install
  boko` no longer places a second binary on `PATH`.
- **Archive entry names are strings.** `Importer::list_assets` returns
  `&[String]`, and `load_asset`/`load_stylesheet` take `&str` — names inside
  EPUB/MOBI/KFX containers are UTF-8 zip paths, not OS paths. `Importer::open`
  still takes `&Path`. `kfx::container::read_u16_le/read_u32_le/read_u64_le`
  now return `Option` instead of panicking on truncated buffers.

### Added

- `boko::Error` and `boko::Result` for programmatic failure handling. Corrupt
  input is classified as `Malformed`, missing resources as `NotFound`, and
  encrypted books as `DrmProtected`, consistently across the EPUB, AZW3, MOBI,
  and KFX importers.
- `KfxExporter`, `Azw3Config`, and `EpubConfig` are now re-exported at the crate
  root alongside the other exporters.
- Embedded-font support: AZW3/MOBI font extraction (decoding Kindle `FONT`
  container records), and the EPUB exporter now writes embedded font assets and
  resolves TOC fragments.
- The `.azw` extension is recognized as MOBI.
- A `cargo-fuzz` scaffold (`fuzz/`) with a `from_bytes` target covering all four
  import parsers; run with `cargo +nightly fuzz run from_bytes`.
- Benchmarks for cold end-to-end conversion, a large synthetic book, and a
  cascade-stress case.

### Performance

- KFX export is roughly 6–7× faster on cold conversions, and `compile_html` ~3×
  faster, from: memoizing the IR→KFX style conversion per chapter, sharing
  parsed stylesheets via `Arc` instead of deep-cloning them per chapter, reusing
  cascade scratch buffers across elements, parsing each chapter's HTML once
  instead of twice, bucketing cascade rules by their rightmost selector, caching
  the user-agent stylesheet per thread, and reusing cached chapter IR during
  export.

### Fixed

- Ion parser: bounds arithmetic is now overflow-checked, and decimals with a
  large-magnitude negative exponent are rejected instead of driving a
  multi-exabyte allocation (found by fuzzing).
- Import parsers are hardened against malformed input (checked lengths, bounded
  recursion, no unbounded allocations).
- `ElementRef::opaque` keys the selector-match cache on the arena node rather
  than a transient reference, preventing cache aliasing when the cache is shared
  across a chapter.
- KFX: strip top-level Ion annotations in `parse_entity_ion` so annotated
  entities are read correctly.

### Internal

- The largest modules were split into directory modules with no behavior change:
  `export/kfx`, `kfx/storyline`, `export/azw3`, and MOBI HTML chapter splitting
  moved to `mobi/split`.
- Dead code removed (`KfxConfig`, an unused chapter-entity assembler, and
  several write-only struct fields).
- A diverse synthetic EPUB test corpus with roundtrip invariants, and
  `tests/error_classification.rs` locking in the error-variant guarantees.

### Contributors

Thanks to [@Imaclean74](https://github.com/Imaclean74) for the embedded-font
and format-compatibility work in this release:

- Decode Kindle `FONT` container records for AZW3/MOBI font extraction (#21)
- EPUB export: resolve TOC fragments and write embedded font assets (#23)
- Strip top-level Ion annotations so annotated KFX entities parse (#19)
- Recognize the `.azw` extension as MOBI (#18)

## [0.3.0]

- GPL-3.0-or-later license; KFX-first README.

[0.5.0]: https://github.com/zacharydenton/boko/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/zacharydenton/boko/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/zacharydenton/boko/releases/tag/v0.3.0
