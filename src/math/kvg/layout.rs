//! TeX-lite math typesetting over the Math AST.
//!
//! Produces positioned glyphs and rules in font units (y-up, baseline at
//! y=0), driven by the font's OpenType MATH constants. The quality bar is
//! "clean readable equation", not TeX-completeness: no glyph assemblies, no
//! cramped styles, no math-kern tables — the same tier Kindle Previewer's
//! own output occupies for the common textbook constructs.

use super::font::MathFont;
use crate::math::{ColAlign, MathExpr, TokenKind};

/// One positioned glyph: `x` advances right, `y` is the baseline offset
/// (y-up), `scale` shrinks script levels.
#[derive(Debug, Clone, Copy)]
pub struct PlacedGlyph {
    /// Glyph id in the math font.
    pub gid: u16,
    /// Pen x of the glyph origin.
    pub x: f32,
    /// Baseline offset (y-up; positive = raised).
    pub y: f32,
    /// Glyph scale (1.0 body, ~0.7 script, ~0.55 scriptscript).
    pub scale: f32,
}

/// An axis-aligned filled rectangle (fraction bar, radical vinculum).
/// `y` is the bottom edge (y-up).
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    /// Left edge.
    pub x: f32,
    /// Bottom edge (y-up).
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Thickness.
    pub h: f32,
}

/// A typeset box: content plus extents about the baseline.
#[derive(Debug, Clone, Default)]
pub struct LayoutBox {
    /// Total advance width.
    pub width: f32,
    /// Extent above the baseline.
    pub ascent: f32,
    /// Extent below the baseline.
    pub descent: f32,
    /// Positioned glyphs.
    pub glyphs: Vec<PlacedGlyph>,
    /// Positioned rules (fraction bars, vincula).
    pub rules: Vec<Rule>,
}

impl LayoutBox {
    fn translated(mut self, dx: f32, dy: f32) -> Self {
        for g in &mut self.glyphs {
            g.x += dx;
            g.y += dy;
        }
        for r in &mut self.rules {
            r.x += dx;
            r.y += dy;
        }
        self
    }

    fn scaled(mut self, s: f32) -> Self {
        for g in &mut self.glyphs {
            g.x *= s;
            g.y *= s;
            g.scale *= s;
        }
        for r in &mut self.rules {
            r.x *= s;
            r.y *= s;
            r.w *= s;
            r.h *= s;
        }
        self.width *= s;
        self.ascent *= s;
        self.descent *= s;
        self
    }
}

/// Script depth. Spacing is suppressed and glyphs shrink below `Text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Text,
    Script,
    ScriptScript,
}

impl Level {
    fn down(self) -> Level {
        match self {
            Level::Text => Level::Script,
            _ => Level::ScriptScript,
        }
    }
}

/// Typeset an expression. `display` selects display-style limits and big
/// operators. Returns None when the expression contains an unmodeled `Raw`
/// node or a glyph the font lacks — callers fall back to the text run.
pub fn typeset(font: &MathFont, expr: &MathExpr, display: bool) -> Option<LayoutBox> {
    let mut ctx = Ctx {
        font,
        display,
        em: font.units_per_em(),
    };
    ctx.layout(expr, Level::Text)
}

struct Ctx<'a> {
    font: &'a MathFont,
    display: bool,
    em: f32,
}

/// Binary operators get 2/9 em of surrounding space, relations 5/18 em,
/// punctuation 1/6 em after — TeX's classic mediummuskip/thickmuskip/
/// thinmuskip, applied only at text level.
fn op_spacing(t: &str) -> (f32, f32) {
    const REL: f32 = 5.0 / 18.0;
    const BIN: f32 = 2.0 / 9.0;
    const PUNCT: f32 = 1.0 / 6.0;
    match t {
        "=" | "<" | ">" | "≤" | "≥" | "≠" | "≈" | "≡" | "≅" | "∼" | "∝" | "→" | "←" | "↔" | "⇒"
        | "⇐" | "⇔" | "∈" | "∉" | "⊂" | "⊆" | "⊃" | "⊇" | "≪" | "≫" | "≺" | "⪯" | "↦" => {
            (REL, REL)
        }
        "+" | "−" | "-" | "±" | "∓" | "×" | "÷" | "⋅" | "·" | "∗" | "∘" | "⊕" | "⊗" | "∪" | "∩"
        | "∧" | "∨" | "∖" => (BIN, BIN),
        "," | ";" | ":" => (0.0, PUNCT),
        _ => (0.0, 0.0),
    }
}

/// Map a single-letter identifier to its mathematical-italic codepoint
/// (the convention every math renderer applies to `<mi>` single letters).
fn math_italic(c: char) -> char {
    match c {
        'h' => '\u{210E}', // planck — the one reserved slot in the block
        'a'..='z' => char::from_u32(0x1D44E + (c as u32 - 'a' as u32)).unwrap_or(c),
        'A'..='Z' => char::from_u32(0x1D434 + (c as u32 - 'A' as u32)).unwrap_or(c),
        'α'..='ω' => char::from_u32(0x1D6FC + (c as u32 - 'α' as u32)).unwrap_or(c),
        _ => c,
    }
}

/// Operators that render with limits above/below in display style.
fn is_big_operator(t: &str) -> bool {
    matches!(t, "∑" | "∏" | "∐" | "⋃" | "⋂" | "⋀" | "⋁")
}

/// Integral-family operators: display style enlarges the glyph but keeps
/// side scripts (TeX convention), unlike the ∑ family.
fn is_integral(t: &str) -> bool {
    matches!(t, "∫" | "∬" | "∭" | "∮" | "∯" | "∰")
}

/// The accent glyph of an over-script, when it is a lone accent character.
fn accent_char(e: &MathExpr) -> Option<char> {
    if let MathExpr::Token { text, .. } = e {
        let t = text.trim();
        let mut chars = t.chars();
        let ch = chars.next()?;
        if chars.next().is_none()
            && matches!(
                ch,
                '→' | '←' | '^' | 'ˆ' | '¯' | '‾' | '˜' | '~' | '˙' | '¨' | '⌢' | '⃗' | 'ˇ'
            )
        {
            return Some(ch);
        }
    }
    None
}

impl Ctx<'_> {
    fn scale_for(&self, level: Level) -> f32 {
        let c = self.font.constants();
        match level {
            Level::Text => 1.0,
            Level::Script => c.script_scale,
            Level::ScriptScript => c.script_script_scale,
        }
    }

    fn glyph_box(&self, c: char) -> Option<LayoutBox> {
        let gid = self.font.glyph(c)?;
        let m = self.font.metrics(gid);
        Some(LayoutBox {
            width: m.advance,
            ascent: m.max_y.max(0.0),
            descent: (-m.min_y).max(0.0),
            glyphs: vec![PlacedGlyph {
                gid,
                x: 0.0,
                y: 0.0,
                scale: 1.0,
            }],
            rules: vec![],
        })
    }

    fn text_box(&self, text: &str, italic: bool) -> Option<LayoutBox> {
        let mut out = LayoutBox::default();
        for ch in text.chars() {
            if ch == ' ' || ch == '\u{a0}' {
                out.width += self.em * 0.25;
                continue;
            }
            let ch = if italic { math_italic(ch) } else { ch };
            let g = self
                .glyph_box(ch)
                .or_else(|| self.glyph_box(if italic { ch } else { math_italic(ch) }))?;
            push_box(&mut out, g, 0.0);
        }
        Some(out)
    }

    fn layout(&mut self, expr: &MathExpr, level: Level) -> Option<LayoutBox> {
        let c = *self.font.constants();
        let em = self.em;
        Some(match expr {
            MathExpr::Row(items) => {
                let mut out = LayoutBox::default();
                // Tracks whether the previous item can end an operand — a
                // sign after an operator, relation, or opening fence (or at
                // the start) is unary and gets no binary spacing.
                let mut prev_is_operand = false;
                for it in items {
                    let (before, after) = match it {
                        MathExpr::Token {
                            kind: TokenKind::Op,
                            text,
                        } if level == Level::Text => {
                            let t = text.trim();
                            if matches!(t, "+" | "\u{2212}" | "-" | "±" | "∓") && !prev_is_operand
                            {
                                (0.0, 0.0)
                            } else {
                                op_spacing(t)
                            }
                        }
                        _ => (0.0, 0.0),
                    };
                    prev_is_operand = match it {
                        MathExpr::Token {
                            kind: TokenKind::Op,
                            text,
                        } => matches!(text.trim(), ")" | "]" | "}" | "|" | "⟩" | "!"),
                        _ => true,
                    };
                    let b = self.layout(it, level)?;
                    if b.width == 0.0 && b.glyphs.is_empty() && b.rules.is_empty() {
                        continue; // invisible operator
                    }
                    out.width += before * em;
                    push_box(&mut out, b, 0.0);
                    out.width += after * em;
                }
                out
            }

            MathExpr::Token { kind, text } => {
                let t = text.trim();
                match kind {
                    TokenKind::Ident => {
                        let single = t.chars().count() == 1;
                        self.text_box(t, single)?
                    }
                    TokenKind::Num | TokenKind::Text => self.text_box(text, false)?,
                    TokenKind::Op => {
                        // Invisible operators occupy no space.
                        if matches!(t, "\u{2061}" | "\u{2062}" | "\u{2063}" | "\u{2064}") {
                            LayoutBox::default()
                        } else if t == "-" {
                            // A hyphen-minus in math is a minus sign.
                            self.text_box("\u{2212}", false)?
                        } else {
                            self.text_box(t, false)?
                        }
                    }
                }
            }

            MathExpr::Sub(base, sub) => {
                let b = self.script_base(base, level)?;
                let s = self
                    .layout(sub, level.down())?
                    .scaled(self.rel_script_scale(level));
                self.attach_scripts(b, Some(s), None)
            }
            MathExpr::Sup(base, sup) => {
                let b = self.script_base(base, level)?;
                let s = self
                    .layout(sup, level.down())?
                    .scaled(self.rel_script_scale(level));
                self.attach_scripts(b, None, Some(s))
            }
            MathExpr::SubSup(base, sub, sup) => {
                let b = self.script_base(base, level)?;
                let lo = self
                    .layout(sub, level.down())?
                    .scaled(self.rel_script_scale(level));
                let hi = self
                    .layout(sup, level.down())?
                    .scaled(self.rel_script_scale(level));
                self.attach_scripts(b, Some(lo), Some(hi))
            }

            MathExpr::Under { base, under } => self.limits(base, Some(under), None, level)?,
            MathExpr::Over { base, over } => self.limits(base, None, Some(over), level)?,
            MathExpr::UnderOver { base, under, over } => {
                self.limits(base, Some(under), Some(over), level)?
            }

            MathExpr::Frac(num, den) => {
                let n = self
                    .layout(num, level.down())?
                    .scaled(if level == Level::Text {
                        1.0
                    } else {
                        self.rel_script_scale(level)
                    });
                let d = self
                    .layout(den, level.down())?
                    .scaled(if level == Level::Text {
                        1.0
                    } else {
                        self.rel_script_scale(level)
                    });
                let rule_t = c.fraction_rule_thickness;
                let axis = c.axis_height;
                let pad = 0.12 * em;
                let w = n.width.max(d.width) + 2.0 * pad;

                let num_shift = c
                    .fraction_numerator_shift_up
                    .max(axis + rule_t / 2.0 + c.fraction_num_gap_min + n.descent);
                let den_shift = c
                    .fraction_denominator_shift_down
                    .max(-axis + rule_t / 2.0 + c.fraction_denom_gap_min + d.ascent);

                let mut out = LayoutBox {
                    width: w,
                    ascent: num_shift + n.ascent,
                    descent: den_shift + d.descent,
                    glyphs: vec![],
                    rules: vec![Rule {
                        x: pad / 2.0,
                        y: axis - rule_t / 2.0,
                        w: w - pad,
                        h: rule_t,
                    }],
                };
                merge_at(
                    &mut out,
                    n.clone(),
                    pad + (w - 2.0 * pad - n.width) / 2.0,
                    num_shift,
                );
                merge_at(
                    &mut out,
                    d.clone(),
                    pad + (w - 2.0 * pad - d.width) / 2.0,
                    -den_shift,
                );
                out
            }

            MathExpr::Sqrt(inner) => self.radical(inner, None, level)?,
            MathExpr::Root(index, inner) => self.radical(inner, Some(index), level)?,

            MathExpr::Fenced { open, close, body } => {
                let b = self.layout(body, level)?;
                let mut out = LayoutBox::default();
                if let Some(d) = self.delimiter(open, &b) {
                    push_box(&mut out, d, 0.0);
                }
                push_box(&mut out, b.clone(), 0.0);
                if let Some(d) = self.delimiter(close, &b) {
                    push_box(&mut out, d, 0.0);
                }
                out
            }

            MathExpr::Table { rows, aligns } => {
                let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
                if ncols == 0 {
                    return Some(LayoutBox::default());
                }
                let mut cells: Vec<Vec<LayoutBox>> = Vec::new();
                for row in rows {
                    let mut out_row = Vec::new();
                    for cell in row {
                        out_row.push(self.layout(cell, level)?);
                    }
                    cells.push(out_row);
                }
                let col_gap = 0.6 * em;
                let row_gap = 0.35 * em;
                let mut col_w = vec![0f32; ncols];
                for row in &cells {
                    for (i, cell) in row.iter().enumerate() {
                        col_w[i] = col_w[i].max(cell.width);
                    }
                }
                let mut out = LayoutBox::default();
                let total_w: f32 =
                    col_w.iter().sum::<f32>() + col_gap * (ncols.saturating_sub(1)) as f32;
                let mut y = 0.0; // running baseline, downward
                let mut first_ascent = 0.0f32;
                for (ri, row) in cells.iter().enumerate() {
                    let r_ascent = row.iter().map(|b| b.ascent).fold(0f32, f32::max);
                    let r_descent = row.iter().map(|b| b.descent).fold(0f32, f32::max);
                    if ri == 0 {
                        first_ascent = r_ascent;
                        y = 0.0;
                    } else {
                        y -= r_ascent + row_gap;
                    }
                    let mut x = 0.0;
                    for (ci, cell) in row.iter().enumerate() {
                        let slack = col_w[ci] - cell.width;
                        let cx = x + match aligns.get(ci) {
                            Some(ColAlign::Left) => 0.0,
                            Some(ColAlign::Right) => slack,
                            _ => slack / 2.0,
                        };
                        merge_at(&mut out, cell.clone(), cx, y);
                        x += col_w[ci] + col_gap;
                    }
                    y -= r_descent;
                }
                // Center the grid on the math axis.
                let top = first_ascent;
                let bottom = -y;
                let height = top + bottom;
                let shift = c.axis_height + height / 2.0 - top;
                out.width = total_w;
                let mut out = out.translated(0.0, shift);
                out.width = total_w;
                out.ascent = top + shift;
                out.descent = bottom - shift;
                out
            }

            MathExpr::Space => LayoutBox {
                width: 0.2 * em,
                ..Default::default()
            },

            // Unmodeled content: the whole equation falls back to text.
            MathExpr::Raw { .. } => return None,
        })
    }

    /// The base of a script construct. Display-style integrals swap in the
    /// display-size glyph variant (side scripts stay, per TeX convention).
    fn script_base(&mut self, base: &MathExpr, level: Level) -> Option<LayoutBox> {
        if self.display
            && level == Level::Text
            && let MathExpr::Token {
                kind: TokenKind::Op,
                text,
            } = base
            && is_integral(text.trim())
            && let Some(ch) = text.trim().chars().next()
            && let Some(gid) = self.font.glyph(ch)
        {
            let c = self.font.constants();
            let big = self
                .font
                .vertical_variant(gid, c.display_operator_min_height);
            let m = self.font.metrics(big);
            let mid = (m.max_y + m.min_y) / 2.0;
            let dy = c.axis_height - mid;
            return Some(LayoutBox {
                width: m.advance,
                ascent: m.max_y + dy,
                descent: -(m.min_y + dy),
                glyphs: vec![PlacedGlyph {
                    gid: big,
                    x: 0.0,
                    y: dy,
                    scale: 1.0,
                }],
                rules: vec![],
            });
        }
        self.layout(base, level)
    }

    /// Script scale relative to the current level (a script inside a script
    /// shrinks by the ratio of the two absolute scales).
    fn rel_script_scale(&self, level: Level) -> f32 {
        self.scale_for(level.down()) / self.scale_for(level)
    }

    fn attach_scripts(
        &self,
        base: LayoutBox,
        sub: Option<LayoutBox>,
        sup: Option<LayoutBox>,
    ) -> LayoutBox {
        let c = self.font.constants();
        let mut out = base;
        let base_w = out.width;
        let mut script_w = 0f32;
        if let Some(hi) = sup {
            let shift = c.superscript_shift_up.max(hi.descent + 0.25 * self.em);
            out.ascent = out.ascent.max(shift + hi.ascent);
            script_w = script_w.max(hi.width);
            merge_at(&mut out, hi, base_w, shift);
        }
        if let Some(lo) = sub {
            let shift = c.subscript_shift_down.max(lo.ascent - 0.35 * self.em);
            out.descent = out.descent.max(shift + lo.descent);
            script_w = script_w.max(lo.width);
            merge_at(&mut out, lo, base_w, -shift);
        }
        out.width = base_w + script_w + c.space_after_script;
        out
    }

    fn limits(
        &mut self,
        base: &MathExpr,
        under: Option<&MathExpr>,
        over: Option<&MathExpr>,
        level: Level,
    ) -> Option<LayoutBox> {
        let c = *self.font.constants();
        // Accents (arrows, hats, bars, dots, tildes) hug the base instead of
        // floating at limit distance, at natural size.
        if under.is_none()
            && let Some(over_expr) = over
            && let Some(ch) = accent_char(over_expr)
        {
            let b = self.layout(base, level)?;
            let gid = self.font.glyph(ch)?;
            // Stretch the accent to span a wide base (arrows over words).
            let m0 = self.font.metrics(gid);
            let gid = if b.width > (m0.max_x - m0.min_x) * 1.25 {
                self.font.horizontal_variant(gid, b.width)
            } else {
                gid
            };
            let m = self.font.metrics(gid);
            let gap = 0.04 * self.em;
            // Ink bottom of the accent sits just above the base's ink top.
            let y = b.ascent + gap - m.min_y;
            let ink_w = m.max_x - m.min_x;
            let dx = (b.width - ink_w) / 2.0 - m.min_x;
            let mut out = LayoutBox {
                width: b.width,
                ascent: (y + m.max_y).max(b.ascent),
                descent: b.descent,
                glyphs: vec![],
                rules: vec![],
            };
            let bw = b.width;
            merge_at(&mut out, b, 0.0, 0.0);
            out.glyphs.push(PlacedGlyph {
                gid,
                x: dx.max(0.0),
                y,
                scale: 1.0,
            });
            out.width = bw;
            return Some(out);
        }
        // Inline non-big-operator limits read fine as scripts.
        let base_is_big = matches!(
            base,
            MathExpr::Token { kind: TokenKind::Op, text } if is_big_operator(text.trim())
        );
        if !self.display && base_is_big {
            let b = self.layout(base, level)?;
            let lo = match under {
                Some(u) => Some(
                    self.layout(u, level.down())?
                        .scaled(self.rel_script_scale(level)),
                ),
                None => None,
            };
            let hi = match over {
                Some(o) => Some(
                    self.layout(o, level.down())?
                        .scaled(self.rel_script_scale(level)),
                ),
                None => None,
            };
            return Some(self.attach_scripts(b, lo, hi));
        }

        let mut b = self.layout(base, level)?;
        if base_is_big && self.display {
            // Swap in the display-size operator variant.
            if let MathExpr::Token { text, .. } = base
                && let Some(ch) = text.trim().chars().next()
                && let Some(gid) = self.font.glyph(ch)
            {
                let big = self
                    .font
                    .vertical_variant(gid, c.display_operator_min_height);
                if big != gid {
                    let m = self.font.metrics(big);
                    // Center the operator on the math axis.
                    let mid = (m.max_y + m.min_y) / 2.0;
                    let dy = c.axis_height - mid;
                    b = LayoutBox {
                        width: m.advance,
                        ascent: m.max_y + dy,
                        descent: -(m.min_y + dy),
                        glyphs: vec![PlacedGlyph {
                            gid: big,
                            x: 0.0,
                            y: dy,
                            scale: 1.0,
                        }],
                        rules: vec![],
                    };
                }
            }
        }

        let lo = match under {
            Some(u) => Some(
                self.layout(u, level.down())?
                    .scaled(self.rel_script_scale(level)),
            ),
            None => None,
        };
        let hi = match over {
            Some(o) => Some(
                self.layout(o, level.down())?
                    .scaled(self.rel_script_scale(level)),
            ),
            None => None,
        };

        let w = b
            .width
            .max(lo.as_ref().map_or(0.0, |x| x.width))
            .max(hi.as_ref().map_or(0.0, |x| x.width));
        let base_dx = (w - b.width) / 2.0;
        let mut out = LayoutBox {
            width: w,
            ascent: b.ascent,
            descent: b.descent,
            glyphs: vec![],
            rules: vec![],
        };
        let b_ascent = b.ascent;
        let b_descent = b.descent;
        merge_at(&mut out, b, base_dx, 0.0);
        if let Some(hi) = hi {
            let y = (b_ascent + c.upper_limit_gap_min + hi.descent)
                .max(c.upper_limit_baseline_rise_min);
            out.ascent = out.ascent.max(y + hi.ascent);
            let dx = (w - hi.width) / 2.0;
            merge_at(&mut out, hi, dx, y);
        }
        if let Some(lo) = lo {
            let y = (b_descent + c.lower_limit_gap_min + lo.ascent)
                .max(c.lower_limit_baseline_drop_min);
            out.descent = out.descent.max(y + lo.descent);
            let dx = (w - lo.width) / 2.0;
            merge_at(&mut out, lo, dx, -y);
        }
        Some(out)
    }

    fn radical(
        &mut self,
        inner: &MathExpr,
        index: Option<&MathExpr>,
        level: Level,
    ) -> Option<LayoutBox> {
        let c = *self.font.constants();
        let body = self.layout(inner, level)?;
        let rule_t = c.radical_rule_thickness;
        let gap = c.radical_vertical_gap;
        let needed = body.ascent + body.descent + gap + rule_t;

        let base_gid = self.font.glyph('√')?;
        let gid = self.font.vertical_variant(base_gid, needed);
        let m = self.font.metrics(gid);

        // Place the radical so its top edge meets the vinculum.
        let rule_y_bottom = body.ascent + gap;
        let glyph_dy = (rule_y_bottom + rule_t) - m.max_y;
        let mut out = LayoutBox {
            width: 0.0,
            ascent: rule_y_bottom + rule_t + c.radical_extra_ascender,
            descent: body.descent.max(-(m.min_y + glyph_dy)),
            glyphs: vec![],
            rules: vec![],
        };

        // Root index (∛-style) tucked above the radical's leading hook.
        let mut x = 0.0;
        if let Some(idx) = index {
            let i = self
                .layout(idx, Level::ScriptScript)?
                .scaled(c.script_script_scale / self.scale_for(level));
            let iy = rule_y_bottom * 0.6;
            out.ascent = out.ascent.max(iy + i.ascent);
            let iw = i.width;
            merge_at(&mut out, i, 0.0, iy);
            x = (iw - 0.4 * self.em).max(0.0);
        }

        out.glyphs.push(PlacedGlyph {
            gid,
            x,
            y: glyph_dy,
            scale: 1.0,
        });
        let body_x = x + m.advance;
        out.rules.push(Rule {
            x: body_x,
            y: rule_y_bottom,
            w: body.width,
            h: rule_t,
        });
        let body_w = body.width;
        merge_at(&mut out, body, body_x, 0.0);
        out.width = body_x + body_w;
        Some(out)
    }

    /// A stretchy fence glyph sized to the body, centered on the math axis.
    fn delimiter(&self, glyph: &str, body: &LayoutBox) -> Option<LayoutBox> {
        let ch = glyph.trim().chars().next()?;
        let gid = self.font.glyph(ch)?;
        let c = self.font.constants();
        let needed = 2.0 * (body.ascent - c.axis_height).max(body.descent + c.axis_height);
        let gid = if needed > self.em * 0.9 {
            self.font.vertical_variant(gid, needed)
        } else {
            gid
        };
        let m = self.font.metrics(gid);
        let mid = (m.max_y + m.min_y) / 2.0;
        let dy = c.axis_height - mid;
        Some(LayoutBox {
            width: m.advance,
            ascent: (m.max_y + dy).max(0.0),
            descent: (-(m.min_y + dy)).max(0.0),
            glyphs: vec![PlacedGlyph {
                gid,
                x: 0.0,
                y: dy,
                scale: 1.0,
            }],
            rules: vec![],
        })
    }
}

/// Append `b` to the right of `out` on the shared baseline.
fn push_box(out: &mut LayoutBox, b: LayoutBox, dy: f32) {
    let dx = out.width;
    out.width += b.width;
    out.ascent = out.ascent.max(b.ascent + dy);
    out.descent = out.descent.max(b.descent - dy);
    for mut g in b.glyphs {
        g.x += dx;
        g.y += dy;
        out.glyphs.push(g);
    }
    for mut r in b.rules {
        r.x += dx;
        r.y += dy;
        out.rules.push(r);
    }
}

/// Merge `b` into `out` at an absolute offset without advancing the pen.
fn merge_at(out: &mut LayoutBox, b: LayoutBox, dx: f32, dy: f32) {
    for mut g in b.glyphs {
        g.x += dx;
        g.y += dy;
        out.glyphs.push(g);
    }
    for mut r in b.rules {
        r.x += dx;
        r.y += dy;
        out.rules.push(r);
    }
    out.ascent = out.ascent.max(b.ascent + dy);
    out.descent = out.descent.max(b.descent - dy);
}
