//! Math expressions as a canonical presentation-math AST.
//!
//! Ebook math is authored as presentation MathML (`<math>` with `<mi>`,
//! `<mo>`, `<msup>`, `<mfrac>`, … children). Rather than convert directly
//! between MathML, LaTeX, and Kindle Vector Graphics pairwise, boko lifts
//! math into one canonical tree ([`MathExpr`]) that every format maps to and
//! from — a hub with one importer/exporter per format:
//!
//! - MathML ⇄ tree ([`mathml`]) — EPUB in/out.
//! - tree → LaTeX ([`latex`]) — markdown out (GitHub renders `$…$`).
//! - tree → KVG — KFX out (deferred; a text fallback is used until then).
//!
//! The tree mirrors presentation MathML because that is what the sources
//! contain and what KFX retains. Anything the tree doesn't model is kept
//! verbatim in a [`MathExpr::Raw`] node so no format silently loses content.

pub mod latex;
pub mod mathml;

/// A math expression attached to a [`Role::Math`](crate::model::Role) node,
/// stored in the chapter's `math` side-table.
#[derive(Debug, Clone, PartialEq)]
pub struct Math {
    /// The expression tree.
    pub expr: MathExpr,
    /// Display (block) vs inline math — from `<math display="block">` or the
    /// element's flow context. Chooses `$$…$$` vs `$…$` on export.
    pub display: bool,
    /// The source `alttext` (usually a spoken form), kept for the KFX text
    /// fallback and accessibility.
    pub alttext: Option<String>,
}

/// The kind of a leaf math token, from the MathML token element it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// `<mi>` — an identifier (variable, function name).
    Ident,
    /// `<mo>` — an operator or fence.
    Op,
    /// `<mn>` — a numeric literal.
    Num,
    /// `<mtext>` / `<ms>` — literal text within math.
    Text,
}

/// A node in the presentation-math tree.
#[derive(Debug, Clone, PartialEq)]
pub enum MathExpr {
    /// `<mrow>` and transparent wrappers (`<mstyle>`, `<mpadded>`): a
    /// horizontal sequence of sub-expressions.
    Row(Vec<MathExpr>),
    /// A leaf token (`<mi>`/`<mo>`/`<mn>`/`<mtext>`/`<ms>`).
    Token {
        /// Which token element this came from.
        kind: TokenKind,
        /// The token's literal text (Unicode).
        text: String,
    },
    /// `<msub>` — base with a subscript.
    Sub(Box<MathExpr>, Box<MathExpr>),
    /// `<msup>` — base with a superscript.
    Sup(Box<MathExpr>, Box<MathExpr>),
    /// `<msubsup>` — base with subscript and superscript.
    SubSup(Box<MathExpr>, Box<MathExpr>, Box<MathExpr>),
    /// `<munder>` — base with an under-script.
    Under {
        /// The base expression.
        base: Box<MathExpr>,
        /// The under-script.
        under: Box<MathExpr>,
    },
    /// `<mover>` — base with an over-script (accents, bars).
    Over {
        /// The base expression.
        base: Box<MathExpr>,
        /// The over-script.
        over: Box<MathExpr>,
    },
    /// `<munderover>` — base with under- and over-scripts (∑ bounds).
    UnderOver {
        /// The base expression.
        base: Box<MathExpr>,
        /// The under-script.
        under: Box<MathExpr>,
        /// The over-script.
        over: Box<MathExpr>,
    },
    /// `<mfrac>` — numerator over denominator.
    Frac(Box<MathExpr>, Box<MathExpr>),
    /// `<msqrt>` — a square root.
    Sqrt(Box<MathExpr>),
    /// `<mroot>` — an nth root: `(index, radicand)`.
    Root(Box<MathExpr>, Box<MathExpr>),
    /// `<mfenced>` — a bracketed group; `open`/`close` are the fence glyphs
    /// (default `(` / `)`).
    Fenced {
        /// The opening fence glyph.
        open: String,
        /// The closing fence glyph.
        close: String,
        /// The fenced content.
        body: Box<MathExpr>,
    },
    /// `<mtable>` — a matrix/array of rows of cells.
    Table(Vec<Vec<MathExpr>>),
    /// `<mspace>` — explicit spacing.
    Space,
    /// An element the tree doesn't model, kept verbatim so no format loses
    /// it. At least one of `mathml`/`latex` is populated.
    Raw {
        /// The element's serialized MathML source, if captured.
        mathml: Option<String>,
        /// A LaTeX rendering, if available.
        latex: Option<String>,
    },
}

impl Math {
    /// A lossy, plain-text linearization of the expression — the KFX interim
    /// fallback (until KVG rendering) and the last-resort fallback anywhere a
    /// richer converter cannot render a construct. Prefers the source
    /// `alttext` when present; otherwise walks the tree, using Unicode
    /// sub/superscript glyphs where they exist.
    pub fn to_text(&self) -> String {
        if let Some(alt) = &self.alttext {
            let alt = alt.trim();
            if !alt.is_empty() {
                return alt.to_string();
            }
        }
        let mut out = String::new();
        self.expr.write_text(&mut out);
        out
    }
}

impl MathExpr {
    fn write_text(&self, out: &mut String) {
        match self {
            MathExpr::Row(items) => {
                for it in items {
                    it.write_text(out);
                }
            }
            MathExpr::Token { text, .. } => out.push_str(text),
            MathExpr::Sub(b, s) => {
                b.write_text(out);
                write_script(out, s, unicode_subscript);
            }
            MathExpr::Sup(b, s) => {
                b.write_text(out);
                write_script(out, s, unicode_superscript);
            }
            MathExpr::SubSup(b, sub, sup) => {
                b.write_text(out);
                write_script(out, sub, unicode_subscript);
                write_script(out, sup, unicode_superscript);
            }
            MathExpr::Under { base, under } => {
                base.write_text(out);
                out.push('_');
                bracket_text(out, under);
            }
            MathExpr::Over { base, over } => {
                base.write_text(out);
                out.push('^');
                bracket_text(out, over);
            }
            MathExpr::UnderOver { base, under, over } => {
                base.write_text(out);
                out.push('_');
                bracket_text(out, under);
                out.push('^');
                bracket_text(out, over);
            }
            MathExpr::Frac(n, d) => {
                out.push('(');
                n.write_text(out);
                out.push_str(")/(");
                d.write_text(out);
                out.push(')');
            }
            MathExpr::Sqrt(x) => {
                out.push('√');
                bracket_text(out, x);
            }
            MathExpr::Root(i, x) => {
                out.push('√');
                out.push('[');
                i.write_text(out);
                out.push(']');
                bracket_text(out, x);
            }
            MathExpr::Fenced { open, close, body } => {
                out.push_str(open);
                body.write_text(out);
                out.push_str(close);
            }
            MathExpr::Table(rows) => {
                out.push('[');
                for (r, row) in rows.iter().enumerate() {
                    if r > 0 {
                        out.push_str("; ");
                    }
                    for (c, cell) in row.iter().enumerate() {
                        if c > 0 {
                            out.push_str(", ");
                        }
                        cell.write_text(out);
                    }
                }
                out.push(']');
            }
            MathExpr::Space => out.push(' '),
            MathExpr::Raw { latex, .. } => {
                if let Some(l) = latex {
                    out.push_str(l);
                }
            }
        }
    }
}

/// Write a sub/superscript as text: use Unicode script glyphs when every
/// character has one (`x^2` → `x²`), otherwise fall back to `^{…}` / `_{…}`.
fn write_script(out: &mut String, expr: &MathExpr, map: fn(char) -> Option<char>) -> Option<()> {
    let mut inner = String::new();
    expr.write_text(&mut inner);
    if let Some(scripted) = inner.chars().map(map).collect::<Option<String>>() {
        out.push_str(&scripted);
        Some(())
    } else {
        // Ambiguous as a bare glyph run — mark it. `^` for super, but the
        // caller distinguishes; here we just wrap so it isn't lost.
        out.push('{');
        out.push_str(&inner);
        out.push('}');
        Some(())
    }
}

/// Emit `expr` in braces when it is more than a single glyph, matching how a
/// reader would parenthesize a compound script/root.
fn bracket_text(out: &mut String, expr: &MathExpr) {
    let mut inner = String::new();
    expr.write_text(&mut inner);
    if inner.chars().count() <= 1 {
        out.push_str(&inner);
    } else {
        out.push('(');
        out.push_str(&inner);
        out.push(')');
    }
}

/// Map a character to its Unicode subscript glyph, if one exists.
fn unicode_subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'o' => 'ₒ',
        'x' => 'ₓ',
        'n' => 'ₙ',
        _ => return None,
    })
}

/// Map a character to its Unicode superscript glyph, if one exists.
fn unicode_superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        _ => return None,
    })
}
