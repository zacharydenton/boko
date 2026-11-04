use super::*;
use std::collections::{HashMap, HashSet};

fn rewrite(html: &[u8]) -> String {
    String::from_utf8(
        rewrite_html_references_fast(
            html,
            "c.xhtml",
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::new(),
            &HashMap::new(),
            0,
        )
        .html,
    )
    .unwrap()
}

#[test]
fn css_comment_does_not_consume_next_rule() {
    let css = b"/* @font-face declarations below */\np { color: red; }\nh1 { color: blue; }";
    let out = rewrite_css_references_fast(css, "style.css", &HashMap::new());
    assert_eq!(String::from_utf8_lossy(&out), String::from_utf8_lossy(css));
}

#[test]
fn css_quoted_brace_does_not_truncate_rule() {
    let css = b"@font-face { font-family: 'Example}'; src: url(missing.ttf); } p { color: red; }";
    let out = rewrite_css_references_fast(css, "style.css", &HashMap::new());
    assert!(
        !String::from_utf8_lossy(&out).contains("missing.ttf"),
        "{}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn font_missing_fallback_removed() {
    let css = b"@font-face { font-family: Test; src: url(missing.ttf), url(real.ttf); }";
    let out =
        rewrite_css_references_fast(css, "style.css", &HashMap::from([("real.ttf".into(), 3)]));
    assert!(
        !String::from_utf8_lossy(&out).contains("missing.ttf"),
        "{}",
        String::from_utf8_lossy(&out)
    );
}

#[test]
fn svg_preserves_id_and_visibility() {
    let html = br#"<svg id="figure" style="display:none" viewBox="0 0 100 100"><image href="figure.png" width="100" height="100"/></svg>"#;
    let out = rewrite(html);
    assert!(
        out.contains("id=\"figure\"") && out.contains("display:none"),
        "{out}"
    );
}

#[test]
fn svg_does_not_discard_crop_transform() {
    let html = br#"<svg viewBox="0 0 100 100"><image href="figure.png" width="200" height="200" transform="translate(-100,-100)"/></svg>"#;
    assert_eq!(rewrite(html), String::from_utf8_lossy(html));
}

#[test]
fn svg_comment_does_not_swallow_real_svg() {
    let html = br#"<!-- <svg> used below --><svg><image href="figure.png"/></svg>"#;
    let out = writer_svg::rewrite_image_references(
        html,
        "c.xhtml",
        &HashMap::from([("figure.png".into(), 1)]),
        &HashMap::new(),
    );
    assert_eq!(
        String::from_utf8(out).unwrap(),
        r#"<!-- <svg> used below --><svg><image href="kindle:embed:0001?mime=image/png"/></svg>"#
    );
}

#[test]
fn svg_only_changes_resolved_image_values() {
    let html = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:v="http://www.w3.org/2000/svg" xmlns:link="http://www.w3.org/1999/xlink"><body><v:svg id="figure" class="diagram" style="display:none" viewBox="10 20 100 200" preserveAspectRatio="xMaxYMin slice" aria-label="A diagram"><v:defs><v:clipPath id="clip"><v:rect width="20" height="30"/></v:clipPath></v:defs><v:g transform="rotate(45)"><v:image id="raster" x="10" y="20" width="200" height="400" link:href = '../Images/a%20%26%20b.png' clip-path="url(#clip)"/><v:text x="1">Visible text &amp; symbols</v:text></v:g></v:svg></body></html>"#;
    let out = writer_svg::rewrite_image_references(
        html.as_bytes(),
        "Text/c.xhtml",
        &HashMap::from([("Images/a & b.png".into(), 7)]),
        &HashMap::new(),
    );
    let expected = html.replace(
        "../Images/a%20%26%20b.png",
        "kindle:embed:0007?mime=image/png",
    );
    assert_eq!(String::from_utf8(out.clone()).unwrap(), expected);
    assert_eq!(
        String::from_utf8(strip_xml_namespaces(&out)).unwrap(),
        expected
    );
}

#[test]
fn svg_base_urls_and_external_references() {
    let html = r##"<html xml:base="../Assets/"><svg xmlns="http://www.w3.org/2000/svg" xml:base="nested/"><image href="../a.png"/><image href="https://example.com/a.png"/><image href="#symbol"/><image href="data:image/png;base64,AAAA"/></svg></html>"##;
    let out = writer_svg::rewrite_image_references(
        html.as_bytes(),
        "Text/c.xhtml",
        &HashMap::from([("Assets/a.png".into(), 2)]),
        &HashMap::new(),
    );
    let expected = html.replace(
        "href=\"../a.png\"",
        "href=\"kindle:embed:0002?mime=image/png\"",
    );
    assert_eq!(String::from_utf8(out.clone()).unwrap(), expected);
    assert_eq!(
        String::from_utf8(strip_xml_namespaces(&out)).unwrap(),
        expected
    );
}

#[test]
fn svg_cdata_and_comments_are_not_elements() {
    let html = r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- <image href="a.png"/> --><metadata><![CDATA[<image href="a.png"/>]]></metadata><image href="a.png" /></svg>"#;
    let out = writer_svg::rewrite_image_references(
        html.as_bytes(),
        "c.xhtml",
        &HashMap::from([("a.png".into(), 2)]),
        &HashMap::new(),
    );
    assert_eq!(
        String::from_utf8(out).unwrap(),
        html.replace(
            "<image href=\"a.png\" />",
            "<image href=\"kindle:embed:0002?mime=image/png\" />"
        )
    );
}

#[test]
fn css_tokens_preserve_comments_strings_and_nested_rules() {
    let css = r#"/* url(missing.ttf) @font-face */ p::before {content:'@font-face { src:url(missing.ttf); }'} @media screen { @FONT-FACE { font-family:'A}B'; SRC: local('Installed'), URL('../Fonts/real.ttf') format('a,b}'), url(missing.ttf); } }"#;
    let out = String::from_utf8(rewrite_css_references_fast(
        css.as_bytes(),
        "Styles/c.css",
        &HashMap::from([("Fonts/real.ttf".into(), 3)]),
    ))
    .unwrap();
    assert!(out.starts_with("/* url(missing.ttf) @font-face */ p::before {content:'@font-face { src:url(missing.ttf); }'}"), "{out}");
    assert!(out.contains("font-family:'A}B'"), "{out}");
    assert!(
        out.contains("url(kindle:embed:0003) format('a,b}')"),
        "{out}"
    );
    assert!(!out.contains("Installed"), "{out}");
    assert_eq!(out.matches("missing.ttf").count(), 2, "{out}");
}

#[test]
fn css_paths_are_relative_to_the_stylesheet_not_suffix_matches() {
    let resources = HashMap::from([("A/font.ttf".into(), 1), ("B/font.ttf".into(), 2)]);
    let css = br"@font-face {src: url(f\6f nt.ttf);} p {background:URL('font.ttf')}";
    let a = String::from_utf8(rewrite_css_references_fast(css, "A/style.css", &resources)).unwrap();
    let b = String::from_utf8(rewrite_css_references_fast(css, "B/style.css", &resources)).unwrap();
    assert_eq!(a.matches("kindle:embed:0001").count(), 2, "{a}");
    assert_eq!(b.matches("kindle:embed:0002").count(), 2, "{b}");
}

#[test]
fn css_data_fonts_and_quoted_url_punctuation_survive() {
    let css = r#"@font-face {font-family: Data; src: url('data:font/ttf;base64,AAAA') format('truetype');} @font-face {font-family: File; src: url('a)b.ttf') format('opentype'), url('missing.ttf');}"#;
    let out = String::from_utf8(rewrite_css_references_fast(
        css.as_bytes(),
        "style.css",
        &HashMap::from([("a)b.ttf".into(), 9)]),
    ))
    .unwrap();
    assert!(out.contains("data:font/ttf;base64,AAAA"), "{out}");
    assert!(
        out.contains("url(kindle:embed:0009) format('opentype')"),
        "{out}"
    );
    assert!(!out.contains("missing.ttf"), "{out}");
}

#[test]
fn css_at_keywords_in_custom_properties_are_not_rules() {
    let css = b"p { --example: @font-face { src: url(missing.ttf); }; content:'@namespace'; }";
    assert_eq!(
        rewrite_css_references_fast(css, "style.css", &HashMap::new()),
        css
    );
}

#[test]
fn css_repeated_src_removes_empty_descriptors() {
    let css = b"@font-face { font-family: Test; src:url(real.ttf); src:url(missing.ttf); }";
    let out = String::from_utf8(rewrite_css_references_fast(
        css,
        "style.css",
        &HashMap::from([("real.ttf".into(), 1)]),
    ))
    .unwrap();
    assert_eq!(out.matches("src:").count(), 1, "{out}");
    assert!(!out.contains("missing.ttf"));
}

#[test]
fn namespace_cleanup_keeps_bindings_for_retained_elements() {
    let html = br#"<html xmlns:ops="http://www.idpf.org/2007/ops" xmlns:epub="http://www.idpf.org/2007/ops"><body epub:type="bodymatter"><ops:switch><ops:default><p>Fallback</p></ops:default></ops:switch></body></html>"#;
    let out = String::from_utf8(strip_xml_namespaces(html)).unwrap();
    assert!(out.contains("xmlns:ops="), "{out}");
    assert!(!out.contains("epub:type="), "{out}");
    assert!(!out.contains("xmlns:epub="), "{out}");
}
