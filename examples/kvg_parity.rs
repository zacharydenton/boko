//! Gold-master parity harness: typeset every equation in a book with the KVG
//! engine and emit one SVG per equation, in reading order, plus a manifest
//! (index, alttext, status) for pairing against the Kindle Previewer gold
//! master's KVG renders (see `tools/kvg2svg.py`).
//!
//! Usage: cargo run --release --example kvg_parity -- <book.epub> <outdir>

use boko::math::kvg::{MathFont, svg, typeset};
use boko::model::{Chapter, NodeId, Role};
use std::io::Write;

fn collect_math(chapter: &Chapter, id: NodeId, out: &mut Vec<NodeId>) {
    if let Some(node) = chapter.node(id) {
        if node.role == Role::Math {
            out.push(id);
        }
        for child in chapter.children(id) {
            collect_math(chapter, child, out);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let book_path = args.next().expect("usage: kvg_parity <book.epub> <outdir>");
    let outdir = args.next().expect("outdir");
    std::fs::create_dir_all(&outdir).expect("outdir");

    let font = MathFont::load_system().expect("system math font");
    let book = boko::Book::open(&book_path).expect("open book");

    let mut manifest = std::fs::File::create(format!("{outdir}/manifest.jsonl")).unwrap();
    let mut n = 0usize;
    let mut ok = 0usize;
    let mut declined = 0usize;

    for entry in book.spine() {
        let chapter = match book.load_chapter(entry.id) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut ids = Vec::new();
        collect_math(&chapter, chapter.root(), &mut ids);
        for id in ids {
            let Some(math) = chapter.math.get(&id) else {
                continue;
            };
            let alttext = math.alttext.clone().unwrap_or_default();
            let status = match typeset(&font, &math.expr, math.display) {
                Some(layout) => {
                    let path = format!("{outdir}/eq{n:04}.svg");
                    std::fs::write(&path, svg::to_svg(&font, &layout)).unwrap();
                    ok += 1;
                    format!(
                        r#""ok","w":{:.0},"h":{:.0}"#,
                        layout.width,
                        layout.ascent + layout.descent
                    )
                }
                None => {
                    declined += 1;
                    r#""declined""#.to_string()
                }
            };
            writeln!(
                manifest,
                r#"{{"i":{n},"status":{status},"alttext":{}}}"#,
                serde_json_escape(&alttext)
            )
            .unwrap();
            n += 1;
        }
    }
    println!("equations: {n}  typeset: {ok}  declined: {declined}");
}

fn serde_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
