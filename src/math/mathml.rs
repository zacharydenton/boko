//! MathML ⇄ [`Math`] tree.
//!
//! [`from_mathml`] lifts a parsed `<math>` DOM subtree into the canonical
//! tree; [`to_mathml`] serializes it back. Presentation MathML is
//! structurally the same tree, so both directions are near-lossless — an
//! element the tree doesn't model is kept verbatim as [`MathExpr::Raw`] with
//! its serialized source, so a round trip never loses content.

use crate::dom::{ArenaDom, ArenaNodeId};

use super::{Math, MathExpr, TokenKind};

/// The MathML namespace URI.
pub const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// Whether an element (by namespace or local name) is a MathML `<math>` root.
pub fn is_math_root(dom: &ArenaDom, id: ArenaNodeId) -> bool {
    dom.element_namespace(id).map(|ns| ns.as_ref()) == Some(MATHML_NS)
        || dom.element_name(id).map(|n| n.as_ref()) == Some("math")
}

/// Build a [`Math`] from a `<math>` element in the arena DOM.
pub fn from_mathml(dom: &ArenaDom, math_id: ArenaNodeId) -> Math {
    let display = matches!(dom.get_attr(math_id, "display"), Some("block"));
    let alttext = dom
        .get_attr(math_id, "alttext")
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    Math {
        expr: build_row(dom, math_id),
        display,
        alttext,
    }
}

/// Build an expression from the element children of `id`, wrapping multiple
/// children in a [`MathExpr::Row`] and unwrapping a single child.
fn build_row(dom: &ArenaDom, id: ArenaNodeId) -> MathExpr {
    let mut items: Vec<MathExpr> = dom
        .children(id)
        .filter(|&c| dom.is_element(c))
        .map(|c| build_expr(dom, c))
        .collect();
    match items.len() {
        0 => MathExpr::Row(Vec::new()),
        1 => items.pop().unwrap(),
        _ => MathExpr::Row(items),
    }
}

/// The element children of `id` as a Vec (for fixed-arity constructs).
fn elem_children(dom: &ArenaDom, id: ArenaNodeId) -> Vec<ArenaNodeId> {
    dom.children(id).filter(|&c| dom.is_element(c)).collect()
}

/// Convert one MathML element to a [`MathExpr`].
fn build_expr(dom: &ArenaDom, id: ArenaNodeId) -> MathExpr {
    let name = dom.element_name(id).map(|n| n.as_ref()).unwrap_or("");
    match name {
        "mi" => token(TokenKind::Ident, dom, id),
        "mn" => token(TokenKind::Num, dom, id),
        "mo" => token(TokenKind::Op, dom, id),
        "mtext" | "ms" => token(TokenKind::Text, dom, id),
        // Transparent grouping wrappers.
        "mrow" | "mstyle" | "mpadded" | "mphantom" => build_row(dom, id),
        "msub" => pair(dom, id, MathExpr::Sub),
        "msup" => pair(dom, id, MathExpr::Sup),
        "msubsup" => triple(dom, id, MathExpr::SubSup),
        "munder" => pair(dom, id, |base, under| MathExpr::Under { base, under }),
        "mover" => pair(dom, id, |base, over| MathExpr::Over { base, over }),
        "munderover" => triple(dom, id, |base, under, over| MathExpr::UnderOver {
            base,
            under,
            over,
        }),
        "mfrac" => pair(dom, id, MathExpr::Frac),
        "msqrt" => MathExpr::Sqrt(Box::new(build_row(dom, id))),
        // `<mroot>base index</mroot>`.
        "mroot" => {
            let kids = elem_children(dom, id);
            let radicand = kids.first().map(|&c| build_expr(dom, c)).unwrap_or_empty();
            let index = kids.get(1).map(|&c| build_expr(dom, c)).unwrap_or_empty();
            MathExpr::Root(Box::new(index), Box::new(radicand))
        }
        "mfenced" => {
            let open = dom.get_attr(id, "open").unwrap_or("(").to_string();
            let close = dom.get_attr(id, "close").unwrap_or(")").to_string();
            MathExpr::Fenced {
                open,
                close,
                body: Box::new(build_row(dom, id)),
            }
        }
        "mtable" => MathExpr::Table(
            elem_children(dom, id)
                .into_iter()
                .filter(|&r| dom.element_name(r).map(|n| n.as_ref()) == Some("mtr"))
                .map(|r| {
                    elem_children(dom, r)
                        .into_iter()
                        .filter(|&c| dom.element_name(c).map(|n| n.as_ref()) == Some("mtd"))
                        .map(|c| build_row(dom, c))
                        .collect()
                })
                .collect(),
        ),
        "mspace" => MathExpr::Space,
        // `<semantics>` wraps a presentation child plus annotations; take the
        // first presentation child.
        "semantics" => elem_children(dom, id)
            .first()
            .map(|&c| build_expr(dom, c))
            .unwrap_or_empty(),
        // Anything unmodeled: keep its source so no round trip loses it.
        _ => MathExpr::Raw {
            mathml: Some(serialize_element(dom, id)),
            latex: None,
        },
    }
}

/// Build a leaf token from a token element's collected text.
fn token(kind: TokenKind, dom: &ArenaDom, id: ArenaNodeId) -> MathExpr {
    MathExpr::Token {
        kind,
        text: collect_text(dom, id),
    }
}

/// Collect all descendant text of an element (token content).
fn collect_text(dom: &ArenaDom, id: ArenaNodeId) -> String {
    let mut out = String::new();
    collect_text_into(dom, id, &mut out);
    out
}

fn collect_text_into(dom: &ArenaDom, id: ArenaNodeId, out: &mut String) {
    if let Some(t) = dom.text_content(id) {
        out.push_str(t);
        return;
    }
    for c in dom.children(id) {
        collect_text_into(dom, c, out);
    }
}

/// Two element children → a binary constructor (missing children fill empty).
fn pair<F>(dom: &ArenaDom, id: ArenaNodeId, f: F) -> MathExpr
where
    F: FnOnce(Box<MathExpr>, Box<MathExpr>) -> MathExpr,
{
    let kids = elem_children(dom, id);
    let a = kids.first().map(|&c| build_expr(dom, c)).unwrap_or_empty();
    let b = kids.get(1).map(|&c| build_expr(dom, c)).unwrap_or_empty();
    f(Box::new(a), Box::new(b))
}

/// Three element children → a ternary constructor (missing children fill empty).
fn triple<F>(dom: &ArenaDom, id: ArenaNodeId, f: F) -> MathExpr
where
    F: FnOnce(Box<MathExpr>, Box<MathExpr>, Box<MathExpr>) -> MathExpr,
{
    let kids = elem_children(dom, id);
    let a = kids.first().map(|&c| build_expr(dom, c)).unwrap_or_empty();
    let b = kids.get(1).map(|&c| build_expr(dom, c)).unwrap_or_empty();
    let c = kids.get(2).map(|&c| build_expr(dom, c)).unwrap_or_empty();
    f(Box::new(a), Box::new(b), Box::new(c))
}

/// Small helper: an empty expression (used for missing script/base slots).
trait UnwrapOrEmpty {
    fn unwrap_or_empty(self) -> MathExpr;
}
impl UnwrapOrEmpty for Option<MathExpr> {
    fn unwrap_or_empty(self) -> MathExpr {
        self.unwrap_or_else(|| MathExpr::Row(Vec::new()))
    }
}

/// Re-serialize an arena element (and its subtree) to a MathML string, for
/// the [`MathExpr::Raw`] escape hatch.
fn serialize_element(dom: &ArenaDom, id: ArenaNodeId) -> String {
    let mut out = String::new();
    serialize_into(dom, id, &mut out);
    out
}

fn serialize_into(dom: &ArenaDom, id: ArenaNodeId, out: &mut String) {
    if let Some(t) = dom.text_content(id) {
        push_escaped_text(out, t);
        return;
    }
    let Some(name) = dom.element_name(id).map(|n| n.as_ref().to_string()) else {
        return;
    };
    out.push('<');
    out.push_str(&name);
    // Preserve open/close/display/alttext-style attributes we know matter.
    for attr in ["open", "close", "display", "mathvariant", "stretchy"] {
        if let Some(v) = dom.get_attr(id, attr) {
            out.push(' ');
            out.push_str(attr);
            out.push_str("=\"");
            push_escaped_attr(out, v);
            out.push('"');
        }
    }
    let children: Vec<ArenaNodeId> = dom.children(id).collect();
    if children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in children {
        serialize_into(dom, c, out);
    }
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
}

/// Serialize a [`Math`] tree back to a `<math>` MathML string.
pub fn to_mathml(math: &Math) -> String {
    let mut out = String::from("<math xmlns=\"");
    out.push_str(MATHML_NS);
    out.push('"');
    if math.display {
        out.push_str(" display=\"block\"");
    }
    if let Some(alt) = &math.alttext {
        out.push_str(" alttext=\"");
        push_escaped_attr(&mut out, alt);
        out.push('"');
    }
    out.push('>');
    write_mathml(&math.expr, &mut out);
    out.push_str("</math>");
    out
}

fn write_mathml(expr: &MathExpr, out: &mut String) {
    match expr {
        MathExpr::Row(items) => {
            out.push_str("<mrow>");
            for it in items {
                write_mathml(it, out);
            }
            out.push_str("</mrow>");
        }
        MathExpr::Token { kind, text } => {
            let tag = match kind {
                TokenKind::Ident => "mi",
                TokenKind::Op => "mo",
                TokenKind::Num => "mn",
                TokenKind::Text => "mtext",
            };
            out.push('<');
            out.push_str(tag);
            out.push('>');
            push_escaped_text(out, text);
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        MathExpr::Sub(b, s) => wrap2(out, "msub", b, s),
        MathExpr::Sup(b, s) => wrap2(out, "msup", b, s),
        MathExpr::SubSup(b, sub, sup) => wrap3(out, "msubsup", b, sub, sup),
        MathExpr::Under { base, under } => wrap2(out, "munder", base, under),
        MathExpr::Over { base, over } => wrap2(out, "mover", base, over),
        MathExpr::UnderOver { base, under, over } => wrap3(out, "munderover", base, under, over),
        MathExpr::Frac(n, d) => wrap2(out, "mfrac", n, d),
        MathExpr::Sqrt(x) => {
            out.push_str("<msqrt>");
            write_mathml(x, out);
            out.push_str("</msqrt>");
        }
        MathExpr::Root(i, x) => {
            // `<mroot>radicand index</mroot>`.
            out.push_str("<mroot>");
            write_mathml(x, out);
            write_mathml(i, out);
            out.push_str("</mroot>");
        }
        MathExpr::Fenced { open, close, body } => {
            out.push_str("<mfenced open=\"");
            push_escaped_attr(out, open);
            out.push_str("\" close=\"");
            push_escaped_attr(out, close);
            out.push_str("\">");
            write_mathml(body, out);
            out.push_str("</mfenced>");
        }
        MathExpr::Table(rows) => {
            out.push_str("<mtable>");
            for row in rows {
                out.push_str("<mtr>");
                for cell in row {
                    out.push_str("<mtd>");
                    write_mathml(cell, out);
                    out.push_str("</mtd>");
                }
                out.push_str("</mtr>");
            }
            out.push_str("</mtable>");
        }
        MathExpr::Space => out.push_str("<mspace/>"),
        MathExpr::Raw { mathml, .. } => {
            if let Some(m) = mathml {
                out.push_str(m);
            }
        }
    }
}

fn wrap2(out: &mut String, tag: &str, a: &MathExpr, b: &MathExpr) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    write_mathml(a, out);
    write_mathml(b, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn wrap3(out: &mut String, tag: &str, a: &MathExpr, b: &MathExpr, c: &MathExpr) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    write_mathml(a, out);
    write_mathml(b, out);
    write_mathml(c, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn push_escaped_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
}

fn push_escaped_attr(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{MathExpr, TokenKind};

    /// Parse an HTML fragment and build the `Math` from its `<math>` element.
    fn parse(html: &str) -> Math {
        let dom = crate::dom::parse_dom(html);
        let math_id = dom.find_by_tag("math").expect("a <math> element");
        from_mathml(&dom, math_id)
    }

    #[test]
    fn imports_structure() {
        let m = parse(r#"<math display="block"><msup><mi>E</mi></msup></math>"#);
        assert!(m.display);
        // E with an (empty) superscript.
        assert!(matches!(m.expr, MathExpr::Sup(..)));

        let m = parse(r#"<math><msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup></math>"#);
        match m.expr {
            MathExpr::SubSup(b, sub, sup) => {
                assert_eq!(
                    *b,
                    MathExpr::Token {
                        kind: TokenKind::Ident,
                        text: "x".into()
                    }
                );
                assert_eq!(
                    *sub,
                    MathExpr::Token {
                        kind: TokenKind::Num,
                        text: "1".into()
                    }
                );
                assert_eq!(
                    *sup,
                    MathExpr::Token {
                        kind: TokenKind::Num,
                        text: "2".into()
                    }
                );
            }
            other => panic!("expected SubSup, got {other:?}"),
        }
    }

    #[test]
    fn fenced_and_table() {
        let m = parse(
            r#"<math><mfenced open="[" close="]"><mtable><mtr><mtd><mn>1</mn></mtd></mtr></mtable></mfenced></math>"#,
        );
        match m.expr {
            MathExpr::Fenced { open, close, body } => {
                assert_eq!(open, "[");
                assert_eq!(close, "]");
                assert!(matches!(*body, MathExpr::Table(_)));
            }
            other => panic!("expected Fenced, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_preserves_structure() {
        // from_mathml → to_mathml → from_mathml must yield the same tree.
        for src in [
            r#"<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>"#,
            r#"<math><msqrt><mrow><mi>x</mi><mo>+</mo><mn>1</mn></mrow></msqrt></math>"#,
            r#"<math><munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover></math>"#,
        ] {
            let m1 = parse(src);
            let serialized = to_mathml(&m1);
            let m2 = parse(&format!("<div>{}</div>", serialized));
            assert_eq!(m1.expr, m2.expr, "round trip changed {src}");
        }
    }

    #[test]
    fn to_text_prefers_alttext_then_linearizes() {
        let m = parse(r#"<math alttext="x squared"><msup><mi>x</mi><mn>2</mn></msup></math>"#);
        assert_eq!(m.to_text(), "x squared");

        // Without alttext, linearize with Unicode scripts.
        let m = parse(r#"<math><msup><mi>x</mi><mn>2</mn></msup></math>"#);
        assert_eq!(m.to_text(), "x²");
    }
}
