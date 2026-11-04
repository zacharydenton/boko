mod common;

use std::io::Cursor;
use std::process::Command;

use boko::export::{EpubExporter, Exporter};
use boko::{Book, Format};
use common::{Doc, EpubBuilder, Nav};

#[test]
fn one_large_html_becomes_navigable_epub_flows() {
    let body = (0..16)
        .map(|index| {
            format!(
                "<p id=\"section{index}\"><a href=\"#section15\">end</a>{}</p>",
                "reading text ".repeat(35)
            )
        })
        .collect::<String>();
    let input = EpubBuilder::new("Large flow")
        .doc(Doc::new("text/all.xhtml", "Large flow", &body))
        .nav(vec![Nav::new("End", "text/all.xhtml#section15")])
        .book();
    assert_eq!(input.spine().len(), 1);

    let mut output = Cursor::new(Vec::new());
    EpubExporter::new()
        .with_flow_size(2048)
        .export(&input, &mut output)
        .unwrap();
    let reopened = Book::from_bytes(&output.into_inner(), Format::Epub).unwrap();
    assert!(reopened.spine().len() > 1);

    let toc_href = &reopened.toc()[0].href;
    let (target_file, target_id) = toc_href.split_once('#').unwrap();
    let relative_href = toc_href.strip_prefix("OEBPS/").unwrap_or(toc_href);
    assert_eq!(target_id, "section15");
    let mut found_target = false;
    for entry in reopened.spine() {
        let path = reopened.source_id(entry.id).unwrap();
        let html = String::from_utf8(reopened.load_raw(entry.id).unwrap()).unwrap();
        if path.ends_with(target_file) {
            assert!(html.contains("id=\"section15\""));
            found_target = true;
        }
        assert!(html.contains(&format!("href=\"{relative_href}\"")));
    }
    assert!(found_target);
}

#[test]
fn mobi_page_breaks_can_be_ignored_before_size_splitting() {
    let bytes = std::fs::read("tests/fixtures/epictetus.mobi").unwrap();
    let default_book = Book::from_bytes(&bytes, Format::Mobi).unwrap();
    assert!(default_book.spine().len() > 1);

    let book = Book::from_mobi_bytes_without_page_break_splits(&bytes).unwrap();
    assert_eq!(book.spine().len(), 1);
    let raw = String::from_utf8(book.load_raw(book.spine()[0].id).unwrap()).unwrap();
    assert!(!raw.contains("<mbp:pagebreak"));
    let first_anchor = book.toc()[0].href.split_once('#').unwrap().1;
    assert!(raw.contains(&format!("id=\"{first_anchor}\"")));

    let mut output = Cursor::new(Vec::new());
    EpubExporter::new()
        .with_flow_size(40 * 1024)
        .export(&book, &mut output)
        .unwrap();
    let reopened = Book::from_bytes(&output.into_inner(), Format::Epub).unwrap();
    assert!(reopened.spine().len() > 1);
    let chapters: Vec<_> = reopened
        .spine()
        .iter()
        .map(|entry| {
            (
                reopened.source_id(entry.id).unwrap().to_string(),
                String::from_utf8(reopened.load_raw(entry.id).unwrap()).unwrap(),
            )
        })
        .collect();
    for toc in reopened.toc() {
        let (file, fragment) = toc.href.split_once('#').unwrap();
        let html = &chapters
            .iter()
            .find(|(path, _)| path.ends_with(file))
            .unwrap_or_else(|| panic!("missing TOC file: {}", toc.href))
            .1;
        assert!(
            html.contains(&format!("id=\"{fragment}\"")),
            "missing TOC anchor: {}",
            toc.href
        );
    }
}

#[test]
fn cli_epub_flow_size_defaults_to_260_and_zero_disables_it() {
    let body = (0..900)
        .map(|index| format!("<p id=\"p{index}\">{}</p>", "word ".repeat(85)))
        .collect::<String>();
    let dir = tempfile::tempdir().unwrap();
    let input_path = dir.path().join("input.epub");
    std::fs::write(
        &input_path,
        EpubBuilder::new("Default flow size")
            .doc(Doc::new("text/all.xhtml", "Default flow size", &body))
            .build(),
    )
    .unwrap();

    let default_output = dir.path().join("default.epub");
    let disabled_output = dir.path().join("disabled.epub");
    for (output, options) in [
        (&default_output, &[][..]),
        (&disabled_output, &["--flow-size", "0"][..]),
    ] {
        let status = Command::new(env!("CARGO_BIN_EXE_boko"))
            .arg("convert")
            .arg(&input_path)
            .arg(output)
            .args(options)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let default_book = Book::open(&default_output).unwrap();
    let disabled_book = Book::open(&disabled_output).unwrap();
    assert!(default_book.spine().len() > 1);
    assert_eq!(disabled_book.spine().len(), 1);
}
