//! KFX navigation targets must resolve in exported EPUB TOCs.
//!
//! Kindle-produced books point TOC entries at `{id, offset}` positions
//! (imported as `#id:offset`) and the cover entry at the cover section's
//! page_template id, which no IR node carries.

mod common;

use std::io::{Cursor, Read};

use boko::Format;
use boko::import::{ChapterId, Importer, KfxImporter};
use boko::kfx::container::{
    extract_doc_symbols, get_symbol_text, parse_container_header, parse_container_info,
    parse_index_table, skip_enty_header,
};
use boko::kfx::ion::{IonParser, IonValue};
use boko::kfx::symbols::KfxSymbol;
use boko::model::{AnchorTarget, TocEntry};
use flate2::read::GzDecoder;

fn get_field(fields: &[(u64, IonValue)], sym: KfxSymbol) -> Option<&IonValue> {
    fields
        .iter()
        .find_map(|(k, v)| (*k == sym as u64).then_some(v))
}

/// Two text chapters around an image-only one, which exports as a
/// scale-fit page-template section.
fn build_test_book() -> Vec<u8> {
    use common::{Doc, EpubBuilder, Nav};
    EpubBuilder::new("Nav Targets")
        .image("images/plate.png", common::tiny_png())
        .doc(Doc::new("text/ch1.xhtml", "One", "<p>First chapter.</p>"))
        .doc(Doc::new(
            "text/plate.xhtml",
            "Plate",
            "<img src=\"../images/plate.png\" alt=\"plate\"/>",
        ))
        .doc(Doc::new("text/ch2.xhtml", "Two", "<p>Second chapter.</p>"))
        .nav(vec![
            Nav::new("One", "text/ch1.xhtml"),
            Nav::new("Two", "text/ch2.xhtml"),
        ])
        .build()
}

/// (section_name, page_template id) for every section in a KFX container.
fn section_templates(kfx: &[u8]) -> Vec<(String, i64)> {
    let header = parse_container_header(&kfx[..18]).expect("header");
    let info_end = header.container_info_offset + header.container_info_length;
    let info = parse_container_info(&kfx[header.container_info_offset..info_end]).expect("info");
    let (index_offset, index_length) = info.index.expect("index");
    let doc_symbols = info
        .doc_symbols
        .map(|(offset, length)| extract_doc_symbols(&kfx[offset..offset + length]))
        .unwrap_or_default();
    let entities = parse_index_table(
        &kfx[index_offset..index_offset + index_length],
        header.header_len,
    );

    let mut out = Vec::new();
    for e in entities
        .iter()
        .filter(|e| e.type_id == KfxSymbol::Section as u32)
    {
        let entity = &kfx[e.offset..e.offset + e.length];
        let Ok(IonValue::Struct(fields)) = IonParser::new(skip_enty_header(entity)).parse() else {
            continue;
        };
        let name = get_field(&fields, KfxSymbol::SectionName)
            .and_then(|v| get_symbol_text(v, &doc_symbols))
            .expect("section name")
            .to_string();
        if let Some(IonValue::List(templates)) = get_field(&fields, KfxSymbol::PageTemplates) {
            for t in templates {
                if let IonValue::Struct(tf) = t
                    && let Some(id) = get_field(tf, KfxSymbol::Id).and_then(|v| v.as_int())
                {
                    out.push((name.clone(), id));
                }
            }
        }
    }
    out
}

#[test]
fn kfx_section_template_id_resolves_to_its_chapter() {
    let epub = build_test_book();
    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    let path =
        std::env::temp_dir().join(format!("boko_kfx_nav_targets_{}.kfx", std::process::id()));
    std::fs::write(&path, &kfx).expect("write kfx");
    let importer = KfxImporter::open(&path).expect("open kfx");
    let _ = std::fs::remove_file(&path);

    let templates = section_templates(&kfx);
    assert!(!templates.is_empty(), "no page templates exported");
    for (section, template_id) in templates {
        let chapter = (0..importer.spine().len())
            .map(|i| ChapterId(i as u32))
            .find(|&id| importer.source_id(id) == Some(section.as_str()))
            .unwrap_or_else(|| panic!("section {section} not in spine"));

        // Bare, `#`-prefixed and `#id:offset` forms all land on the chapter
        // (or, if some element does carry the id, inside it)
        for href in [
            template_id.to_string(),
            format!("#{template_id}"),
            format!("#{template_id}:3"),
        ] {
            match importer.resolve_href(ChapterId(0), &href) {
                Some(AnchorTarget::Chapter(id)) => assert_eq!(id, chapter, "{href}"),
                Some(AnchorTarget::Internal(gid)) => assert_eq!(gid.chapter, chapter, "{href}"),
                other => panic!("{href} (section {section}) resolved to {other:?}"),
            }
        }
    }
}

fn walk<'a>(entries: &'a [TocEntry], out: &mut Vec<&'a TocEntry>) {
    for e in entries {
        out.push(e);
        walk(&e.children, out);
    }
}

/// A content-free real Kindle book (`nav_targets_stripped.kfx.gz`: text
/// replaced by same-length filler, images by a tiny JPEG, metadata
/// anonymized). Its TOC uses `#id:offset` targets and a page_template-id
/// cover entry; every NCX and nav target of the exported EPUB must point at
/// an existing file and id.
#[test]
fn kindle_book_toc_targets_resolve_in_epub() {
    let gz = std::fs::read("tests/fixtures/nav_targets_stripped.kfx.gz").expect("read fixture");
    let mut kfx = Vec::new();
    GzDecoder::new(&gz[..])
        .read_to_end(&mut kfx)
        .expect("decompress fixture");
    let mut book = boko::Book::from_bytes(&kfx, Format::Kfx).expect("import kfx");

    // Non-vacuous: the fixture's TOC uses `#id:offset` targets (its cover
    // entry, a bare page_template id, is covered by the NCX check below)
    let mut entries = Vec::new();
    walk(book.toc(), &mut entries);
    assert!(
        entries.iter().any(|e| e.href.contains(':')),
        "fixture has no #id:offset targets"
    );

    let epub = common::export_to_bytes(&mut book, Format::Epub);
    let mut zip = zip::ZipArchive::new(Cursor::new(epub)).expect("epub zip");
    let mut files = std::collections::HashMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).expect("zip entry");
        let mut s = String::new();
        if f.read_to_string(&mut s).is_ok() {
            files.insert(f.name().to_string(), s);
        }
    }

    let mut checked = 0;
    for (name, text) in &files {
        let attr = if name.ends_with(".ncx") {
            "<content src=\""
        } else if name.ends_with("nav.xhtml") {
            "<a href=\""
        } else {
            continue;
        };
        let dir = name
            .rsplit_once('/')
            .map(|(d, _)| format!("{d}/"))
            .unwrap_or_default();
        for target in text.split(attr).skip(1).map(|s| &s[..s.find('"').unwrap()]) {
            let (file, frag) = target.split_once('#').unwrap_or((target, ""));
            assert!(!file.is_empty(), "{name}: fragment-only target {target:?}");
            let doc = files
                .get(&format!("{dir}{file}"))
                .unwrap_or_else(|| panic!("{name}: target file missing: {target}"));
            assert!(
                frag.is_empty() || doc.contains(&format!("id=\"{frag}\"")),
                "{name}: target id missing: {target}"
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} TOC targets checked");
}
