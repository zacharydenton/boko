//! End-to-end tests for the HTML → IR (with optimizer) → Markdown pipeline.
//!
//! These deliberately run the full compile pipeline (including the optimizer
//! passes that unit tests bypass) and assert on the exported Markdown.

mod common;

use boko::model::Format;
use common::{Doc, EpubBuilder, Nav};

/// Build a single-chapter book from the given body XHTML and export it to
/// Markdown.
fn markdown_for_body(body: &str) -> String {
    let mut book = EpubBuilder::new("Markdown Pipeline")
        .doc(Doc::new("text/ch1.xhtml", "Chapter 1", body))
        .nav(vec![Nav::new("Chapter 1", "text/ch1.xhtml")])
        .book();
    String::from_utf8(common::export_to_bytes(&mut book, Format::Markdown))
        .expect("markdown output is utf-8")
}

// ---------------------------------------------------------------------------
// Tables: the optimizer wraps rows in TableHead/TableBody; the renderer must
// descend into the wrappers (previously a 2x2 table rendered as one broken
// pseudo-row like `| SampleVoltage |`).
// ---------------------------------------------------------------------------

#[test]
fn table_with_th_header_renders_proper_gfm() {
    let md = markdown_for_body(
        "<table>\
           <tr><th>Sample</th><th>Voltage</th></tr>\
           <tr><td>A</td><td>1.5</td></tr>\
         </table>",
    );

    let header = md.find("| Sample | Voltage |").expect("header row");
    let delim = md.find("| --- | --- |").expect("delimiter row");
    let body = md.find("| A | 1.5 |").expect("body row");
    assert!(
        header < delim && delim < body,
        "delimiter must follow the header row: {md}"
    );
    assert!(
        !md.contains("SampleVoltage"),
        "rows must not collapse into one pseudo-row: {md}"
    );
}

#[test]
fn table_without_th_uses_first_row_as_header() {
    let md = markdown_for_body(
        "<table>\
           <tr><td>Sample</td><td>Voltage</td></tr>\
           <tr><td>A</td><td>1.5</td></tr>\
         </table>",
    );

    let first = md.find("| Sample | Voltage |").expect("first row");
    let delim = md.find("| --- | --- |").expect("delimiter row");
    let second = md.find("| A | 1.5 |").expect("second row");
    assert!(
        first < delim && delim < second,
        "delimiter must follow the first row: {md}"
    );
}

#[test]
fn table_with_explicit_thead_tbody_renders_proper_gfm() {
    let md = markdown_for_body(
        "<table>\
           <thead><tr><th>Name</th><th>Value</th></tr></thead>\
           <tbody><tr><td>pi</td><td>3.14</td></tr></tbody>\
         </table>",
    );

    let header = md.find("| Name | Value |").expect("header row");
    let delim = md.find("| --- | --- |").expect("delimiter row");
    let body = md.find("| pi | 3.14 |").expect("body row");
    assert!(header < delim && delim < body, "row order: {md}");
}

#[test]
fn table_cells_escape_markdown_markers() {
    let md = markdown_for_body(
        "<table>\
           <tr><td>a|b</td><td>*em*</td><td>[br]</td></tr>\
           <tr><td>1</td><td>2</td><td>3</td></tr>\
         </table>",
    );

    assert!(md.contains("a\\|b"), "pipe escaped: {md}");
    assert!(md.contains("\\*em\\*"), "asterisks escaped: {md}");
    // Table cells now go through full body escaping, so both brackets escape.
    assert!(md.contains("\\[br\\]"), "bracket escaped: {md}");
}

// ---------------------------------------------------------------------------
// List fusion guards: fragmented lists still fuse, but not when fusing would
// destroy an anchor (id=) or ordered-list numbering (start=).
// ---------------------------------------------------------------------------

#[test]
fn plain_fragmented_lists_still_fuse() {
    let md = markdown_for_body(
        "<ul><li>One</li></ul>\
         <ul><li>Two</li></ul>",
    );

    assert!(md.contains("- One"), "first item: {md}");
    assert!(md.contains("- Two"), "second item: {md}");
    // Fused into a single list: no adjacent-list separator comment.
    assert!(
        !md.contains("<!-- -->"),
        "fused lists need no separator: {md}"
    );
}

#[test]
fn lists_with_id_on_second_are_not_fused() {
    let md = markdown_for_body(
        "<ul><li>One</li></ul>\
         <ul id=\"keep\"><li>Two</li></ul>",
    );

    assert!(md.contains("- One"), "first item: {md}");
    assert!(md.contains("- Two"), "second item: {md}");
    // Two separate lists render with the adjacent-list separator.
    assert!(
        md.contains("<!-- -->"),
        "unfused adjacent lists get a separator: {md}"
    );
}

#[test]
fn ordered_lists_with_start_are_not_fused_and_numbering_survives() {
    let md = markdown_for_body(
        "<ol><li>Alpha</li></ol>\
         <ol start=\"5\"><li>Bravo</li></ol>",
    );

    assert!(md.contains("1. Alpha"), "first list starts at 1: {md}");
    assert!(
        md.contains("5. Bravo"),
        "second list keeps its start attribute: {md}"
    );
}

// ---------------------------------------------------------------------------
// Escaping through the pipeline: line-start markers in plain prose.
// ---------------------------------------------------------------------------

#[test]
fn prose_resembling_list_markers_is_escaped() {
    let md = markdown_for_body(
        "<p>- dash prose</p>\
         <p>1. numbered prose</p>\
         <p>a - b stays</p>",
    );

    assert!(md.contains("\\- dash prose"), "dash escaped: {md}");
    assert!(md.contains("1\\. numbered prose"), "number escaped: {md}");
    assert!(md.contains("a - b stays"), "mid-line dash untouched: {md}");
}
