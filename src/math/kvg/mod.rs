//! KVG math typesetting: Math AST → positioned glyph shapes.
//!
//! The emission side of the reverse-engineered KVG format
//! (`docs/kvg-format.md`): a TeX-lite layout engine driven by an OpenType
//! MATH font (STIX Two Math) producing glyph placements that translate 1:1
//! into KVG `shape_list` entries + deduplicated `path_bundle` outlines.
//!
//! Pipeline: [`font::MathFont`] (glyphs, outlines, MATH constants) →
//! [`layout::typeset`] (positioned glyphs/rules in font units, y-up) →
//! [`svg::to_svg`] for verification, KFX fragment emission for export.

pub mod emit;
pub mod font;
pub mod layout;
pub mod svg;

pub use emit::{KvgEquation, PathBundle, emit};
pub use font::MathFont;
pub use layout::{LayoutBox, typeset};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Math;
    use crate::math::mathml;

    fn parse(s: &str) -> Math {
        let dom = crate::dom::parse_dom(s);
        let root = dom.find_by_tag("math").expect("math root");
        mathml::from_mathml(&dom, root)
    }

    fn font() -> Option<MathFont> {
        MathFont::load_system()
    }

    #[test]
    fn typesets_basic_constructs() {
        let Some(font) = font() else {
            eprintln!("no system math font; skipping");
            return;
        };
        for (mml, min_glyphs) in [
            (r#"<math><msub><mi>x</mi><mn>1</mn></msub></math>"#, 2),
            (
                r#"<math><mfrac><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mn>2</mn></mfrac></math>"#,
                4,
            ),
            (r#"<math><msqrt><mi>x</mi></msqrt></math>"#, 2),
            (
                r#"<math><munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover></math>"#,
                5,
            ),
        ] {
            let m = parse(mml);
            let lb = typeset(&font, &m.expr, m.display).expect("typeset");
            assert!(
                lb.glyphs.len() >= min_glyphs,
                "{mml}: expected ≥{min_glyphs} glyphs, got {}",
                lb.glyphs.len()
            );
            assert!(lb.width > 0.0 && lb.ascent > 0.0);
        }
    }

    #[test]
    fn raw_content_falls_back() {
        let Some(font) = font() else {
            eprintln!("no system math font; skipping");
            return;
        };
        let m = parse(r#"<math><menclose notation="box"><mi>x</mi></menclose></math>"#);
        assert!(
            typeset(&font, &m.expr, m.display).is_none(),
            "unmodeled content must decline so the caller uses the text run"
        );
    }

    #[test]
    fn fraction_stacks_vertically() {
        let Some(font) = font() else {
            eprintln!("no system math font; skipping");
            return;
        };
        let m = parse(r#"<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>"#);
        let lb = typeset(&font, &m.expr, m.display).expect("typeset");
        assert_eq!(lb.rules.len(), 1, "fraction bar");
        let (a, b) = (&lb.glyphs[0], &lb.glyphs[1]);
        assert!(a.y > b.y, "numerator above denominator");
        assert!(lb.ascent > 0.0 && lb.descent > 0.0);
    }
}
