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

/// MathML is imported as a first-class math node and, until native KVG
/// rendering exists, emitted into KFX as its readable-text fallback (the
/// source `alttext`) — present on the device, not dropped. The old behavior
/// flattened `<math>` into deeply-nested containers, leaking token text and
/// producing malformed content chunks.
#[test]
fn mathml_survives_into_kfx_as_text() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Math Book")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>Einstein wrote <math xmlns=\"http://www.w3.org/1998/Math/MathML\" \
             alttext=\"E equals m c squared\"><mi>E</mi><mo>=</mo><mi>m</mi>\
             <msup><mi>c</mi><mn>2</mn></msup></math> in 1905.</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    let text = String::from_utf8_lossy(&kfx);
    // The equation is a classified `math` container carrying its source MathML
    // (rendered live on capable firmware) plus a spoken alt_text and a readable
    // text fallback — not dropped, not flattened into the prose.
    assert!(
        text.contains("E equals m c squared"),
        "math alt_text/fallback must reach the KFX"
    );
    assert!(
        text.contains("<math") && text.contains("</math>"),
        "the source MathML must be carried as an annotation"
    );
    // The surrounding prose stays intact around it.
    assert!(text.contains("Einstein wrote"));
    assert!(text.contains("in 1905."));
}

/// Math inside an inline element (`<span>…<math/>…</span>`) must not be
/// dropped. The inline flattener can't nest a math container mid-style-event,
/// so it emits the equation's readable linearization inline instead of losing
/// it. (Regression: math inside spans previously vanished entirely.)
#[test]
fn math_inside_span_is_not_dropped() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Span Math")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>Let <span>the value <math xmlns=\"http://www.w3.org/1998/Math/MathML\" \
             alttext=\"z sub three\"><msub><mi>z</mi><mn>3</mn></msub></math> vary</span>.</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);
    let text = String::from_utf8_lossy(&kfx);
    assert!(
        text.contains("z sub three"),
        "math inside a span must survive (as inline readable text), not be dropped"
    );
    assert!(text.contains("the value") && text.contains("vary"));
}

/// A floated large-font span at a paragraph's start (the CSS dropcap idiom)
/// is rendered as a native KFX dropcap: `dropcap_lines`/`dropcap_chars` on
/// the paragraph style, matching Kindle Previewer, instead of a floated box
/// that reflows badly on device.
#[test]
fn css_dropcap_becomes_native_kfx_dropcap() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Dropcap Book")
        .css(".dropcap { float: left; font-size: 3.4em; line-height: 3em; }")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p><span class=\"dropcap\">T</span>he kid looked at Tobin \
             but the expriest sat without expression.</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    let mut lines = None;
    let mut chars = None;
    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        if let Some(v) = get_field(fields, KfxSymbol::DropcapLines).and_then(|v| v.as_int()) {
            lines = Some(v);
        }
        if let Some(v) = get_field(fields, KfxSymbol::DropcapChars).and_then(|v| v.as_int()) {
            chars = Some(v);
        }
    }
    assert_eq!(
        chars,
        Some(1),
        "dropcap_chars must count the leading letter"
    );
    assert_eq!(
        lines,
        Some(3),
        "dropcap_lines must span the floated letter's height (3.4em ≈ 3 lines)"
    );

    // No float property survives on any style — the dropcap replaces it.
    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        assert!(
            get_field(fields, KfxSymbol::Float).is_none(),
            "the dropcap span's float must be dropped"
        );
    }
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

/// Adjoining vertical margins must be collapsed statically, like Kindle
/// Previewer output: the Kindle renderer does not collapse margins, so an
/// uncollapsed `margin: 1em 0` paragraph sequence renders double gaps.
/// The collapsed value (max of the adjoining margins, resolved absolutely)
/// rides the following block's margin-top; only the section's last block
/// keeps a margin-bottom.
#[test]
fn adjoining_margins_collapse_statically() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Collapse Book")
        .css(".big { margin-bottom: 3em; } .after { margin-top: 2em; }")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>First paragraph with default margins.</p>\
             <p>Second paragraph with default margins.</p>\
             <p class=\"big\">Three-em bottom margin here.</p>\
             <p class=\"after\">Two-em top margin loses to the three.</p>\
             <p>Last paragraph keeps its bottom margin.</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    // Collect every (margin-top, margin-bottom) pair from emitted styles,
    // in lh units.
    let dim = |fields: &[(u64, IonValue)], sym: KfxSymbol| -> Option<f64> {
        let IonValue::Struct(d) = get_field(fields, sym)? else {
            return None;
        };
        let value = get_field(d, KfxSymbol::Value)?;
        let unit = get_field(d, KfxSymbol::Unit)?.as_symbol()?;
        assert_eq!(
            unit,
            KfxSymbol::Lh as u64,
            "vertical margins must be in lh units"
        );
        value
            .as_float()
            .or_else(|| value.as_int().map(|i| i as f64))
            .or_else(|| match value {
                IonValue::Decimal(s) => s.parse().ok(),
                _ => None,
            })
    };

    let mut margin_bottoms = 0;
    let mut saw_collapsed_3em = false;
    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        if dim(fields, KfxSymbol::MarginBottom).is_some() {
            margin_bottoms += 1;
        }
        if let Some(mt) = dim(fields, KfxSymbol::MarginTop) {
            // max(3em, 2em) = 3em = 2.5lh — the collapsed gap on the
            // following block.
            if (mt - 2.5).abs() < 1e-3 {
                saw_collapsed_3em = true;
            }
            assert!(
                (mt - 2.0 / 1.2).abs() > 1e-3,
                "an uncollapsed 2em margin-top survived (2em should lose to \
                 the preceding 3em margin-bottom)"
            );
        }
    }
    assert!(
        saw_collapsed_3em,
        "the collapsed 3em gap must ride the following block's margin-top"
    );
    assert_eq!(
        margin_bottoms, 1,
        "only the section's last block keeps a margin-bottom"
    );
}

/// `box_align: center` must require an author's explicit `margin: auto`.
/// Margins that were never set are the CSS initial `0`, not `auto` — gold
/// masters never center a `width: 50%` block with unset margins (it sits
/// left), and defaulted UA paragraph margins must not center every block.
#[test]
fn unset_margins_never_produce_box_align() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("No Centering Book")
        .css(".half { width: 50%; }")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>Default-margin paragraph.</p>\
             <p class=\"half\">Half-width block that must stay left.</p>\
             <blockquote><p>Quoted text.</p></blockquote>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        assert!(
            get_field(fields, KfxSymbol::BoxAlign).is_none(),
            "no block in this book sets margin:auto, yet a style carries box_align"
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

/// A bordered image must still be emitted as an image element. The border
/// container-wrapper assumes text content (its inner element is
/// `type: text`), so wrapping an image swallowed it entirely — a childless
/// container carrying resource_name with no image node.
#[test]
fn bordered_image_keeps_image_element() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Bordered Image Book")
        .css("img { border: 2px solid black; }")
        .image("images/shot.png", common::tiny_png())
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>before</p><img src=\"../images/shot.png\" alt=\"shot\"/><p>after</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    // Some node with type: image and a resource_name must exist, and no
    // container may carry a resource_name.
    let mut image_nodes = 0;
    let mut containers_with_resource = 0;
    fn walk(v: &IonValue, images: &mut u32, bad_containers: &mut u32) {
        match v {
            IonValue::Struct(fields) => {
                let node_type = get_field(fields, KfxSymbol::Type).and_then(|v| v.as_symbol());
                let has_resource = get_field(fields, KfxSymbol::ResourceName).is_some();
                if node_type == Some(KfxSymbol::Image as u64) && has_resource {
                    *images += 1;
                }
                if node_type == Some(KfxSymbol::Container as u64) && has_resource {
                    *bad_containers += 1;
                }
                for (_, val) in fields {
                    walk(val, images, bad_containers);
                }
            }
            IonValue::List(items) => items.iter().for_each(|i| walk(i, images, bad_containers)),
            IonValue::Annotated(_, inner) => walk(inner, images, bad_containers),
            _ => {}
        }
    }
    for storyline in parse_entities(&kfx, KfxSymbol::Storyline as u32) {
        walk(&storyline, &mut image_nodes, &mut containers_with_resource);
    }
    assert!(
        image_nodes > 0,
        "bordered image must survive as an image element"
    );
    assert_eq!(
        containers_with_resource, 0,
        "no container may carry a stranded resource_name"
    );
}

/// `visibility` must encode as an Ion boolean (true = visible, false =
/// hidden), matching reference KFX; readers flag symbol values as
/// unexpected style data.
#[test]
fn visibility_encodes_as_boolean() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Visibility Book")
        .css(".h { visibility: hidden; }")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<p>shown</p><p class=\"h\">hidden paragraph</p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    let mut saw_visibility = false;
    for style in parse_entities(&kfx, KfxSymbol::Style as u32) {
        let IonValue::Struct(fields) = &style else {
            continue;
        };
        if let Some(value) = get_field(fields, KfxSymbol::Visibility) {
            saw_visibility = true;
            assert!(
                matches!(value, IonValue::Bool(false)),
                "visibility: hidden must encode as Ion bool false, got: {value:?}"
            );
        }
    }
    assert!(
        saw_visibility,
        "hidden paragraph should emit a visibility property"
    );
}

/// Consecutive empty anchor targets must not produce anchor positions with
/// offsets into dropped marker text. An empty target element emits no
/// content; a second anchor in the same run used to sit at offset 1 past
/// nothing, which readers cannot locate ("locate_offset failed", broken
/// in-book navigation). All anchors into an empty element resolve at 0.
#[test]
fn anchors_into_empty_targets_have_zero_offset() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Anchor Book")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "One",
            "<div><a id=\"first\"></a><a id=\"second\"></a></div>\
             <p>go to <a href=\"#first\">first</a> and <a href=\"#second\">second</a></p>",
        ))
        .nav(vec![Nav::new("One", "text/ch1.xhtml")])
        .build();

    let mut book = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut book, Format::Kfx);

    // Content-bearing eids (those with a content ref or style events have
    // locatable text; anchors may carry offsets only into those).
    let mut text_eids = std::collections::BTreeSet::new();
    fn collect_text_eids(v: &IonValue, out: &mut std::collections::BTreeSet<i64>) {
        match v {
            IonValue::Struct(fields) => {
                let id = get_field(fields, KfxSymbol::Id).and_then(|v| v.as_int());
                if let Some(id) = id
                    && get_field(fields, KfxSymbol::Content).is_some()
                {
                    out.insert(id);
                }
                for (_, val) in fields {
                    collect_text_eids(val, out);
                }
            }
            IonValue::List(items) => items.iter().for_each(|i| collect_text_eids(i, out)),
            IonValue::Annotated(_, inner) => collect_text_eids(inner, out),
            _ => {}
        }
    }
    for storyline in parse_entities(&kfx, KfxSymbol::Storyline as u32) {
        collect_text_eids(&storyline, &mut text_eids);
    }

    let mut checked = 0;
    for anchor in parse_entities(&kfx, KfxSymbol::Anchor as u32) {
        let IonValue::Struct(fields) = &anchor else {
            continue;
        };
        let Some(IonValue::Struct(pos)) = get_field(fields, KfxSymbol::Position) else {
            continue;
        };
        let id = get_field(pos, KfxSymbol::Id)
            .and_then(|v| v.as_int())
            .unwrap_or(-1);
        let offset = get_field(pos, KfxSymbol::Offset)
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        checked += 1;
        if offset > 0 {
            assert!(
                text_eids.contains(&id),
                "anchor offset {offset} points into eid {id}, which has no content"
            );
        }
    }
    assert!(checked > 0, "expected anchor entities in the output");
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
