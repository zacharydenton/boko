//! KFX must preserve table cell spans and ordered-list start values.
//!
//! These live in the IR `SemanticMap` (`col_span`, `row_span`, `list_start`)
//! and are emitted as Ion integers on the storyline element. Before the
//! carriers existed, KFX export dropped them: spanned cells collapsed to 1x1
//! and `<ol start=N>` lost its numbering.

mod common;

use boko::model::{Format, Role};

#[test]
fn kfx_preserves_table_spans_and_ol_start() {
    use common::{Doc, EpubBuilder, Nav};

    let epub = EpubBuilder::new("Spans Book")
        .doc(Doc::new(
            "text/ch1.xhtml",
            "Spans",
            "<h1>Grid</h1>\
             <table><thead><tr><th>Head</th></tr></thead><tbody>\
             <tr><td colspan=\"2\">wide</td><td rowspan=\"3\">tall</td></tr>\
             <tr><td>a</td><td>b</td></tr>\
             </tbody></table>\
             <ol start=\"5\"><li>five</li><li>six</li></ol>",
        ))
        .nav(vec![Nav::new("Grid", "text/ch1.xhtml")])
        .build();

    // Round-trip EPUB → KFX → import.
    let mut src = boko::Book::from_bytes(&epub, Format::Epub).expect("import epub");
    let kfx = common::export_to_bytes(&mut src, Format::Kfx);
    let out = boko::Book::from_bytes(&kfx, Format::Kfx).expect("import kfx");

    // Table cell spans and the th/td distinction survive.
    let (mut saw_colspan, mut saw_rowspan, mut saw_header) = (false, false, false);
    let cell_ids: Vec<_> = {
        let ids: Vec<_> = out.spine().iter().map(|e| e.id).collect();
        let mut v = Vec::new();
        for id in ids {
            let ch = out.load_chapter(id).expect("load");
            for nid in ch.iter_dfs() {
                if ch.node(nid).map(|n| n.role) == Some(Role::TableCell) {
                    if ch.semantics.col_span(nid) == Some(2) {
                        saw_colspan = true;
                    }
                    if ch.semantics.row_span(nid) == Some(3) {
                        saw_rowspan = true;
                    }
                    if ch.semantics.is_header_cell(nid) {
                        saw_header = true;
                    }
                    v.push(nid);
                }
            }
        }
        v
    };
    assert!(!cell_ids.is_empty(), "KFX round-trip lost the table cells");
    assert!(saw_colspan, "colspan=2 did not survive the KFX round trip");
    assert!(saw_rowspan, "rowspan=3 did not survive the KFX round trip");
    assert!(
        saw_header,
        "th header cell did not survive the KFX round trip"
    );

    // Ordered-list start survives.
    let mut saw_start = false;
    let ids: Vec<_> = out.spine().iter().map(|e| e.id).collect();
    for id in ids {
        let ch = out.load_chapter(id).expect("load");
        for nid in ch.iter_dfs() {
            if ch.node(nid).map(|n| n.role) == Some(Role::OrderedList)
                && ch.semantics.list_start(nid) == Some(5)
            {
                saw_start = true;
            }
        }
    }
    assert!(saw_start, "ol start=5 did not survive the KFX round trip");
}
