//! SVG must survive the complete KF8 writer (including namespace cleanup),
//! not just the initial resource-rewriting pass.
mod common;

use boko::model::Format;
use common::{Doc, EpubBuilder, Nav};
use quick_xml::{NsReader, events::Event, name::ResolveResult};

#[test]
fn svg_geometry_namespaces_and_image_asset_survive_azw3() {
    let body = r##"<div xmlns:v="http://www.w3.org/2000/svg" xmlns:l="http://www.w3.org/1999/xlink">
      <p><a href="#figure">Diagram</a></p>
      <v:svg id="figure" class="illustration" viewBox="10 20 100 200" preserveAspectRatio="xMaxYMin slice" aria-label="Diagram">
        <v:g transform="translate(-10,-20)">
          <v:image id="raster" x="10" y="20" width="200" height="400" l:href="../images/pixel.png"/>
          <v:text x="10" y="30">An SVG label</v:text>
        </v:g>
      </v:svg>
    </div>"##;
    let mut source = EpubBuilder::new("SVG preservation")
        .css(".illustration { display:none; } svg image { opacity:.5; }")
        .image("images/pixel.png", common::tiny_png())
        .doc(Doc::new("text/c.xhtml", "Diagram", body))
        .nav(vec![Nav::new("Diagram", "text/c.xhtml#figure")])
        .book();
    let book = common::roundtrip(&mut source, Format::Azw3);
    let raw = book.load_raw(book.spine()[0].id).unwrap();
    let html = std::str::from_utf8(&raw).unwrap();
    for expected in [
        "viewBox=\"10 20 100 200\"",
        "preserveAspectRatio=\"xMaxYMin slice\"",
        "translate(-10,-20)",
        "class=\"illustration\"",
        "aria-label=\"Diagram\"",
        "An SVG label",
    ] {
        assert!(html.contains(expected), "missing {expected}: {html}");
    }
    assert!(!html.contains("<img "), "SVG must not become a raster img");
    let mut reader = NsReader::from_str(html);
    let mut images = 0;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) | Event::Empty(tag) => {
                let (ns, name) = reader.resolver().resolve_element(tag.name());
                assert!(!matches!(ns, ResolveResult::Unknown(_)), "unbound element");
                for attr in tag.attributes() {
                    let attr = attr.unwrap();
                    let (ns, local) = reader.resolver().resolve_attribute(attr.key);
                    assert!(
                        !matches!(ns, ResolveResult::Unknown(_)),
                        "unbound attribute"
                    );
                    if name.as_ref() == "image" && local.as_ref() == "href" {
                        let href = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .unwrap();
                        assert!(
                            !href.contains("kindle:"),
                            "import should resolve the reference"
                        );
                        let asset = href.trim_start_matches("../");
                        assert!(
                            book.list_assets().iter().any(|path| path == asset),
                            "unlisted asset {asset}"
                        );
                        assert_eq!(book.load_asset(asset).unwrap(), common::tiny_png());
                        images += 1;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(images, 1);
    assert_eq!(book.toc()[0].title, "Diagram");
}
