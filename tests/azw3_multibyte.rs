mod common;

use boko::model::Format;
use common::{Doc, EpubBuilder, Nav};

#[test]
fn export_multibyte_records_and_deep_toc() {
    let paragraphs = (0..300)
        .map(|i| format!("<p id=\"p{i}\">中文日本語 한국어 العربية עברית 😀𐐀 {i} <a href=\"#p0\">返回</a></p>"))
        .collect::<String>();
    let builder = EpubBuilder::new("Multibyte and deep TOC")
        .language("zh")
        .css("p { font-family: serif; } p::before { content: '中文😀'; }")
        .doc(Doc::new("c.xhtml", "Chapter", &paragraphs))
        .nav(vec![
            Nav::new("Part A", "c.xhtml#p0").with_children(vec![
                Nav::new("Chapter A1", "c.xhtml#p0").with_children(vec![
                    Nav::new("Section A1a", "c.xhtml#p10"),
                    Nav::new("Section A1b", "c.xhtml#p20"),
                ]),
                Nav::new("Chapter A2", "c.xhtml#p100"),
            ]),
            Nav::new("Part B", "c.xhtml#p200").with_children(vec![
                Nav::new("Chapter B1", "c.xhtml#p210"),
                Nav::new("Chapter B2", "c.xhtml#p250"),
            ]),
        ]);
    let mut source = builder.book();
    let bytes = common::export_to_bytes(&mut source, Format::Azw3);
    let book = boko::Book::from_bytes(&bytes, Format::Azw3).unwrap();
    assert_eq!(common::count_toc(book.toc()), 8);
    assert_eq!(common::max_toc_depth(book.toc(), 0), 3);
    let raw = book.load_raw(book.spine()[0].id).unwrap();
    let html = String::from_utf8(raw).unwrap();
    assert_eq!(
        html.matches("中文日本語 한국어 العربية עברית 😀𐐀").count(),
        300
    );
}
