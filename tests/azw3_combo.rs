//! Combo files hold a MOBI6 half and a KF8 half, and either can hold resources.
//! `kindle:embed` numbers them as one list, the first half's and then the KF8
//! half's, so the KF8 importer must look in both.

use std::io::Cursor;

use boko::{Book, Format};

const COMBO: &[u8] = include_bytes!("fixtures/combo.mobi");

#[test]
fn combo_images_are_found_through_the_kf8_half() {
    let book = Book::from_bytes(COMBO, Format::Azw3).unwrap();

    let images: Vec<_> = book
        .list_assets()
        .iter()
        .filter(|a| a.starts_with("images/"))
        .cloned()
        .collect();
    assert!(images.len() >= 2, "cover and inline image, got {images:?}");
    for image in &images {
        let data = book.load_asset(image).unwrap();
        assert!(data.starts_with(b"\x89PNG") || data.starts_with(b"\xff\xd8"));
    }
    assert!(book.metadata().cover_image.is_some());
}

#[test]
fn combo_epub_export_keeps_images() {
    let book = Book::from_bytes(COMBO, Format::Azw3).unwrap();
    let mut out = Cursor::new(Vec::new());
    book.export(Format::Epub, &mut out).unwrap();
    let exported = Book::from_bytes(out.get_ref(), Format::Epub).unwrap();
    assert!(
        exported
            .list_assets()
            .iter()
            .any(|a| a.contains("images/") || a.ends_with(".png") || a.ends_with(".jpg"))
    );
}

/// `COMBO` with `image` added as a new last record, which the KF8 header then
/// names as its first resource. calibre keeps every resource in the first half,
/// so this is the only way to get a fixture with one in the KF8 half.
fn combo_with_kf8_image(image: &[u8]) -> Vec<u8> {
    const COUNT: usize = 76;
    const TABLE: usize = 78;
    const KF8_RECORD: usize = 13;
    const FIRST_IMAGE: usize = 108;
    let u32_at = |d: &[u8], at: usize| u32::from_be_bytes(d[at..at + 4].try_into().unwrap());

    let records = usize::from(u16::from_be_bytes(
        COMBO[COUNT..COUNT + 2].try_into().unwrap(),
    ));
    let table_end = TABLE + records * 8;
    let mut book = COMBO[..table_end].to_vec();
    for i in 0..records {
        let offset = u32_at(COMBO, TABLE + i * 8) + 8;
        book[TABLE + i * 8..TABLE + i * 8 + 4].copy_from_slice(&offset.to_be_bytes());
    }
    let new_offset = u32::try_from(COMBO.len() + 8).unwrap();
    book.extend_from_slice(&new_offset.to_be_bytes());
    book.extend_from_slice(&u32::try_from(records * 2).unwrap().to_be_bytes());
    book.extend_from_slice(&COMBO[table_end..]);
    book.extend_from_slice(image);
    book[COUNT..COUNT + 2].copy_from_slice(&u16::try_from(records + 1).unwrap().to_be_bytes());

    let header = u32_at(&book, TABLE + KF8_RECORD * 8) as usize + FIRST_IMAGE;
    let first = u32::try_from(records - KF8_RECORD).unwrap();
    book[header..header + 4].copy_from_slice(&first.to_be_bytes());
    book
}

#[test]
fn combo_finds_resources_in_both_halves() {
    let image = b"\x89PNG\r\n\x1a\nimage in the KF8 half";
    let bytes = combo_with_kf8_image(image);
    let book = Book::from_bytes(&bytes, Format::Azw3).unwrap();

    let first_half = Book::from_bytes(COMBO, Format::Azw3).unwrap();
    for asset in first_half.list_assets() {
        if asset.starts_with("images/") {
            assert_eq!(
                book.load_asset(asset).unwrap(),
                first_half.load_asset(asset).unwrap()
            );
        }
    }
    let found = book
        .list_assets()
        .iter()
        .filter(|a| a.starts_with("images/"))
        .any(|a| book.load_asset(a).unwrap() == image);
    assert!(found, "image in the KF8 half: {:?}", book.list_assets());
}
