//! Golden fingerprints of the CSS cascade output.
//!
//! `normalize_book` runs every EPUB chapter through the full HTML+CSS -> IR
//! pipeline (including the cascade in `style::cascade`) and emits synthesized
//! XHTML with `.cNN` classes plus a unified stylesheet. Its output is a
//! deterministic fingerprint of the computed styles for every element, so any
//! change to the cascade that alters a single matched declaration changes a
//! fingerprint here.
//!
//! This guards cascade optimizations (e.g. selector bucketing): the fast path
//! must produce byte-identical computed styles to the exhaustive path.

mod common;

use boko::Book;
use boko::export::normalize_book;
use common::{Doc, EpubBuilder, Nav};

/// SHA-1 of the normalized CSS + every chapter document. Reflects the exact set
/// of declarations the cascade matched to each element.
fn cascade_fingerprint(book: &mut Book) -> String {
    let nc = normalize_book(book).expect("normalize");
    let mut hasher = sha1_smol::Sha1::new();
    hasher.update(b"css\0");
    hasher.update(nc.css.as_bytes());
    for ch in &nc.chapters {
        hasher.update(b"\0doc\0");
        hasher.update(ch.source_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(ch.document.as_bytes());
    }
    hasher.digest().to_string()
}

/// A class-heavy book: exercises class and multi-class selector matching.
fn class_book() -> EpubBuilder {
    EpubBuilder::new("Class Book")
        .css(
            ".intro { font-style: italic; } \
             .warning { color: #900; font-weight: bold; } \
             p.warning.urgent { text-decoration: underline; } \
             .box .label { font-size: 0.8em; } \
             span.tag { letter-spacing: 0.1em; }",
        )
        .doc(Doc::new(
            "text/ch1.xhtml",
            "Classes",
            "<h1>Heading</h1>\
             <p class=\"intro\">An introductory paragraph.</p>\
             <p class=\"warning\">A warning here.</p>\
             <p class=\"warning urgent\">An urgent warning.</p>\
             <div class=\"box\"><span class=\"label\">labelled</span> content</div>\
             <p>Plain <span class=\"tag\">tagged</span> text.</p>",
        ))
        .nav(vec![Nav::new("Heading", "text/ch1.xhtml")])
}

/// An id + descendant/child selector book.
fn descendant_book() -> EpubBuilder {
    EpubBuilder::new("Descendant Book")
        .css(
            "#main { margin: 1em; } \
             #main p { line-height: 1.6; } \
             article > h2 { color: #234; } \
             blockquote em { font-weight: bold; } \
             ul li a { text-decoration: none; }",
        )
        .doc(Doc::new(
            "text/ch1.xhtml",
            "Nested",
            "<div id=\"main\">\
               <article><h2>Section</h2><p>Body text within main.</p></article>\
               <blockquote><p>Quoted <em>emphasis</em> here.</p></blockquote>\
               <ul><li><a href=\"#x\">link</a></li></ul>\
             </div>",
        ))
        .nav(vec![Nav::new("Nested", "text/ch1.xhtml")])
}

// Baseline fingerprints captured on master before the cascade optimization.
// If a cascade change alters computed styles, these fail — investigate before
// updating them.
// Updated after the normalized-export document template moved to the EPUB 3
// HTML5 DOCTYPE / `<meta charset>` and internal-link href rewriting. The cascade
// output (matched declarations) is unchanged — only the surrounding document
// markup differs.
const FP_EPICTETUS: &str = "92888004a84273c70366220364f8007dd6619760";
const FP_CLASS: &str = "8e084650371da2cab255c51438fd8bdb401ebc42";
const FP_DESCENDANT: &str = "e6c0a96d4233a78a589342f442811d84bd15c23f";

#[test]
fn cascade_output_is_stable_epictetus() {
    let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
    let fp = cascade_fingerprint(&mut book);
    println!("FP_EPICTETUS = {fp}");
    if FP_EPICTETUS != "REPLACE_EPICTETUS" {
        assert_eq!(fp, FP_EPICTETUS, "epictetus cascade output changed");
    }
}

#[test]
fn cascade_output_is_stable_classes() {
    let fp = cascade_fingerprint(&mut class_book().book());
    println!("FP_CLASS = {fp}");
    if FP_CLASS != "REPLACE_CLASS" {
        assert_eq!(fp, FP_CLASS, "class-selector cascade output changed");
    }
}

#[test]
fn cascade_output_is_stable_descendants() {
    let fp = cascade_fingerprint(&mut descendant_book().book());
    println!("FP_DESCENDANT = {fp}");
    if FP_DESCENDANT != "REPLACE_DESCENDANT" {
        assert_eq!(
            fp, FP_DESCENDANT,
            "descendant-selector cascade output changed"
        );
    }
}
