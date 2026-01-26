//! KFX format importer.
//!
//! KFX is Amazon's Kindle Format 10, using Ion binary data format.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::book::{Metadata, TocEntry};
use crate::import::{ChapterId, Importer, SpineEntry};
use crate::io::{ByteSource, FileSource};
use crate::kfx::ion::{IonParser, IonValue, ION_MAGIC};
use crate::kfx::symbols::{KfxSymbol, KFX_SYMBOL_TABLE};

/// Shorthand for getting a KfxSymbol as u32 for field lookups.
macro_rules! sym {
    ($variant:ident) => {
        KfxSymbol::$variant as u32
    };
}

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

    /// Cache: section name -> storyline EntityLoc (lazily populated)
    section_storylines: HashMap<String, EntityLoc>,
    /// Whether section→storyline mapping has been built
    section_storylines_indexed: bool,

    /// Resources: name -> EntityLoc (lazily populated)
    resources: HashMap<String, EntityLoc>,
    /// Whether resources have been indexed
    resources_indexed: bool,
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
        let section_name = self
            .section_names
            .get(id.0 as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Chapter not found"))?
            .clone();

        // Find section entity and resolve to storyline
        let storyline_loc = self.resolve_section_to_storyline(&section_name)?;
        self.read_entity(storyline_loc)
    }

    fn list_assets(&self) -> Vec<PathBuf> {
        // Return entity IDs for bcRawMedia (actual asset data)
        self.entities
            .iter()
            .filter(|e| e.type_id == KfxSymbol::Bcrawmedia as u32)
            .map(|e| PathBuf::from(format!("#{}", e.id)))
            .collect()
    }

    fn load_asset(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        // Ensure resources are indexed
        if !self.resources_indexed {
            self.index_resources()?;
        }

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
        // Read container header (18 bytes)
        let header = source.read_at(0, 18)?;
        if &header[0..4] != b"CONT" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not a valid KFX container",
            ));
        }

        let header_len = read_u32_le(&header, 6) as usize;
        let container_info_offset = read_u32_le(&header, 10) as usize;
        let container_info_length = read_u32_le(&header, 14) as usize;

        // Read container info
        let container_info = source.read_at(container_info_offset as u64, container_info_length)?;
        let (index_offset, index_length) =
            parse_container_info_field(&container_info, "bcIndexTabOffset", "bcIndexTabLength")?;
        let (doc_sym_offset, doc_sym_length) =
            parse_container_info_field(&container_info, "bcDocSymbolOffset", "bcDocSymbolLength")
                .unwrap_or((0, 0));

        // Read and parse document symbols
        let doc_symbols = if doc_sym_length > 0 {
            let doc_sym_data = source.read_at(doc_sym_offset as u64, doc_sym_length)?;
            extract_doc_symbols(&doc_sym_data)
        } else {
            Vec::new()
        };

        // Read and parse index table
        let index_data = source.read_at(index_offset as u64, index_length)?;
        let entry_size = 24;
        let num_entries = index_length / entry_size;
        let mut entities = Vec::with_capacity(num_entries);

        for i in 0..num_entries {
            let entry_offset = i * entry_size;
            if entry_offset + entry_size > index_data.len() {
                break;
            }

            let id = read_u32_le(&index_data, entry_offset);
            let type_id = read_u32_le(&index_data, entry_offset + 4);
            let offset = read_u64_le(&index_data, entry_offset + 8) as usize;
            let length = read_u64_le(&index_data, entry_offset + 16) as usize;

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
            section_storylines: HashMap::new(),
            section_storylines_indexed: false,
            resources: HashMap::new(),
            resources_indexed: false,
        };

        // Parse metadata (only reads needed entities)
        importer.parse_metadata()?;

        // Parse navigation (TOC)
        importer.parse_navigation()?;

        // Build section→storyline map (needed for spine sizes and load_raw)
        importer.index_section_storylines()?;

        // Parse spine from reading order (uses section→storyline map for sizes)
        importer.parse_spine()?;

        Ok(importer)
    }

    /// Read an entity's raw data (after ENTY header).
    fn read_entity(&self, loc: EntityLoc) -> io::Result<Vec<u8>> {
        let entity_data = self.source.read_at(loc.offset as u64, loc.length)?;

        // Skip ENTY header if present
        if entity_data.len() >= 10 && &entity_data[0..4] == b"ENTY" {
            let enty_header_len = read_u32_le(&entity_data, 6) as usize;
            if enty_header_len < entity_data.len() {
                return Ok(entity_data[enty_header_len..].to_vec());
            }
        }

        Ok(entity_data)
    }

    /// Parse an entity as Ion and return the parsed value.
    fn parse_entity_ion(&self, loc: EntityLoc) -> io::Result<IonValue> {
        let ion_data = self.read_entity(loc)?;
        let mut parser = IonParser::new(&ion_data);
        parser.parse()
    }

    /// Resolve a symbol ID to its text representation.
    fn resolve_symbol(&self, id: u32) -> Option<&str> {
        let id = id as usize;
        if id < KFX_SYMBOL_TABLE.len() {
            Some(KFX_SYMBOL_TABLE[id])
        } else {
            // Document-local symbols start after the base symbol table
            let doc_idx = id.saturating_sub(KFX_SYMBOL_TABLE.len());
            self.doc_symbols.get(doc_idx).map(|s| s.as_str())
        }
    }

    /// Get a symbol's text from an IonValue (handles both Symbol and String).
    fn get_symbol_text<'a>(&'a self, value: &'a IonValue) -> Option<&'a str> {
        match value {
            IonValue::Symbol(id) => self.resolve_symbol(*id),
            IonValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Parse book metadata.
    fn parse_metadata(&mut self) -> io::Result<()> {
        // Find book_metadata entity
        let loc = self
            .entities
            .iter()
            .find(|e| e.type_id == KfxSymbol::BookMetadata as u32)
            .copied();

        if let Some(loc) = loc {
            let elem = self.parse_entity_ion(loc)?;

            if let Some(fields) = elem.as_struct() {
                // Look for categorised_metadata
                if let Some(cat_meta) = get_field(fields, sym!(CategorisedMetadata)) {
                    if let Some(list) = cat_meta.as_list() {
                        for category_elem in list {
                            if let Some(cat_fields) = category_elem.as_struct() {
                                let category = get_field(cat_fields, sym!(Category))
                                    .and_then(|v| self.get_symbol_text(v))
                                    .unwrap_or("");

                                if category == "kindle_title_metadata" {
                                    if let Some(metadata_list) =
                                        get_field(cat_fields, sym!(Metadata)).and_then(|v| v.as_list())
                                    {
                                        for meta in metadata_list {
                                            if let Some(meta_fields) = meta.as_struct() {
                                                let key = get_field(meta_fields, sym!(Key))
                                                    .and_then(|v| v.as_string())
                                                    .unwrap_or("");
                                                let value = get_field(meta_fields, sym!(Value))
                                                    .and_then(|v| v.as_string())
                                                    .unwrap_or("");

                                                match key {
                                                    "title" => {
                                                        self.metadata.title = value.to_string()
                                                    }
                                                    "author" => {
                                                        self.metadata.authors.push(value.to_string())
                                                    }
                                                    "publisher" => {
                                                        self.metadata.publisher =
                                                            Some(value.to_string())
                                                    }
                                                    "language" => {
                                                        self.metadata.language = value.to_string()
                                                    }
                                                    "description" => {
                                                        self.metadata.description =
                                                            Some(value.to_string())
                                                    }
                                                    "book_id" => {
                                                        self.metadata.identifier = value.to_string()
                                                    }
                                                    "issue_date" => {
                                                        self.metadata.date = Some(value.to_string())
                                                    }
                                                    "cover_image" => {
                                                        let value_elem =
                                                            get_field(meta_fields, sym!(Value));
                                                        if let Some(cover) =
                                                            self.resolve_cover_value(value_elem)
                                                        {
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

        Ok(())
    }

    /// Parse book navigation (TOC).
    fn parse_navigation(&mut self) -> io::Result<()> {
        // Find book_navigation entity
        let loc = self
            .entities
            .iter()
            .find(|e| e.type_id == KfxSymbol::BookNavigation as u32)
            .copied();

        if let Some(loc) = loc {
            let elem = self.parse_entity_ion(loc)?;

            // book_navigation is a list of reading orders
            if let Some(list) = elem.as_list() {
                for reading_order in list {
                    if let Some(ro_fields) = reading_order.as_struct() {
                        // Look for nav_containers
                        if let Some(containers) =
                            get_field(ro_fields, sym!(NavContainers)).and_then(|v| v.as_list())
                        {
                            for container in containers {
                                // Unwrap annotation if present
                                let inner = container.unwrap_annotated();
                                if let Some(container_fields) = inner.as_struct() {
                                    // Check nav_type - we want "toc"
                                    let nav_type = get_field(container_fields, sym!(NavType))
                                        .and_then(|v| self.get_symbol_text(v));

                                    if nav_type == Some("toc") {
                                        self.toc = self.parse_nav_entries(container_fields);
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
    fn parse_nav_entries(&self, container: &[(u32, IonValue)]) -> Vec<TocEntry> {
        let mut entries = Vec::new();

        if let Some(entry_list) = get_field(container, sym!(Entries)).and_then(|v| v.as_list()) {
            for entry in entry_list {
                // Unwrap annotation if present (nav_unit::...)
                let inner = entry.unwrap_annotated();
                if let Some(entry_fields) = inner.as_struct() {
                    // Get label (try representation.label first, then direct label)
                    let label = get_field(entry_fields, sym!(Representation))
                        .and_then(|v| v.as_struct())
                        .and_then(|s| get_field(s, sym!(Label)))
                        .and_then(|v| v.as_string())
                        .or_else(|| {
                            get_field(entry_fields, sym!(Label)).and_then(|v| v.as_string())
                        })
                        .unwrap_or("Untitled");

                    // Skip placeholder labels
                    if label == "heading-nav-unit" || label == "Untitled" {
                        continue;
                    }

                    // Get target position (includes id and optionally section_name)
                    let target_pos = get_field(entry_fields, sym!(TargetPosition))
                        .and_then(|v| v.as_struct());
                    let href = target_pos
                        .and_then(|s| get_field(s, sym!(Id)))
                        .and_then(|v| v.as_int())
                        .map(|id| format!("#{}", id))
                        .unwrap_or_default();

                    // Recursively parse children
                    let children = self.parse_nav_entries(entry_fields);

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

    /// Parse spine from reading_orders.
    ///
    /// Uses the section→storyline cache to get size estimates.
    fn parse_spine(&mut self) -> io::Result<()> {
        let section_names = self.get_reading_order_sections()?;

        for (idx, name) in section_names.into_iter().enumerate() {
            // Get size from cached storyline location
            let size_estimate = self
                .section_storylines
                .get(&name)
                .map(|loc| loc.length)
                .unwrap_or(0);

            self.section_names.push(name);
            self.spine.push(SpineEntry {
                id: ChapterId(idx as u32),
                size_estimate,
            });
        }

        Ok(())
    }

    /// Resolve a section name to its storyline entity location.
    fn resolve_section_to_storyline(&self, section_name: &str) -> io::Result<EntityLoc> {
        self.section_storylines
            .get(section_name)
            .copied()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Could not resolve section: {}", section_name),
                )
            })
    }

    /// Build the section name → storyline location cache.
    fn index_section_storylines(&mut self) -> io::Result<()> {
        if self.section_storylines_indexed {
            return Ok(());
        }

        // First, build a map of story_name → storyline EntityLoc
        let mut storyline_map: HashMap<String, EntityLoc> = HashMap::new();
        for loc in &self.entities {
            if loc.type_id == KfxSymbol::Storyline as u32 {
                if let Ok(elem) = self.parse_entity_ion(*loc) {
                    if let Some(fields) = elem.as_struct() {
                        if let Some(name) = get_field(fields, sym!(StoryName))
                            .and_then(|v| self.get_symbol_text(v))
                        {
                            storyline_map.insert(name.to_string(), *loc);
                        }
                    }
                }
            }
        }

        // Then, map each section to its storyline
        for loc in &self.entities {
            if loc.type_id == KfxSymbol::Section as u32 {
                if let Ok(elem) = self.parse_entity_ion(*loc) {
                    if let Some(fields) = elem.as_struct() {
                        let section_name = get_field(fields, sym!(SectionName))
                            .and_then(|v| self.get_symbol_text(v));

                        let story_name = get_field(fields, sym!(PageTemplates))
                            .and_then(|v| v.as_list())
                            .and_then(|templates| templates.first())
                            .and_then(|t| t.as_struct())
                            .and_then(|f| get_field(f, sym!(StoryName)))
                            .and_then(|v| self.get_symbol_text(v));

                        if let (Some(sec_name), Some(story_name)) = (section_name, story_name) {
                            if let Some(storyline_loc) = storyline_map.get(story_name) {
                                self.section_storylines
                                    .insert(sec_name.to_string(), *storyline_loc);
                            }
                        }
                    }
                }
            }
        }

        self.section_storylines_indexed = true;
        Ok(())
    }

    /// Extract section names from reading_orders in document_data or metadata.
    ///
    /// Prefers the "default" reading order if multiple are present.
    fn get_reading_order_sections(&self) -> io::Result<Vec<String>> {
        // Try document_data ($538) first, then metadata ($258)
        let doc_data_loc = self
            .entities
            .iter()
            .find(|e| e.type_id == KfxSymbol::DocumentData as u32)
            .copied();

        let metadata_loc = self
            .entities
            .iter()
            .find(|e| e.type_id == KfxSymbol::Metadata as u32)
            .copied();

        for loc in [doc_data_loc, metadata_loc].into_iter().flatten() {
            if let Ok(elem) = self.parse_entity_ion(loc) {
                if let Some(fields) = elem.as_struct() {
                    if let Some(orders) =
                        get_field(fields, sym!(ReadingOrders)).and_then(|v| v.as_list())
                    {
                        // First pass: look for "default" reading order
                        for order in orders {
                            if let Some(order_fields) = order.as_struct() {
                                let order_name = get_field(order_fields, sym!(ReadingOrderName))
                                    .and_then(|v| self.get_symbol_text(v));

                                if order_name == Some("default") {
                                    if let Some(sections) = self.extract_sections(order_fields) {
                                        return Ok(sections);
                                    }
                                }
                            }
                        }

                        // Second pass: take first reading order with sections
                        for order in orders {
                            if let Some(order_fields) = order.as_struct() {
                                if let Some(sections) = self.extract_sections(order_fields) {
                                    return Ok(sections);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Vec::new())
    }

    /// Extract section names from a reading order struct.
    fn extract_sections(&self, order_fields: &[(u32, IonValue)]) -> Option<Vec<String>> {
        let sections = get_field(order_fields, sym!(Sections))?.as_list()?;
        let mut section_names = Vec::new();
        for section in sections {
            if let Some(name) = self.get_symbol_text(section) {
                section_names.push(name.to_string());
            }
        }
        if section_names.is_empty() {
            None
        } else {
            Some(section_names)
        }
    }

    /// Resolve cover_image value which can be a string or list with symbol/string reference.
    fn resolve_cover_value(&self, value: Option<&IonValue>) -> Option<String> {
        let value = value?;

        // Format 1: Direct string
        if let Some(s) = value.as_string() {
            return Some(s.to_string());
        }

        // Format 2: List containing a symbol or string reference
        if let Some(list) = value.as_list() {
            if let Some(first) = list.first() {
                // Try as symbol first
                if let Some(text) = self.get_symbol_text(first) {
                    return Some(text.to_string());
                }
            }
        }

        None
    }

    /// Index external resources.
    fn index_resources(&mut self) -> io::Result<()> {
        if self.resources_indexed {
            return Ok(());
        }

        // Collect entities to process to avoid borrow conflicts
        let locs: Vec<_> = self
            .entities
            .iter()
            .filter(|e| e.type_id == KfxSymbol::ExternalResource as u32)
            .copied()
            .collect();

        for loc in locs {
            if let Ok(elem) = self.parse_entity_ion(loc) {
                if let Some(fields) = elem.as_struct() {
                    // Use location as key (e.g., "resource/rsrc7")
                    let location = get_field(fields, sym!(Location))
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());

                    // Also index by resource_name (e.g., "eF") for cover lookup
                    let name = get_field(fields, sym!(ResourceName))
                        .and_then(|v| resolve_symbol_text(v, &self.doc_symbols))
                        .map(|s| s.to_string());

                    if let Some(loc_str) = &location {
                        if !loc_str.is_empty() {
                            self.resources.insert(loc_str.clone(), loc);
                        }
                    }
                    if let Some(name_str) = &name {
                        if !name_str.is_empty() && Some(name_str) != location.as_ref() {
                            self.resources.insert(name_str.clone(), loc);
                        }
                    }
                }
            }
        }

        self.resources_indexed = true;
        Ok(())
    }
}

// --- Helper functions ---

/// Get a field from a struct by symbol ID.
#[inline]
fn get_field(fields: &[(u32, IonValue)], symbol_id: u32) -> Option<&IonValue> {
    fields
        .iter()
        .find(|(k, _)| *k == symbol_id)
        .map(|(_, v)| v)
}

/// Resolve a symbol or string value to its text representation.
fn resolve_symbol_text<'a>(value: &'a IonValue, doc_symbols: &'a [String]) -> Option<&'a str> {
    match value {
        IonValue::Symbol(id) => {
            let id = *id as usize;
            if id < KFX_SYMBOL_TABLE.len() {
                Some(KFX_SYMBOL_TABLE[id])
            } else {
                // Document-local symbols start after the base symbol table
                let doc_idx = id.saturating_sub(KFX_SYMBOL_TABLE.len());
                doc_symbols.get(doc_idx).map(|s| s.as_str())
            }
        }
        IonValue::String(s) => Some(s.as_str()),
        _ => None,
    }
}

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
fn parse_container_info_field(
    data: &[u8],
    offset_field: &str,
    length_field: &str,
) -> io::Result<(usize, usize)> {
    let mut parser = IonParser::new(data);
    let elem = parser.parse()?;

    if let Some(fields) = elem.as_struct() {
        // Look up symbol IDs for field names
        let offset_sym = symbol_id_for_name(offset_field)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Unknown symbol"))?;
        let length_sym = symbol_id_for_name(length_field)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Unknown symbol"))?;

        let offset = get_field(fields, offset_sym)
            .and_then(|v| v.as_int())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Missing {}", offset_field))
            })?;

        let length = get_field(fields, length_sym)
            .and_then(|v| v.as_int())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Missing {}", length_field))
            })?;

        Ok((offset as usize, length as usize))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Container info is not a struct",
        ))
    }
}

/// Look up a symbol ID by name from the static symbol table.
fn symbol_id_for_name(name: &str) -> Option<u32> {
    KFX_SYMBOL_TABLE
        .iter()
        .position(|&s| s == name)
        .map(|i| i as u32)
}

/// Extract document-specific symbols from the doc symbols section.
fn extract_doc_symbols(data: &[u8]) -> Vec<String> {
    let mut symbols = Vec::new();

    let start = if data.len() >= 4 && data[0..4] == ION_MAGIC {
        4
    } else {
        0
    };

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
