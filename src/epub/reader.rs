use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::Path;
use zip::ZipArchive;

use crate::book::{Book, Metadata, TocEntry};
use crate::error::{Error, Result};

/// Parsed OPF content
struct OpfData {
    metadata: Metadata,
    /// Maps manifest id -> (href, media_type)
    manifest: HashMap<String, (String, String)>,
    spine_ids: Vec<String>,
    ncx_href: Option<String>,
}

/// Read an EPUB file from disk into a [`Book`].
///
/// Supports EPUB 2 and EPUB 3 formats. Extracts metadata, spine, table of contents,
/// and all resources (content documents, images, CSS, fonts).
///
/// # Example
///
/// ```no_run
/// use boko::read_epub;
///
/// let book = read_epub("path/to/book.epub")?;
/// println!("Title: {}", book.metadata.title);
/// # Ok::<(), boko::Error>(())
/// ```
pub fn read_epub<P: AsRef<Path>>(path: P) -> Result<Book> {
    let file = std::fs::File::open(path)?;
    read_epub_from_reader(file)
}

/// Read an EPUB from any [`Read`] + [`Seek`] source.
///
/// Useful for reading from memory buffers or network streams.
///
/// # Example
///
/// ```no_run
/// use std::io::Cursor;
/// use boko::epub::read_epub_from_reader;
///
/// let epub_data: Vec<u8> = std::fs::read("book.epub")?;
/// let book = read_epub_from_reader(Cursor::new(epub_data))?;
/// # Ok::<(), boko::Error>(())
/// ```
pub fn read_epub_from_reader<R: Read + Seek>(reader: R) -> Result<Book> {
    let mut archive = ZipArchive::new(reader)?;

    // 1. Find the OPF file path from container.xml
    let opf_path = find_opf_path(&mut archive)?;
    let opf_dir = Path::new(&opf_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // 2. Parse the OPF file
    let opf_content = read_archive_file(&mut archive, &opf_path)?;
    let OpfData {
        metadata,
        manifest,
        spine_ids,
        ncx_href,
    } = parse_opf(&opf_content, &opf_dir)?;

    // 3. Build the Book structure
    let mut book = Book::new();
    book.metadata = metadata;

    // 4. Load all resources from manifest
    for (href, media_type) in manifest.values() {
        let full_path = resolve_path(&opf_dir, href);
        if let Ok(data) = read_archive_file_bytes(&mut archive, &full_path) {
            book.add_resource(href.clone(), data, media_type.clone());
        }
    }

    // 5. Build spine from spine IDs
    for id in spine_ids {
        if let Some((href, media_type)) = manifest.get(&id) {
            book.add_spine_item(&id, href.clone(), media_type.clone());
        }
    }

    // 6. Parse NCX for table of contents (if present)
    if let Some(ncx_href) = ncx_href {
        let ncx_path = resolve_path(&opf_dir, &ncx_href);
        if let Ok(ncx_content) = read_archive_file(&mut archive, &ncx_path) {
            book.toc = parse_ncx(&ncx_content)?;
        }
    }

    Ok(book)
}

fn find_opf_path<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<String> {
    let container = read_archive_file(archive, "META-INF/container.xml")?;

    let mut reader = Reader::from_str(&container);
    reader.config_mut().trim_text(true);

    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"rootfile" => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"full-path" {
                        return Ok(String::from_utf8(attr.value.to_vec())?);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e)),
            _ => {}
        }
    }

    Err(Error::InvalidEpub(
        "No rootfile found in container.xml".into(),
    ))
}

/// Manifest item with properties (for EPUB3 cover-image detection)
struct ManifestItem {
    href: String,
    media_type: String,
    properties: Option<String>,
}

fn parse_opf(content: &str, _opf_dir: &str) -> Result<OpfData> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut metadata = Metadata::default();
    let mut manifest_items: HashMap<String, ManifestItem> = HashMap::new();
    let mut spine_ids: Vec<String> = Vec::new();
    let mut ncx_href: Option<String> = None;
    let mut toc_id: Option<String> = None;
    let mut epub2_cover_id: Option<String> = None;

    let mut in_metadata = false;
    let mut current_element: Option<String> = None;
    let mut buf_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local_name = local_name(name.as_ref());

                match local_name {
                    b"metadata" => in_metadata = true,
                    b"title" | b"creator" | b"language" | b"identifier" | b"publisher"
                    | b"description" | b"subject" | b"date" | b"rights" => {
                        if in_metadata {
                            current_element = Some(String::from_utf8_lossy(local_name).to_string());
                            buf_text.clear();
                        }
                    }
                    b"spine" => {
                        // Get toc attribute for NCX reference
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"toc" {
                                toc_id = Some(String::from_utf8(attr.value.to_vec())?);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local_name = local_name(name.as_ref());

                match local_name {
                    b"item" => {
                        let mut id = String::new();
                        let mut href = String::new();
                        let mut media_type = String::new();
                        let mut properties: Option<String> = None;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = String::from_utf8(attr.value.to_vec())?,
                                b"href" => href = String::from_utf8(attr.value.to_vec())?,
                                b"media-type" => {
                                    media_type = String::from_utf8(attr.value.to_vec())?
                                }
                                b"properties" => {
                                    properties = Some(String::from_utf8(attr.value.to_vec())?)
                                }
                                _ => {}
                            }
                        }

                        if !id.is_empty() {
                            manifest_items.insert(
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
                                spine_ids.push(String::from_utf8(attr.value.to_vec())?);
                            }
                        }
                    }
                    b"meta" => {
                        // Handle EPUB2 cover image meta
                        let mut is_cover = false;
                        let mut cover_id = String::new();

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"name" if attr.value.as_ref() == b"cover" => is_cover = true,
                                b"content" => cover_id = String::from_utf8(attr.value.to_vec())?,
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
                    let raw = String::from_utf8_lossy(e.as_ref());
                    buf_text.push_str(&raw);
                }
            }
            Ok(Event::GeneralRef(e)) => {
                // Handle entity references like &apos; &lt; etc
                if current_element.is_some() {
                    let entity = String::from_utf8_lossy(e.as_ref());
                    let resolved = match entity.as_ref() {
                        "apos" => "'",
                        "quot" => "\"",
                        "lt" => "<",
                        "gt" => ">",
                        "amp" => "&",
                        _ => "",
                    };
                    buf_text.push_str(resolved);
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local_name = local_name(name.as_ref());

                if local_name == b"metadata" {
                    in_metadata = false;
                }

                if let Some(ref elem) = current_element {
                    match elem.as_str() {
                        "title" => metadata.title = buf_text.clone(),
                        "creator" => metadata.authors.push(buf_text.clone()),
                        "language" => metadata.language = buf_text.clone(),
                        "identifier" => {
                            if metadata.identifier.is_empty() {
                                metadata.identifier = buf_text.clone();
                            }
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
            Err(e) => return Err(Error::Xml(e)),
            _ => {}
        }
    }

    // Detect cover image: EPUB3 "cover-image" property takes priority over EPUB2 meta
    // EPUB3: <item properties="cover-image" .../>
    let epub3_cover = manifest_items.values().find(|item| {
        item.properties
            .as_ref()
            .is_some_and(|props| props.split_ascii_whitespace().any(|p| p == "cover-image"))
    });

    if let Some(cover_item) = epub3_cover {
        metadata.cover_image = Some(cover_item.href.clone());
    } else if let Some(cover_id) = epub2_cover_id {
        // EPUB2 fallback: <meta name="cover" content="cover-image-id"/>
        if let Some(item) = manifest_items.get(&cover_id) {
            metadata.cover_image = Some(item.href.clone());
        }
    }

    // Convert manifest_items to simple (href, media_type) map
    let manifest: HashMap<String, (String, String)> = manifest_items
        .into_iter()
        .map(|(id, item)| (id, (item.href, item.media_type)))
        .collect();

    // Resolve NCX href from toc_id
    if let Some(toc_id) = toc_id
        && let Some((href, _)) = manifest.get(&toc_id)
    {
        ncx_href = Some(href.clone());
    }

    Ok(OpfData {
        metadata,
        manifest,
        spine_ids,
        ncx_href,
    })
}

fn parse_ncx(content: &str) -> Result<Vec<TocEntry>> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    // State for each navPoint level: (children, text, src, play_order)
    // We need to save/restore these when entering/exiting nested navPoints
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
                let name_bytes = name.as_ref();
                let local = local_name(name_bytes);
                match local {
                    b"navPoint" => {
                        // Extract playOrder attribute
                        let mut play_order = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"playOrder"
                                && let Ok(order_str) = String::from_utf8(attr.value.to_vec())
                            {
                                play_order = order_str.parse().ok();
                            }
                        }
                        // Push new state for this navPoint
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
                let name_bytes = name.as_ref();
                let local = local_name(name_bytes);
                if local == b"content" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"src"
                            && let Some(state) = stack.last_mut()
                        {
                            state.src = Some(String::from_utf8(attr.value.to_vec())?);
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
                // Handle entity references like &apos; &lt; etc
                if in_text && let Some(state) = stack.last_mut() {
                    let entity = String::from_utf8_lossy(e.as_ref());
                    let resolved = match entity.as_ref() {
                        "apos" => "'",
                        "quot" => "\"",
                        "lt" => "<",
                        "gt" => ">",
                        "amp" => "&",
                        _ => "",
                    };
                    match &mut state.text {
                        Some(existing) => existing.push_str(resolved),
                        None => state.text = Some(resolved.to_string()),
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let name_bytes = name.as_ref();
                let local = local_name(name_bytes);
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
            Err(e) => return Err(Error::Xml(e)),
            _ => {}
        }
    }

    Ok(stack.pop().map(|s| s.children).unwrap_or_default())
}

fn read_archive_file<R: Read + Seek>(archive: &mut ZipArchive<R>, path: &str) -> Result<String> {
    let bytes = read_archive_file_bytes(archive, path)?;
    // Strip UTF-8 BOM if present
    let bytes = strip_bom(&bytes);
    Ok(String::from_utf8(bytes.to_vec())?)
}

fn read_archive_file_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
) -> Result<Vec<u8>> {
    // Try direct lookup first
    match archive.by_name(path) {
        Ok(mut file) => {
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;
            return Ok(contents);
        }
        Err(zip::result::ZipError::FileNotFound) => {}
        Err(e) => return Err(e.into()),
    }

    // Fallback: try percent-decoded path (handles malformed EPUBs)
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| Error::InvalidEpub(format!("Invalid UTF-8 in path: {}", path)))?;

    let mut file = archive.by_name(&decoded)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(contents)
}

/// Strip UTF-8 BOM (byte order mark) if present
fn strip_bom(data: &[u8]) -> &[u8] {
    // UTF-8 BOM: EF BB BF
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &data[3..]
    } else {
        data
    }
}

fn resolve_path(base: &str, href: &str) -> String {
    if base.is_empty() {
        href.to_string()
    } else {
        format!("{}/{}", base, href)
    }
}

/// Extract local name from potentially namespaced XML name
fn local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .rposition(|&b| b == b':')
        .map(|i| &name[i + 1..])
        .unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_name() {
        assert_eq!(local_name(b"dc:title"), b"title");
        assert_eq!(local_name(b"title"), b"title");
        assert_eq!(local_name(b"opf:meta"), b"meta");
    }

    #[test]
    fn test_xml_entity_parsing() {
        let xml = r#"<text>Don&apos;t Stop</text>"#;
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut full_text = String::new();
        loop {
            match reader.read_event() {
                Ok(Event::Text(e)) => {
                    let raw = String::from_utf8_lossy(e.as_ref());
                    full_text.push_str(&raw);
                }
                Ok(Event::GeneralRef(e)) => {
                    let entity = String::from_utf8_lossy(e.as_ref());
                    let resolved = match entity.as_ref() {
                        "apos" => "'",
                        "quot" => "\"",
                        "lt" => "<",
                        "gt" => ">",
                        "amp" => "&",
                        _ => "",
                    };
                    full_text.push_str(resolved);
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
        }
        println!("Full text: {:?}", full_text);
        assert_eq!(full_text, "Don't Stop");
    }
}
