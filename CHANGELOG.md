# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
  Ion codec; `ion-rs` was only needed by the `kfx-dump` binary. Consumers using
  `default-features = false` who relied on `ion-rs` being pulled in transitively
  must enable the `cli` feature.

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

## [0.3.0]

- GPL-3.0-or-later license; KFX-first README.

[0.4.0]: https://github.com/zacharydenton/boko/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/zacharydenton/boko/releases/tag/v0.3.0
