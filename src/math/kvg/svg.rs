//! Debug/verification SVG rendering of a typeset [`LayoutBox`].
//!
//! Mirrors the KVG→SVG decode rules from `docs/kvg-format.md`: y-up glyph
//! outlines placed with `[s, 0, 0, -s, x, baseline]` transforms inside a
//! y-down viewBox. What renders correctly here will render correctly as KVG,
//! since the coordinate model is identical.

use super::font::MathFont;
use super::layout::LayoutBox;
use std::fmt::Write;

/// Render a typeset box as a standalone SVG (black-on-transparent).
pub fn to_svg(font: &MathFont, layout: &LayoutBox) -> String {
    let pad = font.units_per_em() * 0.05;
    let w = layout.width + 2.0 * pad;
    let h = layout.ascent + layout.descent + 2.0 * pad;
    let baseline_y = layout.ascent + pad; // y-down viewBox

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.1} {h:.1}" width="{w:.1}" height="{h:.1}">"#
    );

    for g in &layout.glyphs {
        let outline = font.outline(g.gid);
        if outline.0.is_empty() {
            continue;
        }
        let d = opcodes_to_d(&outline.0);
        // y-up outline → y-down viewBox: flip about the baseline.
        let _ = write!(
            out,
            r#"<path transform="matrix({s:.4} 0 0 {ns:.4} {x:.1} {y:.1})" d="{d}"/>"#,
            s = g.scale,
            ns = -g.scale,
            x = g.x + pad,
            y = baseline_y - g.y,
        );
    }
    for r in &layout.rules {
        let _ = write!(
            out,
            r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}"/>"#,
            x = r.x + pad,
            y = baseline_y - r.y - r.h,
            w = r.w,
            h = r.h,
        );
    }
    out.push_str("</svg>");
    out
}

/// KVG opcode array → SVG path data (shared with the format spec:
/// 0=M 1=L 2=Q 3=C 4=Z).
pub fn opcodes_to_d(ops: &[f32]) -> String {
    let mut d = String::new();
    let mut i = 0;
    while i < ops.len() {
        let (cmd, n) = match ops[i] as u8 {
            0 => ("M", 2),
            1 => ("L", 2),
            2 => ("Q", 4),
            3 => ("C", 6),
            4 => ("Z", 0),
            _ => break,
        };
        d.push_str(cmd);
        for k in 0..n {
            let _ = write!(d, " {:.1}", ops[i + 1 + k]);
        }
        d.push(' ');
        i += 1 + n;
    }
    d.trim_end().to_string()
}
