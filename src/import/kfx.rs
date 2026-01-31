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

use crate::import::{ChapterId, Importer, SpineEntry};
use crate::io::{ByteSource, FileSource};
use crate::kfx::container::{
    self, ContainerError, EntityLoc, extract_doc_symbols, get_field, get_symbol_text,
    parse_container_header, parse_container_info, parse_index_table, skip_enty_header,
};
use crate::kfx::ion::{IonParser, IonValue};
use crate::kfx::schema::schema;
use crate::kfx::storyline::parse_storyline_to_ir;
use crate::kfx::symbols::KfxSymbol;
use crate::model::Chapter;
use crate::model::{AnchorTarget, CollectionInfo, Contributor, GlobalNodeId, Landmark, Metadata, TocEntry};

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

    /// Landmarks (structural navigation points).
    landmarks: Vec<Landmark>,

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

    /// Style map: style_name -> KFX style properties (for style resolution)
    styles: HashMap<String, Vec<(u64, IonValue)>>,
    /// Whether styles have been indexed
    styles_indexed: bool,

    // --- Link resolution ---
    /// Internal anchors: anchor_name -> (position_id, offset)
    internal_anchors: HashMap<String, (i64, i64)>,

    /// Maps element string ID -> GlobalNodeId (built during index_anchors)
    element_id_map: HashMap<String, GlobalNodeId>,
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

    fn landmarks(&self) -> &[Landmark] {
        &self.landmarks
    }

    fn spine(&self) -> &[SpineEntry] {
        &self.spine
    }

    fn source_id(&self, id: ChapterId) -> Option<&str> {
        self.section_names.get(id.0 as usize).map(|s| s.as_str())
    }

    fn load_chapter(&mut self, id: ChapterId) -> io::Result<Chapter> {
        // Ensure anchors and styles are indexed
        self.index_anchor_entities()?;
        self.index_styles()?;

        let section_name = self
            .section_names
            .get(id.0 as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Chapter not found"))?
            .clone();

        // Get storyline location
        let storyline_loc = self.resolve_section_to_storyline(&section_name)?;

        // Parse storyline entity
        let storyline_ion = self.parse_entity_ion(storyline_loc)?;

        // Clone doc_symbols, anchors, and styles to avoid borrow conflict with content lookup closure
        let doc_symbols = self.doc_symbols.clone();
        let anchors = self.anchors.clone();
        let styles = self.styles.clone();

        // Parse storyline and build IR using schema-driven tokenization
        let mut chapter = parse_storyline_to_ir(
            &storyline_ion,
            &doc_symbols,
            Some(&anchors),
            Some(&styles),
            |name, index| self.lookup_content_text(name, index),
        );

        // Run optimization passes (KFX builds IR directly, not through compile_html)
        crate::dom::optimize::optimize(&mut chapter);

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
        let name = path.to_string_lossy();

        // Handle direct entity ID lookup (e.g., "#1102" from list_assets)
        if let Some(id_str) = name.strip_prefix('#') {
            if let Ok(id) = id_str.parse::<u32>() {
                // Find entity by ID
                if let Some(loc) = self.entities.iter().find(|e| e.id == id) {
                    return self.read_entity(*loc);
                }
            }
            return Err(io::Error::new(io::ErrorKind::NotFound, "Entity not found"));
        }

        // Ensure resources are indexed for name-based lookup
        if !self.resources_indexed {
            self.index_resources()?;
        }

        let loc = self
            .resources
            .get(name.as_ref())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Asset not found"))?;

        self.read_entity(*loc)
    }

    fn requires_normalized_export(&self) -> bool {
        // KFX load_raw returns binary Ion data, not HTML
        true
    }

    fn index_anchors(&mut self, chapters: &[(ChapterId, Arc<Chapter>)]) {
        self.element_id_map.clear();

        // Build element_id → GlobalNodeId map from chapters
        for (chapter_id, chapter) in chapters {
            for node_id in chapter.iter_dfs() {
                if let Some(id) = chapter.semantics.id(node_id) {
                    self.element_id_map
                        .insert(id.to_string(), GlobalNodeId::new(*chapter_id, node_id));
                }
            }
        }
    }

    fn resolve_href(&self, _from_chapter: ChapterId, href: &str) -> Option<AnchorTarget> {
        let href = href.trim();

        // External URLs
        if href.starts_with("http://")
            || href.starts_with("https://")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
        {
            return Some(AnchorTarget::External(href.to_string()));
        }

        // Strip leading # if present for anchor/element lookups
        let anchor_name = href.strip_prefix('#').unwrap_or(href);

        // Handle #id:offset format (KFX TOC/nav format)
        let anchor_name = if let Some(colon_pos) = anchor_name.find(':') {
            &anchor_name[..colon_pos]
        } else {
            anchor_name
        };

        // Check external anchors map (anchor_name → uri)
        if let Some(uri) = self.anchors.get(anchor_name) {
            return Some(AnchorTarget::External(uri.clone()));
        }

        // Check internal anchors (anchor_name → position.id → element_id)
        // The position.id in anchor entities references element IDs in storylines
        if let Some(&(pos_id, _offset)) = self.internal_anchors.get(anchor_name) {
            let id_str = pos_id.to_string();
            if let Some(target) = self.element_id_map.get(&id_str) {
                return Some(AnchorTarget::Internal(*target));
            }
        }

        // Try direct element ID lookup (anchor_name might be the numeric ID directly)
        if let Some(target) = self.element_id_map.get(anchor_name) {
            return Some(AnchorTarget::Internal(*target));
        }

        // Try parsing as numeric ID
        if let Ok(numeric_id) = anchor_name.parse::<i64>() {
            let id_str = numeric_id.to_string();
            if let Some(target) = self.element_id_map.get(&id_str) {
                return Some(AnchorTarget::Internal(*target));
            }
        }

        // Not found
        None
    }
}

impl KfxImporter {
    /// Create an importer from a ByteSource.
    pub fn from_source(source: Arc<dyn ByteSource>) -> io::Result<Self> {
        // Read and parse container header (18 bytes)
        let header_data = source.read_at(0, 18)?;
        let header = parse_container_header(&header_data)?;

        // Read and parse container info
        let container_info_data = source.read_at(
            header.container_info_offset as u64,
            header.container_info_length,
        )?;
        let container_info = parse_container_info(&container_info_data)?;

        // Get index table location (required)
        let (index_offset, index_length) = container_info.index.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Missing index table in container",
            )
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
            landmarks: Vec::new(), // TODO: Parse from KFX landmarks nav_container
            spine: Vec::new(),
            section_names: Vec::new(),
            section_storylines: HashMap::new(),
            section_storylines_indexed: false,
            resources: HashMap::new(),
            resources_indexed: false,
            content_cache: HashMap::new(),
            anchors: HashMap::new(),
            anchors_indexed: false,
            styles: HashMap::new(),
            styles_indexed: false,
            internal_anchors: HashMap::new(),
            element_id_map: HashMap::new(),
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

            if let Some(fields) = elem.as_struct()
                && let Some(list) =
                    get_field(fields, sym!(CategorisedMetadata)).and_then(|m| m.as_list())
            {
                for category_elem in list {
                    if let Some(cat_fields) = category_elem.as_struct() {
                        let category = get_field(cat_fields, sym!(Category))
                            .and_then(|v| self.get_symbol_text(v))
                            .unwrap_or("");

                        if category == "kindle_title_metadata"
                            && let Some(metadata_list) =
                                get_field(cat_fields, sym!(Metadata)).and_then(|v| v.as_list())
                        {
                            for meta in metadata_list {
                                let Some(meta_fields) = meta.as_struct() else {
                                    continue;
                                };
                                let key = get_field(meta_fields, sym!(Key))
                                    .and_then(|v| v.as_string())
                                    .unwrap_or("");
                                let value = get_field(meta_fields, sym!(Value))
                                    .and_then(|v| v.as_string())
                                    .unwrap_or("");

                                match key {
                                    "title" => self.metadata.title = value.to_string(),
                                    "author" => self.metadata.authors.push(value.to_string()),
                                    "publisher" => {
                                        self.metadata.publisher = Some(value.to_string())
                                    }
                                    "language" => self.metadata.language = value.to_string(),
                                    "description" => {
                                        self.metadata.description = Some(value.to_string())
                                    }
                                    "book_id" => self.metadata.identifier = value.to_string(),
                                    "issue_date" => self.metadata.date = Some(value.to_string()),
                                    "cover_image" => {
                                        let value_elem = get_field(meta_fields, sym!(Value));
                                        if let Some(cover) = self.resolve_cover_value(value_elem) {
                                            self.metadata.cover_image = Some(cover);
                                        }
                                    }
                                    "modified_date" => {
                                        self.metadata.modified_date = Some(value.to_string())
                                    }
                                    "translator" => self.metadata.contributors.push(Contributor {
                                        name: value.to_string(),
                                        file_as: None,
                                        role: Some("trl".to_string()),
                                    }),
                                    "title_pronunciation" => {
                                        self.metadata.title_sort = Some(value.to_string())
                                    }
                                    "author_pronunciation" => {
                                        self.metadata.author_sort = Some(value.to_string())
                                    }
                                    "series_name" => {
                                        if let Some(ref mut coll) = self.metadata.collection {
                                            coll.name = value.to_string();
                                        } else {
                                            self.metadata.collection = Some(CollectionInfo {
                                                name: value.to_string(),
                                                collection_type: Some("series".to_string()),
                                                position: None,
                                            });
                                        }
                                    }
                                    "series_position" => {
                                        if let Ok(pos) = value.parse::<f64>() {
                                            if let Some(ref mut coll) = self.metadata.collection {
                                                coll.position = Some(pos);
                                            } else {
                                                self.metadata.collection = Some(CollectionInfo {
                                                    name: String::new(),
                                                    collection_type: Some("series".to_string()),
                                                    position: Some(pos),
                                                });
                                            }
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

        Ok(())
    }

    /// Parse book navigation (TOC and landmarks).
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
                                    // Check nav_type
                                    let nav_type = get_field(container_fields, sym!(NavType))
                                        .and_then(|v| self.get_symbol_text(v));

                                    match nav_type {
                                        Some("toc") => {
                                            self.toc = self.parse_nav_entries(container_fields);
                                        }
                                        Some("landmarks") => {
                                            self.landmarks =
                                                self.parse_landmark_entries(container_fields);
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

        Ok(())
    }

    /// Parse landmark entries from a landmarks nav_container.
    fn parse_landmark_entries(&self, container: &[(u64, IonValue)]) -> Vec<Landmark> {
        let mut landmarks = Vec::new();

        if let Some(entry_list) = get_field(container, sym!(Entries)).and_then(|v| v.as_list()) {
            for entry in entry_list {
                // Unwrap annotation if present
                let inner = entry.unwrap_annotated();
                if let Some(entry_fields) = inner.as_struct() {
                    // Get landmark_type symbol and convert via schema
                    let landmark_type =
                        get_field(entry_fields, sym!(LandmarkType)).and_then(|v| match v {
                            IonValue::Symbol(id) => schema().landmark_from_kfx(*id),
                            _ => None,
                        });

                    // Skip unknown landmark types
                    let Some(landmark_type) = landmark_type else {
                        continue;
                    };

                    // Get label from representation.label
                    let label = get_field(entry_fields, sym!(Representation))
                        .and_then(|v| v.as_struct())
                        .and_then(|s| get_field(s, sym!(Label)))
                        .and_then(|v| v.as_string())
                        .unwrap_or("")
                        .to_string();

                    // Get target position (id and offset)
                    let target_pos =
                        get_field(entry_fields, sym!(TargetPosition)).and_then(|v| v.as_struct());
                    let href = if let Some(pos) = target_pos {
                        let id = get_field(pos, sym!(Id)).and_then(|v| v.as_int());
                        let offset = get_field(pos, sym!(Offset)).and_then(|v| v.as_int());
                        match (id, offset) {
                            (Some(id), Some(off)) if off > 0 => format!("#{}:{}", id, off),
                            (Some(id), _) => format!("#{}", id),
                            _ => String::new(),
                        }
                    } else {
                        String::new()
                    };

                    landmarks.push(Landmark {
                        landmark_type,
                        href,
                        label,
                    });
                }
            }
        }

        landmarks
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

                    // Get target position (includes id and offset for within-section navigation)
                    let target_pos =
                        get_field(entry_fields, sym!(TargetPosition)).and_then(|v| v.as_struct());
                    let href = if let Some(pos) = target_pos {
                        let id = get_field(pos, sym!(Id)).and_then(|v| v.as_int());
                        let offset = get_field(pos, sym!(Offset)).and_then(|v| v.as_int());
                        match (id, offset) {
                            (Some(id), Some(off)) if off > 0 => format!("#{}:{}", id, off),
                            (Some(id), _) => format!("#{}", id),
                            _ => String::new(),
                        }
                    } else {
                        String::new()
                    };

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
            if loc.type_id == KfxSymbol::Storyline as u32
                && let Ok(elem) = self.parse_entity_ion(*loc)
                && let Some(fields) = elem.as_struct()
                && let Some(name) =
                    get_field(fields, sym!(StoryName)).and_then(|v| self.get_symbol_text(v))
            {
                storyline_map.insert(name.to_string(), *loc);
            }
        }

        // Then, map each section to its storyline
        for loc in &self.entities {
            if loc.type_id == KfxSymbol::Section as u32
                && let Ok(elem) = self.parse_entity_ion(*loc)
                && let Some(fields) = elem.as_struct()
            {
                let section_name =
                    get_field(fields, sym!(SectionName)).and_then(|v| self.get_symbol_text(v));

                let story_name = get_field(fields, sym!(PageTemplates))
                    .and_then(|v| v.as_list())
                    .and_then(|templates| templates.first())
                    .and_then(|t| t.as_struct())
                    .and_then(|f| get_field(f, sym!(StoryName)))
                    .and_then(|v| self.get_symbol_text(v));

                if let (Some(sec_name), Some(story_name)) = (section_name, story_name)
                    && let Some(storyline_loc) = storyline_map.get(story_name)
                {
                    self.section_storylines
                        .insert(sec_name.to_string(), *storyline_loc);
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
            if let Ok(elem) = self.parse_entity_ion(loc)
                && let Some(fields) = elem.as_struct()
                && let Some(orders) =
                    get_field(fields, sym!(ReadingOrders)).and_then(|v| v.as_list())
            {
                // First pass: look for "default" reading order
                for order in orders {
                    if let Some(order_fields) = order.as_struct() {
                        let order_name = get_field(order_fields, sym!(ReadingOrderName))
                            .and_then(|v| self.get_symbol_text(v));

                        if order_name == Some("default")
                            && let Some(sections) = self.extract_sections(order_fields)
                        {
                            return Ok(sections);
                        }
                    }
                }

                // Second pass: take first reading order with sections
                for order in orders {
                    if let Some(order_fields) = order.as_struct()
                        && let Some(sections) = self.extract_sections(order_fields)
                    {
                        return Ok(sections);
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
        if let Some(list) = value.as_list()
            && let Some(first) = list.first()
            && let Some(text) = self.get_symbol_text(first)
        {
            return Some(text.to_string());
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
            if loc.type_id == KfxSymbol::Content as u32
                && let Ok(elem) = self.parse_entity_ion(*loc)
                && let Some(fields) = elem.as_struct()
            {
                // Check if name matches
                let entity_name =
                    get_field(fields, sym!(Name)).and_then(|v| self.get_symbol_text(v));

                if entity_name == Some(name)
                    && let Some(list) =
                        get_field(fields, sym!(ContentList)).and_then(|v| v.as_list())
                {
                    return Some(
                        list.iter()
                            .filter_map(|v| v.as_string().map(|s| s.to_string()))
                            .collect(),
                    );
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
            if let Ok(elem) = self.parse_entity_ion(loc)
                && let Some(fields) = elem.as_struct()
            {
                // Use location as key (e.g., "resource/rsrc7")
                let location = get_field(fields, sym!(Location))
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_string());

                // Also index by resource_name (e.g., "eF") for cover lookup
                let name = get_field(fields, sym!(ResourceName))
                    .and_then(|v| container::get_symbol_text(v, &self.doc_symbols))
                    .map(|s| s.to_string());

                if let Some(loc_str) = &location
                    && !loc_str.is_empty()
                {
                    self.resources.insert(loc_str.clone(), loc);
                }
                if let Some(name_str) = &name
                    && !name_str.is_empty()
                    && Some(name_str) != location.as_ref()
                {
                    self.resources.insert(name_str.clone(), loc);
                }
            }
        }

        self.resources_indexed = true;
        Ok(())
    }

    /// Index anchor entities to build anchor_name → uri/position maps.
    ///
    /// This enables resolution of both external and internal links where
    /// `link_to` contains an anchor name.
    fn index_anchor_entities(&mut self) -> io::Result<()> {
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
            if let Ok(elem) = self.parse_entity_ion(loc)
                && let Some(fields) = elem.as_struct()
            {
                // Get anchor_name
                let anchor_name = get_field(fields, sym!(AnchorName))
                    .and_then(|v| container::get_symbol_text(v, &self.doc_symbols))
                    .map(|s| s.to_string());

                let Some(name) = anchor_name else {
                    continue;
                };

                // Get uri (present for external links)
                let uri = get_field(fields, sym!(Uri))
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_string());

                if let Some(uri) = uri {
                    // External anchor
                    self.anchors.insert(name, uri);
                } else if let Some(position) =
                    get_field(fields, sym!(Position)).and_then(|v| v.as_struct())
                {
                    // Internal anchor with position
                    let id = get_field(position, sym!(Id)).and_then(|v| v.as_int());
                    let offset = get_field(position, sym!(Offset))
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);

                    if let Some(pos_id) = id {
                        self.internal_anchors.insert(name, (pos_id, offset));
                    }
                }
            }
        }

        self.anchors_indexed = true;
        Ok(())
    }

    /// Index style entities to build style_name → properties map.
    ///
    /// This enables resolution of style references in storyline elements.
    /// Style entities ($157) contain properties like font_weight, text_alignment, margins, etc.
    fn index_styles(&mut self) -> io::Result<()> {
        if self.styles_indexed {
            return Ok(());
        }

        // Find all style entities (type $157)
        let locs: Vec<_> = self
            .entities
            .iter()
            .filter(|e| e.type_id == KfxSymbol::Style as u32)
            .copied()
            .collect();

        for loc in locs {
            if let Ok(elem) = self.parse_entity_ion(loc)
                && let Some(fields) = elem.as_struct()
            {
                // Get style_name
                let style_name = get_field(fields, sym!(StyleName))
                    .and_then(|v| container::get_symbol_text(v, &self.doc_symbols))
                    .map(|s| s.to_string());

                if let Some(name) = style_name {
                    // Store all fields (cloned) for later interpretation
                    let props: Vec<(u64, IonValue)> = fields
                        .iter()
                        .filter(|(k, _)| *k != sym!(StyleName)) // Exclude the name itself
                        .map(|(k, v)| (*k, v.clone()))
                        .collect();

                    self.styles.insert(name, props);
                }
            }
        }

        self.styles_indexed = true;
        Ok(())
    }
}
