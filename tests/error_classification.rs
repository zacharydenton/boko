//! The typed `boko::Error` variants must classify failures consistently
//! across formats: a missing resource is `NotFound`, corrupt bytes are
//! `Malformed`, and an export-only format requested for import is
//! `UnsupportedFormat`. These are the guarantees the 0.4 error API makes.

use std::path::Path;

use boko::{Book, Error, Format};

const EPUB: &str = "tests/fixtures/epictetus.epub";
const KFX: &str = "tests/fixtures/epictetus.kfx";

#[test]
fn missing_asset_is_not_found_for_every_format() {
    for (path, format) in [(EPUB, "epub"), (KFX, "kfx")] {
        if !Path::new(path).exists() {
            continue;
        }
        let mut book = Book::open(path).expect("fixture opens");
        let err = book
            .load_asset(Path::new("does/not/exist.xyz"))
            .expect_err("missing asset must error");
        assert!(
            matches!(err, Error::NotFound { .. }),
            "{format}: missing asset should be NotFound, got {err:?}"
        );
    }
}

#[test]
fn corrupt_kfx_is_malformed_not_io() {
    // Truncated KFX: the container header is intact enough to open but entity
    // parsing fails. The failure must classify as Malformed { Kfx }, not the
    // generic Io that a real disk error would produce.
    let Ok(bytes) = std::fs::read(KFX) else {
        return;
    };
    // Corrupt the tail so entity/Ion parsing hits garbage.
    let mut corrupt = bytes.clone();
    for b in corrupt.iter_mut().skip(bytes.len() / 2) {
        *b = 0xFF;
    }

    match Book::from_bytes(&corrupt, Format::Kfx) {
        // Failure at open time is acceptable, but it must be typed.
        Err(e) => assert!(
            matches!(
                e,
                Error::Malformed {
                    format: Format::Kfx,
                    ..
                }
            ),
            "corrupt KFX open should be Malformed, got {e:?}"
        ),
        // If it opens, driving a chapter must surface Malformed, never Io.
        Ok(mut book) => {
            let spine: Vec<_> = book.spine().to_vec();
            for entry in spine {
                if let Err(e) = book.load_chapter(entry.id) {
                    assert!(
                        matches!(
                            e,
                            Error::Malformed {
                                format: Format::Kfx,
                                ..
                            }
                        ),
                        "corrupt KFX chapter should be Malformed, got {e:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn markdown_import_is_unsupported_format() {
    match Book::from_bytes(b"# hi", Format::Markdown) {
        Err(e) => assert!(matches!(e, Error::UnsupportedFormat { .. }), "got {e:?}"),
        Ok(_) => panic!("markdown import should be unsupported"),
    }
}
