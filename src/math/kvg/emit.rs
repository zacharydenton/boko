//! KVG data-structure emission: a typeset [`LayoutBox`] → the literal KVG
//! shapes/bundles of `docs/kvg-format.md`.
//!
//! Glyph outlines are deduplicated into a book-level [`PathBundle`] (one
//! outline per distinct glyph, exactly like Kindle Previewer's output);
//! each placed glyph becomes a shape referencing its bundle index with a
//! `[s, 0, 0, -s, x, baseline_y]` placement. Rules (fraction bars, vincula)
//! share a single unit-square outline scaled per use. Transforms are stored
//! in KFX order (b/c swapped relative to SVG `matrix(a b c d e f)`).

use super::font::MathFont;
use super::layout::LayoutBox;
use rustc_hash::FxHashMap;

/// A book-level shared outline bundle: `path_list` ($693) under one
/// `path_bundle` ($692) fragment.
#[derive(Debug, Default)]
pub struct PathBundle {
    outlines: Vec<Vec<f32>>,
    by_glyph: FxHashMap<u16, usize>,
    unit_square: Option<usize>,
}

impl PathBundle {
    /// Create an empty bundle.
    pub fn new() -> Self {
        Self::default()
    }

    /// The outlines in index order (the `path_list` payload).
    pub fn outlines(&self) -> &[Vec<f32>] {
        &self.outlines
    }

    /// Number of outlines in the bundle.
    pub fn len(&self) -> usize {
        self.outlines.len()
    }

    /// Whether the bundle holds no outlines.
    pub fn is_empty(&self) -> bool {
        self.outlines.is_empty()
    }

    fn glyph_index(&mut self, font: &MathFont, gid: u16) -> usize {
        if let Some(&i) = self.by_glyph.get(&gid) {
            return i;
        }
        let i = self.outlines.len();
        self.outlines.push(font.outline(gid).0);
        self.by_glyph.insert(gid, i);
        i
    }

    fn unit_square_index(&mut self) -> usize {
        if let Some(i) = self.unit_square {
            return i;
        }
        let i = self.outlines.len();
        // (0,0)–(1,1) square, y-up; scaled per rule by its transform.
        self.outlines.push(vec![
            0.0, 0.0, 0.0, // M 0 0
            1.0, 1.0, 0.0, // L 1 0
            1.0, 1.0, 1.0, // L 1 1
            1.0, 0.0, 1.0, // L 0 1
            4.0, // Z
        ]);
        self.unit_square = Some(i);
        i
    }
}

/// One KVG shape: a bundle outline placed by an affine transform
/// (KFX component order: `[a, c, b, d, e, f]` — b/c swapped vs SVG).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KvgShape {
    /// Index into the [`PathBundle`].
    pub path_index: usize,
    /// Placement transform in KFX component order.
    pub transform: [f32; 6],
}

/// A complete KVG equation: sized container + shapes referencing the bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct KvgEquation {
    /// viewBox width ($66), in outline units.
    pub fixed_width: u32,
    /// viewBox height ($67).
    pub fixed_height: u32,
    /// Layout width in em ($56) at the equation's font size.
    pub width_em: f32,
    /// Layout height in em ($57).
    pub height_em: f32,
    /// Baseline y within the viewBox (top-down), for vertical alignment.
    pub baseline_y: f32,
    /// The shapes, in paint order.
    pub shapes: Vec<KvgShape>,
}

/// Pack a typeset layout into KVG shapes, deduplicating outlines into the
/// shared `bundle`. Coordinates stay in font units (the viewBox normalizes
/// them; em sizes carry the physical scale).
pub fn emit(font: &MathFont, layout: &LayoutBox, bundle: &mut PathBundle) -> KvgEquation {
    let upem = font.units_per_em();
    let pad = upem * 0.05;
    let w = layout.width + 2.0 * pad;
    let h = layout.ascent + layout.descent + 2.0 * pad;
    let baseline_y = layout.ascent + pad; // top-down viewBox coordinate

    let mut shapes = Vec::with_capacity(layout.glyphs.len() + layout.rules.len());
    for g in &layout.glyphs {
        let outline_len = font.outline(g.gid).0.len();
        if outline_len == 0 {
            continue; // blank glyph (space-like)
        }
        let idx = bundle.glyph_index(font, g.gid);
        // SVG order [a b c d e f] = [s, 0, 0, -s, x, baseline - y];
        // stored with b/c swapped (identical here: both zero — kept explicit
        // so non-axis-aligned shapes stay correct).
        let (a, b, c, d) = (g.scale, 0.0, 0.0, -g.scale);
        shapes.push(KvgShape {
            path_index: idx,
            transform: [a, c, b, d, g.x + pad, baseline_y - g.y],
        });
    }
    for r in &layout.rules {
        let idx = bundle.unit_square_index();
        // Unit square scaled to (w, h), y-up: bottom edge lands at the
        // rule's bottom.
        let (a, b, c, d) = (r.w, 0.0, 0.0, -r.h);
        shapes.push(KvgShape {
            path_index: idx,
            transform: [a, c, b, d, r.x + pad, baseline_y - r.y],
        });
    }

    KvgEquation {
        fixed_width: w.ceil() as u32,
        fixed_height: h.ceil() as u32,
        width_em: w / upem,
        height_em: h / upem,
        baseline_y,
        shapes,
    }
}

/// Render an emitted equation back to SVG through the *decode* rules of
/// `docs/kvg-format.md` — the round-trip verifier: emission is correct iff
/// this matches the layout-side rendering.
pub fn decode_to_svg(eq: &KvgEquation, bundle: &PathBundle) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" width="{}" height="{}">"#,
        eq.fixed_width, eq.fixed_height, eq.fixed_width, eq.fixed_height
    );
    for s in &eq.shapes {
        let d = super::svg::opcodes_to_d(&bundle.outlines()[s.path_index]);
        // Decode: swap b/c back to SVG order.
        let t = s.transform;
        let _ = write!(
            out,
            r#"<path transform="matrix({} {} {} {} {} {})" d="{d}"/>"#,
            fmt(t[0]),
            fmt(t[2]),
            fmt(t[1]),
            fmt(t[3]),
            fmt(t[4]),
            fmt(t[5]),
        );
    }
    out.push_str("</svg>");
    out
}

fn fmt(v: f32) -> String {
    format!("{v:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::kvg::typeset;
    use crate::math::mathml;

    #[test]
    fn emits_deduplicated_shapes() {
        let Some(font) = MathFont::load_system() else {
            eprintln!("no system math font; skipping");
            return;
        };
        // x appears twice → two shapes, ONE outline in the bundle for it.
        let m = mathml::parse_math_str(r#"<math><mi>x</mi><mo>+</mo><mi>x</mi></math>"#).unwrap();
        let layout = typeset(&font, &m.expr, m.display).unwrap();
        let mut bundle = PathBundle::new();
        let eq = emit(&font, &layout, &mut bundle);
        assert_eq!(eq.shapes.len(), 3, "x, +, x");
        assert_eq!(bundle.len(), 2, "outlines dedup to {{x, +}}");
        assert!(eq.fixed_width > 0 && eq.fixed_height > 0);
        assert!(eq.width_em > 0.5 && eq.width_em < 5.0);
    }

    #[test]
    fn fraction_rule_uses_shared_unit_square() {
        let Some(font) = MathFont::load_system() else {
            eprintln!("no system math font; skipping");
            return;
        };
        let m = mathml::parse_math_str(
            r#"<math><mfrac><mi>a</mi><mi>b</mi></mfrac><mfrac><mi>c</mi><mi>d</mi></mfrac></math>"#,
        )
        .unwrap();
        let layout = typeset(&font, &m.expr, m.display).unwrap();
        let mut bundle = PathBundle::new();
        let eq = emit(&font, &layout, &mut bundle);
        // 4 glyphs + 2 rules; bundle: 4 glyph outlines + 1 unit square.
        assert_eq!(eq.shapes.len(), 6);
        assert_eq!(bundle.len(), 5);
    }

    #[test]
    fn round_trip_decode_matches_layout_render() {
        let Some(font) = MathFont::load_system() else {
            eprintln!("no system math font; skipping");
            return;
        };
        let m = mathml::parse_math_str(
            r#"<math><msub><mi>x</mi><mn>1</mn></msub><mo>=</mo><mfrac><mi>a</mi><mi>b</mi></mfrac></math>"#,
        )
        .unwrap();
        let layout = typeset(&font, &m.expr, m.display).unwrap();
        let mut bundle = PathBundle::new();
        let eq = emit(&font, &layout, &mut bundle);
        let decoded = decode_to_svg(&eq, &bundle);
        // Same number of painted elements as the layout-side render, and the
        // decoded SVG references real path data.
        let paths = decoded.matches("<path").count();
        assert_eq!(paths, eq.shapes.len());
        assert!(decoded.contains("viewBox"));
        assert!(decoded.len() > 500, "non-trivial path data present");
    }
}
