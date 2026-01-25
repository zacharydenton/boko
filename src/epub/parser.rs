//! EPUB parsing utilities (OPF, NCX, container.xml)

use std::collections::HashMap;
use std::io;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::book::{Metadata, TocEntry};

/// Parsed OPF package data.
pub struct OpfData {
    pub metadata: Metadata,
    /// Maps manifest id -> (href, media_type)
    pub manifest: HashMap<String, (String, String)>,
    pub spine_ids: Vec<String>,
    pub ncx_href: Option<String>,
}

/// Parse META-INF/container.xml to find the OPF path.
pub fn parse_container_xml(bytes: &[u8]) -> io::Result<String> {
    let content = String::from_utf8(strip_bom(bytes).to_vec())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"rootfile" => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"full-path" {
                        return String::from_utf8(attr.value.to_vec())
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(e)),
            _ => {}
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "No rootfile found in container.xml",
    ))
}

/// Parse OPF package document.
pub fn parse_opf(content: &str) -> io::Result<OpfData> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut metadata = Metadata::default();
    let mut manifest: HashMap<String, ManifestItem> = HashMap::new();
    let mut spine_ids: Vec<String> = Vec::new();
    let mut toc_id: Option<String> = None;
    let mut epub2_cover_id: Option<String> = None;

    let mut in_metadata = false;
    let mut current_element: Option<String> = None;
    let mut buf_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                match local {
                    b"metadata" => in_metadata = true,
                    b"title" | b"creator" | b"language" | b"identifier" | b"publisher"
                    | b"description" | b"subject" | b"date" | b"rights" => {
                        if in_metadata {
                            current_element = Some(String::from_utf8_lossy(local).to_string());
                            buf_text.clear();
                        }
                    }
                    b"spine" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"toc" {
                                toc_id = Some(
                                    String::from_utf8(attr.value.to_vec())
                                        .map_err(io::Error::other)?,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                match local {
                    b"item" => {
                        let mut id = String::new();
                        let mut href = String::new();
                        let mut media_type = String::new();
                        let mut properties: Option<String> = None;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => {
                                    id = String::from_utf8(attr.value.to_vec())
                                        .map_err(io::Error::other)?
                                }
                                b"href" => {
                                    href = String::from_utf8(attr.value.to_vec())
                                        .map_err(io::Error::other)?
                                }
                                b"media-type" => {
                                    media_type = String::from_utf8(attr.value.to_vec())
                                        .map_err(io::Error::other)?
                                }
                                b"properties" => {
                                    properties = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    )
                                }
                                _ => {}
                            }
                        }

                        if !id.is_empty() {
                            manifest.insert(
                                id,
                                ManifestItem {
                                    href,
                                    media_type,
                                    properties,
                                },
                            );
                        }
                    }
                    b"itemref" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"idref" {
                                spine_ids.push(
                                    String::from_utf8(attr.value.to_vec())
                                        .map_err(io::Error::other)?,
                                );
                            }
                        }
                    }
                    b"meta" => {
                        let mut is_cover = false;
                        let mut cover_id = String::new();

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" if attr.value.as_ref() == b"cover" => is_cover = true,
                                b"content" => {
                                    cover_id = String::from_utf8(attr.value.to_vec())
                                        .map_err(io::Error::other)?
                                }
                                _ => {}
                            }
                        }

                        if is_cover && !cover_id.is_empty() {
                            epub2_cover_id = Some(cover_id);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if current_element.is_some() {
                    buf_text.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if current_element.is_some() {
                    let entity = String::from_utf8_lossy(e.as_ref());
                    if let Some(resolved) = resolve_entity(&entity) {
                        buf_text.push_str(&resolved);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                if local == b"metadata" {
                    in_metadata = false;
                }

                if let Some(ref elem) = current_element {
                    match elem.as_str() {
                        "title" => metadata.title = buf_text.clone(),
                        "creator" => metadata.authors.push(buf_text.clone()),
                        "language" => metadata.language = buf_text.clone(),
                        "identifier" if metadata.identifier.is_empty() => {
                            metadata.identifier = buf_text.clone()
                        }
                        "publisher" => metadata.publisher = Some(buf_text.clone()),
                        "description" => metadata.description = Some(buf_text.clone()),
                        "subject" => metadata.subjects.push(buf_text.clone()),
                        "date" => metadata.date = Some(buf_text.clone()),
                        "rights" => metadata.rights = Some(buf_text.clone()),
                        _ => {}
                    }
                    current_element = None;
                    buf_text.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(e)),
            _ => {}
        }
    }

    // Detect cover image (EPUB3 property takes priority)
    let epub3_cover = manifest.values().find(|item| {
        item.properties
            .as_ref()
            .is_some_and(|props| props.split_ascii_whitespace().any(|p| p == "cover-image"))
    });

    if let Some(cover_item) = epub3_cover {
        metadata.cover_image = Some(cover_item.href.clone());
    } else if let Some(cover_id) = epub2_cover_id
        && let Some(item) = manifest.get(&cover_id) {
            metadata.cover_image = Some(item.href.clone());
        }

    // Convert manifest to simple map
    let manifest_simple: HashMap<String, (String, String)> = manifest
        .into_iter()
        .map(|(id, item)| (id, (item.href, item.media_type)))
        .collect();

    // Resolve NCX href
    let ncx_href = toc_id.and_then(|id| manifest_simple.get(&id).map(|(href, _)| href.clone()));

    Ok(OpfData {
        metadata,
        manifest: manifest_simple,
        spine_ids,
        ncx_href,
    })
}

/// Parse NCX table of contents.
pub fn parse_ncx(content: &str) -> io::Result<Vec<TocEntry>> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    struct NavPointState {
        children: Vec<TocEntry>,
        text: Option<String>,
        src: Option<String>,
        play_order: Option<usize>,
    }

    let mut stack: Vec<NavPointState> = vec![NavPointState {
        children: Vec::new(),
        text: None,
        src: None,
        play_order: None,
    }];
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"navPoint" => {
                        let mut play_order = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"playOrder"
                                && let Ok(order_str) = String::from_utf8(attr.value.to_vec())
                            {
                                play_order = order_str.parse().ok();
                            }
                        }
                        stack.push(NavPointState {
                            children: Vec::new(),
                            text: None,
                            src: None,
                            play_order,
                        });
                    }
                    b"text" => in_text = true,
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"content" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"src"
                            && let Some(state) = stack.last_mut()
                        {
                            state.src = Some(
                                String::from_utf8(attr.value.to_vec()).map_err(io::Error::other)?,
                            );
                        }
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if in_text && let Some(state) = stack.last_mut() {
                    let raw = String::from_utf8_lossy(e.as_ref());
                    match &mut state.text {
                        Some(existing) => existing.push_str(&raw),
                        None => state.text = Some(raw.into_owned()),
                    }
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if in_text && let Some(state) = stack.last_mut() {
                    let entity = String::from_utf8_lossy(e.as_ref());
                    if let Some(resolved) = resolve_entity(&entity) {
                        match &mut state.text {
                            Some(existing) => existing.push_str(&resolved),
                            None => state.text = Some(resolved),
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"text" => in_text = false,
                    b"navPoint" => {
                        if let Some(state) = stack.pop()
                            && let (Some(text), Some(src)) = (state.text, state.src)
                        {
                            let mut entry = TocEntry::new(text, src);
                            entry.children = state.children;
                            entry.play_order = state.play_order;

                            if let Some(parent) = stack.last_mut() {
                                parent.children.push(entry);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(e)),
            _ => {}
        }
    }

    Ok(stack.pop().map(|s| s.children).unwrap_or_default())
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

struct ManifestItem {
    href: String,
    media_type: String,
    properties: Option<String>,
}

/// Strip UTF-8 BOM if present.
pub fn strip_bom(data: &[u8]) -> &[u8] {
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &data[3..]
    } else {
        data
    }
}

/// Extract local name from namespaced XML name (e.g., "dc:title" -> "title").
fn local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .rposition(|&b| b == b':')
        .map(|i| &name[i + 1..])
        .unwrap_or(name)
}

/// Resolve XML entity references.
fn resolve_entity(entity: &str) -> Option<String> {
    match entity {
        "apos" => return Some("'".to_string()),
        "quot" => return Some("\"".to_string()),
        "lt" => return Some("<".to_string()),
        "gt" => return Some(">".to_string()),
        "amp" => return Some("&".to_string()),
        _ => {}
    }

    if let Some(hex) = entity.strip_prefix("#x") {
        if let Ok(code) = u32::from_str_radix(hex, 16)
            && let Some(c) = char::from_u32(code)
        {
            return Some(c.to_string());
        }
    } else if let Some(dec) = entity.strip_prefix('#')
        && let Ok(code) = dec.parse::<u32>()
            && let Some(c) = char::from_u32(code)
        {
            return Some(c.to_string());
        }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_bom() {
        // With BOM
        let with_bom = &[0xEF, 0xBB, 0xBF, b'h', b'i'];
        assert_eq!(strip_bom(with_bom), b"hi");

        // Without BOM
        let without_bom = b"hello";
        assert_eq!(strip_bom(without_bom), b"hello");

        // Empty
        assert_eq!(strip_bom(&[]), &[]);

        // Partial BOM (not stripped)
        let partial = &[0xEF, 0xBB, b'x'];
        assert_eq!(strip_bom(partial), partial);
    }

    #[test]
    fn test_local_name() {
        assert_eq!(local_name(b"title"), b"title");
        assert_eq!(local_name(b"dc:title"), b"title");
        assert_eq!(local_name(b"opf:meta"), b"meta");
        assert_eq!(local_name(b""), b"");
    }

    #[test]
    fn test_resolve_entity() {
        // Named entities
        assert_eq!(resolve_entity("apos"), Some("'".to_string()));
        assert_eq!(resolve_entity("quot"), Some("\"".to_string()));
        assert_eq!(resolve_entity("lt"), Some("<".to_string()));
        assert_eq!(resolve_entity("gt"), Some(">".to_string()));
        assert_eq!(resolve_entity("amp"), Some("&".to_string()));

        // Decimal numeric
        assert_eq!(resolve_entity("#65"), Some("A".to_string()));
        assert_eq!(resolve_entity("#8217"), Some("\u{2019}".to_string())); // right single quote

        // Hex numeric
        assert_eq!(resolve_entity("#x41"), Some("A".to_string()));
        assert_eq!(resolve_entity("#x2019"), Some("\u{2019}".to_string()));

        // Unknown
        assert_eq!(resolve_entity("nbsp"), None);
        assert_eq!(resolve_entity("invalid"), None);
    }

    #[test]
    fn test_parse_container_xml() {
        let container = br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

        let result = parse_container_xml(container).unwrap();
        assert_eq!(result, "OEBPS/content.opf");
    }

    #[test]
    fn test_parse_container_xml_with_bom() {
        let mut container = vec![0xEF, 0xBB, 0xBF]; // BOM
        container.extend_from_slice(br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#);

        let result = parse_container_xml(&container).unwrap();
        assert_eq!(result, "content.opf");
    }

    #[test]
    fn test_parse_opf_metadata() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:creator>Author One</dc:creator>
    <dc:creator>Author Two</dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier>urn:isbn:1234567890</dc:identifier>
    <dc:publisher>Test Publisher</dc:publisher>
    <dc:description>A test book description.</dc:description>
    <dc:subject>Fiction</dc:subject>
    <dc:subject>Adventure</dc:subject>
    <dc:date>2024-01-15</dc:date>
    <dc:rights>Public Domain</dc:rights>
  </metadata>
  <manifest>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="chapter1"/>
  </spine>
</package>"#;

        let result = parse_opf(opf).unwrap();

        assert_eq!(result.metadata.title, "Test Book");
        assert_eq!(result.metadata.authors, vec!["Author One", "Author Two"]);
        assert_eq!(result.metadata.language, "en");
        assert_eq!(result.metadata.identifier, "urn:isbn:1234567890");
        assert_eq!(result.metadata.publisher, Some("Test Publisher".to_string()));
        assert_eq!(result.metadata.description, Some("A test book description.".to_string()));
        assert_eq!(result.metadata.subjects, vec!["Fiction", "Adventure"]);
        assert_eq!(result.metadata.date, Some("2024-01-15".to_string()));
        assert_eq!(result.metadata.rights, Some("Public Domain".to_string()));

        assert_eq!(result.spine_ids, vec!["chapter1"]);
        assert_eq!(result.ncx_href, Some("toc.ncx".to_string()));
    }

    #[test]
    fn test_parse_opf_cover_epub3() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata><dc:title xmlns:dc="http://purl.org/dc/elements/1.1/">Book</dc:title></metadata>
  <manifest>
    <item id="cover-img" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.metadata.cover_image, Some("images/cover.jpg".to_string()));
    }

    #[test]
    fn test_parse_opf_cover_epub2() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata>
    <dc:title xmlns:dc="http://purl.org/dc/elements/1.1/">Book</dc:title>
    <meta name="cover" content="cover-id"/>
  </metadata>
  <manifest>
    <item id="cover-id" href="cover.png" media-type="image/png"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.metadata.cover_image, Some("cover.png".to_string()));
    }

    #[test]
    fn test_parse_ncx_flat() {
        let ncx = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="np1" playOrder="1">
      <navLabel><text>Chapter 1</text></navLabel>
      <content src="ch1.xhtml"/>
    </navPoint>
    <navPoint id="np2" playOrder="2">
      <navLabel><text>Chapter 2</text></navLabel>
      <content src="ch2.xhtml"/>
    </navPoint>
  </navMap>
</ncx>"#;

        let result = parse_ncx(ncx).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title, "Chapter 1");
        assert_eq!(result[0].href, "ch1.xhtml");
        assert_eq!(result[0].play_order, Some(1));
        assert_eq!(result[1].title, "Chapter 2");
        assert_eq!(result[1].href, "ch2.xhtml");
        assert_eq!(result[1].play_order, Some(2));
    }

    #[test]
    fn test_parse_ncx_nested() {
        let ncx = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="part1" playOrder="1">
      <navLabel><text>Part I</text></navLabel>
      <content src="part1.xhtml"/>
      <navPoint id="ch1" playOrder="2">
        <navLabel><text>Chapter 1</text></navLabel>
        <content src="ch1.xhtml"/>
      </navPoint>
      <navPoint id="ch2" playOrder="3">
        <navLabel><text>Chapter 2</text></navLabel>
        <content src="ch2.xhtml"/>
      </navPoint>
    </navPoint>
  </navMap>
</ncx>"#;

        let result = parse_ncx(ncx).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Part I");
        assert_eq!(result[0].children.len(), 2);
        assert_eq!(result[0].children[0].title, "Chapter 1");
        assert_eq!(result[0].children[1].title, "Chapter 2");
    }
}
