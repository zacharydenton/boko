//! Direction metadata, CSS inheritance, and mixed-direction conversion regressions.
mod common;

use boko::Book;
use boko::model::{Chapter, Format, Metadata, Role};
use boko::style::{ComputedStyle, Direction, TextAlign};
use common::{Doc, EpubBuilder};

fn paragraph_style(chapter: &Chapter, marker: &str) -> ComputedStyle {
    for id in chapter.iter_dfs() {
        let node = chapter.node(id).unwrap();
        if node.role != Role::Text || !chapter.text(node.text).contains(marker) {
            continue;
        }
        let mut parent = node.parent;
        while let Some(id) = parent {
            let node = chapter.node(id).unwrap();
            if node.role == Role::Paragraph {
                return chapter.styles.get(node.style).unwrap().clone();
            }
            parent = node.parent;
        }
    }
    panic!("missing paragraph containing {marker}");
}

#[test]
fn explicit_page_direction_survives_repeated_conversions() {
    for format in [Format::Kfx, Format::Azw3] {
        for direction in ["ltr", "rtl"] {
            let bytes = EpubBuilder::new("Explicit direction")
                .language("ar")
                .direction(direction)
                .doc(Doc::new("c.xhtml", "Chapter", "<p>نص عربي.</p>"))
                .build();
            let mut book = Book::from_bytes(&bytes, Format::Epub).unwrap();
            for _ in 0..2 {
                book = common::roundtrip(&mut book, format);
                assert_eq!(
                    book.metadata().page_progression_direction.as_deref(),
                    Some(direction),
                    "{format:?}"
                );
                book = common::roundtrip(&mut book, Format::Epub);
                assert_eq!(
                    book.metadata().page_progression_direction.as_deref(),
                    Some(direction),
                    "{format:?} to EPUB"
                );
            }
        }
    }
}

#[test]
fn script_inferred_direction_survives_kindle_language_serialization() {
    for format in [Format::Kfx, Format::Azw3] {
        for (lang, direction) in [("ar-Latn", "ltr"), ("ku-Latn", "ltr"), ("az-Arab", "rtl")] {
            let bytes = EpubBuilder::new("Script direction")
                .language(lang)
                .doc(Doc::new("c.xhtml", "Chapter", "<p>Text</p>"))
                .build();
            let mut book = Book::from_bytes(&bytes, Format::Epub).unwrap();
            for _ in 0..2 {
                book = common::roundtrip(&mut book, format);
                assert_eq!(
                    book.metadata().page_progression_direction.as_deref(),
                    Some(direction),
                    "{lang} through {format:?}"
                );
            }
        }
    }
}

#[test]
fn direction_fallback_respects_scripts_and_explicit_page_order() {
    for (lang, rtl) in [
        ("ar", true),
        ("ara", true),
        ("ar-SA", true),
        ("he", true),
        ("heb", true),
        ("fa", true),
        ("ur", true),
        ("yi", true),
        ("ckb", true),
        ("ku", false),
        ("ar-Latn", false),
        ("he-Latn", false),
        ("ku-Latn", false),
        ("ku-Arab", true),
        ("az-Arab", true),
        ("sd-Deva", false),
        ("en-Hebr", true),
        ("EN_arab", true),
        ("und-Adlm", true),
        ("en-x-Arab", false),
        ("en-u-Arab", false),
        ("x-Arab", false),
        (" ar-latn ", false),
        ("", false),
    ] {
        let mut meta = Metadata {
            language: lang.into(),
            ..Default::default()
        };
        assert_eq!(meta.is_rtl(), rtl, "language {lang}");
        meta.page_progression_direction = Some("default".into());
        assert_eq!(meta.is_rtl(), rtl, "default direction, {lang}");
        for (direction, expected) in [("ltr", false), ("rtl", true)] {
            meta.page_progression_direction = Some(direction.into());
            assert_eq!(meta.is_rtl(), expected, "explicit {direction}, {lang}");
        }
    }
}

#[test]
fn css_direction_keywords_reset_lower_priority_declarations() {
    for (parent, expected) in [("rtl", Direction::Rtl), ("ltr", Direction::Ltr)] {
        let opposite = if parent == "rtl" { "ltr" } else { "rtl" };
        for keyword in ["unset", "inherit", "UNSET", "INHERIT"] {
            let chapter = boko::compile_html(
                &format!(
                    "<div dir='{parent}'><p dir='{opposite}' style='direction:{opposite};direction:{keyword}'>marker</p></div>"
                ),
                &[],
            );
            assert_eq!(
                paragraph_style(&chapter, "marker").direction,
                expected,
                "{parent}, {keyword}"
            );
        }
    }
    let chapter = boko::compile_html(
        "<div dir='rtl'><p style='direction:initial'>marker</p></div>",
        &[],
    );
    assert_eq!(
        paragraph_style(&chapter, "marker").direction,
        Direction::Ltr
    );
}

#[test]
fn direction_inheritance_obeys_important_and_inline_cascade() {
    let chapter = boko::compile_html(
        "<div dir='rtl'><p style='direction:ltr'>marker</p></div>",
        &[(
            boko::Stylesheet::parse("p { direction: unset !important; }"),
            boko::Origin::Author,
        )],
    );
    assert_eq!(
        paragraph_style(&chapter, "marker").direction,
        Direction::Rtl
    );
    let chapter = boko::compile_html(
        "<div dir='rtl'><p style='direction:initial !important'>marker</p></div>",
        &[(
            boko::Stylesheet::parse("p { direction: rtl !important; }"),
            boko::Origin::Author,
        )],
    );
    assert_eq!(
        paragraph_style(&chapter, "marker").direction,
        Direction::Ltr
    );
}

#[test]
fn mixed_direction_alignment_survives_kfx_to_epub() {
    for (parent_dir, child_dir) in [("rtl", "ltr"), ("ltr", "rtl")] {
        for align in ["start", "end", "left", "right", "center", "justify"] {
            let body = format!(
                "<div style='text-align:{align}'><p>parent-marker</p><p dir='{child_dir}'>child-marker</p></div>"
            );
            let bytes = EpubBuilder::new("Mixed direction")
                .language("ar")
                .doc(Doc::new("c.xhtml", "Chapter", &body).lang_dir("ar", parent_dir))
                .build();
            let mut source = Book::from_bytes(&bytes, Format::Epub).unwrap();
            let mut kfx = common::roundtrip(&mut source, Format::Kfx);
            let epub = common::roundtrip(&mut kfx, Format::Epub);
            let chapter = epub.load_chapter(epub.spine()[0].id).unwrap();
            for (marker, direction) in [("parent-marker", parent_dir), ("child-marker", child_dir)]
            {
                let expected = match (align, direction) {
                    ("start", "rtl") | ("end", "ltr") | ("right", _) => TextAlign::Right,
                    ("start", _) | ("end", _) | ("left", _) => TextAlign::Left,
                    ("center", _) => TextAlign::Center,
                    _ => TextAlign::Justify,
                };
                let style = paragraph_style(&chapter, marker);
                let actual = match (style.text_align, style.direction) {
                    (TextAlign::Start, Direction::Rtl) | (TextAlign::End, Direction::Ltr) => {
                        TextAlign::Right
                    }
                    (TextAlign::Start, _) | (TextAlign::End, _) => TextAlign::Left,
                    (value, _) => value,
                };
                assert_eq!(
                    actual, expected,
                    "{parent_dir}/{child_dir}, {align}, {marker}"
                );
            }
        }
    }
}

fn text_and_directions(book: &Book) -> (String, Vec<bool>) {
    let mut text = String::new();
    let mut directions = Vec::new();
    for entry in book.spine() {
        let chapter = book.load_chapter(entry.id).unwrap();
        for id in chapter.iter_dfs() {
            let node = chapter.node(id).unwrap();
            if node.role != Role::Text {
                continue;
            }
            let mut current = Some(id);
            let mut rtl = book.metadata().is_rtl();
            while let Some(id) = current {
                let n = chapter.node(id).unwrap();
                match chapter.styles.get(n.style).unwrap().direction {
                    Direction::Rtl => {
                        rtl = true;
                        break;
                    }
                    Direction::Ltr => {
                        rtl = false;
                        break;
                    }
                    Direction::Unset => current = n.parent,
                }
            }
            // KFX uses zero-width spaces to carry otherwise empty anchors.
            // Ignore these and layout whitespace, but retain bidi controls,
            // combining marks, ZWNJ, and ZWJ.
            for c in chapter
                .text(node.text)
                .chars()
                .filter(|c| !c.is_whitespace() && *c != '\u{200b}')
            {
                text.push(c);
                directions.push(rtl);
            }
        }
    }
    (text, directions)
}

/// External books are kept outside git. See tests/fixtures/rtl-corpus.md.
#[test]
#[ignore = "requires downloaded books; set BOKO_RTL_CORPUS"]
fn open_rtl_books_preserve_text_and_direction() {
    let root =
        std::path::PathBuf::from(std::env::var("BOKO_RTL_CORPUS").expect("set BOKO_RTL_CORPUS"));
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "epub"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no EPUBs in corpus");
    let mut failures = Vec::new();
    for path in paths {
        let mut source = Book::open(&path).unwrap();
        let (source_text, source_directions) = text_and_directions(&source);
        assert!(
            source.metadata().is_rtl(),
            "expected RTL corpus book: {}",
            path.display()
        );
        for format in [Format::Kfx, Format::Azw3] {
            let bytes = common::export_to_bytes(&mut source, format);
            let mut kindle = Book::from_bytes(&bytes, format).unwrap();
            let epub_bytes = common::export_to_bytes(&mut kindle, Format::Epub);
            let mut epub = Book::from_bytes(&epub_bytes, Format::Epub).unwrap();
            let (text, directions) = text_and_directions(&epub);
            let same_text = text == source_text;
            let same_directions = same_text && directions == source_directions;
            let again = common::roundtrip(&mut epub, format);
            let page_direction_ok = [kindle.metadata(), epub.metadata(), again.metadata()]
                .iter()
                .all(|m| m.page_progression_direction.as_deref() == Some("rtl"));
            let name = path.file_name().unwrap().to_string_lossy();
            eprintln!(
                "{name} {format:?}: chars {} -> {}; text={same_text}; text_direction={same_directions}; page_direction={page_direction_ok}",
                source_directions.len(),
                directions.len()
            );
            if let Ok(output) = std::env::var("BOKO_RTL_OUTPUT") {
                let output = std::path::PathBuf::from(output);
                std::fs::create_dir_all(&output).unwrap();
                let stem = path.file_stem().unwrap().to_string_lossy();
                std::fs::write(output.join(format!("{stem}-source.txt")), &source_text).unwrap();
                std::fs::write(output.join(format!("{stem}-{format:?}.txt")), &text).unwrap();
                std::fs::write(output.join(format!("{stem}-{format:?}.epub")), epub_bytes).unwrap();
                std::fs::write(
                    output.join(format!("{stem}.{}", format!("{format:?}").to_lowercase())),
                    bytes,
                )
                .unwrap();
            }
            if !(same_text && same_directions && page_direction_ok) {
                failures.push(format!("{name} {format:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "corpus regressions: {failures:?}");
}
