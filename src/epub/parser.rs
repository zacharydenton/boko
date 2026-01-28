//! EPUB parsing utilities (OPF, NCX, container.xml)

use std::collections::HashMap;
use std::io;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::book::{CollectionInfo, Contributor, Landmark, LandmarkType, Metadata, TocEntry};

/// Parsed OPF package data.
pub struct OpfData {
    pub metadata: Metadata,
    /// Maps manifest id -> (href, media_type)
    pub manifest: HashMap<String, (String, String)>,
    pub spine_ids: Vec<String>,
    pub ncx_href: Option<String>,
    /// EPUB 3 nav document href (has properties="nav")
    pub nav_href: Option<String>,
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

/// Types of metadata elements that can have refinements applied.
#[derive(Debug, Clone)]
enum MetaElement {
    Title,
    Creator(String),
    Contributor(String),
    Collection,
}

/// A refinement from an EPUB3 meta element.
#[derive(Debug)]
struct Refinement {
    /// ID of the element being refined (without #)
    refines: String,
    /// Property name (role, file-as, collection-type, group-position)
    property: String,
    /// The value
    value: String,
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

    // Track elements with IDs for refinement linking
    let mut element_ids: HashMap<String, MetaElement> = HashMap::new();
    // Collect refinements to apply after parsing
    let mut refinements: Vec<Refinement> = Vec::new();

    let mut in_metadata = false;
    let mut current_element: Option<String> = None;
    let mut current_element_id: Option<String> = None;
    let mut buf_text = String::new();

    // For meta elements with content (non-empty tags)
    let mut in_meta = false;
    let mut meta_property: Option<String> = None;
    let mut meta_refines: Option<String> = None;
    let mut meta_id: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                match local {
                    b"metadata" => in_metadata = true,
                    b"title" | b"creator" | b"language" | b"identifier" | b"publisher"
                    | b"description" | b"subject" | b"date" | b"rights" | b"contributor" => {
                        if in_metadata {
                            current_element = Some(String::from_utf8_lossy(local).to_string());
                            buf_text.clear();
                            // Check for id attribute
                            current_element_id = None;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"id" {
                                    current_element_id = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                            }
                        }
                    }
                    b"meta" if in_metadata => {
                        // EPUB3 meta with property attribute
                        in_meta = true;
                        buf_text.clear();
                        meta_property = None;
                        meta_refines = None;
                        meta_id = None;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"property" => {
                                    meta_property = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                b"refines" => {
                                    meta_refines = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                b"id" => {
                                    meta_id = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                _ => {}
                            }
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
                    b"meta" if in_metadata => {
                        // Handle both EPUB2 style (name/content) and EPUB3 empty meta
                        let mut is_cover = false;
                        let mut cover_id = String::new();
                        let mut property: Option<String> = None;
                        let mut refines: Option<String> = None;
                        let mut content: Option<String> = None;
                        let mut elem_id: Option<String> = None;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" if attr.value.as_ref() == b"cover" => is_cover = true,
                                b"content" => {
                                    let val = String::from_utf8(attr.value.to_vec())
                                        .map_err(io::Error::other)?;
                                    cover_id = val.clone();
                                    content = Some(val);
                                }
                                b"property" => {
                                    property = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                b"refines" => {
                                    refines = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                b"id" => {
                                    elem_id = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                _ => {}
                            }
                        }

                        if is_cover && !cover_id.is_empty() {
                            epub2_cover_id = Some(cover_id);
                        }

                        // EPUB3 meta with property but no content - value is in text
                        // EPUB3 empty meta with content attribute
                        if let Some(ref prop) = property {
                            if let Some(ref r) = refines {
                                // This is a refinement
                                let refines_id = r.strip_prefix('#').unwrap_or(r).to_string();
                                if let Some(val) = content {
                                    refinements.push(Refinement {
                                        refines: refines_id,
                                        property: prop.clone(),
                                        value: val,
                                    });
                                }
                            } else if let Some(ref val) = content {
                                // Top-level meta without refines
                                handle_meta_property(prop, val, &mut metadata, &mut element_ids, elem_id.as_deref());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if current_element.is_some() || in_meta {
                    buf_text.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if current_element.is_some() || in_meta {
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

                if local == b"meta" && in_meta {
                    // Handle EPUB3 meta element with text content
                    if let Some(ref prop) = meta_property {
                        let value = buf_text.trim().to_string();
                        if !value.is_empty() {
                            if let Some(ref r) = meta_refines {
                                let refines_id = r.strip_prefix('#').unwrap_or(r).to_string();
                                refinements.push(Refinement {
                                    refines: refines_id,
                                    property: prop.clone(),
                                    value,
                                });
                            } else {
                                handle_meta_property(prop, &value, &mut metadata, &mut element_ids, meta_id.as_deref());
                            }
                        }
                    }
                    in_meta = false;
                    meta_property = None;
                    meta_refines = None;
                    meta_id = None;
                    buf_text.clear();
                }

                if let Some(ref elem) = current_element {
                    let text = buf_text.clone();
                    match elem.as_str() {
                        "title" => {
                            metadata.title = text;
                            if let Some(ref id) = current_element_id {
                                element_ids.insert(id.clone(), MetaElement::Title);
                            }
                        }
                        "creator" => {
                            metadata.authors.push(text.clone());
                            if let Some(ref id) = current_element_id {
                                element_ids.insert(id.clone(), MetaElement::Creator(text));
                            }
                        }
                        "contributor" => {
                            // Store contributor for later refinement processing
                            if let Some(ref id) = current_element_id {
                                element_ids.insert(id.clone(), MetaElement::Contributor(text.clone()));
                            }
                            // Add basic contributor without role
                            metadata.contributors.push(Contributor {
                                name: text,
                                file_as: None,
                                role: None,
                            });
                        }
                        "language" => metadata.language = text,
                        "identifier" if metadata.identifier.is_empty() => {
                            metadata.identifier = text
                        }
                        "publisher" => metadata.publisher = Some(text),
                        "description" => metadata.description = Some(text),
                        "subject" => metadata.subjects.push(text),
                        "date" => metadata.date = Some(text),
                        "rights" => metadata.rights = Some(text),
                        _ => {}
                    }
                    current_element = None;
                    current_element_id = None;
                    buf_text.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(e)),
            _ => {}
        }
    }

    // Apply refinements to their target elements
    apply_refinements(&mut metadata, &element_ids, &refinements);

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

    // Detect EPUB3 nav document (properties="nav")
    let nav_href = manifest
        .values()
        .find(|item| {
            item.properties
                .as_ref()
                .is_some_and(|props| props.split_ascii_whitespace().any(|p| p == "nav"))
        })
        .map(|item| item.href.clone());

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
        nav_href,
    })
}

/// Handle a top-level meta property (no refines attribute).
fn handle_meta_property(
    property: &str,
    value: &str,
    metadata: &mut Metadata,
    element_ids: &mut HashMap<String, MetaElement>,
    elem_id: Option<&str>,
) {
    // Strip namespace prefix if present (e.g., "dcterms:modified" -> "modified")
    let prop_local = property
        .rsplit(':')
        .next()
        .unwrap_or(property);

    match prop_local {
        "modified" => {
            metadata.modified_date = Some(value.to_string());
        }
        "belongs-to-collection" => {
            // Initialize collection if not present
            if metadata.collection.is_none() {
                metadata.collection = Some(CollectionInfo {
                    name: value.to_string(),
                    collection_type: None,
                    position: None,
                });
            } else if let Some(ref mut coll) = metadata.collection {
                coll.name = value.to_string();
            }
            // Track the collection element ID for refinements
            if let Some(id) = elem_id {
                element_ids.insert(id.to_string(), MetaElement::Collection);
            }
        }
        _ => {}
    }
}

/// Apply collected refinements to their target elements.
fn apply_refinements(
    metadata: &mut Metadata,
    element_ids: &HashMap<String, MetaElement>,
    refinements: &[Refinement],
) {
    // Track collection refinements by collection element ID
    let mut collection_id: Option<String> = None;

    for (id, elem) in element_ids {
        if matches!(elem, MetaElement::Collection) {
            collection_id = Some(id.clone());
            break;
        }
    }

    for refinement in refinements {
        let prop_local = refinement.property
            .rsplit(':')
            .next()
            .unwrap_or(&refinement.property);

        if let Some(elem) = element_ids.get(&refinement.refines) {
            match elem {
                MetaElement::Title => {
                    if prop_local == "file-as" {
                        metadata.title_sort = Some(refinement.value.clone());
                    }
                }
                MetaElement::Creator(name) => {
                    if prop_local == "file-as" {
                        // Find first author and set author_sort
                        if metadata.authors.first().map(|a| a == name).unwrap_or(false) {
                            metadata.author_sort = Some(refinement.value.clone());
                        }
                    }
                }
                MetaElement::Contributor(name) => {
                    // Find the contributor and update it
                    for contrib in &mut metadata.contributors {
                        if contrib.name == *name {
                            match prop_local {
                                "role" => contrib.role = Some(refinement.value.clone()),
                                "file-as" => contrib.file_as = Some(refinement.value.clone()),
                                _ => {}
                            }
                            break;
                        }
                    }
                }
                MetaElement::Collection => {
                    if let Some(ref mut coll) = metadata.collection {
                        match prop_local {
                            "collection-type" => {
                                coll.collection_type = Some(refinement.value.clone());
                            }
                            "group-position" => {
                                coll.position = refinement.value.parse().ok();
                            }
                            _ => {}
                        }
                    }
                }
            }
        } else {
            // Check if this is a refinement for a collection that wasn't tracked
            // by belongs-to-collection (some EPUBs use meta property="belongs-to-collection"
            // with an id, then refine that)
            if let Some(ref coll_id) = collection_id {
                if &refinement.refines == coll_id {
                    if let Some(ref mut coll) = metadata.collection {
                        match prop_local {
                            "collection-type" => {
                                coll.collection_type = Some(refinement.value.clone());
                            }
                            "group-position" => {
                                coll.position = refinement.value.parse().ok();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
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

/// Parse EPUB 3 nav document landmarks.
///
/// Landmarks are in a `<nav epub:type="landmarks">` element containing
/// an ordered list of anchor elements with epub:type attributes.
pub fn parse_nav_landmarks(content: &str) -> io::Result<Vec<Landmark>> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut landmarks = Vec::new();
    let mut in_landmarks_nav = false;
    let mut current_href: Option<String> = None;
    let mut current_epub_type: Option<String> = None;
    let mut current_label = String::new();
    let mut in_anchor = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                match local {
                    b"nav" => {
                        // Check for epub:type="landmarks"
                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            let key_local = local_name(key);
                            if key_local == b"type" {
                                let value = String::from_utf8_lossy(&attr.value);
                                if value.split_ascii_whitespace().any(|v| v == "landmarks") {
                                    in_landmarks_nav = true;
                                }
                            }
                        }
                    }
                    b"a" if in_landmarks_nav => {
                        in_anchor = true;
                        current_label.clear();

                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            let key_local = local_name(key);

                            match key_local {
                                b"href" => {
                                    current_href = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                b"type" => {
                                    current_epub_type = Some(
                                        String::from_utf8(attr.value.to_vec())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if in_anchor {
                    current_label.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if in_anchor {
                    let entity = String::from_utf8_lossy(e.as_ref());
                    if let Some(resolved) = resolve_entity(&entity) {
                        current_label.push_str(&resolved);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                match local {
                    b"nav" => {
                        in_landmarks_nav = false;
                    }
                    b"a" if in_anchor => {
                        in_anchor = false;

                        // Create landmark if we have the required data
                        if let (Some(href), Some(epub_type)) =
                            (current_href.take(), current_epub_type.take())
                        {
                            if let Some(landmark_type) = epub_type_to_landmark(&epub_type) {
                                landmarks.push(Landmark {
                                    landmark_type,
                                    href,
                                    label: current_label.clone(),
                                });
                            }
                        }
                        current_label.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(e)),
            _ => {}
        }
    }

    Ok(landmarks)
}

/// Map EPUB epub:type value to LandmarkType.
fn epub_type_to_landmark(epub_type: &str) -> Option<LandmarkType> {
    // epub:type can have multiple space-separated values; check each
    for value in epub_type.split_ascii_whitespace() {
        match value {
            "cover" => return Some(LandmarkType::Cover),
            "bodymatter" => return Some(LandmarkType::BodyMatter),
            "frontmatter" => return Some(LandmarkType::FrontMatter),
            "backmatter" => return Some(LandmarkType::BackMatter),
            "toc" => return Some(LandmarkType::Toc),
            "titlepage" | "title-page" => return Some(LandmarkType::TitlePage),
            "acknowledgments" | "acknowledgements" => return Some(LandmarkType::Acknowledgements),
            "bibliography" => return Some(LandmarkType::Bibliography),
            "glossary" => return Some(LandmarkType::Glossary),
            "index" => return Some(LandmarkType::Index),
            "preface" => return Some(LandmarkType::Preface),
            "endnotes" | "footnotes" | "notes" | "rearnotes" => return Some(LandmarkType::Endnotes),
            "loi" => return Some(LandmarkType::Loi),
            "lot" => return Some(LandmarkType::Lot),
            _ => {}
        }
    }
    None
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
        let empty: &[u8] = &[];
        assert_eq!(strip_bom(empty), empty);

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

    #[test]
    fn test_parse_nav_landmarks_basic() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav id="landmarks" epub:type="landmarks">
      <ol>
        <li><a href="text/cover.xhtml" epub:type="cover">Cover</a></li>
        <li><a href="text/chapter1.xhtml" epub:type="bodymatter">Start Reading</a></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_landmarks(nav).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].landmark_type, LandmarkType::Cover);
        assert_eq!(result[0].href, "text/cover.xhtml");
        assert_eq!(result[0].label, "Cover");
        assert_eq!(result[1].landmark_type, LandmarkType::BodyMatter);
        assert_eq!(result[1].href, "text/chapter1.xhtml");
        assert_eq!(result[1].label, "Start Reading");
    }

    #[test]
    fn test_parse_nav_landmarks_all_types() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="landmarks">
      <ol>
        <li><a href="cover.xhtml" epub:type="cover">Cover</a></li>
        <li><a href="title.xhtml" epub:type="titlepage">Title Page</a></li>
        <li><a href="toc.xhtml" epub:type="toc">Table of Contents</a></li>
        <li><a href="front.xhtml" epub:type="frontmatter">Front Matter</a></li>
        <li><a href="body.xhtml" epub:type="bodymatter">Body</a></li>
        <li><a href="back.xhtml" epub:type="backmatter">Back Matter</a></li>
        <li><a href="ack.xhtml" epub:type="acknowledgments">Acknowledgments</a></li>
        <li><a href="bib.xhtml" epub:type="bibliography">Bibliography</a></li>
        <li><a href="gloss.xhtml" epub:type="glossary">Glossary</a></li>
        <li><a href="index.xhtml" epub:type="index">Index</a></li>
        <li><a href="preface.xhtml" epub:type="preface">Preface</a></li>
        <li><a href="notes.xhtml" epub:type="endnotes">Endnotes</a></li>
        <li><a href="loi.xhtml" epub:type="loi">List of Illustrations</a></li>
        <li><a href="lot.xhtml" epub:type="lot">List of Tables</a></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_landmarks(nav).unwrap();

        assert_eq!(result.len(), 14);
        assert_eq!(result[0].landmark_type, LandmarkType::Cover);
        assert_eq!(result[1].landmark_type, LandmarkType::TitlePage);
        assert_eq!(result[2].landmark_type, LandmarkType::Toc);
        assert_eq!(result[3].landmark_type, LandmarkType::FrontMatter);
        assert_eq!(result[4].landmark_type, LandmarkType::BodyMatter);
        assert_eq!(result[5].landmark_type, LandmarkType::BackMatter);
        assert_eq!(result[6].landmark_type, LandmarkType::Acknowledgements);
        assert_eq!(result[7].landmark_type, LandmarkType::Bibliography);
        assert_eq!(result[8].landmark_type, LandmarkType::Glossary);
        assert_eq!(result[9].landmark_type, LandmarkType::Index);
        assert_eq!(result[10].landmark_type, LandmarkType::Preface);
        assert_eq!(result[11].landmark_type, LandmarkType::Endnotes);
        assert_eq!(result[12].landmark_type, LandmarkType::Loi);
        assert_eq!(result[13].landmark_type, LandmarkType::Lot);
    }

    #[test]
    fn test_parse_nav_landmarks_ignores_toc_nav() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="toc">
      <ol>
        <li><a href="ch1.xhtml">Chapter 1</a></li>
      </ol>
    </nav>
    <nav epub:type="landmarks">
      <ol>
        <li><a href="cover.xhtml" epub:type="cover">Cover</a></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_landmarks(nav).unwrap();

        // Should only get the landmark, not the TOC entry
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].landmark_type, LandmarkType::Cover);
    }

    #[test]
    fn test_parse_nav_landmarks_footnotes_variants() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="landmarks">
      <ol>
        <li><a href="fn.xhtml" epub:type="footnotes">Footnotes</a></li>
        <li><a href="notes.xhtml" epub:type="notes">Notes</a></li>
        <li><a href="rn.xhtml" epub:type="rearnotes">Rear Notes</a></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_landmarks(nav).unwrap();

        // All variants should map to Endnotes
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].landmark_type, LandmarkType::Endnotes);
        assert_eq!(result[1].landmark_type, LandmarkType::Endnotes);
        assert_eq!(result[2].landmark_type, LandmarkType::Endnotes);
    }

    #[test]
    fn test_parse_nav_landmarks_unknown_type_skipped() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="landmarks">
      <ol>
        <li><a href="cover.xhtml" epub:type="cover">Cover</a></li>
        <li><a href="unknown.xhtml" epub:type="custom-type">Custom</a></li>
        <li><a href="body.xhtml" epub:type="bodymatter">Body</a></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_landmarks(nav).unwrap();

        // Unknown types should be skipped
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].landmark_type, LandmarkType::Cover);
        assert_eq!(result[1].landmark_type, LandmarkType::BodyMatter);
    }

    #[test]
    fn test_parse_nav_landmarks_empty() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="toc">
      <ol>
        <li><a href="ch1.xhtml">Chapter 1</a></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_landmarks(nav).unwrap();

        // No landmarks nav, should return empty
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_opf_nav_href() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata><dc:title xmlns:dc="http://purl.org/dc/elements/1.1/">Book</dc:title></metadata>
  <manifest>
    <item id="nav" href="toc.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.nav_href, Some("toc.xhtml".to_string()));
    }

    #[test]
    fn test_parse_opf_no_nav() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata><dc:title xmlns:dc="http://purl.org/dc/elements/1.1/">Book</dc:title></metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="ch1"/></spine>
</package>"#;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.nav_href, None);
    }

    #[test]
    fn test_parse_opf_dcterms_modified() {
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2024-01-15T12:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.metadata.modified_date, Some("2024-01-15T12:00:00Z".to_string()));
    }

    #[test]
    fn test_parse_opf_contributor_with_role() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <dc:contributor id="contrib1">John Translator</dc:contributor>
    <meta refines="#contrib1" property="role" scheme="marc:relators">trl</meta>
    <meta refines="#contrib1" property="file-as">Translator, John</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"##;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.metadata.contributors.len(), 1);
        assert_eq!(result.metadata.contributors[0].name, "John Translator");
        assert_eq!(result.metadata.contributors[0].role, Some("trl".to_string()));
        assert_eq!(result.metadata.contributors[0].file_as, Some("Translator, John".to_string()));
    }

    #[test]
    fn test_parse_opf_collection_series() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>The Second Book</dc:title>
    <dc:language>en</dc:language>
    <meta property="belongs-to-collection" id="series1">My Awesome Series</meta>
    <meta refines="#series1" property="collection-type">series</meta>
    <meta refines="#series1" property="group-position">2</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"##;

        let result = parse_opf(opf).unwrap();
        assert!(result.metadata.collection.is_some());
        let coll = result.metadata.collection.unwrap();
        assert_eq!(coll.name, "My Awesome Series");
        assert_eq!(coll.collection_type, Some("series".to_string()));
        assert_eq!(coll.position, Some(2.0));
    }

    #[test]
    fn test_parse_opf_title_file_as() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title id="title1">The Great Adventure</dc:title>
    <meta refines="#title1" property="file-as">Great Adventure, The</meta>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"##;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.metadata.title, "The Great Adventure");
        assert_eq!(result.metadata.title_sort, Some("Great Adventure, The".to_string()));
    }

    #[test]
    fn test_parse_opf_author_file_as() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:creator id="author1">Jane Doe</dc:creator>
    <meta refines="#author1" property="file-as">Doe, Jane</meta>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"##;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.metadata.authors, vec!["Jane Doe"]);
        assert_eq!(result.metadata.author_sort, Some("Doe, Jane".to_string()));
    }

    #[test]
    fn test_parse_opf_collection_fractional_position() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Book 3.5</dc:title>
    <dc:language>en</dc:language>
    <meta property="belongs-to-collection" id="s1">Series Name</meta>
    <meta refines="#s1" property="group-position">3.5</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"##;

        let result = parse_opf(opf).unwrap();
        assert!(result.metadata.collection.is_some());
        let coll = result.metadata.collection.unwrap();
        assert_eq!(coll.position, Some(3.5));
    }

    #[test]
    fn test_parse_opf_multiple_contributors() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title>
    <dc:language>en</dc:language>
    <dc:contributor id="c1">Translator Name</dc:contributor>
    <meta refines="#c1" property="role">trl</meta>
    <dc:contributor id="c2">Editor Name</dc:contributor>
    <meta refines="#c2" property="role">edt</meta>
    <dc:contributor id="c3">Illustrator Name</dc:contributor>
    <meta refines="#c3" property="role">ill</meta>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"##;

        let result = parse_opf(opf).unwrap();
        assert_eq!(result.metadata.contributors.len(), 3);

        // Find translator
        let translator = result.metadata.contributors.iter().find(|c| c.role == Some("trl".to_string()));
        assert!(translator.is_some());
        assert_eq!(translator.unwrap().name, "Translator Name");

        // Find editor
        let editor = result.metadata.contributors.iter().find(|c| c.role == Some("edt".to_string()));
        assert!(editor.is_some());
        assert_eq!(editor.unwrap().name, "Editor Name");
    }
}
