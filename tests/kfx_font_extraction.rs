//! Test KFX bcRawFont entity extraction.
//!
//! Uses a stripped KFX fixture (fonts_only.kfx.gz) containing only
//! bcRawFont ($418) and Font ($262) entities from a real book.

use boko::Book;
use flate2::read::GzDecoder;
use sha1_smol::Sha1;
use std::io::Read;
use std::path::Path;

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::from(bytes).hexdigest()
}

/// Decompress the gzipped fixture to a temp file, returning the path.
fn decompress_fixture() -> Option<tempfile::NamedTempFile> {
    let gz_path = "tests/fixtures/fonts_only.kfx.gz";
    if !Path::new(gz_path).exists() {
        eprintln!("Skipping test - fixture not found: {gz_path}");
        return None;
    }

    let gz_data = std::fs::read(gz_path).expect("Failed to read fixture");
    let mut decoder = GzDecoder::new(&gz_data[..]);
    let mut kfx_data = Vec::new();
    decoder
        .read_to_end(&mut kfx_data)
        .expect("Failed to decompress fixture");

    let mut tmp = tempfile::Builder::new()
        .suffix(".kfx")
        .tempfile()
        .expect("Failed to create temp file");
    std::io::Write::write_all(&mut tmp, &kfx_data).expect("Failed to write temp file");
    Some(tmp)
}

#[test]
fn test_kfx_font_assets_discovered() {
    let Some(tmp) = decompress_fixture() else {
        return;
    };

    let book = Book::open(tmp.path()).expect("Should open stripped KFX");
    let assets = book.list_assets();

    let font_assets: Vec<_> = assets
        .iter()
        .filter(|p| p.to_string_lossy().starts_with("fonts/"))
        .collect();

    assert_eq!(
        font_assets.len(),
        3,
        "Expected 3 font assets, got: {:?}",
        font_assets
    );

    // Verify paths follow the expected naming pattern
    for (i, asset) in font_assets.iter().enumerate() {
        let expected = format!("fonts/font_{i:04}.otf");
        assert_eq!(
            asset.to_string_lossy(),
            expected,
            "Unexpected font path at index {i}"
        );
    }
}

#[test]
fn test_kfx_font_assets_loadable() {
    let Some(tmp) = decompress_fixture() else {
        return;
    };

    let mut book = Book::open(tmp.path()).expect("Should open stripped KFX");

    // Load each font and verify it contains real font data (not Ion metadata)
    let font_paths = [
        "fonts/font_0000.otf",
        "fonts/font_0001.otf",
        "fonts/font_0002.otf",
    ];

    for font_path in &font_paths {
        let bytes = book
            .load_asset(Path::new(font_path))
            .unwrap_or_else(|e| panic!("Failed to load {font_path}: {e}"));

        // Font data should be substantial (real OTF files)
        assert!(
            bytes.len() > 1000,
            "{font_path}: expected substantial font data, got {} bytes",
            bytes.len()
        );

        // Should NOT be Ion binary data (Ion BVM starts with E0 01 00 EA)
        assert!(
            !bytes.starts_with(&[0xE0, 0x01, 0x00, 0xEA]),
            "{font_path}: got Ion metadata instead of font data ({} bytes)",
            bytes.len()
        );

        // Should start with valid font magic bytes
        // OTF/CFF: 4F 54 54 4F ("OTTO")
        // TTF: 00 01 00 00
        // WOFF: 77 4F 46 46 ("wOFF")
        let is_otf = bytes.starts_with(b"OTTO");
        let is_ttf = bytes.starts_with(&[0x00, 0x01, 0x00, 0x00]);
        let is_woff = bytes.starts_with(b"wOFF");
        assert!(
            is_otf || is_ttf || is_woff,
            "{font_path}: unrecognized font magic bytes: {:02X?}",
            &bytes[..4.min(bytes.len())]
        );
    }
}

#[test]
fn test_kfx_font_stable_hashes() {
    let Some(tmp) = decompress_fixture() else {
        return;
    };

    let mut book = Book::open(tmp.path()).expect("Should open stripped KFX");

    // Verify stable SHA-1 hashes for fixture fonts (regression test)
    let expected = [
        (
            "fonts/font_0000.otf",
            "f35ec96c680303b372706d550bf4d70ce44223a7",
        ),
        (
            "fonts/font_0001.otf",
            "d2b42aac1adf8015ad96cbf006dc7dc7b28b13ed",
        ),
        (
            "fonts/font_0002.otf",
            "a85a006557052633a8684213d6554d345ff82925",
        ),
    ];

    for (font_path, expected_sha1) in &expected {
        let bytes = book
            .load_asset(Path::new(font_path))
            .unwrap_or_else(|e| panic!("Failed to load {font_path}: {e}"));
        assert_eq!(
            sha1_hex(&bytes),
            *expected_sha1,
            "SHA-1 mismatch for {font_path}"
        );
    }
}
