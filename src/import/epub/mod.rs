//! EPUB format importer.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zip::ZipArchive;

use crate::book::{Metadata, TocEntry};
use crate::import::{ChapterId, Importer, SpineEntry};
use crate::io::{ByteSource, ByteSourceCursor, FileSource};

/// EPUB format importer with random-access ZIP reading.
pub struct EpubImporter {
    /// Random-access byte source for the ZIP file.
    source: Arc<dyn ByteSource>,

    /// Cached ZIP entry locations: path -> (offset, compressed_size, compression_method).
    zip_index: HashMap<String, ZipEntryLoc>,

    /// Book metadata.
    metadata: Metadata,

    /// Table of contents.
    toc: Vec<TocEntry>,

    /// Reading order (spine).
    spine: Vec<SpineEntry>,

    /// Maps ChapterId -> ZIP path (e.g., "OEBPS/text/ch01.xhtml").
    spine_paths: Vec<String>,

    /// All asset paths in the ZIP.
    assets: Vec<PathBuf>,

    /// Base path of OPF file (e.g., "OEBPS/").
    opf_base: String,
}

#[derive(Clone, Copy)]
struct ZipEntryLoc {
    /// Offset to the compressed data within the ZIP file.
    data_offset: u64,
    /// Size of the compressed data.
    compressed_size: u64,
    /// Compression method (0 = Store, 8 = Deflate).
    compression: u16,
}

impl Importer for EpubImporter {
    fn open(path: &Path) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let source = Arc::new(FileSource::new(file)?);
        Self::from_source(source)
    }

    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn spine(&self) -> &[SpineEntry] {
        &self.spine
    }

    fn source_id(&self, id: ChapterId) -> Option<&str> {
        self.spine_paths.get(id.0 as usize).map(|s| s.as_str())
    }

    fn load_raw(&mut self, id: ChapterId) -> io::Result<Vec<u8>> {
        let path = self.spine_paths.get(id.0 as usize).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Chapter ID {} not found", id.0),
            )
        })?;
        self.read_zip_entry(path)
    }

    fn list_assets(&self) -> Vec<PathBuf> {
        self.assets.clone()
    }

    fn load_asset(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        let key = path.to_string_lossy().replace('\\', "/");
        self.read_zip_entry(&key)
    }
}

impl EpubImporter {
    /// Create an importer from a ByteSource.
    pub fn from_source(source: Arc<dyn ByteSource>) -> io::Result<Self> {
        // 1. Scan ZIP directory and cache entry locations
        let cursor = ByteSourceCursor::new(source.clone());
        let mut archive = ZipArchive::new(cursor)?;

        let mut zip_index = HashMap::new();
        let mut assets = Vec::new();

        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            let name = file.name().to_string();

            zip_index.insert(
                name.clone(),
                ZipEntryLoc {
                    data_offset: file.data_start(),
                    compressed_size: file.compressed_size(),
                    compression: compression_to_u16(file.compression()),
                },
            );
            assets.push(PathBuf::from(name));
        }

        // 2. Find OPF path from container.xml
        let container_bytes = read_zip_entry_raw(&source, &zip_index, "META-INF/container.xml")?;
        let opf_path = parse_container_xml(&container_bytes)?;
        let opf_base = Path::new(&opf_path)
            .parent()
            .map(|p| {
                let s = p.to_string_lossy();
                if s.is_empty() {
                    String::new()
                } else {
                    format!("{}/", s)
                }
            })
            .unwrap_or_default();

        // 3. Parse OPF
        let opf_bytes = read_zip_entry_raw(&source, &zip_index, &opf_path)?;
        let opf_str = String::from_utf8(strip_bom(&opf_bytes).to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let opf = parse_opf(&opf_str, &opf_base)?;

        // 4. Build spine
        let mut spine = Vec::new();
        let mut spine_paths = Vec::new();

        for (i, spine_id) in opf.spine_ids.iter().enumerate() {
            if let Some((href, _media_type)) = opf.manifest.get(spine_id) {
                let full_path = format!("{}{}", opf_base, href);
                let size_estimate = zip_index
                    .get(&full_path)
                    .map(|loc| loc.compressed_size as usize)
                    .unwrap_or(0);

                spine.push(SpineEntry {
                    id: ChapterId(i as u32),
                    size_estimate,
                });
                spine_paths.push(full_path);
            }
        }

        // 5. Parse TOC (NCX)
        let toc = if let Some(ncx_href) = &opf.ncx_href {
            let ncx_path = format!("{}{}", opf_base, ncx_href);
            if let Ok(ncx_bytes) = read_zip_entry_raw(&source, &zip_index, &ncx_path) {
                let ncx_str = String::from_utf8(strip_bom(&ncx_bytes).to_vec())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                parse_ncx(&ncx_str)?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            source,
            zip_index,
            metadata: opf.metadata,
            toc,
            spine,
            spine_paths,
            assets,
            opf_base,
        })
    }

    /// Read and decompress a ZIP entry.
    fn read_zip_entry(&self, path: &str) -> io::Result<Vec<u8>> {
        read_zip_entry_raw(&self.source, &self.zip_index, path)
    }
}

// ----------------------------------------------------------------------------
// ZIP Helpers
// ----------------------------------------------------------------------------

fn read_zip_entry_raw(
    source: &Arc<dyn ByteSource>,
    index: &HashMap<String, ZipEntryLoc>,
    path: &str,
) -> io::Result<Vec<u8>> {
    let loc = index.get(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("File not found in ZIP: {}", path),
        )
    })?;

    // Read compressed data
    let compressed = source.read_at(loc.data_offset, loc.compressed_size as usize)?;

    // Decompress
    match loc.compression {
        0 => Ok(compressed), // Stored (no compression)
        8 => {
            // Deflate
            let mut decoder = flate2::read::DeflateDecoder::new(&compressed[..]);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out)?;
            Ok(out)
        }
        method => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("Unsupported compression method: {}", method),
        )),
    }
}

fn strip_bom(data: &[u8]) -> &[u8] {
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &data[3..]
    } else {
        data
    }
}

// ----------------------------------------------------------------------------
// XML Parsing (adapted from epub/reader.rs)
// ----------------------------------------------------------------------------

use quick_xml::events::Event;
use quick_xml::Reader;

fn parse_container_xml(bytes: &[u8]) -> io::Result<String> {
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

struct OpfData {
    metadata: Metadata,
    manifest: HashMap<String, (String, String)>, // id -> (href, media_type)
    spine_ids: Vec<String>,
    ncx_href: Option<String>,
}

fn parse_opf(content: &str, _opf_base: &str) -> io::Result<OpfData> {
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
                            current_element =
                                Some(String::from_utf8_lossy(local).to_string());
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

    // Detect cover image
    let epub3_cover = manifest.values().find(|item| {
        item.properties
            .as_ref()
            .is_some_and(|props| props.split_ascii_whitespace().any(|p| p == "cover-image"))
    });

    if let Some(cover_item) = epub3_cover {
        metadata.cover_image = Some(cover_item.href.clone());
    } else if let Some(cover_id) = epub2_cover_id {
        if let Some(item) = manifest.get(&cover_id) {
            metadata.cover_image = Some(item.href.clone());
        }
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

struct ManifestItem {
    href: String,
    media_type: String,
    properties: Option<String>,
}

fn parse_ncx(content: &str) -> io::Result<Vec<TocEntry>> {
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

fn local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .rposition(|&b| b == b':')
        .map(|i| &name[i + 1..])
        .unwrap_or(name)
}

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
    } else if let Some(dec) = entity.strip_prefix('#') {
        if let Ok(code) = dec.parse::<u32>()
            && let Some(c) = char::from_u32(code)
        {
            return Some(c.to_string());
        }
    }

    None
}

fn compression_to_u16(method: zip::CompressionMethod) -> u16 {
    match method {
        zip::CompressionMethod::Stored => 0,
        zip::CompressionMethod::Deflated => 8,
        _ => 255, // Unknown
    }
}
