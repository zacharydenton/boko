//! Render MathML expressions through the KVG typesetting engine to SVG for
//! visual verification.
//!
//! Usage: cargo run --release --example kvg_typeset -- <outdir>

use boko::math::kvg::{MathFont, PathBundle, emit, svg, typeset};

const SAMPLES: &[(&str, &str)] = &[
    ("sub", r#"<math><msub><mi>x</mi><mn>1</mn></msub></math>"#),
    (
        "func",
        r#"<math><mi>y</mi><mo>=</mo><mi>f</mi><mo>(</mo><msub><mi>x</mi><mn>1</mn></msub><mo>,</mo><msub><mi>x</mi><mn>2</mn></msub><mo>,</mo><msub><mi>x</mi><mn>3</mn></msub><mo>)</mo></math>"#,
    ),
    (
        "frac",
        r#"<math display="block"><mi>f</mi><mo>(</mo><mi>x</mi><mo>)</mo><mo>=</mo><mfrac><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mrow><mn>2</mn><mi>c</mi></mrow></mfrac></math>"#,
    ),
    (
        "quadratic",
        r#"<math display="block"><mi>x</mi><mo>=</mo><mfrac><mrow><mo>−</mo><mi>b</mi><mo>±</mo><msqrt><mrow><msup><mi>b</mi><mn>2</mn></msup><mo>−</mo><mn>4</mn><mi>a</mi><mi>c</mi></mrow></msqrt></mrow><mrow><mn>2</mn><mi>a</mi></mrow></mfrac></math>"#,
    ),
    (
        "sum",
        r#"<math display="block"><munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover><msup><mi>x</mi><mi>i</mi></msup></math>"#,
    ),
    (
        "matrix",
        r#"<math display="block"><mi>M</mi><mo>=</mo><mfenced open="(" close=")"><mtable><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr><mtd><mi>c</mi></mtd><mtd><mi>d</mi></mtd></mtr></mtable></mfenced></math>"#,
    ),
    (
        "emc2",
        r#"<math><mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup></math>"#,
    ),
];

fn main() {
    let outdir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/kvg-out".into());
    std::fs::create_dir_all(&outdir).expect("outdir");
    let font = MathFont::load_system().expect("system math font (STIX Two Math)");

    for (name, mml) in SAMPLES {
        let math = boko::math::mathml::parse_math_str(mml).expect("parse");
        match typeset(&font, &math.expr, math.display) {
            Some(layout) => {
                let svg_text = svg::to_svg(&font, &layout);
                let path = format!("{outdir}/{name}.svg");
                std::fs::write(&path, svg_text).expect("write");
                // Full KVG round trip: emit the literal KVG structures, then
                // render them back through the decode rules.
                let mut bundle = PathBundle::new();
                let eq = emit(&font, &layout, &mut bundle);
                std::fs::write(
                    format!("{outdir}/{name}-kvg.svg"),
                    boko::math::kvg::emit::decode_to_svg(&eq, &bundle),
                )
                .expect("write kvg round trip");
                println!(
                    "{name}: {} glyphs, {} rules, {:.0}x{:.0} units -> {path}",
                    layout.glyphs.len(),
                    layout.rules.len(),
                    layout.width,
                    layout.ascent + layout.descent,
                );
            }
            None => println!("{name}: declined (fallback to text)"),
        }
    }
}
