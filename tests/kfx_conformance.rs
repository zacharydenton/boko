//! KFX conformance checks against the reference content model.
//!
//! These tests encode invariants learned from validating boko's output with
//! jhowell's kfxlib (`tools/kfxcheck.py`) against Kindle Previewer gold
//! masters:
//!
//! - Scale-fit page-template images must reference an empty style — the
//!   template carries all positioning, and readers flag any leftover image
//!   properties as an unexpected image style.
//! - Styles must never declare `font-size` in percent. Reference KFX only
//!   uses em/rem, and KFX consumers prune inherited percentage values, which
//!   breaks font-size resolution for descendants.
//! - The full kfxcheck validation (structural + position maps + trial EPUB
//!   conversion) must report zero errors for a real conversion.

mod common;

use boko::Format;
use boko::kfx::container::{
    extract_doc_symbols, parse_container_header, parse_container_info, parse_index_table,
    skip_enty_header,
};
use boko::kfx::ion::{IonParser, IonValue};
use boko::kfx::symbols::{KFX_SYMBOL_TABLE, KfxSymbol};

/// Parse every entity of the given fragment type into Ion values.
fn parse_entities(kfx: &[u8], type_id: u32) -> Vec<IonValue> {
    let header = parse_container_header(&kfx[..18]).expect("container header");
    let info = parse_container_info(
        &kfx[header.container_info_offset
            ..header.container_info_offset + header.container_info_length],
    )
    .expect("container info");
    let (index_offset, index_length) = info.index.expect("index table");
    let entities = parse_index_table(
        &kfx[index_offset..index_offset + index_length],
        header.header_len,
    );

    entities
        .iter()
        .filter(|loc| loc.type_id == type_id)
        .filter_map(|loc| {
            let entity = &kfx[loc.offset..loc.offset + loc.length];
            IonParser::new(skip_enty_header(entity)).parse().ok()
        })
        .collect()
}

/// Document symbols (local symbol table) of the container.
fn doc_symbols(kfx: &[u8]) -> Vec<String> {
    let header = parse_container_header(&kfx[..18]).expect("container header");
    let info = parse_container_info(
        &kfx[header.container_info_offset
            ..header.container_info_offset + header.container_info_length],
    )
    .expect("container info");
    match info.doc_symbols {
        Some((off, len)) if len > 0 => extract_doc_symbols(&kfx[off..off + len]),
        _ => Vec::new(),
    }
}

fn resolve_symbol(doc_symbols: &[String], id: u64) -> String {
    let base = KFX_SYMBOL_TABLE.len() as u64;
    if id < base {
        KFX_SYMBOL_TABLE[id as usize].to_string()
    } else {
        doc_symbols
            .get((id - base) as usize)
            .cloned()
            .unwrap_or_default()
    }
}

fn get_field(fields: &[(u64, IonValue)], sym: KfxSymbol) -> Option<&IonValue> {
    fields
        .iter()
        .find_map(|(k, v)| (*k == sym as u64).then_some(v))
}

/// A one-page PNG "cover" chapter plus a text chapter, with CSS that would
/// previously leak image styling and percentage font sizes into the KFX.
fn build_test_book() -> Vec<u8> {
    use common::{Doc, EpubBuilder, Nav};
    EpubBuilder::new("Conformance Book")
        .css("img{max-width:95%;border:0;padding:0} body{font-size:100%} p{font-size:100%} .note{font-size:80%}")
        .image("images/plate.png", common::tiny_png())
        .doc(Doc::new(
            "text/plate.xhtml",
            "Plate",
            "<img src=\"../images/plate.png\" alt=\"plate\"/>",
        ))
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>Plain paragraph <span class=\"note\">with a smaller note</span>.</p>",
        ))
        .nav(vec![
            Nav::new("Plate", "text/plate.xhtml"),
            Nav::new("One", "text/ch1.xhtml"),
        ])
        .build()
}

/// Scale-fit page templates carry all positioning (fixed dims, scale_fit,
/// float center); the image node inside their storyline must reference a
/// style with no properties, exactly like Kindle Previewer output.
#[test]
fn scale_fit_images_reference_empty_style() {
    let epub = build_test_book();
    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);
    let symbols = doc_symbols(&kfx);

    // Collect story names of scale-fit sections and the style of each
    // storyline image.
    let mut scale_fit_stories = std::collections::BTreeSet::new();
    for section in parse_entities(&kfx, KfxSymbol::Section as u32) {
        let IonValue::Struct(fields) = &section else {
            continue;
        };
        let Some(IonValue::List(templates)) = get_field(fields, KfxSymbol::PageTemplates) else {
            continue;
        };
        for template in templates {
            let IonValue::Struct(tf) = template else {
                continue;
            };
            let is_scale_fit = get_field(tf, KfxSymbol::Layout)
                .and_then(|v| v.as_symbol())
                .is_some_and(|s| s == KfxSymbol::ScaleFit as u64);
            if is_scale_fit
                && let Some(story) = get_field(tf, KfxSymbol::StoryName).and_then(|v| v.as_symbol())
            {
                scale_fit_stories.insert(resolve_symbol(&symbols, story));
            }
        }
    }
    assert!(
        !scale_fit_stories.is_empty(),
        "image-only chapter should produce a scale-fit section"
    );

    // Style names referenced by scale-fit storyline images.
    let mut image_styles = std::collections::BTreeSet::new();
    for storyline in parse_entities(&kfx, KfxSymbol::Storyline as u32) {
        let IonValue::Struct(fields) = &storyline else {
            continue;
        };
        let story = get_field(fields, KfxSymbol::StoryName)
            .and_then(|v| v.as_symbol())
            .map(|s| resolve_symbol(&symbols, s))
            .unwrap_or_default();
        if !scale_fit_stories.contains(&story) {
            continue;
        }
        let Some(IonValue::List(content)) = get_field(fields, KfxSymbol::ContentList) else {
            continue;
        };
        for node in content {
            let IonValue::Struct(nf) = node else { continue };
            let is_image = get_field(nf, KfxSymbol::Type)
                .and_then(|v| v.as_symbol())
                .is_some_and(|s| s == KfxSymbol::Image as u64);
            if is_image
                && let Some(style) = get_field(nf, KfxSymbol::Style).and_then(|v| v.as_symbol())
            {
                image_styles.insert(resolve_symbol(&symbols, style));
            }
        }
    }
    assert!(
        !image_styles.is_empty(),
        "scale-fit storyline should contain an image node with a style"
    );

    // Each referenced style must have no properties besides its name.
    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        let name = get_field(fields, KfxSymbol::StyleName)
            .and_then(|v| v.as_symbol())
            .map(|s| resolve_symbol(&symbols, s))
            .unwrap_or_default();
        if image_styles.contains(&name) {
            assert_eq!(
                fields.len(),
                1,
                "scale-fit image style {name} must be empty (style_name only), got: {fields:?}"
            );
        }
    }
}

/// Styles must never carry `font-size` in percent: reference KFX uses only
/// em/rem, and consumers prune inherited percentage values, breaking
/// font-size resolution for descendants. 100% folds to 1em, 80% to 0.8em.
#[test]
fn font_size_is_never_percent() {
    let epub = build_test_book();
    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    let mut saw_font_size = false;
    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        let Some(IonValue::Struct(dim)) = get_field(fields, KfxSymbol::FontSize) else {
            continue;
        };
        saw_font_size = true;
        let unit = get_field(dim, KfxSymbol::Unit).and_then(|v| v.as_symbol());
        assert_ne!(
            unit,
            Some(KfxSymbol::Percent as u64),
            "font-size must not use percent units: {dim:?}"
        );
    }
    assert!(
        saw_font_size,
        "test book declares font sizes; none reached the KFX styles"
    );
}

/// `box_align` centers a block within its container. Reference KFX carries
/// it on block element styles (verified against calibre/KP gold masters) but
/// never on style_events — readers only consume it on blocks, and it
/// survives into the output as unexpected data otherwise.
#[test]
fn box_align_rides_block_styles_not_style_events() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Centering Book")
        .css(".c { margin: 0 auto; width: 60%; } .m { margin-top: 1em; }")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<div class=\"c\"><p>centered block</p></div>\
             <p>before <span class=\"m\">margined inline span</span> after</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);
    let symbols = doc_symbols(&kfx);

    // Styles referenced from style_events vs. from element style fields.
    let mut event_styles = std::collections::BTreeSet::new();
    let mut block_styles = std::collections::BTreeSet::new();
    fn walk(
        v: &IonValue,
        in_events: bool,
        event_styles: &mut std::collections::BTreeSet<u64>,
        block_styles: &mut std::collections::BTreeSet<u64>,
    ) {
        match v {
            IonValue::Struct(fields) => {
                for (k, val) in fields {
                    if *k == KfxSymbol::Style as u64
                        && let IonValue::Symbol(s) = val
                    {
                        if in_events {
                            event_styles.insert(*s);
                        } else {
                            block_styles.insert(*s);
                        }
                    }
                    let entering_events = *k == KfxSymbol::StyleEvents as u64;
                    walk(
                        val,
                        in_events || entering_events,
                        event_styles,
                        block_styles,
                    );
                }
            }
            IonValue::List(items) => {
                for item in items {
                    walk(item, in_events, event_styles, block_styles);
                }
            }
            IonValue::Annotated(_, inner) => walk(inner, in_events, event_styles, block_styles),
            _ => {}
        }
    }
    for storyline in parse_entities(&kfx, KfxSymbol::Storyline as u32) {
        walk(&storyline, false, &mut event_styles, &mut block_styles);
    }

    // Which style names carry box_align?
    let mut box_align_styles = std::collections::BTreeSet::new();
    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        if get_field(fields, KfxSymbol::BoxAlign).is_some()
            && let Some(name) = get_field(fields, KfxSymbol::StyleName).and_then(|v| v.as_symbol())
        {
            box_align_styles.insert(resolve_symbol(&symbols, name));
        }
    }
    assert!(
        !box_align_styles.is_empty(),
        "the centered div must keep box_align on its block style"
    );

    for style_sym in &event_styles {
        let name = resolve_symbol(&symbols, *style_sym);
        assert!(
            !box_align_styles.contains(&name),
            "style_event references box_align style {name}"
        );
    }
}

/// An empty chapter must still produce a (possibly empty) content_list —
/// a null content_list is rejected by KFX consumers ("unknown content_list
/// data type: NoneType").
#[test]
fn empty_chapter_storyline_has_list_content_list() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Empty Chapter Book")
        .doc(Doc::new("text/blank.xhtml", "Blank", ""))
        .doc(Doc::new("text/ch1.xhtml", "One", "<p>text</p>"))
        .nav(vec![
            Nav::new("Blank", "text/blank.xhtml"),
            Nav::new("One", "text/ch1.xhtml"),
        ])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    let storylines = parse_entities(&kfx, KfxSymbol::Storyline as u32);
    assert!(storylines.len() >= 2, "expected a storyline per chapter");
    for storyline in &storylines {
        let IonValue::Struct(fields) = storyline else {
            panic!("storyline is not a struct");
        };
        match get_field(fields, KfxSymbol::ContentList) {
            Some(IonValue::List(_)) => {}
            other => panic!("content_list must be a list, got: {other:?}"),
        }
    }
}

/// Books without a source identifier still get a deterministic content_id
/// (seeded from title+author): the Kindle keys sideloaded cover thumbnails
/// by content_id, so an id-less book can never show its cover.
#[test]
fn identifierless_book_gets_content_id() {
    use common::{Doc, EpubBuilder, Nav};

    let build = || {
        EpubBuilder::new("No Identifier Book")
            .identifier("")
            .doc(Doc::new("text/ch1.xhtml", "One", "<p>text</p>"))
            .nav(vec![Nav::new("One", "text/ch1.xhtml")])
            .build()
    };
    let extract_content_id = |epub: &[u8]| -> String {
        let mut book = boko::Book::from_bytes(epub, Format::Epub).expect("import epub");
        assert!(
            book.metadata().identifier.is_empty(),
            "fixture must lack an identifier"
        );
        let kfx = common::export_to_bytes(&mut book, Format::Kfx);
        let symbols = doc_symbols(&kfx);
        for meta in parse_entities(&kfx, KfxSymbol::BookMetadata as u32) {
            let IonValue::Struct(fields) = &meta else {
                continue;
            };
            let Some(IonValue::List(cats)) = get_field(fields, KfxSymbol::CategorisedMetadata)
            else {
                continue;
            };
            for cat in cats {
                let IonValue::Struct(cf) = cat else { continue };
                let Some(IonValue::List(entries)) = get_field(cf, KfxSymbol::Metadata) else {
                    continue;
                };
                for entry in entries {
                    let IonValue::Struct(ef) = entry else {
                        continue;
                    };
                    let key = get_field(ef, KfxSymbol::Key);
                    let is_content_id = match key {
                        Some(IonValue::String(s)) => s == "content_id",
                        Some(IonValue::Symbol(s)) => resolve_symbol(&symbols, *s) == "content_id",
                        _ => false,
                    };
                    if is_content_id
                        && let Some(IonValue::String(v)) = get_field(ef, KfxSymbol::Value)
                    {
                        return v.clone();
                    }
                }
            }
        }
        panic!("no content_id in book_metadata");
    };

    let id1 = extract_content_id(&build());
    let id2 = extract_content_id(&build());
    assert_eq!(id1.len(), 32, "content_id shape: {id1}");
    assert_eq!(id1, id2, "content_id must be deterministic");
}

/// End-to-end: the full kfxcheck validation (structural checks, position and
/// location map verification, trial EPUB conversion via kfxlib) must report
/// zero errors for a real EPUB conversion. Skipped when `uv` or the kfxlib
/// plugin source is unavailable.
#[test]
fn kfxcheck_reports_no_errors() {
    let uv = std::process::Command::new("uv").arg("--version").output();
    if uv.is_err() {
        eprintln!("Skipping test - uv not installed");
        return;
    }

    let mut book = common::open_fixture("epictetus.epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);
    let tmp = tempfile::Builder::new()
        .suffix(".kfx")
        .tempfile()
        .expect("temp file");
    std::fs::write(tmp.path(), &kfx).expect("write kfx");

    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/kfxcheck.py");
    let output = std::process::Command::new("uv")
        .args(["run", "--script", script, "-q"])
        .arg(tmp.path())
        .output()
        .expect("run kfxcheck");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Exit code 3 = kfxlib source unavailable (offline environment): skip.
    if output.status.code() == Some(3) {
        eprintln!("Skipping test - kfxlib unavailable: {stderr}");
        return;
    }
    assert!(
        output.status.success(),
        "kfxcheck reported problems:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("0 errors"),
        "kfxcheck reported errors:\n{stdout}"
    );
}
