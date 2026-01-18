//! KFX container and content reader.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use crate::book::{Book, Metadata, Resource, SpineItem, TocEntry};

use super::ion::{ION_MAGIC, IonParser, IonValue};

/// KFX container magic
const CONTAINER_MAGIC: &[u8; 4] = b"CONT";

/// Entity magic
const ENTITY_MAGIC: &[u8; 4] = b"ENTY";

// Known entity type IDs
const ENTITY_TYPE_TEXT_CONTENT: u32 = 145;
const ENTITY_TYPE_DOCUMENT_DATA: u32 = 258;
const ENTITY_TYPE_SECTION: u32 = 260;
const ENTITY_TYPE_RESOURCE: u32 = 417;
const ENTITY_TYPE_METADATA: u32 = 490;
const ENTITY_TYPE_RESOURCE_INFO: u32 = 164;

// Known property symbol IDs
const PROP_TEXT_ARRAY: u64 = 146;
const PROP_METADATA_ENTRIES: u64 = 491;
const PROP_METADATA_KEY: u64 = 492;
const PROP_METADATA_VALUE: u64 = 307;
const PROP_DOC_DATA: u64 = 258;
const PROP_RESOURCE_PATH: u64 = 165;
const PROP_MIME_TYPE: u64 = 162;

/// KFX entity entry from container header
#[derive(Debug)]
struct EntityEntry {
    id: u32,
    entity_type: u32,
    offset: u64,
    length: u64,
}

/// Parsed KFX container
struct KfxContainer {
    #[allow(dead_code)]
    version: u16,
    header_len: u32,
    entities: Vec<EntityEntry>,
    data: Vec<u8>,
}

/// Read a KFX file and convert to Book
pub fn read_kfx(path: impl AsRef<Path>) -> io::Result<Book> {
    let file = File::open(path)?;
    read_kfx_from_reader(BufReader::new(file))
}

/// Read a KFX from any reader and convert to Book
pub fn read_kfx_from_reader<R: Read>(mut reader: R) -> io::Result<Book> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;

    let container = parse_container(&data)?;
    convert_to_book(&container)
}

/// Parse the KFX container structure
fn parse_container(data: &[u8]) -> io::Result<KfxContainer> {
    if data.len() < 10 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "KFX file too small",
        ));
    }

    // Check magic
    if &data[0..4] != CONTAINER_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Not a KFX file (invalid magic)",
        ));
    }

    // Parse header
    let version = u16::from_le_bytes([data[4], data[5]]);
    let header_len = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);

    // Skip 8 unknown bytes after header fields
    let mut pos = 18;

    // Read entity table until we hit ION magic
    let mut entities = Vec::new();
    while pos + 4 <= data.len() && &data[pos..pos + 4] != ION_MAGIC {
        if pos + 24 > data.len() {
            break;
        }

        let id = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let entity_type =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let offset = u64::from_le_bytes([
            data[pos + 8],
            data[pos + 9],
            data[pos + 10],
            data[pos + 11],
            data[pos + 12],
            data[pos + 13],
            data[pos + 14],
            data[pos + 15],
        ]);
        let length = u64::from_le_bytes([
            data[pos + 16],
            data[pos + 17],
            data[pos + 18],
            data[pos + 19],
            data[pos + 20],
            data[pos + 21],
            data[pos + 22],
            data[pos + 23],
        ]);

        entities.push(EntityEntry {
            id,
            entity_type,
            offset,
            length,
        });
        pos += 24;
    }

    Ok(KfxContainer {
        version,
        header_len,
        entities,
        data: data.to_vec(),
    })
}

/// Parse an entity's payload
fn parse_entity_payload(container: &KfxContainer, entry: &EntityEntry) -> io::Result<IonValue> {
    let start = container.header_len as usize + entry.offset as usize;
    let end = start + entry.length as usize;

    if end > container.data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "entity extends beyond file",
        ));
    }

    let entity_data = &container.data[start..end];

    // Check entity magic
    if entity_data.len() < 10 || &entity_data[0..4] != ENTITY_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid entity header",
        ));
    }

    // Get entity header length
    let ent_header_len = u32::from_le_bytes([
        entity_data[6],
        entity_data[7],
        entity_data[8],
        entity_data[9],
    ]) as usize;

    if ent_header_len > entity_data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "entity header length exceeds entity size",
        ));
    }

    let payload = &entity_data[ent_header_len..];

    // Check if payload is ION
    if payload.len() >= 4 && &payload[0..4] == ION_MAGIC {
        let mut parser = IonParser::new(payload);
        parser.parse()
    } else {
        // Binary data (like images)
        Ok(IonValue::Blob(payload.to_vec()))
    }
}

/// Convert parsed KFX container to Book
fn convert_to_book(container: &KfxContainer) -> io::Result<Book> {
    let mut book = Book::new();

    // Group entities by type
    let mut entities_by_type: HashMap<u32, Vec<&EntityEntry>> = HashMap::new();
    for entity in &container.entities {
        entities_by_type
            .entry(entity.entity_type)
            .or_default()
            .push(entity);
    }

    // Collect all text content (type 145)
    let mut text_by_id: HashMap<u32, Vec<String>> = HashMap::new();
    if let Some(text_entities) = entities_by_type.get(&ENTITY_TYPE_TEXT_CONTENT) {
        for entity in text_entities {
            if let Ok(value) = parse_entity_payload(container, entity) {
                if let Some(texts) = extract_text_content(&value) {
                    text_by_id.insert(entity.id, texts);
                }
            }
        }
    }

    // Extract metadata (type 490)
    if let Some(metadata_entities) = entities_by_type.get(&ENTITY_TYPE_METADATA) {
        for entity in metadata_entities {
            if let Ok(value) = parse_entity_payload(container, entity) {
                extract_metadata(&value, &mut book.metadata);
            }
        }
    }

    // Collect resource info (type 164) for path/mimetype mapping
    let mut resource_info: HashMap<u32, (String, String)> = HashMap::new();
    if let Some(info_entities) = entities_by_type.get(&ENTITY_TYPE_RESOURCE_INFO) {
        for entity in info_entities {
            if let Ok(value) = parse_entity_payload(container, entity) {
                if let Some(info) = extract_resource_info(&value) {
                    resource_info.insert(entity.id, info);
                }
            }
        }
    }

    // Extract resources (type 417)
    if let Some(resource_entities) = entities_by_type.get(&ENTITY_TYPE_RESOURCE) {
        for (i, entity) in resource_entities.iter().enumerate() {
            if let Ok(IonValue::Blob(data)) = parse_entity_payload(container, entity) {
                let (path, media_type) =
                    resource_info.get(&entity.id).cloned().unwrap_or_else(|| {
                        let ext = guess_extension(&data);
                        (
                            format!("resource/rsrc{}.{}", i, ext),
                            guess_media_type(&data),
                        )
                    });

                book.resources.insert(
                    path.clone(),
                    Resource {
                        data,
                        media_type: media_type.clone(),
                    },
                );

                // Track cover image (usually first image resource)
                if book.metadata.cover_image.is_none() && media_type.starts_with("image/") {
                    book.metadata.cover_image = Some(path);
                }
            }
        }
    }

    // Get section order from document data (type 258) - reserved for future use
    // when we implement proper section ordering
    if let Some(doc_entities) = entities_by_type.get(&ENTITY_TYPE_DOCUMENT_DATA) {
        if let Some(entity) = doc_entities.first() {
            let _ = parse_entity_payload(container, entity);
        }
    }

    // Track sections (type 260) - reserved for future use
    if let Some(section_entities) = entities_by_type.get(&ENTITY_TYPE_SECTION) {
        let _ = section_entities.len();
    }

    // Generate XHTML content for each section by finding associated text
    // For now, combine all text content into chapters based on the text entities
    for (i, (_text_id, texts)) in text_by_id.iter().enumerate() {
        let href = format!("chapter_{}.xhtml", i);

        // Build XHTML from text fragments
        let content = build_xhtml(&texts, &book.metadata.title);

        book.resources.insert(
            href.clone(),
            Resource {
                data: content.into_bytes(),
                media_type: "application/xhtml+xml".to_string(),
            },
        );

        book.spine.push(SpineItem {
            id: format!("chapter_{}", i),
            href: href.clone(),
            media_type: "application/xhtml+xml".to_string(),
            linear: true,
        });

        // Add to TOC
        let title = texts.first().map(|s| {
            let truncated: String = s.chars().take(50).collect();
            if truncated.len() < s.len() {
                format!("{}...", truncated)
            } else {
                truncated
            }
        });
        if let Some(title) = title {
            book.toc.push(TocEntry::new(title, href));
        }
    }

    // Set defaults if missing
    if book.metadata.title.is_empty() {
        book.metadata.title = "Unknown".to_string();
    }
    if book.metadata.language.is_empty() {
        book.metadata.language = "en".to_string();
    }
    if book.metadata.identifier.is_empty() {
        book.metadata.identifier = format!("kfx-{}", uuid_v4());
    }

    Ok(book)
}

/// Extract text content from entity value
fn extract_text_content(value: &IonValue) -> Option<Vec<String>> {
    let value = value.unwrap_annotated();

    if let Some(map) = value.as_struct() {
        if let Some(text_array) = map.get(&PROP_TEXT_ARRAY) {
            if let Some(items) = text_array.as_list() {
                let texts: Vec<String> = items
                    .iter()
                    .filter_map(|item| {
                        let item = item.unwrap_annotated();
                        item.as_string().map(|s| s.to_string())
                    })
                    .collect();
                if !texts.is_empty() {
                    return Some(texts);
                }
            }
        }
    }

    None
}

/// Extract metadata from entity value
fn extract_metadata(value: &IonValue, metadata: &mut Metadata) {
    let value = value.unwrap_annotated();

    if let Some(map) = value.as_struct() {
        // Look for metadata entries ($491)
        if let Some(entries) = map.get(&PROP_METADATA_ENTRIES) {
            if let Some(entries_list) = entries.as_list() {
                for entry in entries_list {
                    process_metadata_entry(entry, metadata);
                }
            }
        }
    }
}

fn process_metadata_entry(entry: &IonValue, metadata: &mut Metadata) {
    let entry = entry.unwrap_annotated();

    if let Some(map) = entry.as_struct() {
        // Look for $258 (document data array with key-value pairs)
        if let Some(doc_data) = map.get(&PROP_DOC_DATA) {
            if let Some(items) = doc_data.as_list() {
                for item in items {
                    let item = item.unwrap_annotated();
                    if let Some(item_map) = item.as_struct() {
                        let key = item_map
                            .get(&PROP_METADATA_KEY)
                            .and_then(|v| v.unwrap_annotated().as_string())
                            .unwrap_or("");
                        let val = item_map
                            .get(&PROP_METADATA_VALUE)
                            .map(|v| v.unwrap_annotated());

                        match key {
                            "title" => {
                                if let Some(s) = val.and_then(|v| v.as_string()) {
                                    metadata.title = s.to_string();
                                }
                            }
                            "author" => {
                                if let Some(s) = val.and_then(|v| v.as_string()) {
                                    if !metadata.authors.contains(&s.to_string()) {
                                        metadata.authors.push(s.to_string());
                                    }
                                }
                            }
                            "language" => {
                                if let Some(s) = val.and_then(|v| v.as_string()) {
                                    metadata.language = s.to_string();
                                }
                            }
                            "publisher" => {
                                if let Some(s) = val.and_then(|v| v.as_string()) {
                                    metadata.publisher = Some(s.to_string());
                                }
                            }
                            "description" => {
                                if let Some(s) = val.and_then(|v| v.as_string()) {
                                    metadata.description = Some(s.to_string());
                                }
                            }
                            "ASIN" | "content_id" => {
                                if let Some(s) = val.and_then(|v| v.as_string()) {
                                    if metadata.identifier.is_empty() {
                                        metadata.identifier = s.to_string();
                                    }
                                }
                            }
                            "issue_date" => {
                                if let Some(s) = val.and_then(|v| v.as_string()) {
                                    metadata.date = Some(s.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

/// Extract resource info (path and MIME type)
fn extract_resource_info(value: &IonValue) -> Option<(String, String)> {
    let value = value.unwrap_annotated();

    if let Some(map) = value.as_struct() {
        let path = map
            .get(&PROP_RESOURCE_PATH)
            .and_then(|v| v.unwrap_annotated().as_string())
            .unwrap_or("resource");
        let mime = map
            .get(&PROP_MIME_TYPE)
            .and_then(|v| v.unwrap_annotated().as_string())
            .unwrap_or("application/octet-stream");

        return Some((path.to_string(), mime.to_string()));
    }

    None
}

/// Build XHTML document from text fragments
fn build_xhtml(texts: &[String], title: &str) -> String {
    let mut html = String::new();
    html.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    html.push_str("<!DOCTYPE html>\n");
    html.push_str("<html xmlns=\"http://www.w3.org/1999/xhtml\">\n");
    html.push_str("<head>\n");
    html.push_str(&format!("  <title>{}</title>\n", escape_xml(title)));
    html.push_str("</head>\n");
    html.push_str("<body>\n");

    for text in texts {
        // Wrap each text fragment in a paragraph
        html.push_str(&format!("  <p>{}</p>\n", escape_xml(text)));
    }

    html.push_str("</body>\n");
    html.push_str("</html>\n");
    html
}

/// Escape XML special characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Guess file extension from data
fn guess_extension(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        "png"
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpg"
    } else if data.starts_with(b"GIF") {
        "gif"
    } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP" {
        "webp"
    } else {
        "bin"
    }
}

/// Guess MIME type from data
fn guess_media_type(data: &[u8]) -> String {
    if data.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        "image/png"
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if data.starts_with(b"GIF") {
        "image/gif"
    } else if data.starts_with(b"RIFF") && data.len() > 12 && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
    .to_string()
}

/// Generate a simple UUID v4
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // Simple PRNG for UUID generation
    let mut state = seed;
    let mut bytes = [0u8; 16];
    for byte in &mut bytes {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (state >> 56) as u8;
    }

    // Set version (4) and variant (RFC 4122)
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}
