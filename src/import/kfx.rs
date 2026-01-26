//! KFX format importer.
//!
//! KFX is Amazon's Kindle Format 10, using Ion binary data format.
//!
//! This module handles I/O operations for reading KFX containers.
//! Pure parsing functions are in `crate::kfx::container`.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::book::{Metadata, TocEntry};
use crate::import::{ChapterId, Importer, SpineEntry};
use crate::io::{ByteSource, FileSource};
use crate::ir::IRChapter;
use crate::kfx::container::{
    self, extract_doc_symbols, get_field, get_symbol_text, parse_container_header,
    parse_container_info, parse_index_table, skip_enty_header, ContainerError, EntityLoc,
};
use crate::kfx::ion::{IonParser, IonValue};
use crate::kfx::storyline::parse_storyline_to_ir;
use crate::kfx::symbols::KfxSymbol;

/// Shorthand for getting a KfxSymbol as u32 for field lookups.
macro_rules! sym {
    ($variant:ident) => {
        KfxSymbol::$variant as u64
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

    /// Content cache: name -> list of strings (lazily populated)
    content_cache: HashMap<String, Vec<String>>,

    /// Anchor map: anchor_name -> uri (for external link resolution)
    anchors: HashMap<String, String>,
    /// Whether anchors have been indexed
    anchors_indexed: bool,
}

impl From<ContainerError> for io::Error {
    fn from(e: ContainerError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, e.to_string())
    }
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

    fn load_chapter(&mut self, id: ChapterId) -> io::Result<IRChapter> {
        // Ensure anchors are indexed (for external link resolution)
        self.index_anchors()?;

        let section_name = self
            .section_names
            .get(id.0 as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Chapter not found"))?
            .clone();

        // Get storyline location
        let storyline_loc = self.resolve_section_to_storyline(&section_name)?;

        // Parse storyline entity
        let storyline_ion = self.parse_entity_ion(storyline_loc)?;

        // Clone doc_symbols and anchors to avoid borrow conflict with content lookup closure
        let doc_symbols = self.doc_symbols.clone();
        let anchors = self.anchors.clone();

        // Parse storyline and build IR using schema-driven tokenization
        let chapter = parse_storyline_to_ir(&storyline_ion, &doc_symbols, Some(&anchors), |name, index| {
            self.lookup_content_text(name, index)
        });

        Ok(chapter)
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
        // Read and parse container header (18 bytes)
        let header_data = source.read_at(0, 18)?;
        let header = parse_container_header(&header_data)?;

        // Read and parse container info
        let container_info_data =
            source.read_at(header.container_info_offset as u64, header.container_info_length)?;
        let container_info = parse_container_info(&container_info_data)?;

        // Get index table location (required)
        let (index_offset, index_length) = container_info.index.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Missing index table in container")
        })?;

        // Read and parse document symbols (optional)
        let doc_symbols = if let Some((offset, length)) = container_info.doc_symbols {
            if length > 0 {
                let doc_sym_data = source.read_at(offset as u64, length)?;
                extract_doc_symbols(&doc_sym_data)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Read and parse index table
        let index_data = source.read_at(index_offset as u64, index_length)?;
        let entities = parse_index_table(&index_data, header.header_len);

        let mut importer = Self {
            source,
            header_len: header.header_len,
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
            content_cache: HashMap::new(),
            anchors: HashMap::new(),
            anchors_indexed: false,
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

        // Use pure function to skip ENTY header
        let payload = skip_enty_header(&entity_data);
        if payload.len() != entity_data.len() {
            Ok(payload.to_vec())
        } else {
            Ok(entity_data)
        }
    }

    /// Parse an entity as Ion and return the parsed value.
    fn parse_entity_ion(&self, loc: EntityLoc) -> io::Result<IonValue> {
        let ion_data = self.read_entity(loc)?;
        let mut parser = IonParser::new(&ion_data);
        parser.parse()
    }

    /// Get a symbol's text from an IonValue (handles both Symbol and String).
    fn get_symbol_text<'a>(&'a self, value: &'a IonValue) -> Option<&'a str> {
        get_symbol_text(value, &self.doc_symbols)
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
    fn parse_nav_entries(&self, container: &[(u64, IonValue)]) -> Vec<TocEntry> {
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
    fn extract_sections(&self, order_fields: &[(u64, IonValue)]) -> Option<Vec<String>> {
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

    /// Look up text content by name and index.
    ///
    /// Lazily loads and caches content entities as needed.
    fn lookup_content_text(&mut self, name: &str, index: usize) -> Option<String> {
        // Check cache first
        if let Some(content_list) = self.content_cache.get(name) {
            return content_list.get(index).cloned();
        }

        // Load and cache the content entity
        if let Some(content_list) = self.load_content_entity(name) {
            let result = content_list.get(index).cloned();
            self.content_cache.insert(name.to_string(), content_list);
            return result;
        }

        None
    }

    /// Load a content entity by name and return its string list.
    fn load_content_entity(&self, name: &str) -> Option<Vec<String>> {
        // Find content entity with matching name
        for loc in &self.entities {
            if loc.type_id == KfxSymbol::Content as u32 {
                if let Ok(elem) = self.parse_entity_ion(*loc) {
                    if let Some(fields) = elem.as_struct() {
                        // Check if name matches
                        let entity_name = get_field(fields, sym!(Name))
                            .and_then(|v| self.get_symbol_text(v));

                        if entity_name == Some(name) {
                            // Extract content_list
                            if let Some(list) = get_field(fields, sym!(ContentList))
                                .and_then(|v| v.as_list())
                            {
                                return Some(
                                    list.iter()
                                        .filter_map(|v| v.as_string().map(|s| s.to_string()))
                                        .collect(),
                                );
                            }
                        }
                    }
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
                        .and_then(|v| container::get_symbol_text(v, &self.doc_symbols))
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

    /// Index anchor entities to build anchor_name → uri map.
    ///
    /// This enables resolution of external links where `link_to` contains
    /// an anchor name that maps to an external URI.
    fn index_anchors(&mut self) -> io::Result<()> {
        if self.anchors_indexed {
            return Ok(());
        }

        // Find all anchor entities (type $266)
        let locs: Vec<_> = self
            .entities
            .iter()
            .filter(|e| e.type_id == KfxSymbol::Anchor as u32)
            .copied()
            .collect();

        for loc in locs {
            if let Ok(elem) = self.parse_entity_ion(loc) {
                if let Some(fields) = elem.as_struct() {
                    // Get anchor_name
                    let anchor_name = get_field(fields, sym!(AnchorName))
                        .and_then(|v| container::get_symbol_text(v, &self.doc_symbols))
                        .map(|s| s.to_string());

                    // Get uri (only present for external links)
                    let uri = get_field(fields, sym!(Uri))
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_string());

                    // If we have both anchor_name and uri, add to map
                    if let (Some(name), Some(uri)) = (anchor_name, uri) {
                        self.anchors.insert(name, uri);
                    }
                }
            }
        }

        self.anchors_indexed = true;
        Ok(())
    }
}
