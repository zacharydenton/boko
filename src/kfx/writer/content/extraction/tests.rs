//! Tests for XHTML content extraction.

use std::collections::HashSet;

use kuchiki::traits::*;

use crate::css::{ParsedStyle, Stylesheet};
use crate::kfx::writer::content::{ContentItem, ListType, StyleRun};
use crate::kfx::writer::content::merging::{flatten_containers, merge_text_with_inline_runs};
use crate::kfx::writer::symbols::sym;

use super::extract_from_node;
use super::extract_content_from_xhtml;

/// Helper to collect all text content from ContentItems, splitting by newlines
fn collect_all_texts(items: &[ContentItem]) -> Vec<String> {
    let mut texts = Vec::new();
    for item in items {
        match item {
            ContentItem::Text { text, .. } => {
                for line in text.split('\n') {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        texts.push(trimmed.to_string());
                    }
                }
            }
            ContentItem::Container { children, .. } => {
                texts.extend(collect_all_texts(children));
            }
            _ => {}
        }
    }
    texts
}

#[test]
fn test_br_tag_creates_line_break() {
    // Poetry with <br> tags should produce separate text entries
    let html = r#"<html><body>
        <p>Line one<br/>Line two<br/>Line three</p>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Should have a Container with Text items containing newline markers
    // When collected into TEXT_CONTENT, should become 3 separate entries
    let texts = collect_all_texts(&flattened);
    assert_eq!(
        texts.len(),
        3,
        "BR tags should create separate text entries, got: {:?}",
        texts
    );
    assert_eq!(texts[0], "Line one");
    assert_eq!(texts[1], "Line two");
    assert_eq!(texts[2], "Line three");
}

#[test]
fn test_br_with_spans_like_poetry() {
    // This matches the actual Standard Ebooks poetry structure
    let html = r#"<html><body>
        <p>
            <span>Lead me, O Zeus, and thou O Destiny,</span>
            <br/>
            <span>The way that I am bid by you to go:</span>
            <br/>
            <span>To follow I am ready.</span>
        </p>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    let texts = collect_all_texts(&flattened);
    assert_eq!(
        texts.len(),
        3,
        "Poetry with span+br structure should create separate text entries, got: {:?}",
        texts
    );
    assert_eq!(texts[0], "Lead me, O Zeus, and thou O Destiny,");
    assert_eq!(texts[1], "The way that I am bid by you to go:");
    assert_eq!(texts[2], "To follow I am ready.");
}

#[test]
fn test_poetry_br_in_actual_epub() {
    // Test that BR tags in actual EPUB poetry are handled correctly
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

    // Find the-enchiridion.xhtml which contains the "Lead me, O Zeus" poetry
    let enchiridion = book
        .resources
        .iter()
        .find(|(k, _)| k.contains("enchiridion"))
        .map(|(k, v)| (k.clone(), v));

    if let Some((enchiridion_path, resource)) = enchiridion {
        // Collect CSS like the builder does
        fn extract_css_hrefs(data: &[u8], base_path: &str) -> Vec<String> {
            let html = String::from_utf8_lossy(data);
            let document = kuchiki::parse_html().one(html.as_ref());
            let base_dir = if let Some(pos) = base_path.rfind('/') {
                &base_path[..pos + 1]
            } else {
                ""
            };

            let mut hrefs = Vec::new();
            if let Ok(links) = document.select("link[rel='stylesheet']") {
                for link in links {
                    if let Some(href) = link.attributes.borrow().get("href") {
                        let resolved = if href.starts_with('/') {
                            href.to_string()
                        } else {
                            format!("{}{}", base_dir, href)
                        };
                        hrefs.push(resolved);
                    }
                }
            }
            hrefs
        }

        let css_hrefs = extract_css_hrefs(&resource.data, &enchiridion_path);
        let mut combined_css = String::new();
        for css_href in &css_hrefs {
            if let Some(css_resource) = book.resources.get(css_href) {
                combined_css.push_str(&String::from_utf8_lossy(&css_resource.data));
                combined_css.push('\n');
            }
        }

        // Use the same stylesheet parsing as the builder
        let stylesheet = Stylesheet::parse_with_defaults(&combined_css);
        let content =
            extract_content_from_xhtml(&resource.data, &stylesheet, &enchiridion_path);

        // Collect all text content, looking for the Zeus poetry
        fn find_zeus_text(item: &ContentItem, found: &mut Vec<String>, raw: &mut Vec<String>) {
            match item {
                ContentItem::Text { text, is_verse, .. } => {
                    if text.contains("Zeus") || text.contains("Destiny") {
                        // Store raw text to see if newlines are present
                        raw.push(format!("RAW (is_verse={}): {:?}", is_verse, text));
                        // Split by newlines and add each
                        for line in text.split('\n') {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                found.push(trimmed.to_string());
                            }
                        }
                    }
                }
                ContentItem::Container { children, .. } => {
                    for child in children {
                        find_zeus_text(child, found, raw);
                    }
                }
                _ => {}
            }
        }

        let mut zeus_texts = Vec::new();
        let mut raw_texts = Vec::new();
        for item in &content {
            find_zeus_text(item, &mut zeus_texts, &mut raw_texts);
        }

        // The poetry should be split into separate lines
        assert!(
            zeus_texts.len() >= 2,
            "Poetry should be split into multiple lines, found: {:?}",
            zeus_texts
        );

        // Verify the first line doesn't contain the second line's text
        if !zeus_texts.is_empty() {
            assert!(
                !zeus_texts[0].contains("The way that I am bid"),
                "First line should not contain second line's text. Got: {}",
                zeus_texts[0]
            );
        }
    }
}

#[test]
fn test_builder_collect_texts_preserves_newlines() {
    // Test that the builder's collect_texts function correctly splits by newlines
    let html = r#"<html><body>
        <p>
            <span>Lead me, O Zeus, and thou O Destiny,</span>
            <br/>
            <span>The way that I am bid by you to go:</span>
        </p>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // This mimics the builder's collect_texts function
    fn builder_collect_texts(item: &ContentItem, texts: &mut Vec<String>) {
        match item {
            ContentItem::Text { text, .. } => {
                for line in text.split('\n') {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        texts.push(trimmed.to_string());
                    }
                }
            }
            ContentItem::Image { .. } => {}
            ContentItem::Container { children, .. } => {
                for child in children {
                    builder_collect_texts(child, texts);
                }
            }
        }
    }

    let mut texts = Vec::new();
    for item in &flattened {
        builder_collect_texts(item, &mut texts);
    }

    assert_eq!(
        texts.len(),
        2,
        "Should produce 2 separate text entries, got: {:?}",
        texts
    );
    assert_eq!(texts[0], "Lead me, O Zeus, and thou O Destiny,");
    assert_eq!(texts[1], "The way that I am bid by you to go:");
}

#[test]
fn test_is_verse_preserved_through_flatten() {
    // Verify is_verse survives the full pipeline: extract -> flatten -> chunk -> collect
    let html = r#"<html><body>
        <blockquote epub:type="z3998:verse">
            <p>
                <span>Line one</span>
                <br/>
                <span>Line two</span>
            </p>
        </blockquote>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Simulate what the builder does: flatten items and check is_verse
    let mut found_verse_text = false;
    for item in &flattened {
        for leaf in item.flatten() {
            if let ContentItem::Text { text, is_verse, .. } = leaf {
                if text.contains('\n') {
                    assert!(
                        *is_verse,
                        "is_verse should be true for BR-separated text after flatten"
                    );
                    found_verse_text = true;
                }
            }
        }
    }
    assert!(found_verse_text, "Should have found text with newlines");
}

#[test]
fn test_lang_attribute_extraction_from_fixture() {
    // Test lang extraction from actual EPUB fixture
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

    // Collect all lang values found in content
    let mut langs_found = HashSet::new();

    fn collect_langs(item: &ContentItem, langs: &mut HashSet<String>) {
        match item {
            ContentItem::Text { style, .. } | ContentItem::Image { style, .. } => {
                if let Some(ref lang) = style.lang {
                    langs.insert(lang.clone());
                }
            }
            ContentItem::Container {
                style, children, ..
            } => {
                if let Some(ref lang) = style.lang {
                    langs.insert(lang.clone());
                }
                for child in children {
                    collect_langs(child, langs);
                }
            }
        }
    }

    // Extract content from each spine item
    for spine_item in &book.spine {
        if let Some(resource) = book.resources.get(&spine_item.href) {
            let stylesheet = Stylesheet::default();
            let content =
                extract_content_from_xhtml(&resource.data, &stylesheet, &spine_item.href);
            for item in &content {
                collect_langs(item, &mut langs_found);
            }
        }
    }

    // The Epictetus EPUB contains Greek (grc) and Latin (la) text
    assert!(
        langs_found.contains("grc") || langs_found.contains("la") || langs_found.contains("en"),
        "Should find language tags in EPUB content, found: {:?}",
        langs_found
    );
}

#[test]
fn test_br_inherits_is_verse_from_context() {
    // Verify that BR tags inherit is_verse from parent context
    // In non-verse context (plain HTML), BR creates newline but is_verse=false
    // This means text stays as single paragraph (soft line break)
    let html = r#"<html><body>
        <p>Line one<br/>Line two</p>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Find the merged text item
    for item in &flattened {
        if let ContentItem::Container { children, .. } = item {
            for child in children {
                if let ContentItem::Text { text, is_verse, .. } = child {
                    if text.contains('\n') {
                        // In non-verse context, BR creates newline but is_verse=false
                        // This is correct - only verse context (epub:type="z3998:verse") should split
                        assert!(
                            !*is_verse,
                            "BR in non-verse context should have is_verse=false, but got true. Text: {:?}",
                            text
                        );
                        return;
                    }
                }
            }
        }
    }
    panic!("Did not find merged text with newline");
}

#[test]
fn test_normalize_text_for_kfx_splits_verse() {
    // Test that normalize_text_for_kfx correctly splits verse text
    // Create a simple book with verse content
    let html = r#"<html><body>
        <blockquote epub:type="z3998:verse">
            <p>
                <span>Line one</span>
                <br/>
                <span>Line two</span>
            </p>
        </blockquote>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Find all text items and verify is_verse
    let mut found_text = None;
    for item in &flattened {
        for leaf in item.flatten() {
            if let ContentItem::Text { text, is_verse, .. } = leaf {
                if text.contains('\n') {
                    found_text = Some((text.clone(), *is_verse));
                }
            }
        }
    }

    let (text, is_verse) = found_text.expect("Should find text with newline");
    assert!(is_verse, "is_verse should be true");

    // Now test normalize_text_for_kfx directly
    fn normalize_text_for_kfx(text: &str, is_verse: bool) -> Vec<String> {
        if is_verse {
            text.split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            if text.trim().is_empty() {
                vec![]
            } else {
                vec![text.to_string()]
            }
        }
    }

    let normalized = normalize_text_for_kfx(&text, is_verse);
    assert_eq!(
        normalized.len(),
        2,
        "Should split into 2 lines, got: {:?}",
        normalized
    );
    assert_eq!(normalized[0].trim(), "Line one");
    assert_eq!(normalized[1].trim(), "Line two");
}

#[test]
fn test_ordered_list_creates_container_with_list_type() {
    // Test that <ol> creates a Container with list_type: Ordered
    let html = r#"<html><body>
        <ol>
            <li>First item</li>
            <li>Second item</li>
            <li>Third item</li>
        </ol>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Should have one Container for the <ol>
    assert_eq!(
        flattened.len(),
        1,
        "Should have one top-level item (the ol container)"
    );

    // The container should have list_type: Ordered
    match &flattened[0] {
        ContentItem::Container {
            tag,
            children,
            list_type,
            ..
        } => {
            assert_eq!(tag, "ol", "Container should be an ol element");
            assert_eq!(
                *list_type,
                Some(ListType::Ordered),
                "ol should have list_type: Ordered"
            );
            // Should have 3 children (li items)
            assert_eq!(children.len(), 3, "ol should have 3 li children");

            // Each li should be a Container with its text
            for (i, child) in children.iter().enumerate() {
                match child {
                    ContentItem::Container { tag, .. } => {
                        assert_eq!(tag, "li", "Child {} should be an li element", i);
                    }
                    _ => panic!("Child {} should be a Container (li), got {:?}", i, child),
                }
            }
        }
        _ => panic!("Expected Container, got {:?}", flattened[0]),
    }
}

#[test]
fn test_unordered_list_creates_container_with_list_type() {
    // Test that <ul> creates a Container with list_type: Unordered
    let html = r#"<html><body>
        <ul>
            <li>Bullet one</li>
            <li>Bullet two</li>
        </ul>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    assert_eq!(
        flattened.len(),
        1,
        "Should have one top-level item (the ul container)"
    );

    match &flattened[0] {
        ContentItem::Container {
            tag,
            children,
            list_type,
            ..
        } => {
            assert_eq!(tag, "ul", "Container should be a ul element");
            assert_eq!(
                *list_type,
                Some(ListType::Unordered),
                "ul should have list_type: Unordered"
            );
            assert_eq!(children.len(), 2, "ul should have 2 li children");
        }
        _ => panic!("Expected Container, got {:?}", flattened[0]),
    }
}

#[test]
fn test_display_none_elements_skipped() {
    // Elements with display:none should be skipped entirely
    let html = r#"<html><body>
        <p>Visible content</p>
        <p class="hidden">Hidden content</p>
        <p>More visible</p>
    </body></html>"#;

    let css = ".hidden { display: none; }";
    let stylesheet = Stylesheet::parse(css);
    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);
    let texts = collect_all_texts(&flattened);
    let all_text = texts.join(" ");

    assert!(
        all_text.contains("Visible content"),
        "visible content should be kept"
    );
    assert!(
        all_text.contains("More visible"),
        "visible content should be kept"
    );
    assert!(
        !all_text.contains("Hidden content"),
        "display:none content should be skipped, got: {}",
        all_text
    );
}

#[test]
fn test_mobi_fallback_skipped_via_display_none() {
    // mobi fallback content with display:none should be skipped (epub/mobi conditional)
    let html = r#"<html><body>
        <span class="epub">Keep this epub content</span>
        <span class="mobi">Skip this mobi fallback</span>
    </body></html>"#;

    let css = ".epub { display: inline; } .mobi { display: none; }";
    let stylesheet = Stylesheet::parse(css);
    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);
    let texts = collect_all_texts(&flattened);
    let all_text = texts.join(" ");

    assert!(
        all_text.contains("Keep this"),
        "epub content should be kept"
    );
    assert!(
        !all_text.contains("Skip this"),
        "mobi content (display:none) should be skipped, got: {}",
        all_text
    );
}

#[test]
fn test_mathml_preserved_as_xml_string() {
    // MathML elements should be serialized as raw XML strings
    let html = r#"<html><body>
        <p>Before equation</p>
        <math xmlns="http://www.w3.org/1998/Math/MathML"><mi>x</mi><mo>+</mo><mn>1</mn></math>
        <p>After equation</p>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Find MathML text
    let mathml_text = flattened.iter().find_map(|item| match item {
        ContentItem::Text { text, .. } if text.contains("<math") => Some(text.clone()),
        _ => None,
    });

    assert!(
        mathml_text.is_some(),
        "MathML should be preserved as XML string"
    );

    let xml = mathml_text.unwrap();
    assert!(
        xml.contains("<mi>x</mi>"),
        "Should preserve element structure"
    );
    assert!(xml.contains("<mo>+</mo>"), "Should preserve operators");
    assert!(xml.contains("<mn>1</mn>"), "Should preserve numbers");
}

#[test]
fn test_mathml_with_mobi_fallback() {
    // Full epub/mobi conditional with MathML - should use MathML, skip fallback image
    let html = r#"<html><body>
        <span class="epub"><math xmlns="http://www.w3.org/1998/Math/MathML"><mi>y</mi></math></span>
        <span class="mobi"><img src="../images/eq1.jpg" alt="equation"/></span>
    </body></html>"#;

    let css = ".epub { display: inline; } .mobi { display: none; }";
    let stylesheet = Stylesheet::parse(css);
    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Should have MathML
    let has_mathml = flattened
        .iter()
        .any(|item| matches!(item, ContentItem::Text { text, .. } if text.contains("<math")));
    assert!(has_mathml, "Should preserve MathML from epub span");

    // Should NOT have fallback image
    let has_fallback_image = flattened.iter().any(|item| {
        matches!(item, ContentItem::Image { resource_href, .. } if resource_href.contains("eq1.jpg"))
    });
    assert!(
        !has_fallback_image,
        "Should NOT include mobi fallback image (display:none)"
    );
}

#[test]
fn test_endnotes_list_from_fixture() {
    // Test that actual endnotes from EPUB are extracted as list container
    let epub_data = std::fs::read("tests/fixtures/epictetus.epub").unwrap();
    let book = crate::read_epub_from_reader(std::io::Cursor::new(epub_data)).unwrap();

    // Find endnotes.xhtml
    let endnotes = book
        .resources
        .iter()
        .find(|(k, _)| k.contains("endnotes"))
        .map(|(k, v)| (k.clone(), v));

    let (endnotes_path, resource) = endnotes.expect("Should find endnotes.xhtml");

    // Extract content
    let stylesheet = Stylesheet::default();
    let content = extract_content_from_xhtml(&resource.data, &stylesheet, &endnotes_path);

    // Find the ol container
    fn find_list_container(items: &[ContentItem]) -> Option<&ContentItem> {
        for item in items {
            match item {
                ContentItem::Container {
                    list_type: Some(_), ..
                } => return Some(item),
                ContentItem::Container { children, .. } => {
                    if let Some(found) = find_list_container(children) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let list = find_list_container(&content);
    assert!(list.is_some(), "Should find a list container in endnotes");

    if let Some(ContentItem::Container {
        tag,
        list_type,
        children,
        ..
    }) = list
    {
        assert_eq!(tag, "ol", "Endnotes list should be an ol");
        assert_eq!(
            *list_type,
            Some(ListType::Ordered),
            "Endnotes should have Ordered list type"
        );
        // Epictetus has 98+ endnotes
        assert!(
            children.len() > 90,
            "Endnotes should have many li items, got {}",
            children.len()
        );
    }
}

#[test]
fn test_backlink_creates_inline_run_with_anchor() {
    // Verify that backlinks (↩︎) in endnotes create inline runs with anchor_href
    // This is critical for links to work in KFX output
    let html = r#"<html><body>
        <p>Some note text. <a href="chapter.xhtml#noteref-1" role="doc-backlink">↩︎</a></p>
    </body></html>"#;

    let stylesheet = Stylesheet::default();
    let content = extract_content_from_xhtml(html.as_bytes(), &stylesheet, "endnotes.xhtml");

    // Find the paragraph with inline runs
    fn find_text_with_backlink(items: &[ContentItem]) -> Option<Vec<StyleRun>> {
        for item in items {
            match item {
                ContentItem::Text {
                    text, inline_runs, ..
                } if text.contains("↩︎") => {
                    return Some(inline_runs.clone());
                }
                ContentItem::Container { children, .. } => {
                    if let Some(runs) = find_text_with_backlink(children) {
                        return Some(runs);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let inline_runs = find_text_with_backlink(&content);
    assert!(inline_runs.is_some(), "Should find text with backlink");

    let runs = inline_runs.unwrap();
    assert!(!runs.is_empty(), "Should have inline runs for backlink");

    // Find the inline run with anchor_href
    let backlink_run = runs.iter().find(|r| r.anchor_href.is_some());
    assert!(
        backlink_run.is_some(),
        "Should have inline run with anchor_href"
    );

    let run = backlink_run.unwrap();
    assert!(
        run.anchor_href.as_ref().unwrap().contains("noteref-1"),
        "Anchor href should reference noteref-1, got {:?}",
        run.anchor_href
    );
}

#[test]
fn test_span_with_css_class_preserves_line_height() {
    // This tests the text-xs Tailwind pattern: font-size and line-height on an inline span
    // The span's style (including line-height) should be preserved in the extracted content
    let css = r#"
        .text-xs { font-size: 0.75rem; line-height: 1rem; }
    "#;
    let html = r#"<html><body>
        <p><span class="text-xs">Test content</span></p>
    </body></html>"#;

    let stylesheet = Stylesheet::parse(css);
    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Find the text item and check its style has line-height
    fn find_text_style(items: &[ContentItem]) -> Option<ParsedStyle> {
        for item in items {
            match item {
                ContentItem::Text { text, style, .. } if text.contains("Test content") => {
                    return Some(style.clone());
                }
                ContentItem::Container { children, .. } => {
                    if let Some(s) = find_text_style(children) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let style = find_text_style(&flattened).expect("Should find text item");

    // Check font-size is preserved
    assert!(
        matches!(style.font_size, Some(crate::css::CssValue::Rem(v)) if (v - 0.75).abs() < 0.01),
        "Style should have font-size: 0.75rem, got {:?}",
        style.font_size
    );

    // Check line-height is preserved - THIS IS THE KEY ASSERTION
    assert!(
        matches!(style.line_height, Some(crate::css::CssValue::Rem(v)) if (v - 1.0).abs() < 0.01),
        "Style should have line-height: 1rem, got {:?}",
        style.line_height
    );
}

#[test]
fn test_footnote_classification_from_epub_type() {
    // Test that epub:type="footnote" sets classification
    let html = r#"
        <html xmlns:epub="http://www.idpf.org/2007/ops">
        <body>
            <aside epub:type="footnote" id="fn1">
                <p>This is a footnote</p>
            </aside>
        </body>
        </html>
    "#;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);

    // Find the aside container
    fn find_classification(items: &[ContentItem]) -> Option<u64> {
        for item in items {
            if let ContentItem::Container {
                classification,
                children,
                ..
            } = item
            {
                if classification.is_some() {
                    return *classification;
                }
                if let Some(c) = find_classification(children) {
                    return Some(c);
                }
            }
        }
        None
    }

    let classification = find_classification(&flattened);
    assert_eq!(
        classification,
        Some(sym::FOOTNOTE),
        "Container with epub:type='footnote' should have FOOTNOTE classification ($618)"
    );
}

#[test]
fn test_endnote_classification_from_epub_type() {
    // Test that epub:type="endnote" sets classification
    let html = r#"
        <html xmlns:epub="http://www.idpf.org/2007/ops">
        <body>
            <aside epub:type="endnote" id="en1">
                <p>This is an endnote</p>
            </aside>
        </body>
        </html>
    "#;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);

    fn find_classification(items: &[ContentItem]) -> Option<u64> {
        for item in items {
            if let ContentItem::Container {
                classification,
                children,
                ..
            } = item
            {
                if classification.is_some() {
                    return *classification;
                }
                if let Some(c) = find_classification(children) {
                    return Some(c);
                }
            }
        }
        None
    }

    let classification = find_classification(&flattened);
    assert_eq!(
        classification,
        Some(sym::ENDNOTE),
        "Container with epub:type='endnote' should have ENDNOTE classification ($619)"
    );
}

#[test]
fn test_footnote_classification_from_aria_role() {
    // Test that role="doc-footnote" sets classification
    let html = r#"
        <html>
        <body>
            <aside role="doc-footnote" id="fn1">
                <p>This is a footnote via ARIA</p>
            </aside>
        </body>
        </html>
    "#;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);

    fn find_classification(items: &[ContentItem]) -> Option<u64> {
        for item in items {
            if let ContentItem::Container {
                classification,
                children,
                ..
            } = item
            {
                if classification.is_some() {
                    return *classification;
                }
                if let Some(c) = find_classification(children) {
                    return Some(c);
                }
            }
        }
        None
    }

    let classification = find_classification(&flattened);
    assert_eq!(
        classification,
        Some(sym::FOOTNOTE),
        "Container with role='doc-footnote' should have FOOTNOTE classification ($618)"
    );
}

#[test]
fn test_noteref_detection_from_epub_type() {
    // Test that epub:type="noteref" sets is_noteref on text
    let html = r##"
        <html xmlns:epub="http://www.idpf.org/2007/ops">
        <body>
            <p>See note<a epub:type="noteref" href="#fn1">1</a> for details.</p>
        </body>
        </html>
    "##;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);
    let merged = merge_text_with_inline_runs(flattened);

    // Find text with inline runs containing noteref
    fn find_noteref_run(items: &[ContentItem]) -> Option<bool> {
        for item in items {
            match item {
                ContentItem::Text { inline_runs, .. } => {
                    for run in inline_runs {
                        if run.is_noteref && run.anchor_href.is_some() {
                            return Some(true);
                        }
                    }
                }
                ContentItem::Container { children, .. } => {
                    if let Some(result) = find_noteref_run(children) {
                        return Some(result);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let has_noteref = find_noteref_run(&merged);
    assert_eq!(
        has_noteref,
        Some(true),
        "Link with epub:type='noteref' should have is_noteref=true in inline run"
    );
}

#[test]
fn test_noteref_detection_from_aria_role() {
    // Test that role="doc-noteref" sets is_noteref on text
    let html = r##"
        <html>
        <body>
            <p>See note<a role="doc-noteref" href="#fn1">1</a> for details.</p>
        </body>
        </html>
    "##;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);
    let merged = merge_text_with_inline_runs(flattened);

    fn find_noteref_run(items: &[ContentItem]) -> Option<bool> {
        for item in items {
            match item {
                ContentItem::Text { inline_runs, .. } => {
                    for run in inline_runs {
                        if run.is_noteref && run.anchor_href.is_some() {
                            return Some(true);
                        }
                    }
                }
                ContentItem::Container { children, .. } => {
                    if let Some(result) = find_noteref_run(children) {
                        return Some(result);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let has_noteref = find_noteref_run(&merged);
    assert_eq!(
        has_noteref,
        Some(true),
        "Link with role='doc-noteref' should have is_noteref=true in inline run"
    );
}

#[test]
fn test_regular_link_not_noteref() {
    // Test that regular links don't have is_noteref
    let html = r#"
        <html>
        <body>
            <p>Visit <a href="https://example.com">this site</a> for more.</p>
        </body>
        </html>
    "#;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);
    let merged = merge_text_with_inline_runs(flattened);

    // Find inline run with anchor_href and check is_noteref
    fn check_link_not_noteref(items: &[ContentItem]) -> Option<bool> {
        for item in items {
            match item {
                ContentItem::Text { inline_runs, .. } => {
                    for run in inline_runs {
                        if run.anchor_href.is_some() {
                            // Found a link - should NOT be noteref
                            return Some(!run.is_noteref);
                        }
                    }
                }
                ContentItem::Container { children, .. } => {
                    if let Some(result) = check_link_not_noteref(children) {
                        return Some(result);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let link_not_noteref = check_link_not_noteref(&merged);
    assert_eq!(
        link_not_noteref,
        Some(true),
        "Regular link without epub:type/role should have is_noteref=false"
    );
}

#[test]
fn test_figure_element_sets_is_figure() {
    // Test that <figure> elements set is_figure on style
    let html = r#"
        <html>
        <body>
            <figure>
                <img src="image.jpg" alt="Test image"/>
            </figure>
        </body>
        </html>
    "#;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);

    fn find_figure_style(items: &[ContentItem]) -> Option<bool> {
        for item in items {
            if let ContentItem::Container {
                style, tag, children, ..
            } = item
            {
                if tag == "figure" {
                    return Some(style.is_figure);
                }
                if let Some(result) = find_figure_style(children) {
                    return Some(result);
                }
            }
        }
        None
    }

    let is_figure = find_figure_style(&flattened);
    assert_eq!(
        is_figure,
        Some(true),
        "<figure> element should have is_figure=true on its style"
    );
}

#[test]
fn test_figcaption_element_sets_is_caption() {
    // Test that <figcaption> elements set is_caption on style
    let html = r#"
        <html>
        <body>
            <figure>
                <img src="image.jpg" alt="Test image"/>
                <figcaption>This is a caption</figcaption>
            </figure>
        </body>
        </html>
    "#;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);

    fn find_figcaption_style(items: &[ContentItem]) -> Option<bool> {
        for item in items {
            if let ContentItem::Container {
                style, tag, children, ..
            } = item
            {
                if tag == "figcaption" {
                    return Some(style.is_caption);
                }
                if let Some(result) = find_figcaption_style(children) {
                    return Some(result);
                }
            }
        }
        None
    }

    let is_caption = find_figcaption_style(&flattened);
    assert_eq!(
        is_caption,
        Some(true),
        "<figcaption> element should have is_caption=true on its style"
    );
}

#[test]
fn test_heading_element_sets_is_heading() {
    // Test that h1-h6 elements set is_heading on style
    let html = r#"
        <html>
        <body>
            <h2>Chapter Title</h2>
        </body>
        </html>
    "#;

    let stylesheet = Stylesheet::parse("");
    let document = kuchiki::parse_html().one(html);
    let body = document.select("body").unwrap().next().unwrap();
    let items = extract_from_node(
        body.as_node(),
        &stylesheet,
        &ParsedStyle::default(),
        "",
        None,
        false,
        false,
    );
    let flattened = flatten_containers(items);

    fn find_heading_style(items: &[ContentItem]) -> Option<bool> {
        for item in items {
            if let ContentItem::Container {
                style, tag, children, ..
            } = item
            {
                if tag.starts_with('h') && tag.len() == 2 {
                    return Some(style.is_heading);
                }
                if let Some(result) = find_heading_style(children) {
                    return Some(result);
                }
            }
        }
        None
    }

    let is_heading = find_heading_style(&flattened);
    assert_eq!(
        is_heading,
        Some(true),
        "<h2> element should have is_heading=true on its style"
    );
}

#[test]
fn test_horizontal_rule_creates_container() {
    // <hr> elements should create an empty Container with tag "hr"
    let html = r#"<html><body>
        <p>Before</p>
        <hr />
        <p>After</p>
    </body></html>"#;

    let document = kuchiki::parse_html().one(html);
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap();

    let stylesheet = Stylesheet::default();
    let items = extract_from_node(&body, &stylesheet, &ParsedStyle::default(), "", None, false, false);
    let flattened = flatten_containers(items);

    // Should have 3 items: p, hr, p
    assert_eq!(flattened.len(), 3, "Should have 3 items (p, hr, p)");

    // Find the hr container
    let hr_item = flattened.iter().find(|item| {
        matches!(item, ContentItem::Container { tag, .. } if tag == "hr")
    });

    assert!(hr_item.is_some(), "Should find an <hr> container");

    // Verify hr container has no children
    if let Some(ContentItem::Container { children, tag, .. }) = hr_item {
        assert_eq!(tag, "hr");
        assert!(children.is_empty(), "HR container should have no children");
    }
}
