//! Math font access for KVG typesetting.
//!
//! Wraps `ttf_parser` for the pieces typesetting needs: char→glyph lookup,
//! advances, outline extraction in KVG opcode form, and the OpenType MATH
//! table (typesetting constants + stretchy vertical variants).
//!
//! Outlines come out in font units, y-up, exactly as KVG path arrays expect
//! (the y-flip happens in the per-shape transform at emission).

use rustc_hash::FxHashMap;
use ttf_parser::{Face, GlyphId};

/// Well-known locations for an OpenType MATH font. STIX Two Math ships with
/// macOS; Linux distributions carry it via the `fonts-stix`/`stix-fonts`
/// packages or TeX Live.
const FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/STIXTwoMath.otf",
    "/usr/share/fonts/opentype/stix-word/STIXTwoMath-Regular.otf",
    "/usr/share/fonts/OTF/STIXTwoMath-Regular.otf",
    "/usr/local/share/fonts/STIXTwoMath-Regular.otf",
];

/// Locate a usable MATH-table font on this system.
pub fn find_system_math_font() -> Option<std::path::PathBuf> {
    FONT_CANDIDATES
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
}

/// A glyph outline as a KVG-opcode instruction array
/// (0=M x y, 1=L x y, 2=Q cx cy x y, 3=C c1x c1y c2x c2y x y, 4=Z),
/// in font units, y-up.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outline(pub Vec<f32>);

struct OpcodeBuilder(Vec<f32>);

impl ttf_parser::OutlineBuilder for OpcodeBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.extend_from_slice(&[0.0, x, y]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.extend_from_slice(&[1.0, x, y]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.extend_from_slice(&[2.0, x1, y1, x, y]);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.extend_from_slice(&[3.0, x1, y1, x2, y2, x, y]);
    }
    fn close(&mut self) {
        self.0.push(4.0);
    }
}

/// Metrics of one glyph, in font units.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlyphMetrics {
    /// Horizontal advance width.
    pub advance: f32,
    /// Tight bounding box, left edge (y-up space).
    pub min_x: f32,
    /// Tight bounding box, bottom edge.
    pub min_y: f32,
    /// Tight bounding box, right edge.
    pub max_x: f32,
    /// Tight bounding box, top edge.
    pub max_y: f32,
    /// MATH-table italic correction (extra advance before a superscript).
    pub italic_correction: f32,
}

/// TeX-style typesetting constants pulled from the MATH table, in font units.
#[derive(Debug, Clone, Copy)]
pub struct MathConstants {
    /// First-level script (sub/superscript) scale factor.
    pub script_scale: f32,
    /// Second-level script scale factor.
    pub script_script_scale: f32,
    /// Math axis height above the baseline (fraction bars center here).
    pub axis_height: f32,
    /// Default subscript baseline drop.
    pub subscript_shift_down: f32,
    /// Default superscript baseline raise.
    pub superscript_shift_up: f32,
    /// Minimum vertical gap between stacked sub/superscript.
    pub sub_superscript_gap_min: f32,
    /// Advance added after a scripted element.
    pub space_after_script: f32,
    /// Fraction bar thickness.
    pub fraction_rule_thickness: f32,
    /// Numerator baseline raise.
    pub fraction_numerator_shift_up: f32,
    /// Denominator baseline drop.
    pub fraction_denominator_shift_down: f32,
    /// Minimum gap between numerator and the bar.
    pub fraction_num_gap_min: f32,
    /// Minimum gap between denominator and the bar.
    pub fraction_denom_gap_min: f32,
    /// Radical vinculum (overbar) thickness.
    pub radical_rule_thickness: f32,
    /// Gap between radicand and vinculum.
    pub radical_vertical_gap: f32,
    /// Extra space above the vinculum.
    pub radical_extra_ascender: f32,
    /// Minimum gap between operator and upper limit.
    pub upper_limit_gap_min: f32,
    /// Minimum upper-limit baseline raise.
    pub upper_limit_baseline_rise_min: f32,
    /// Minimum gap between operator and lower limit.
    pub lower_limit_gap_min: f32,
    /// Minimum lower-limit baseline drop.
    pub lower_limit_baseline_drop_min: f32,
    /// Minimum height of big operators in display style.
    pub display_operator_min_height: f32,
}

/// A loaded math font: owned bytes + parsed face + caches.
pub struct MathFont {
    data: Vec<u8>,
    units_per_em: f32,
    constants: MathConstants,
    outline_cache: std::cell::RefCell<FxHashMap<u16, Outline>>,
}

impl MathFont {
    /// Load from raw font bytes. Fails if the font has no MATH table.
    pub fn from_bytes(data: Vec<u8>) -> Option<Self> {
        let face = Face::parse(&data, 0).ok()?;
        let upem = face.units_per_em() as f32;
        let math = face.tables().math?;
        let c = math.constants?;
        let v = |mv: ttf_parser::math::MathValue| mv.value as f32;
        let constants = MathConstants {
            script_scale: c.script_percent_scale_down() as f32 / 100.0,
            script_script_scale: c.script_script_percent_scale_down() as f32 / 100.0,
            axis_height: v(c.axis_height()),
            subscript_shift_down: v(c.subscript_shift_down()),
            superscript_shift_up: v(c.superscript_shift_up()),
            sub_superscript_gap_min: v(c.sub_superscript_gap_min()),
            space_after_script: v(c.space_after_script()),
            fraction_rule_thickness: v(c.fraction_rule_thickness()),
            fraction_numerator_shift_up: v(c.fraction_numerator_shift_up()),
            fraction_denominator_shift_down: v(c.fraction_denominator_shift_down()),
            fraction_num_gap_min: v(c.fraction_numerator_gap_min()),
            fraction_denom_gap_min: v(c.fraction_denominator_gap_min()),
            radical_rule_thickness: v(c.radical_rule_thickness()),
            radical_vertical_gap: v(c.radical_vertical_gap()),
            radical_extra_ascender: v(c.radical_extra_ascender()),
            upper_limit_gap_min: v(c.upper_limit_gap_min()),
            upper_limit_baseline_rise_min: v(c.upper_limit_baseline_rise_min()),
            lower_limit_gap_min: v(c.lower_limit_gap_min()),
            lower_limit_baseline_drop_min: v(c.lower_limit_baseline_drop_min()),
            display_operator_min_height: c.display_operator_min_height() as f32,
        };
        Some(Self {
            data,
            units_per_em: upem,
            constants,
            outline_cache: std::cell::RefCell::new(FxHashMap::default()),
        })
    }

    /// Load the first available system math font.
    pub fn load_system() -> Option<Self> {
        let path = find_system_math_font()?;
        Self::from_bytes(std::fs::read(path).ok()?)
    }

    fn face(&self) -> Face<'_> {
        // Parse is header validation only — cheap enough per call, and it
        // sidesteps a self-referential owned-bytes + borrowed-face struct.
        Face::parse(&self.data, 0).expect("validated at construction")
    }

    /// Font design units per em.
    pub fn units_per_em(&self) -> f32 {
        self.units_per_em
    }

    /// MATH-table typesetting constants (font units).
    pub fn constants(&self) -> &MathConstants {
        &self.constants
    }

    /// Glyph id for a character, if the font covers it.
    pub fn glyph(&self, c: char) -> Option<u16> {
        self.face().glyph_index(c).map(|g| g.0)
    }

    /// Metrics for a glyph, in font units.
    pub fn metrics(&self, gid: u16) -> GlyphMetrics {
        let face = self.face();
        let g = GlyphId(gid);
        let advance = face.glyph_hor_advance(g).unwrap_or(0) as f32;
        let bbox = face.glyph_bounding_box(g);
        let italic_correction = face
            .tables()
            .math
            .and_then(|m| m.glyph_info)
            .and_then(|gi| gi.italic_corrections)
            .and_then(|ic| ic.get(g))
            .map(|mv| mv.value as f32)
            .unwrap_or(0.0);
        GlyphMetrics {
            advance,
            min_x: bbox.map(|b| b.x_min as f32).unwrap_or(0.0),
            min_y: bbox.map(|b| b.y_min as f32).unwrap_or(0.0),
            max_x: bbox.map(|b| b.x_max as f32).unwrap_or(0.0),
            max_y: bbox.map(|b| b.y_max as f32).unwrap_or(0.0),
            italic_correction,
        }
    }

    /// The glyph's outline as KVG opcodes (font units, y-up). Cached.
    pub fn outline(&self, gid: u16) -> Outline {
        if let Some(o) = self.outline_cache.borrow().get(&gid) {
            return o.clone();
        }
        let mut b = OpcodeBuilder(Vec::new());
        self.face().outline_glyph(GlyphId(gid), &mut b);
        let outline = Outline(b.0);
        self.outline_cache.borrow_mut().insert(gid, outline.clone());
        outline
    }

    /// Choose a vertical variant of `gid` at least `min_height` font units
    /// tall (e.g. stretchy parens, radical signs). Returns the base glyph
    /// when no taller variant exists. Glyph assembly (part composition) is
    /// not built; the tallest variant is the practical ceiling.
    pub fn vertical_variant(&self, gid: u16, min_height: f32) -> u16 {
        let face = self.face();
        let Some(variants) = face.tables().math.and_then(|m| m.variants) else {
            return gid;
        };
        let Some(construction) = variants.vertical_constructions.get(GlyphId(gid)) else {
            return gid;
        };
        let mut best = gid;
        for var in construction.variants {
            best = var.variant_glyph.0;
            if var.advance_measurement as f32 >= min_height {
                break; // records are ordered smallest→largest
            }
        }
        best
    }

    /// Choose a horizontal variant of `gid` at least `min_width` font units
    /// wide (stretchy accents: arrows, bars, braces over a base). Returns the
    /// base glyph when no wider variant exists.
    pub fn horizontal_variant(&self, gid: u16, min_width: f32) -> u16 {
        let face = self.face();
        let Some(variants) = face.tables().math.and_then(|m| m.variants) else {
            return gid;
        };
        let Some(construction) = variants.horizontal_constructions.get(GlyphId(gid)) else {
            return gid;
        };
        let mut best = gid;
        for var in construction.variants {
            best = var.variant_glyph.0;
            if var.advance_measurement as f32 >= min_width {
                break;
            }
        }
        best
    }
}
