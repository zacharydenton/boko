//! Tests for `Book::optimize` — the pass-based asset shrinker.
//!
//! The `images` pass re-encodes oversized raster images as JPEG, keeping the
//! original whenever the re-encode isn't meaningfully smaller. Renames
//! (PNG→JPEG) must rewrite chapter `src` references and the cover path, and
//! must force normalized export so raw-passthrough EPUB output can't ship
//! dangling references.

mod common;

use std::io::Cursor;

use boko::Format;

const JPEG_MAGIC: [u8; 2] = [0xFF, 0xD8];

/// A smooth 2D color field: expensive for PNG (every pixel differs), cheap
/// for JPEG. Comfortably above the pass's 10 KB minimum at 256x256.
fn photographic_image() -> image::RgbImage {
    image::RgbImage::from_fn(256, 256, |x, y| {
        let (fx, fy) = (x as f32 / 256.0, y as f32 / 256.0);
        image::Rgb([
            (128.0 + 127.0 * (fx * 7.1).sin() * (fy * 5.3).cos()) as u8,
            (128.0 + 127.0 * (fx * 3.7 + fy * 2.9).sin()) as u8,
            (128.0 + 127.0 * ((fx + fy) * 4.3).cos()) as u8,
        ])
    })
}

fn photographic_png() -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    photographic_image()
        .write_to(&mut buf, image::ImageFormat::Png)
        .expect("encode png");
    buf.into_inner()
}

fn high_quality_jpeg() -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    let img = photographic_image();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 100)
        .encode(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgb8,
        )
        .expect("encode jpeg");
    buf.into_inner()
}

#[test]
fn optimize_transcodes_large_png_and_rewrites_references() {
    use common::{Doc, EpubBuilder, Nav};

    let png = photographic_png();
    assert!(
        png.len() > 10 * 1024,
        "fixture must exceed the pass minimum"
    );

    let epub = EpubBuilder::new("Optimize Book")
        .image("images/photo.png", png.clone())
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>before</p><img src=\"../images/photo.png\" alt=\"photo\"/><p>after</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let report = book.optimize();

    assert_eq!(report.passes.len(), 1);
    assert_eq!(report.passes[0].pass, "images");
    assert_eq!(report.assets_changed(), 1);
    assert!(report.bytes_saved() > 0);

    // The asset list serves the renamed JPEG; the PNG name is gone.
    let assets = book.list_assets().to_vec();
    let jpg_path = assets
        .iter()
        .find(|p| p.ends_with("photo.jpg"))
        .expect("renamed asset listed")
        .clone();
    assert!(!assets.iter().any(|p| p.ends_with("photo.png")));

    // New bytes are JPEG and smaller; the old path still resolves to them.
    let new_data = book.load_asset(&jpg_path).expect("load renamed");
    assert_eq!(new_data[..2], JPEG_MAGIC);
    assert!(new_data.len() < png.len());
    let old_path = jpg_path.replace(".jpg", ".png");
    assert_eq!(book.load_asset(&old_path).expect("old path"), new_data);

    // Chapter src references are rewritten.
    let ids: Vec<_> = book.spine().iter().map(|e| e.id).collect();
    let mut saw_jpg_src = false;
    for id in ids {
        let ch = book.load_chapter(id).expect("load chapter");
        for node in ch.iter_dfs() {
            if let Some(src) = ch.semantics.src(node) {
                assert!(!src.ends_with(".png"), "src not rewritten: {src}");
                saw_jpg_src |= src.ends_with("photo.jpg");
            }
        }
    }
    assert!(saw_jpg_src, "rewritten image src not found");

    // Every output format ships the JPEG under the new name.
    for format in [Format::Epub, Format::Kfx, Format::Azw3] {
        let out = common::export_to_bytes(&mut book, format);
        assert!(!out.is_empty(), "{format:?} export produced no output");
    }
    let epub_out = common::export_to_bytes(&mut book, Format::Epub);
    let mut zip = zip::ZipArchive::new(Cursor::new(epub_out)).expect("open exported epub");
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with("photo.jpg")),
        "exported EPUB missing renamed image: {names:?}"
    );
    assert!(!names.iter().any(|n| n.ends_with("photo.png")));
}

#[test]
fn optimize_recompresses_large_jpeg_in_place() {
    use common::{Doc, EpubBuilder, Nav};

    let jpeg = high_quality_jpeg();
    assert!(
        jpeg.len() > 10 * 1024,
        "fixture must exceed the pass minimum"
    );

    let epub = EpubBuilder::new("JPEG Book")
        .image("images/photo.jpg", jpeg.clone())
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>x</p><img src=\"../images/photo.jpg\" alt=\"photo\"/>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let report = book.optimize();

    assert_eq!(report.assets_changed(), 1);
    // Same path, smaller JPEG bytes.
    let path = book
        .list_assets()
        .iter()
        .find(|p| p.ends_with("photo.jpg"))
        .expect("jpeg still listed")
        .clone();
    let data = book.load_asset(&path).expect("load");
    assert_eq!(data[..2], JPEG_MAGIC);
    assert!(data.len() < jpeg.len());
}

#[test]
fn optimize_keeps_small_flat_and_css_referenced_images() {
    use common::{Doc, EpubBuilder, Nav};

    let big_png = photographic_png();
    let small_png = common::tiny_png();

    let epub = EpubBuilder::new("Untouched Book")
        .css(".hero { background-image: url(../images/wallpaper.png); }")
        .image("images/wallpaper.png", big_png.clone())
        .image("images/icon.png", small_png.clone())
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p class=\"hero\">styled</p><img src=\"../images/icon.png\" alt=\"icon\"/>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let report = book.optimize();

    // The CSS-referenced image can't be renamed; the icon is below the size
    // minimum. Nothing changes.
    assert_eq!(report.assets_changed(), 0, "report: {report:?}");
    let assets = book.list_assets().to_vec();
    assert!(assets.iter().any(|p| p.ends_with("wallpaper.png")));
    assert!(assets.iter().any(|p| p.ends_with("icon.png")));
    let wallpaper = assets
        .iter()
        .find(|p| p.ends_with("wallpaper.png"))
        .unwrap();
    assert_eq!(book.load_asset(wallpaper).expect("load"), big_png);
}
