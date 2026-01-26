//! KFX format importer.
//!
//! KFX is Amazon's Kindle Format 10, using Ion binary data format.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ion_rs::{AnyEncoding, Decoder, Element, ElementReader, MapCatalog, Reader, SharedSymbolTable};

use crate::book::{Metadata, TocEntry};
use crate::import::{ChapterId, Importer, SpineEntry};
use crate::io::{ByteSource, FileSource};
use crate::kfx::symbols::{KfxSymbol, KFX_SYMBOL_TABLE};

/// Ion 1.0 Binary Version Marker
const ION_BVM: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];

/// KFX format importer.
pub struct KfxImporter {
    /// Random-access byte source.
    source: Arc<dyn ByteSource>,

    /// Container header length (offset to entity data).
    #[allow(dead_code)]
    header_len: usize,

    /// Entity index: maps (type_id, entity_idx) -> EntityLoc
    entities: Vec<EntityLoc>,

    /// Document-specific symbols (extended symbol table).
    doc_symbols: Vec<String>,

    /// Book metadata.
    metadata: Metadata,

    /// Table of contents.
    toc: Vec<TocEntry>,

    /// Reading order (spine).
    spine: Vec<SpineEntry>,

    /// Section names for spine entries.
    section_names: Vec<String>,

    /// Storyline entity locations for spine entries.
    storyline_locs: Vec<EntityLoc>,

    /// Resources: name -> EntityLoc
    resources: HashMap<String, EntityLoc>,
}

#[derive(Clone, Copy, Debug)]
struct EntityLoc {
    #[allow(dead_code)]
    id: u32,
    type_id: u32,
    offset: usize,
    length: usize,
}


impl Importer for KfxImporter {
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
        self.section_names.get(id.0 as usize).map(|s| s.as_str())
    }

    fn load_raw(&mut self, id: ChapterId) -> io::Result<Vec<u8>> {
        let loc = self
            .storyline_locs
            .get(id.0 as usize)
            .copied()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Chapter not found"))?;

        self.read_entity(loc)
    }

    fn list_assets(&self) -> Vec<PathBuf> {
        // Only return location paths, not resource name aliases
        self.resources
            .keys()
            .filter(|k| k.starts_with("resource/"))
            .map(PathBuf::from)
            .collect()
    }

    fn load_asset(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        let name = path.to_string_lossy();
        let loc = self
            .resources
            .get(name.as_ref())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Asset not found"))?;

        self.read_entity(*loc)
    }
}

impl KfxImporter {
    /// Create an importer from a ByteSource.
    pub fn from_source(source: Arc<dyn ByteSource>) -> io::Result<Self> {
        // Read the entire file for now (KFX files are typically small)
        // TODO: Implement lazy loading for large files
        let data = source.read_at(0, source.len() as usize)?;

        // Verify KFX container format
        if data.len() < 18 || &data[0..4] != b"CONT" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not a valid KFX container",
            ));
        }

        // Parse container header
        let header_len = read_u32_le(&data, 6) as usize;
        let container_info_offset = read_u32_le(&data, 10) as usize;
        let container_info_length = read_u32_le(&data, 14) as usize;

        if container_info_offset + container_info_length > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid container info bounds",
            ));
        }

        // Parse container info to get index table and doc symbols locations
        let container_info = &data[container_info_offset..container_info_offset + container_info_length];
        let (index_offset, index_length) = parse_container_info_field(container_info, "bcIndexTabOffset", "bcIndexTabLength")?;
        let (doc_sym_offset, doc_sym_length) = parse_container_info_field(container_info, "bcDocSymbolOffset", "bcDocSymbolLength")
            .unwrap_or((0, 0));

        // Extract document-specific symbols
        let doc_symbols = if doc_sym_length > 0 && doc_sym_offset + doc_sym_length <= data.len() {
            extract_doc_symbols(&data[doc_sym_offset..doc_sym_offset + doc_sym_length])
        } else {
            Vec::new()
        };

        // Parse entity index table
        let entry_size = 24;
        let num_entries = index_length / entry_size;
        let mut entities = Vec::with_capacity(num_entries);

        for i in 0..num_entries {
            let entry_offset = index_offset + i * entry_size;
            if entry_offset + entry_size > data.len() {
                break;
            }

            let id = read_u32_le(&data, entry_offset);
            let type_id = read_u32_le(&data, entry_offset + 4);
            let offset = read_u64_le(&data, entry_offset + 8) as usize;
            let length = read_u64_le(&data, entry_offset + 16) as usize;

            entities.push(EntityLoc {
                id,
                type_id,
                offset: header_len + offset,
                length,
            });
        }

        let mut importer = Self {
            source,
            header_len,
            entities,
            doc_symbols,
            metadata: Metadata::default(),
            toc: Vec::new(),
            spine: Vec::new(),
            section_names: Vec::new(),
            storyline_locs: Vec::new(),
            resources: HashMap::new(),
        };

        // Parse metadata
        importer.parse_metadata(&data)?;

        // Parse navigation (TOC)
        importer.parse_navigation(&data)?;

        // Parse storylines (spine)
        importer.parse_spine(&data)?;

        // Index resources
        importer.index_resources(&data)?;

        // Resolve cover_image alias to resource path
        importer.resolve_cover_image();

        Ok(importer)
    }

    /// Read an entity's data.
    fn read_entity(&self, loc: EntityLoc) -> io::Result<Vec<u8>> {
        let data = self.source.read_at(0, self.source.len() as usize)?;

        if loc.offset + loc.length > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Entity out of bounds",
            ));
        }

        let entity_data = &data[loc.offset..loc.offset + loc.length];

        // Check for ENTY header
        if entity_data.len() >= 10 && &entity_data[0..4] == b"ENTY" {
            let enty_header_len = read_u32_le(entity_data, 6) as usize;
            if enty_header_len < entity_data.len() {
                return Ok(entity_data[enty_header_len..].to_vec());
            }
        }

        Ok(entity_data.to_vec())
    }

    /// Parse an entity as Ion and return the first element.
    fn parse_entity_ion(&self, data: &[u8], loc: EntityLoc) -> io::Result<Element> {
        if loc.offset + loc.length > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Entity out of bounds",
            ));
        }

        let entity_data = &data[loc.offset..loc.offset + loc.length];

        // Skip ENTY header if present
        let ion_data = if entity_data.len() >= 10 && &entity_data[0..4] == b"ENTY" {
            let enty_header_len = read_u32_le(entity_data, 6) as usize;
            &entity_data[enty_header_len..]
        } else {
            entity_data
        };

        parse_ion_element(ion_data, &self.doc_symbols)
    }

    /// Parse book metadata.
    fn parse_metadata(&mut self, data: &[u8]) -> io::Result<()> {
        // Find book_metadata entity
        let loc = self
            .entities
            .iter()
            .find(|e| e.type_id == KfxSymbol::BookMetadata as u32)
            .copied();

        if let Some(loc) = loc {
            let elem = self.parse_entity_ion(data, loc)?;

            if let Some(strukt) = elem.as_struct() {
                // Look for categorised_metadata
                for (name, value) in strukt.iter() {
                    if name.text() == Some("categorised_metadata") {
                        if let Some(list) = value.as_list() {
                            for category_elem in list.iter() {
                                if let Some(cat_struct) = category_elem.as_struct() {
                                    let category = cat_struct
                                        .get("category")
                                        .and_then(|v| v.as_string())
                                        .unwrap_or("");

                                    if category == "kindle_title_metadata" {
                                        if let Some(metadata_list) = cat_struct.get("metadata").and_then(|v| v.as_list()) {
                                            for meta in metadata_list.iter() {
                                                if let Some(meta_struct) = meta.as_struct() {
                                                    let key = meta_struct
                                                        .get("key")
                                                        .and_then(|v| v.as_string())
                                                        .unwrap_or("");
                                                    let value = meta_struct
                                                        .get("value")
                                                        .and_then(|v| v.as_string())
                                                        .unwrap_or("");

                                                    match key {
                                                        "title" => self.metadata.title = value.to_string(),
                                                        "author" => self.metadata.authors.push(value.to_string()),
                                                        "publisher" => self.metadata.publisher = Some(value.to_string()),
                                                        "language" => self.metadata.language = value.to_string(),
                                                        "description" => self.metadata.description = Some(value.to_string()),
                                                        "book_id" => self.metadata.identifier = value.to_string(),
                                                        "issue_date" => self.metadata.date = Some(value.to_string()),
                                                        "cover_image" => {
                                                            // cover_image can be a string or a list with a symbol reference
                                                            let value_elem = meta_struct.get("value");
                                                            if let Some(cover) = self.resolve_cover_value(value_elem) {
                                                                self.metadata.cover_image = Some(cover);
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
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Parse book navigation (TOC).
    fn parse_navigation(&mut self, data: &[u8]) -> io::Result<()> {
        // Find book_navigation entity
        let loc = self
            .entities
            .iter()
            .find(|e| e.type_id == KfxSymbol::BookNavigation as u32)
            .copied();

        if let Some(loc) = loc {
            let elem = self.parse_entity_ion(data, loc)?;

            // book_navigation is a list of reading orders
            if let Some(list) = elem.as_list() {
                for reading_order in list.iter() {
                    if let Some(ro_struct) = reading_order.as_struct() {
                        // Look for nav_containers
                        if let Some(containers) = ro_struct.get("nav_containers").and_then(|v| v.as_list()) {
                            for container in containers.iter() {
                                if let Some(container_struct) = container.as_struct() {
                                    // Check nav_type - we want "toc"
                                    let nav_type = container_struct
                                        .get("nav_type")
                                        .and_then(|v| v.as_symbol())
                                        .and_then(|s| s.text());

                                    if nav_type == Some("toc") {
                                        self.toc = Self::parse_nav_entries(container_struct);
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Recursively parse nav entries into a tree structure.
    fn parse_nav_entries(container: &ion_rs::Struct) -> Vec<TocEntry> {
        let mut entries = Vec::new();

        if let Some(entry_list) = container.get("entries").and_then(|v| v.as_list()) {
            for entry in entry_list.iter() {
                if let Some(entry_struct) = entry.as_struct() {
                    // Get label
                    let label = entry_struct
                        .get("representation")
                        .and_then(|v| v.as_struct())
                        .and_then(|s| s.get("label"))
                        .and_then(|v| v.as_string())
                        .unwrap_or("Untitled");

                    // Skip placeholder labels
                    if label == "heading-nav-unit" || label == "Untitled" {
                        continue;
                    }

                    // Get target position
                    let href = entry_struct
                        .get("target_position")
                        .and_then(|v| v.as_struct())
                        .and_then(|s| s.get("id"))
                        .and_then(|v| v.as_i64())
                        .map(|id| format!("#{}", id))
                        .unwrap_or_default();

                    // Recursively parse children
                    let children = Self::parse_nav_entries(entry_struct);

                    entries.push(TocEntry {
                        title: label.to_string(),
                        href,
                        children,
                        play_order: None,
                    });
                }
            }
        }

        entries
    }

    /// Parse spine from reading_orders → sections → page_templates → storylines.
    fn parse_spine(&mut self, data: &[u8]) -> io::Result<()> {
        // Step 1: Get reading_orders from document_data ($538) or metadata ($258)
        let section_names = self.get_reading_order_sections(data)?;

        // Step 2: Build a map of section_name → section entity
        let mut section_map: HashMap<String, EntityLoc> = HashMap::new();
        for loc in &self.entities {
            if loc.type_id == KfxSymbol::Section as u32 {
                if let Ok(elem) = self.parse_entity_ion(data, *loc) {
                    if let Some(strukt) = elem.as_struct() {
                        let name = strukt
                            .get("section_name")
                            .and_then(|v| v.as_symbol().and_then(|s| s.text()).or_else(|| v.as_string()))
                            .unwrap_or("");
                        if !name.is_empty() {
                            section_map.insert(name.to_string(), *loc);
                        }
                    }
                }
            }
        }

        // Step 3: Build a map of story_name → storyline entity
        let mut storyline_map: HashMap<String, EntityLoc> = HashMap::new();
        for loc in &self.entities {
            if loc.type_id == KfxSymbol::Storyline as u32 {
                if let Ok(elem) = self.parse_entity_ion(data, *loc) {
                    if let Some(strukt) = elem.as_struct() {
                        let name = strukt
                            .get("story_name")
                            .and_then(|v| v.as_symbol().and_then(|s| s.text()).or_else(|| v.as_string()))
                            .unwrap_or("");
                        if !name.is_empty() {
                            storyline_map.insert(name.to_string(), *loc);
                        }
                    }
                }
            }
        }

        // Step 4: For each section in reading order, get storylines from page_templates
        let mut spine_idx = 0u32;
        for section_name in section_names {
            if let Some(section_loc) = section_map.get(&section_name) {
                if let Ok(elem) = self.parse_entity_ion(data, *section_loc) {
                    if let Some(strukt) = elem.as_struct() {
                        if let Some(templates) = strukt.get("page_templates").and_then(|v| v.as_list()) {
                            for template in templates.iter() {
                                if let Some(tmpl_struct) = template.as_struct() {
                                    let story_name = tmpl_struct
                                        .get("story_name")
                                        .and_then(|v| v.as_symbol().and_then(|s| s.text()).or_else(|| v.as_string()))
                                        .unwrap_or("");

                                    if !story_name.is_empty() {
                                        if let Some(storyline_loc) = storyline_map.get(story_name) {
                                            self.section_names.push(format!("#{}", storyline_loc.id));
                                            self.storyline_locs.push(*storyline_loc);
                                            self.spine.push(SpineEntry {
                                                id: ChapterId(spine_idx),
                                                size_estimate: storyline_loc.length,
                                            });
                                            spine_idx += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Extract section names from reading_orders in document_data or metadata.
    fn get_reading_order_sections(&self, data: &[u8]) -> io::Result<Vec<String>> {
        // Try document_data ($538) first, then metadata ($258)
        let doc_data_loc = self.entities.iter()
            .find(|e| e.type_id == KfxSymbol::DocumentData as u32)
            .copied();

        let metadata_loc = self.entities.iter()
            .find(|e| e.type_id == KfxSymbol::Metadata as u32)
            .copied();

        for loc in [doc_data_loc, metadata_loc].into_iter().flatten() {
            if let Ok(elem) = self.parse_entity_ion(data, loc) {
                if let Some(strukt) = elem.as_struct() {
                    if let Some(orders) = strukt.get("reading_orders").and_then(|v| v.as_list()) {
                        for order in orders.iter() {
                            if let Some(order_struct) = order.as_struct() {
                                if let Some(sections) = order_struct.get("sections").and_then(|v| v.as_list()) {
                                    let mut section_names = Vec::new();
                                    for section in sections.iter() {
                                        let name = section.as_symbol()
                                            .and_then(|s| s.text())
                                            .or_else(|| section.as_string());
                                        if let Some(n) = name {
                                            section_names.push(n.to_string());
                                        }
                                    }
                                    if !section_names.is_empty() {
                                        return Ok(section_names);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Vec::new())
    }

    /// Resolve cover_image value which can be a string or list with symbol/string reference.
    fn resolve_cover_value(&self, value: Option<&Element>) -> Option<String> {
        let value = value?;

        // Format 1: Direct string
        if let Some(s) = value.as_string() {
            return Some(s.to_string());
        }

        // Format 2: List containing a symbol or string reference
        if let Some(list) = value.as_list() {
            if let Some(first) = list.iter().next() {
                // Try as symbol first
                if let Some(sym) = first.as_symbol() {
                    if let Some(text) = sym.text() {
                        return Some(text.to_string());
                    }
                }
                // Then try as string
                if let Some(s) = first.as_string() {
                    return Some(s.to_string());
                }
            }
        }

        None
    }

    /// Resolve cover_image alias to entity ID.
    fn resolve_cover_image(&mut self) {
        if let Some(alias) = &self.metadata.cover_image {
            // Look up resource by alias to get entity ID
            if let Some(loc) = self.resources.get(alias) {
                self.metadata.cover_image = Some(format!("#{}", loc.id));
            }
        }
    }

    /// Index external resources.
    fn index_resources(&mut self, data: &[u8]) -> io::Result<()> {
        for loc in &self.entities {
            if loc.type_id == KfxSymbol::ExternalResource as u32 {
                if let Ok(elem) = self.parse_entity_ion(data, *loc) {
                    if let Some(strukt) = elem.as_struct() {
                        // Use location as key (e.g., "resource/rsrc7")
                        let location = strukt
                            .get("location")
                            .and_then(|v| v.as_string())
                            .unwrap_or("");

                        // Also index by resource_name (e.g., "eF") for cover lookup
                        let name = strukt
                            .get("resource_name")
                            .and_then(|v| v.as_symbol())
                            .and_then(|s| s.text())
                            .unwrap_or("");

                        if !location.is_empty() {
                            self.resources.insert(location.to_string(), *loc);
                        }
                        if !name.is_empty() && name != location {
                            self.resources.insert(name.to_string(), *loc);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// --- Helper functions ---

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_u64_le(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

/// Parse container info to extract a pair of offset/length fields.
fn parse_container_info_field(data: &[u8], offset_field: &str, length_field: &str) -> io::Result<(usize, usize)> {
    let elem = parse_ion_element(data, &[])?;

    if let Some(strukt) = elem.as_struct() {
        let offset = strukt
            .get(offset_field)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("Missing {}", offset_field)))?;
        let length = strukt
            .get(length_field)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("Missing {}", length_field)))?;

        Ok((offset as usize, length as usize))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Container info is not a struct",
        ))
    }
}

/// Parse Ion data with KFX symbol table.
fn parse_ion_element(data: &[u8], doc_symbols: &[String]) -> io::Result<Element> {
    // Build symbol table
    let mut all_symbols: Vec<&str> = KFX_SYMBOL_TABLE[10..].to_vec();
    for sym in doc_symbols {
        all_symbols.push(sym.as_str());
    }

    let max_id = (848 + doc_symbols.len()) as i64;

    let mut catalog = MapCatalog::new();
    if let Ok(table) = SharedSymbolTable::new("YJ_symbols", 10, all_symbols.iter().copied()) {
        catalog.insert_table(table);
    }

    // Build preamble with symbol table import
    let preamble = build_symbol_table_preamble(max_id);
    let mut full_data = preamble;

    if data.len() >= 4 && data[0..4] == ION_BVM {
        full_data.extend_from_slice(&data[4..]);
    } else {
        full_data.extend_from_slice(data);
    }

    let mut reader = Reader::new(AnyEncoding.with_catalog(catalog), &full_data[..])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    for element in reader.elements() {
        match element {
            Ok(elem) => return Ok(elem),
            Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "No Ion elements found",
    ))
}

/// Build an Ion binary preamble that imports the KFX symbol table.
fn build_symbol_table_preamble(max_id: i64) -> Vec<u8> {
    use ion_rs::{ion_list, ion_struct, Element, ElementWriter, IntoAnnotatedElement, WriteConfig, Writer};
    use ion_rs::v1_0::Binary;

    let import = ion_struct! {
        "name": "YJ_symbols",
        "version": 10i64,
        "max_id": max_id,
    };

    let symbol_table: Element = ion_struct! {
        "imports": ion_list![import],
    }
    .with_annotations(["$ion_symbol_table"]);

    let buffer = Vec::new();
    let mut writer = Writer::new(WriteConfig::<Binary>::new(), buffer).unwrap();
    writer.write_element(&symbol_table).unwrap();
    writer.close().unwrap()
}

/// Extract document-specific symbols from the doc symbols section.
fn extract_doc_symbols(data: &[u8]) -> Vec<String> {
    let mut symbols = Vec::new();

    let start = if data.len() >= 4 && data[0..4] == ION_BVM { 4 } else { 0 };

    let mut i = start;
    while i < data.len() {
        let type_byte = data[i];
        let type_code = (type_byte >> 4) & 0x0F;

        // Type 8 = string
        if type_code == 8 {
            let len_nibble = type_byte & 0x0F;
            let (str_len, header_len) = if len_nibble == 14 {
                if i + 1 < data.len() {
                    let len = data[i + 1] as usize;
                    if len & 0x80 == 0 {
                        (len, 2)
                    } else {
                        ((len & 0x7F), 2)
                    }
                } else {
                    break;
                }
            } else if len_nibble == 15 {
                i += 1;
                continue;
            } else {
                (len_nibble as usize, 1)
            };

            if i + header_len + str_len <= data.len() {
                let str_bytes = &data[i + header_len..i + header_len + str_len];
                if let Ok(s) = std::str::from_utf8(str_bytes) {
                    if !s.starts_with("YJ_symbols") && !s.is_empty() && !s.contains("version") {
                        symbols.push(s.to_string());
                    }
                }
                i += header_len + str_len;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    symbols.retain(|s| seen.insert(s.clone()));

    symbols
}
