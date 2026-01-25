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
