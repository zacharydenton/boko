//! EPUB parsing utilities (OPF, NCX, container.xml)

use std::collections::HashMap;
use std::io;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::model::{CollectionInfo, Contributor, Landmark, LandmarkType, Metadata, TocEntry};

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
                        return attr
                            .unescape_value()
                            .map(|v| v.into_owned())
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
    // No trim_text: values may contain nested markup (<dc:title>Foo <i>Bar</i>
    // Baz</dc:title>) whose surrounding spaces per-event trimming would eat.
    // Values are trimmed once fully assembled instead.

    let mut parser = OpfParser::default();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => parser.handle_start(&e)?,
            Ok(Event::Empty(e)) => parser.handle_empty(&e)?,
            Ok(Event::Text(e)) if parser.collecting_text() => {
                parser
                    .buf_text
                    .push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::GeneralRef(e)) if parser.collecting_text() => {
                let entity = String::from_utf8_lossy(e.as_ref());
                if let Some(resolved) = resolve_entity(&entity) {
                    parser.buf_text.push_str(&resolved);
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                parser.handle_end(local_name(name.as_ref()));
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(e)),
            _ => {}
        }
    }

    Ok(parser.finish())
}

/// Streaming state for a single pass over the OPF package document.
#[derive(Default)]
struct OpfParser {
    metadata: Metadata,
    manifest: HashMap<String, ManifestItem>,
    spine_ids: Vec<String>,
    toc_id: Option<String>,
    epub2_cover_id: Option<String>,

    /// Track elements with IDs for refinement linking.
    element_ids: HashMap<String, MetaElement>,
    /// Collected refinements, applied after parsing.
    refinements: Vec<Refinement>,

    in_metadata: bool,
    /// Local name of the DC metadata element currently being read.
    current_element: Option<String>,
    current_element_id: Option<String>,
    buf_text: String,

    /// For meta elements with text content (non-empty tags).
    in_meta: bool,
    meta_property: Option<String>,
    meta_refines: Option<String>,
    meta_id: Option<String>,
    /// `content` attribute of a non-self-closing meta (fallback value when
    /// the element has no text content).
    meta_content: Option<String>,
}

impl OpfParser {
    /// Whether text/entity events should be accumulated into `buf_text`.
    fn collecting_text(&self) -> bool {
        self.current_element.is_some() || self.in_meta
    }

    fn handle_start(&mut self, e: &BytesStart) -> io::Result<()> {
        let name = e.name();
        let local = local_name(name.as_ref());

        match local {
            b"metadata" => self.in_metadata = true,
            b"title" | b"creator" | b"language" | b"identifier" | b"publisher" | b"description"
            | b"subject" | b"date" | b"rights" | b"contributor"
                if self.in_metadata =>
            {
                self.start_dc_element(local, e)?;
            }
            b"meta" if self.in_metadata => self.start_meta(e)?,
            // <item ...></item> is XML-equivalent to <item .../>; an OPF
            // authored with explicit close tags must not lose its entire
            // manifest and spine.
            b"item" => self.parse_manifest_item(e)?,
            b"itemref" => self.parse_spine_itemref(e)?,
            b"spine" => {
                if let Some(toc) = attr(e, b"toc")? {
                    self.toc_id = Some(toc);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_empty(&mut self, e: &BytesStart) -> io::Result<()> {
        let name = e.name();
        let local = local_name(name.as_ref());

        match local {
            b"item" => self.parse_manifest_item(e)?,
            b"itemref" => self.parse_spine_itemref(e)?,
            b"meta" if self.in_metadata => self.parse_empty_meta(e)?,
            _ => {}
        }
        Ok(())
    }

    fn handle_end(&mut self, local: &[u8]) {
        if local == b"metadata" {
            self.in_metadata = false;
        }

        if local == b"meta" && self.in_meta {
            self.end_meta();
        }

        // Commit only on the element's own end tag: nested markup like
        // <dc:title>Foo <i>Bar</i> Baz</dc:title> must not commit early on
        // </i> and lose the trailing text.
        if self
            .current_element
            .as_ref()
            .is_some_and(|cur| cur.as_bytes() == local)
        {
            self.end_dc_element();
        }
    }

    /// Begin reading a Dublin Core metadata element (dc:title, dc:creator, ...).
    fn start_dc_element(&mut self, local: &[u8], e: &BytesStart) -> io::Result<()> {
        self.current_element = Some(String::from_utf8_lossy(local).to_string());
        self.buf_text.clear();
        self.current_element_id = attr(e, b"id")?;
        Ok(())
    }

    /// Begin reading an EPUB3 `<meta property="...">` element with text content.
    fn start_meta(&mut self, e: &BytesStart) -> io::Result<()> {
        self.in_meta = true;
        self.buf_text.clear();
        self.meta_property = attr(e, b"property")?;
        self.meta_refines = attr(e, b"refines")?;
        self.meta_id = attr(e, b"id")?;
        self.meta_content = attr(e, b"content")?;

        // EPUB2 <meta name="cover" content="..."> may be written with an
        // explicit close tag; its data lives entirely in attributes.
        let is_cover = e
            .attributes()
            .flatten()
            .any(|a| a.key.as_ref() == b"name" && &*a.value == b"cover");
        if is_cover
            && let Some(ref cover_id) = self.meta_content
            && !cover_id.is_empty()
        {
            self.epub2_cover_id = Some(cover_id.clone());
        }
        Ok(())
    }

    /// Parse a `<manifest>` `<item>` entry.
    fn parse_manifest_item(&mut self, e: &BytesStart) -> io::Result<()> {
        let id = attr(e, b"id")?.unwrap_or_default();
        let href = attr(e, b"href")?.unwrap_or_default();
        let media_type = attr(e, b"media-type")?.unwrap_or_default();
        let properties = attr(e, b"properties")?;

        if !id.is_empty() {
            self.manifest.insert(
                id,
                ManifestItem {
                    href,
                    media_type,
                    properties,
                },
            );
        }
        Ok(())
    }

    /// Parse a `<spine>` `<itemref>` entry.
    fn parse_spine_itemref(&mut self, e: &BytesStart) -> io::Result<()> {
        if let Some(idref) = attr(e, b"idref")? {
            self.spine_ids.push(idref);
        }
        Ok(())
    }

    /// Parse a self-closing `<meta/>`: EPUB2 style (name/content) and EPUB3
    /// empty meta with a content attribute.
    fn parse_empty_meta(&mut self, e: &BytesStart) -> io::Result<()> {
        let is_cover = e
            .attributes()
            .flatten()
            .any(|a| a.key.as_ref() == b"name" && &*a.value == b"cover");
        let content = attr(e, b"content")?;
        let property = attr(e, b"property")?;
        let refines = attr(e, b"refines")?;
        let elem_id = attr(e, b"id")?;

        if is_cover
            && let Some(ref cover_id) = content
            && !cover_id.is_empty()
        {
            self.epub2_cover_id = Some(cover_id.clone());
        }

        if let Some(ref prop) = property {
            if let Some(ref r) = refines {
                // This is a refinement
                let refines_id = r.strip_prefix('#').unwrap_or(r).to_string();
                if let Some(val) = content {
                    self.refinements.push(Refinement {
                        refines: refines_id,
                        property: prop.clone(),
                        value: val,
                    });
                }
            } else if let Some(ref val) = content {
                // Top-level meta without refines
                handle_meta_property(
                    prop,
                    val,
                    &mut self.metadata,
                    &mut self.element_ids,
                    elem_id.as_deref(),
                );
            }
        }
        Ok(())
    }

    /// Finish an EPUB3 meta element whose value was in its text content
    /// (falling back to its `content` attribute when the body is empty).
    fn end_meta(&mut self) {
        if let Some(ref prop) = self.meta_property {
            let mut value = self.buf_text.trim().to_string();
            if value.is_empty()
                && let Some(ref content) = self.meta_content
            {
                value = content.trim().to_string();
            }
            if !value.is_empty() {
                if let Some(ref r) = self.meta_refines {
                    let refines_id = r.strip_prefix('#').unwrap_or(r).to_string();
                    self.refinements.push(Refinement {
                        refines: refines_id,
                        property: prop.clone(),
                        value,
                    });
                } else {
                    handle_meta_property(
                        prop,
                        &value,
                        &mut self.metadata,
                        &mut self.element_ids,
                        self.meta_id.as_deref(),
                    );
                }
            }
        }
        self.in_meta = false;
        self.meta_property = None;
        self.meta_refines = None;
        self.meta_id = None;
        self.meta_content = None;
        self.buf_text.clear();
    }

    /// Finish the Dublin Core element currently being read, committing its
    /// accumulated text into the metadata.
    fn end_dc_element(&mut self) {
        let Some(elem) = self.current_element.take() else {
            return;
        };
        // trim_text is off (values may span nested markup), so trim the
        // assembled value once here.
        let text = std::mem::take(&mut self.buf_text).trim().to_string();
        let elem_id = self.current_element_id.take();

        match elem.as_str() {
            "title" => {
                if self.metadata.title.is_empty() {
                    self.metadata.title = text;
                }
                if let Some(id) = elem_id {
                    self.element_ids.insert(id, MetaElement::Title);
                }
            }
            "creator" => {
                self.metadata.authors.push(text.clone());
                if let Some(id) = elem_id {
                    self.element_ids.insert(id, MetaElement::Creator(text));
                }
            }
            "contributor" => {
                // Store contributor for later refinement processing
                if let Some(id) = elem_id {
                    self.element_ids
                        .insert(id, MetaElement::Contributor(text.clone()));
                }
                // Add basic contributor without role
                self.metadata.contributors.push(Contributor {
                    name: text,
                    file_as: None,
                    role: None,
                });
            }
            "language" => self.metadata.language = text,
            "identifier" if self.metadata.identifier.is_empty() => {
                self.metadata.identifier = text;
            }
            "publisher" => self.metadata.publisher = Some(text),
            "description" => self.metadata.description = Some(text),
            "subject" => self.metadata.subjects.push(text),
            "date" => self.metadata.date = Some(text),
            "rights" => self.metadata.rights = Some(text),
            _ => {}
        }
    }

    /// Apply refinements, resolve cover/nav/NCX references and produce OpfData.
    fn finish(mut self) -> OpfData {
        // Apply refinements to their target elements
        apply_refinements(&mut self.metadata, &self.element_ids, &self.refinements);

        // Detect cover image (EPUB3 property takes priority)
        let epub3_cover = self.manifest.values().find(|item| {
            item.properties
                .as_ref()
                .is_some_and(|props| props.split_ascii_whitespace().any(|p| p == "cover-image"))
        });

        if let Some(cover_item) = epub3_cover {
            self.metadata.cover_image = Some(cover_item.href.clone());
        } else if let Some(cover_id) = self.epub2_cover_id
            && let Some(item) = self.manifest.get(&cover_id)
        {
            self.metadata.cover_image = Some(item.href.clone());
        }

        // Detect EPUB3 nav document (properties="nav")
        let nav_href = self
            .manifest
            .values()
            .find(|item| {
                item.properties
                    .as_ref()
                    .is_some_and(|props| props.split_ascii_whitespace().any(|p| p == "nav"))
            })
            .map(|item| item.href.clone());

        // Convert manifest to simple map
        let manifest: HashMap<String, (String, String)> = self
            .manifest
            .into_iter()
            .map(|(id, item)| (id, (item.href, item.media_type)))
            .collect();

        // Resolve NCX href
        let ncx_href = self
            .toc_id
            .and_then(|id| manifest.get(&id).map(|(href, _)| href.clone()));

        OpfData {
            metadata: self.metadata,
            manifest,
            spine_ids: self.spine_ids,
            ncx_href,
            nav_href,
        }
    }
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
    let prop_local = property.rsplit(':').next().unwrap_or(property);

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
    for refinement in refinements {
        let prop_local = refinement
            .property
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
        }
    }
}

/// Parse NCX table of contents.
pub fn parse_ncx(content: &str) -> io::Result<Vec<TocEntry>> {
    let mut reader = Reader::from_str(content);
    // Do NOT enable trim_text here: labels containing entity references
    // arrive as alternating Text/GeneralRef events, and per-event trimming
    // would eat the whitespace-only Text segments between entities
    // ("Caf&eacute; &amp; Society" would collapse to "Café&Society").
    // Labels are trimmed once fully assembled instead.

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
                        // Reject pathologically deep nesting: the resulting
                        // TocEntry tree is consumed (and dropped) recursively, so
                        // an unbounded-depth NCX would overflow the stack later.
                        if stack.len() > crate::util::MAX_TREE_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "NCX navPoint nesting too deep",
                            ));
                        }
                        let mut play_order = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"playOrder"
                                && let Ok(order_str) = attr.unescape_value().map(|v| v.into_owned())
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
                                attr.unescape_value()
                                    .map(|v| v.into_owned())
                                    .map_err(io::Error::other)?,
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
                        if let Some(state) = stack.pop() {
                            match (&state.text, &state.src) {
                                // Whitespace-only labels are dropped, as they
                                // were when per-event trimming removed them
                                // entirely.
                                (Some(text), Some(src)) if !text.trim().is_empty() => {
                                    let mut entry = TocEntry::new(text.trim(), src.clone());
                                    entry.children = state.children;
                                    entry.play_order = state.play_order;
                                    if let Some(parent) = stack.last_mut() {
                                        parent.children.push(entry);
                                    }
                                }
                                // A structural navPoint missing its label or
                                // src must not take its whole subtree with
                                // it: hoist the already-parsed children.
                                _ => {
                                    if let Some(parent) = stack.last_mut() {
                                        parent.children.extend(state.children);
                                    }
                                }
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

/// Parse EPUB 3 nav document table of contents.
///
/// The TOC lives in a `<nav epub:type="toc">` element as nested ordered
/// lists: each `<li>` holds an `<a href="...">` (or, for unlinked headings, a
/// `<span>`) label, optionally followed by a nested `<ol>` of children. EPUB 3
/// makes this nav document the canonical TOC — the NCX is optional there — so
/// the importer falls back to this when no usable NCX exists.
pub fn parse_nav_toc(content: &str) -> io::Result<Vec<TocEntry>> {
    let mut reader = Reader::from_str(content);
    // No trim_text: labels may contain nested inline elements
    // (`<a>Chapter <span>1</span>: The Start</a>`) whose surrounding spaces
    // per-event trimming would eat. Titles are trimmed once assembled.

    struct ItemState {
        title: String,
        href: Option<String>,
        children: Vec<TocEntry>,
        /// Whether this item's label element (`<a>`/`<span>`) has been seen;
        /// only the first one names the entry.
        labeled: bool,
    }

    let mut root: Vec<TocEntry> = Vec::new();
    let mut stack: Vec<ItemState> = Vec::new();
    let mut in_toc_nav = false;
    let mut in_label = false;
    // Nesting depth of <a>/<span> elements inside the current label, so a
    // nested </span> doesn't terminate label collection early and drop the
    // rest of the title.
    let mut label_depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                match local {
                    b"nav" => {
                        // Check for epub:type="toc"
                        for attr in e.attributes().flatten() {
                            if local_name(attr.key.as_ref()) == b"type" {
                                let value = String::from_utf8_lossy(&attr.value);
                                if value.split_ascii_whitespace().any(|v| v == "toc") {
                                    in_toc_nav = true;
                                }
                            }
                        }
                    }
                    b"li" if in_toc_nav => {
                        // Same guard as parse_ncx: the resulting TocEntry tree
                        // is consumed (and dropped) recursively downstream, so
                        // unbounded nesting would overflow the stack later.
                        if stack.len() > crate::util::MAX_TREE_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "nav TOC list nesting too deep",
                            ));
                        }
                        stack.push(ItemState {
                            title: String::new(),
                            href: None,
                            children: Vec::new(),
                            labeled: false,
                        });
                    }
                    b"a" | b"span" if in_toc_nav => {
                        if in_label {
                            // Nested inline element within the label.
                            label_depth += 1;
                        } else if let Some(item) = stack.last_mut()
                            && !item.labeled
                        {
                            item.labeled = true;
                            in_label = true;
                            label_depth = 1;
                            if local == b"a" {
                                for attr in e.attributes().flatten() {
                                    if local_name(attr.key.as_ref()) == b"href" {
                                        item.href = Some(
                                            attr.unescape_value()
                                                .map(|v| v.into_owned())
                                                .map_err(io::Error::other)?,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if in_label => {
                if let Some(item) = stack.last_mut() {
                    item.title.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::GeneralRef(e)) if in_label => {
                if let Some(item) = stack.last_mut() {
                    let entity = String::from_utf8_lossy(e.as_ref());
                    if let Some(resolved) = resolve_entity(&entity) {
                        item.title.push_str(&resolved);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());

                match local {
                    b"a" | b"span" => {
                        if in_label {
                            label_depth = label_depth.saturating_sub(1);
                            if label_depth == 0 {
                                in_label = false;
                            }
                        }
                    }
                    b"li" if in_toc_nav => {
                        if let Some(item) = stack.pop() {
                            // Keep linked entries and unlinked headings that
                            // still contribute children; drop empty <li>s.
                            if item.href.is_some() || !item.children.is_empty() {
                                let mut entry =
                                    TocEntry::new(item.title.trim(), item.href.unwrap_or_default());
                                entry.children = item.children;
                                match stack.last_mut() {
                                    Some(parent) => parent.children.push(entry),
                                    None => root.push(entry),
                                }
                            }
                        }
                    }
                    b"nav" if in_toc_nav => break, // finished the toc nav
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(io::Error::other(e)),
            _ => {}
        }
    }

    Ok(root)
}

/// Parse EPUB 3 nav document landmarks.
///
/// Landmarks are in a `<nav epub:type="landmarks">` element containing
/// an ordered list of anchor elements with epub:type attributes.
pub fn parse_nav_landmarks(content: &str) -> io::Result<Vec<Landmark>> {
    let mut reader = Reader::from_str(content);
    // No trim_text (see parse_nav_toc): labels with nested markup would lose
    // their internal spaces. Labels are trimmed once assembled.

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
                                        attr.unescape_value()
                                            .map(|v| v.into_owned())
                                            .map_err(io::Error::other)?,
                                    );
                                }
                                b"type" => {
                                    current_epub_type = Some(
                                        attr.unescape_value()
                                            .map(|v| v.into_owned())
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
            Ok(Event::Text(e)) if in_anchor => {
                current_label.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::GeneralRef(e)) if in_anchor => {
                let entity = String::from_utf8_lossy(e.as_ref());
                if let Some(resolved) = resolve_entity(&entity) {
                    current_label.push_str(&resolved);
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
                            && let Some(landmark_type) = epub_type_to_landmark(&epub_type)
                        {
                            landmarks.push(Landmark {
                                landmark_type,
                                href,
                                label: current_label.trim().to_string(),
                            });
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
            "endnotes" | "footnotes" | "notes" | "rearnotes" => {
                return Some(LandmarkType::Endnotes);
            }
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

/// Get an attribute's unescaped value, matching the exact attribute name.
fn attr(e: &BytesStart, key: &[u8]) -> io::Result<Option<String>> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == key {
            return a
                .unescape_value()
                .map(|v| Some(v.into_owned()))
                .map_err(io::Error::other);
        }
    }
    Ok(None)
}

/// Resolve XML/HTML entity references found in metadata text and TOC labels.
///
/// Handles the five XML predefined entities, numeric character references
/// (`&#NNN;` / `&#xHH;`), and a table of named HTML entities commonly seen in
/// real-world OPF/NCX/nav documents (see [`resolve_named_entity`]).
///
/// Unknown entities resolve to `None` and are dropped by callers — the same
/// lenient, best-effort stance the surrounding parsers take toward malformed
/// attributes and invalid UTF-8.
fn resolve_entity(entity: &str) -> Option<String> {
    if let Some(named) = resolve_named_entity(entity) {
        return Some(named.to_string());
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

/// Resolve a named character entity.
///
/// Covers the XML five, the full Latin-1 (ISO 8859-1) set, and the common
/// HTML typographic entities (spaces, dashes, curly quotes, ellipsis, etc.)
/// that ebook tools routinely emit into NCX labels without declaring.
#[rustfmt::skip]
fn resolve_named_entity(entity: &str) -> Option<&'static str> {
    Some(match entity {
        // XML predefined
        "amp" => "&", "lt" => "<", "gt" => ">", "quot" => "\"", "apos" => "'",
        // Spaces and format controls
        "nbsp" => "\u{00A0}", "ensp" => "\u{2002}", "emsp" => "\u{2003}",
        "thinsp" => "\u{2009}", "shy" => "\u{00AD}",
        "zwnj" => "\u{200C}", "zwj" => "\u{200D}",
        // Dashes, quotes, and other typography
        "ndash" => "\u{2013}", "mdash" => "\u{2014}",
        "lsquo" => "\u{2018}", "rsquo" => "\u{2019}", "sbquo" => "\u{201A}",
        "ldquo" => "\u{201C}", "rdquo" => "\u{201D}", "bdquo" => "\u{201E}",
        "lsaquo" => "\u{2039}", "rsaquo" => "\u{203A}",
        "hellip" => "\u{2026}", "bull" => "\u{2022}",
        "dagger" => "\u{2020}", "Dagger" => "\u{2021}",
        "prime" => "\u{2032}", "Prime" => "\u{2033}",
        "permil" => "\u{2030}", "minus" => "\u{2212}",
        "euro" => "\u{20AC}", "trade" => "\u{2122}",
        // Latin Extended-A letters common in European text
        "OElig" => "\u{0152}", "oelig" => "\u{0153}",
        "Scaron" => "\u{0160}", "scaron" => "\u{0161}", "Yuml" => "\u{0178}",
        // Latin-1 punctuation, symbols, and signs (U+00A1..U+00BF)
        "iexcl" => "\u{00A1}", "cent" => "\u{00A2}", "pound" => "\u{00A3}",
        "curren" => "\u{00A4}", "yen" => "\u{00A5}", "brvbar" => "\u{00A6}",
        "sect" => "\u{00A7}", "uml" => "\u{00A8}", "copy" => "\u{00A9}",
        "ordf" => "\u{00AA}", "laquo" => "\u{00AB}", "not" => "\u{00AC}",
        "reg" => "\u{00AE}", "macr" => "\u{00AF}", "deg" => "\u{00B0}",
        "plusmn" => "\u{00B1}", "sup2" => "\u{00B2}", "sup3" => "\u{00B3}",
        "acute" => "\u{00B4}", "micro" => "\u{00B5}", "para" => "\u{00B6}",
        "middot" => "\u{00B7}", "cedil" => "\u{00B8}", "sup1" => "\u{00B9}",
        "ordm" => "\u{00BA}", "raquo" => "\u{00BB}", "frac14" => "\u{00BC}",
        "frac12" => "\u{00BD}", "frac34" => "\u{00BE}", "iquest" => "\u{00BF}",
        // Latin-1 accented letters (U+00C0..U+00FF)
        "Agrave" => "\u{00C0}", "Aacute" => "\u{00C1}", "Acirc" => "\u{00C2}",
        "Atilde" => "\u{00C3}", "Auml" => "\u{00C4}", "Aring" => "\u{00C5}",
        "AElig" => "\u{00C6}", "Ccedil" => "\u{00C7}", "Egrave" => "\u{00C8}",
        "Eacute" => "\u{00C9}", "Ecirc" => "\u{00CA}", "Euml" => "\u{00CB}",
        "Igrave" => "\u{00CC}", "Iacute" => "\u{00CD}", "Icirc" => "\u{00CE}",
        "Iuml" => "\u{00CF}", "ETH" => "\u{00D0}", "Ntilde" => "\u{00D1}",
        "Ograve" => "\u{00D2}", "Oacute" => "\u{00D3}", "Ocirc" => "\u{00D4}",
        "Otilde" => "\u{00D5}", "Ouml" => "\u{00D6}", "times" => "\u{00D7}",
        "Oslash" => "\u{00D8}", "Ugrave" => "\u{00D9}", "Uacute" => "\u{00DA}",
        "Ucirc" => "\u{00DB}", "Uuml" => "\u{00DC}", "Yacute" => "\u{00DD}",
        "THORN" => "\u{00DE}", "szlig" => "\u{00DF}", "agrave" => "\u{00E0}",
        "aacute" => "\u{00E1}", "acirc" => "\u{00E2}", "atilde" => "\u{00E3}",
        "auml" => "\u{00E4}", "aring" => "\u{00E5}", "aelig" => "\u{00E6}",
        "ccedil" => "\u{00E7}", "egrave" => "\u{00E8}", "eacute" => "\u{00E9}",
        "ecirc" => "\u{00EA}", "euml" => "\u{00EB}", "igrave" => "\u{00EC}",
        "iacute" => "\u{00ED}", "icirc" => "\u{00EE}", "iuml" => "\u{00EF}",
        "eth" => "\u{00F0}", "ntilde" => "\u{00F1}", "ograve" => "\u{00F2}",
        "oacute" => "\u{00F3}", "ocirc" => "\u{00F4}", "otilde" => "\u{00F5}",
        "ouml" => "\u{00F6}", "divide" => "\u{00F7}", "oslash" => "\u{00F8}",
        "ugrave" => "\u{00F9}", "uacute" => "\u{00FA}", "ucirc" => "\u{00FB}",
        "uuml" => "\u{00FC}", "yacute" => "\u{00FD}", "thorn" => "\u{00FE}",
        "yuml" => "\u{00FF}",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_opf_accepts_explicit_close_tags() {
        // <item ...></item> is XML-equivalent to <item .../>; an OPF written
        // this way used to parse to an EMPTY manifest and spine (whole book
        // silently lost).
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>T</dc:title>
    <meta name="cover" content="cov"></meta>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"></item>
    <item id="cov" href="cover.jpg" media-type="image/jpeg"></item>
  </manifest>
  <spine>
    <itemref idref="ch1"></itemref>
  </spine>
</package>"#;
        let data = parse_opf(opf).unwrap();
        assert_eq!(data.manifest.len(), 2);
        assert_eq!(data.spine_ids, vec!["ch1"]);
        assert_eq!(data.metadata.cover_image.as_deref(), Some("cover.jpg"));
    }

    #[test]
    fn parse_opf_keeps_text_after_nested_markup() {
        // Nested inline markup inside a DC element must not commit the value
        // early on the inner end tag (previously yielded "FooBar", losing
        // " Baz" and the internal spaces).
        let opf = r#"<package xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Foo <i>Bar</i> Baz</dc:title>
    <dc:creator>A <span>B</span></dc:creator>
  </metadata>
  <manifest/><spine/>
</package>"#;
        let data = parse_opf(opf).unwrap();
        assert_eq!(data.metadata.title, "Foo Bar Baz");
        assert_eq!(data.metadata.authors, vec!["A B"]);
    }

    #[test]
    fn parse_nav_toc_keeps_label_after_nested_span() {
        // A nested <span> inside the <a> label used to terminate collection
        // and drop everything after it ("Chapter1" instead of the full title).
        let nav = r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol>
  <li><a href="ch1.xhtml">Chapter <span>1</span>: The Start</a></li>
</ol></nav>
</body></html>"#;
        let toc = parse_nav_toc(nav).unwrap();
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Chapter 1: The Start");
        assert_eq!(toc[0].href, "ch1.xhtml");
    }

    #[test]
    fn parse_ncx_hoists_children_of_unlabeled_navpoint() {
        // A structural navPoint without its own label/src must not take its
        // whole subtree with it.
        let ncx = r#"<ncx><navMap>
  <navPoint>
    <navPoint><navLabel><text>One</text></navLabel><content src="a.xhtml"/></navPoint>
    <navPoint><navLabel><text>Two</text></navLabel><content src="b.xhtml"/></navPoint>
  </navPoint>
</navMap></ncx>"#;
        let toc = parse_ncx(ncx).unwrap();
        assert_eq!(toc.len(), 2);
        assert_eq!(toc[0].title, "One");
        assert_eq!(toc[1].title, "Two");
    }

    #[test]
    fn parse_ncx_rejects_pathological_nesting() {
        // ~5000 nested navPoints: without a depth cap the resulting TocEntry
        // tree would overflow the stack on its recursive Drop / consumers.
        let depth = 5000;
        let mut ncx = String::from("<ncx><navMap>");
        for _ in 0..depth {
            ncx.push_str("<navPoint><navLabel><text>x</text></navLabel><content src=\"a\"/>");
        }
        for _ in 0..depth {
            ncx.push_str("</navPoint>");
        }
        ncx.push_str("</navMap></ncx>");
        let err = parse_ncx(&ncx).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

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

        // Common named HTML entities
        assert_eq!(resolve_entity("nbsp"), Some("\u{00A0}".to_string()));
        assert_eq!(resolve_entity("mdash"), Some("\u{2014}".to_string()));
        assert_eq!(resolve_entity("ndash"), Some("\u{2013}".to_string()));
        assert_eq!(resolve_entity("hellip"), Some("\u{2026}".to_string()));
        assert_eq!(resolve_entity("rsquo"), Some("\u{2019}".to_string()));
        assert_eq!(resolve_entity("ldquo"), Some("\u{201C}".to_string()));
        assert_eq!(resolve_entity("copy"), Some("\u{00A9}".to_string()));
        assert_eq!(resolve_entity("trade"), Some("\u{2122}".to_string()));
        assert_eq!(resolve_entity("eacute"), Some("\u{00E9}".to_string()));
        assert_eq!(resolve_entity("Ccedil"), Some("\u{00C7}".to_string()));
        assert_eq!(resolve_entity("szlig"), Some("\u{00DF}".to_string()));

        // Case-sensitive: entity names must match exactly
        assert_eq!(resolve_entity("NBSP"), None);

        // Unknown entities are dropped (lenient best-effort parsing)
        assert_eq!(resolve_entity("invalid"), None);
        assert_eq!(resolve_entity(""), None);
    }

    #[test]
    fn test_parse_ncx_named_entities_in_labels() {
        let ncx = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="np1" playOrder="1">
      <navLabel><text>Ch&nbsp;1&mdash;Intro</text></navLabel>
      <content src="ch1.xhtml"/>
    </navPoint>
    <navPoint id="np2" playOrder="2">
      <navLabel><text>Caf&eacute; &amp; Society&hellip;</text></navLabel>
      <content src="ch2.xhtml"/>
    </navPoint>
  </navMap>
</ncx>"#;

        let result = parse_ncx(ncx).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title, "Ch\u{00A0}1\u{2014}Intro");
        assert_eq!(result[1].title, "Caf\u{00E9} & Society\u{2026}");
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
        container.extend_from_slice(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        );

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
        assert_eq!(
            result.metadata.publisher,
            Some("Test Publisher".to_string())
        );
        assert_eq!(
            result.metadata.description,
            Some("A test book description.".to_string())
        );
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
        assert_eq!(
            result.metadata.cover_image,
            Some("images/cover.jpg".to_string())
        );
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
    fn test_parse_nav_toc_flat() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="toc">
      <ol>
        <li><a href="text/ch1.xhtml">Chapter 1</a></li>
        <li><a href="text/ch2.xhtml#start">Chapter 2</a></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_toc(nav).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title, "Chapter 1");
        assert_eq!(result[0].href, "text/ch1.xhtml");
        assert_eq!(result[1].title, "Chapter 2");
        assert_eq!(result[1].href, "text/ch2.xhtml#start");
    }

    #[test]
    fn test_parse_nav_toc_nested() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="toc">
      <ol>
        <li><a href="part1.xhtml">Part I</a>
          <ol>
            <li><a href="ch1.xhtml">Chapter 1</a></li>
            <li><a href="ch2.xhtml">Chapter 2</a>
              <ol>
                <li><a href="ch2.xhtml#sec1">Section 1</a></li>
              </ol>
            </li>
          </ol>
        </li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_toc(nav).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Part I");
        assert_eq!(result[0].children.len(), 2);
        assert_eq!(result[0].children[0].title, "Chapter 1");
        assert_eq!(result[0].children[1].title, "Chapter 2");
        assert_eq!(result[0].children[1].children.len(), 1);
        assert_eq!(result[0].children[1].children[0].href, "ch2.xhtml#sec1");
    }

    #[test]
    fn test_parse_nav_toc_span_heading_and_ignores_other_navs() {
        // An unlinked <span> heading keeps its children; the landmarks nav
        // must not leak entries into the TOC.
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="landmarks">
      <ol>
        <li><a href="cover.xhtml" epub:type="cover">Cover</a></li>
      </ol>
    </nav>
    <nav epub:type="toc">
      <ol>
        <li><span>Front Matter</span>
          <ol>
            <li><a href="preface.xhtml">Preface</a></li>
          </ol>
        </li>
        <li><span>Empty heading, no children</span></li>
      </ol>
    </nav>
  </body>
</html>"#;

        let result = parse_nav_toc(nav).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Front Matter");
        assert_eq!(result[0].href, "");
        assert_eq!(result[0].children.len(), 1);
        assert_eq!(result[0].children[0].title, "Preface");
        assert_eq!(result[0].children[0].href, "preface.xhtml");
    }

    #[test]
    fn test_parse_nav_toc_no_toc_nav() {
        let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="landmarks">
      <ol><li><a href="cover.xhtml" epub:type="cover">Cover</a></li></ol>
    </nav>
  </body>
</html>"#;

        assert!(parse_nav_toc(nav).unwrap().is_empty());
    }

    #[test]
    fn test_parse_nav_toc_rejects_pathological_nesting() {
        let depth = 5000;
        let mut nav = String::from(
            r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc">"#,
        );
        for _ in 0..depth {
            nav.push_str("<ol><li><a href=\"a.xhtml\">x</a>");
        }
        for _ in 0..depth {
            nav.push_str("</li></ol>");
        }
        nav.push_str("</nav></body></html>");

        let err = parse_nav_toc(&nav).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
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
        assert_eq!(
            result.metadata.modified_date,
            Some("2024-01-15T12:00:00Z".to_string())
        );
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
        assert_eq!(
            result.metadata.contributors[0].role,
            Some("trl".to_string())
        );
        assert_eq!(
            result.metadata.contributors[0].file_as,
            Some("Translator, John".to_string())
        );
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
        assert_eq!(
            result.metadata.title_sort,
            Some("Great Adventure, The".to_string())
        );
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
        let translator = result
            .metadata
            .contributors
            .iter()
            .find(|c| c.role == Some("trl".to_string()));
        assert!(translator.is_some());
        assert_eq!(translator.unwrap().name, "Translator Name");

        // Find editor
        let editor = result
            .metadata
            .contributors
            .iter()
            .find(|c| c.role == Some("edt".to_string()));
        assert!(editor.is_some());
        assert_eq!(editor.unwrap().name, "Editor Name");
    }

    #[test]
    fn test_parse_opf_multiple_titles_uses_first() {
        let opf = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title id="title1">The First Title</dc:title>
    <dc:title id="title2">The Second Title</dc:title>
    <dc:title id="title3">The Third Title</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"##;

        let result = parse_opf(opf).unwrap();
        // Should only use the first title
        assert_eq!(result.metadata.title, "The First Title");
    }
}
