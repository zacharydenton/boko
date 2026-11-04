//! Font resources must be advertised to the Kindle reader as well as packaged.
mod common;

use boko::model::Format;
use common::{Doc, EpubBuilder};

fn u32_at(bytes: &[u8], offset: usize) -> usize {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

#[test]
fn embedded_font_flag_matches_packaged_font_records() {
    for include_font in [false, true] {
        let mut epub = EpubBuilder::new("Font metadata")
            .doc(Doc::new("text/c.xhtml", "Chapter", "<p>Text</p>"))
            .cover_png()
            .css("@font-face { font-family: Publisher; src:url('../fonts/test.ttf'); } p { font-family: Publisher; }");
        if include_font {
            // Transport fixture: sufficient to exercise FONT packaging without
            // depending on an installed font or claiming to test rendering.
            let mut font = vec![0, 1, 0, 0];
            font.extend((0..2048).map(|i| (i % 251) as u8));
            epub = epub.font("fonts/test.ttf", font);
        }
        let bytes = common::export_to_bytes(&mut epub.book(), Format::Azw3);
        let record0 = u32_at(&bytes, 78);
        let count = u16::from_be_bytes(bytes[76..78].try_into().unwrap()) as usize;
        let fonts = (1..count)
            .filter(|&i| {
                let start = u32_at(&bytes, 78 + i * 8);
                bytes.get(start..start + 4) == Some(b"FONT")
            })
            .count();
        assert_eq!(fonts, usize::from(include_font));
        let flags = u32_at(&bytes, record0 + 128);
        assert_ne!(flags & 0x40, 0, "EXTH block must remain advertised");
        assert_eq!(
            flags & 0x1000 != 0,
            fonts > 0,
            "font flag must reflect FONT records, not just CSS declarations or images"
        );
    }
}
