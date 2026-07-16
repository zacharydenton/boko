//! KFX export must be byte-for-byte reproducible.
//!
//! Two conversions of the same book have to produce identical bytes: the
//! container ID is derived from the book's identity, style-schema rules are
//! iterated in registration order, and landmark/navigation ordering is
//! totally ordered. Any HashMap-iteration dependence in the export path
//! breaks this test.

mod common;

use boko::model::Format;

#[test]
fn kfx_export_is_byte_reproducible() {
    let export = || {
        let mut book = common::open_fixture("epictetus.epub");
        common::export_to_bytes(&mut book, Format::Kfx)
    };

    let first = export();
    let second = export();

    assert!(!first.is_empty());
    if first != second {
        let diverge = first
            .iter()
            .zip(second.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(first.len().min(second.len()));
        panic!(
            "KFX export is not reproducible: sizes {} vs {}, first divergence at byte {}",
            first.len(),
            second.len(),
            diverge
        );
    }
}

#[test]
fn azw3_export_is_reproducible_modulo_pdb_timestamps() {
    let export = || {
        let mut book = common::open_fixture("epictetus.epub");
        common::export_to_bytes(&mut book, Format::Azw3)
    };
    let mut first = export();
    let mut second = export();
    assert!(first.len() > 48);

    // The PDB header legitimately stamps the current time: creation,
    // modification, and last-backup dates at offsets 36..48. Everything
    // else must be identical.
    first[36..48].fill(0);
    second[36..48].fill(0);
    assert_eq!(
        first, second,
        "AZW3 export differs beyond the PDB header timestamps"
    );
}

#[test]
fn epub_export_is_byte_reproducible() {
    let export = || {
        let mut book = common::open_fixture("epictetus.epub");
        common::export_to_bytes(&mut book, Format::Epub)
    };
    let first = export();
    let second = export();
    assert!(!first.is_empty());
    assert_eq!(first, second, "EPUB export is not reproducible");
}
