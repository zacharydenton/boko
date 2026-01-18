//! KFX container and content writer.
//!
//! Based on analysis of Amazon's KFX format from the calibre-kfx-input plugin.
//! KFX uses a fragment-based model where each piece of content is a fragment with:
//! - ftype: Fragment type (like $145 for text, $260 for sections, etc.)
//! - fid: Fragment ID (unique identifier for this fragment)
//! - value: The actual ION data

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::book::Book;
use crate::css::{CssValue, ParsedStyle, Stylesheet, TextAlign};

use super::ion::{encode_kfx_decimal, IonValue, IonWriter};

// =============================================================================
// YJ_SYMBOLS - Shared symbol table (subset of the full 800+ symbols)
// =============================================================================

/// Symbol IDs from YJ_symbols shared table (version 10)
/// These are the well-known symbols used in KFX format.
#[allow(dead_code)]
pub mod sym {
    // Core property symbols
    pub const ID: u64 = 4; // $4 - generic id field
    pub const LANGUAGE: u64 = 10; // $10 - language

    // Style property symbols
    pub const FONT_FAMILY: u64 = 12; // $12 - font family
    pub const FONT_SIZE: u64 = 13; // $13 - font size
    pub const LINE_HEIGHT: u64 = 16; // $16 - line height
    pub const MARGIN_TOP: u64 = 42; // $42 - margin top
    pub const TEXT_ALIGN: u64 = 44; // $44 - text alignment
    pub const BOLD: u64 = 45; // $45 - bold flag
    pub const ITALIC: u64 = 46; // $46 - italic flag
    pub const MARGIN_BOTTOM: u64 = 47; // $47 - margin bottom
    pub const MARGIN_LEFT: u64 = 49; // $49 - margin left
    pub const MARGIN_RIGHT: u64 = 51; // $51 - margin right

    // Style value symbols
    pub const UNIT: u64 = 306; // $306 - unit field
    // Note: $307 is VALUE, used for both metadata and unit struct values
    pub const UNIT_EM: u64 = 310; // $310 - em unit
    pub const FONT_DEFAULT: u64 = 350; // $350 - default font family
    pub const UNIT_PX: u64 = 361; // $361 - px unit
    pub const ALIGN_JUSTIFY: u64 = 370; // $370 - text-align: justify
    pub const ALIGN_CENTER: u64 = 371; // $371 - text-align: center
    pub const ALIGN_LEFT: u64 = 372; // $372 - text-align: left
    pub const ALIGN_RIGHT: u64 = 373; // $373 - text-align: right
    pub const FONT_SERIF: u64 = 382; // $382 - serif font family
    pub const UNIT_PERCENT: u64 = 505; // $505 - percent unit
    pub const STYLE_CLASS: u64 = 760; // $760 - style class
    pub const STYLE_CLASSES: u64 = 761; // $761 - style classes list

    // Content symbols
    pub const SECTION_CONTENT: u64 = 141; // $141 - section content list
    pub const TEXT_CONTENT: u64 = 145; // $145 - text content fragment type
    pub const CONTENT_ARRAY: u64 = 146; // $146 - array of content items
    pub const DESCRIPTION: u64 = 154; // $154 - description
    pub const POSITION: u64 = 155; // $155 - position / EID
    pub const STYLE: u64 = 157; // $157 - style fragment type
    pub const CONTENT_TYPE: u64 = 159; // $159 - content type symbol
    pub const FORMAT: u64 = 161; // $161 - format
    pub const LOCATION: u64 = 165; // $165 - resource location
    pub const READING_ORDERS: u64 = 169; // $169 - reading orders list
    pub const SECTIONS_LIST: u64 = 170; // $170 - list of sections
    pub const STYLE_NAME: u64 = 173; // $173 - style name/id
    pub const SECTION_NAME: u64 = 174; // $174 - section name/id
    pub const RESOURCE_NAME: u64 = 175; // $175 - external resource name
    pub const CONTENT_NAME: u64 = 176; // $176 - content block name/id
    pub const READING_ORDER_NAME: u64 = 178; // $178 - reading order name
    pub const ENTITY_LIST: u64 = 181; // $181 - list of entities
    pub const LOCATION_ENTRIES: u64 = 182; // $182 - location entries list

    // Navigation symbols
    pub const OFFSET: u64 = 143; // $143 - offset within section/content
    pub const COUNT: u64 = 144; // $144 - count/length
    pub const NAV_TYPE: u64 = 235; // $235 - navigation type
    pub const TOC: u64 = 212; // $212 - table of contents nav type
    pub const LANDMARKS: u64 = 237; // $237 - landmarks
    pub const NAV_ID: u64 = 239; // $239 - nav container id reference
    pub const NAV_UNIT_REF: u64 = 240; // $240 - nav unit reference
    pub const NAV_TITLE: u64 = 241; // $241 - navigation title struct
    pub const TEXT: u64 = 244; // $244 - text content field
    pub const NAV_TARGET: u64 = 246; // $246 - navigation target struct
    pub const NAV_ENTRIES: u64 = 247; // $247 - navigation entries list
    pub const NAV_CONTAINER: u64 = 249; // $249 - nav container
    pub const CONTAINER_CONTENTS: u64 = 252; // $252 - container contents
    pub const ENTITY_DEPS: u64 = 253; // $253 - entity dependencies
    pub const MANDATORY_DEPS: u64 = 254; // $254 - mandatory dependencies

    // Metadata symbols
    pub const METADATA: u64 = 258; // $258 - metadata fragment type
    pub const CONTENT_BLOCK: u64 = 259; // $259 - content block fragment type
    pub const SECTION: u64 = 260; // $260 - section fragment type
    pub const POSITION_MAP: u64 = 264; // $264 - position map
    pub const POSITION_ID_MAP: u64 = 265; // $265 - position id map
    pub const PAGE_TEMPLATE: u64 = 266; // $266 - page template
    pub const CONTENT_PARAGRAPH: u64 = 269; // $269 - paragraph content type
    pub const CONTAINER_INFO: u64 = 270; // $270 - container info fragment type

    // Value/metadata symbols
    pub const VALUE: u64 = 307; // $307 - metadata value
    pub const DEFAULT_READING_ORDER: u64 = 351; // $351 - default reading order name

    // Navigation fragment symbols
    pub const BOOK_NAVIGATION: u64 = 389; // $389 - book navigation fragment type
    pub const NAV_CONTAINER_TYPE: u64 = 391; // $391 - nav container fragment type
    pub const NAV_CONTAINER_REF: u64 = 392; // $392 - nav container reference
    pub const NAV_DEFINITION: u64 = 393; // $393 - nav definition
    pub const NAV_UNIT: u64 = 394; // $394 - nav unit fragment type
    pub const NAV_UNIT_LIST: u64 = 395; // $395 - nav unit list fragment type

    // Resource symbols
    pub const TEXT_OFFSET: u64 = 403; // $403 - text offset
    pub const CONTAINER_ID: u64 = 409; // $409 - container ID string
    pub const COMPRESSION_TYPE: u64 = 410; // $410 - compression type
    pub const DRM_SCHEME: u64 = 411; // $411 - DRM scheme
    pub const CHUNK_SIZE: u64 = 412; // $412 - chunk size
    pub const INDEX_TABLE_OFFSET: u64 = 413; // $413 - index table offset
    pub const INDEX_TABLE_LENGTH: u64 = 414; // $414 - index table length
    pub const SYMBOL_TABLE_OFFSET: u64 = 415; // $415 - symbol table offset
    pub const SYMBOL_TABLE_LENGTH: u64 = 416; // $416 - symbol table length
    pub const RAW_MEDIA: u64 = 417; // $417 - raw media fragment type
    pub const CONTAINER_ENTITY_MAP: u64 = 419; // $419 - container entity map
    pub const WIDTH: u64 = 422; // $422 - image width in pixels
    pub const HEIGHT: u64 = 423; // $423 - image height in pixels

    // Resource symbols
    pub const RESOURCE: u64 = 164; // $164 - resource fragment type
    pub const IMAGE_FORMAT: u64 = 285; // $285 - image format type
    pub const IMAGE_CONTENT: u64 = 271; // $271 - image content type
    pub const MIME_TYPE: u64 = 162; // $162 - MIME type string
    pub const FONT_FORMAT: u64 = 286; // $286 - font format type

    // Metadata entry symbols
    pub const KINDLE_METADATA: u64 = 490; // $490 - kindle metadata fragment type
    pub const METADATA_ENTRIES: u64 = 491; // $491 - metadata entries list
    pub const METADATA_KEY: u64 = 492; // $492 - metadata key
    pub const METADATA_GROUP: u64 = 495; // $495 - metadata group name

    // Document structure symbols
    pub const DOCUMENT_DATA: u64 = 538; // $538 - document data fragment type
    pub const LOCATION_MAP: u64 = 550; // $550 - location map fragment type

    // Position map symbols
    pub const ENTITY_ID_LIST: u64 = 181; // $181 - list of entity IDs
    pub const EID_INDEX: u64 = 184; // $184 - EID index (character offset)
    pub const EID_VALUE: u64 = 185; // $185 - EID value (position ID)

    // Format capabilities symbols
    pub const FORMAT_CAPABILITIES_OLD: u64 = 585; // $585 - old format capabilities
    pub const CAPABILITY_NAME: u64 = 586; // $586 - capability provider name
    pub const MIN_VERSION: u64 = 587; // $587 - kfxgen app version / min ver
    pub const VERSION: u64 = 588; // $588 - kfxgen package version / version
    pub const CAPABILITY_VERSION: u64 = 589; // $589 - capability version struct
    pub const CAPABILITIES_LIST: u64 = 590; // $590 - capabilities list
    pub const FORMAT_CAPABILITIES: u64 = 593; // $593 - format capabilities fragment type
    pub const FC_OFFSET: u64 = 594; // $594 - format capabilities offset
    pub const FC_LENGTH: u64 = 595; // $595 - format capabilities length

    // Auxiliary data symbols
    pub const AUXILIARY_DATA: u64 = 597; // $597 - auxiliary/section metadata
    pub const AUX_DATA_REF: u64 = 598; // $598 - auxiliary data reference

    // Special singleton ID
    pub const SINGLETON_ID: u64 = 348; // $348 - used for singleton entity IDs
}

// =============================================================================
// Fragment Abstraction
// =============================================================================

/// A KFX fragment - the fundamental unit of KFX content
#[derive(Debug, Clone)]
pub struct KfxFragment {
    /// Fragment type (symbol ID like $260, $145, etc.)
    pub ftype: u64,
    /// Fragment ID (unique identifier, or same as ftype for singletons)
    pub fid: String,
    /// The ION value payload
    pub value: IonValue,
}

impl KfxFragment {
    /// Create a new fragment
    pub fn new(ftype: u64, fid: impl Into<String>, value: IonValue) -> Self {
        Self {
            ftype,
            fid: fid.into(),
            value,
        }
    }

    /// Create a singleton fragment (fid equals ftype name)
    pub fn singleton(ftype: u64, value: IonValue) -> Self {
        Self {
            ftype,
            fid: format!("${}", ftype),
            value,
        }
    }

    /// Check if this is a singleton fragment
    pub fn is_singleton(&self) -> bool {
        self.fid == format!("${}", self.ftype)
    }

    /// Get the entity ID number for serialization
    #[allow(dead_code)]
    pub fn entity_id(&self, symtab: &SymbolTable) -> u32 {
        if self.is_singleton() {
            sym::SINGLETON_ID as u32
        } else {
            symtab.get(&self.fid).unwrap_or(sym::SINGLETON_ID) as u32
        }
    }
}

// =============================================================================
// Symbol Table
// =============================================================================

/// Simple symbol table for tracking local symbols
#[derive(Debug, Default)]
pub struct SymbolTable {
    /// Local symbols (book-specific IDs)
    local_symbols: Vec<String>,
    /// Map from symbol name to ID
    symbol_map: HashMap<String, u64>,
    /// Next local symbol ID (starts after YJ_symbols max_id)
    next_id: u64,
}

impl SymbolTable {
    /// YJ_symbols has ~850 symbols, local IDs start after
    const LOCAL_MIN_ID: u64 = 860;

    pub fn new() -> Self {
        Self {
            local_symbols: Vec::new(),
            symbol_map: HashMap::new(),
            next_id: Self::LOCAL_MIN_ID,
        }
    }

    /// Get or create a symbol ID for a name
    pub fn get_or_intern(&mut self, name: &str) -> u64 {
        // Check if it's a shared symbol reference (starts with $)
        if let Some(id_str) = name.strip_prefix('$')
            && let Ok(id) = id_str.parse::<u64>() {
                return id;
            }

        // Check if already interned
        if let Some(&id) = self.symbol_map.get(name) {
            return id;
        }

        // Create new local symbol
        let id = self.next_id;
        self.next_id += 1;
        self.local_symbols.push(name.to_string());
        self.symbol_map.insert(name.to_string(), id);
        id
    }

    /// Get symbol ID without interning (returns None if not found)
    pub fn get(&self, name: &str) -> Option<u64> {
        if let Some(id_str) = name.strip_prefix('$')
            && let Ok(id) = id_str.parse::<u64>() {
                return Some(id);
            }
        self.symbol_map.get(name).copied()
    }

    /// Get local symbols for $ion_symbol_table fragment
    #[allow(dead_code)]
    pub fn local_symbols(&self) -> &[String] {
        &self.local_symbols
    }

    /// Create the $ion_symbol_table import structure
    pub fn create_import(&self) -> IonValue {
        let mut import = HashMap::new();
        import.insert(4, IonValue::String("YJ_symbols".to_string())); // name
        import.insert(5, IonValue::Int(10)); // version
        import.insert(8, IonValue::Int(Self::LOCAL_MIN_ID as i64 - 1)); // max_id

        let mut symtab = HashMap::new();
        symtab.insert(6, IonValue::List(vec![IonValue::Struct(import)])); // imports

        if !self.local_symbols.is_empty() {
            let symbols: Vec<IonValue> = self
                .local_symbols
                .iter()
                .map(|s| IonValue::String(s.clone()))
                .collect();
            symtab.insert(7, IonValue::List(symbols)); // symbols
        }

        IonValue::Struct(symtab)
    }
}

// =============================================================================
// KFX Book Builder
// =============================================================================

/// Builder for creating a complete KFX book
pub struct KfxBookBuilder {
    symtab: SymbolTable,
    fragments: Vec<KfxFragment>,
    container_id: String,
    /// Map from parsed style to style symbol
    style_map: HashMap<ParsedStyle, u64>,
}

impl KfxBookBuilder {
    pub fn new() -> Self {
        Self {
            symtab: SymbolTable::new(),
            fragments: Vec::new(),
            container_id: generate_container_id(),
            style_map: HashMap::new(),
        }
    }

    /// Build a KFX book from a Book structure
    pub fn from_book(book: &Book) -> Self {
        let mut builder = Self::new();

        // 1. Extract and parse all CSS stylesheets from resources
        let mut combined_css = String::new();
        // Add default user-agent styles for common elements
        combined_css.push_str("h1, h2, h3, h4, h5, h6 { font-weight: bold; margin-top: 1em; margin-bottom: 1em; }\n");
        combined_css.push_str("h1 { font-size: 2em; text-align: center; }\n");
        combined_css.push_str("h2 { font-size: 1.5em; }\n");
        combined_css.push_str("h3 { font-size: 1.25em; }\n");
        combined_css.push_str("p { text-align: justify; }\n");
        combined_css.push_str("blockquote { margin-left: 2em; margin-right: 2em; }\n");
        combined_css.push_str("li { margin-left: 1em; }\n");

        for resource in book.resources.values() {
            if resource.media_type == "text/css" {
                combined_css.push_str(&String::from_utf8_lossy(&resource.data));
                combined_css.push('\n');
            }
        }
        let stylesheet = Stylesheet::parse(&combined_css);

        // Build a map from href to TOC title for lookup
        let toc_titles: std::collections::HashMap<&str, &str> = book
            .toc
            .iter()
            .map(|entry| (entry.href.as_str(), entry.title.as_str()))
            .collect();

        // 2. Extract content from spine with computed styles
        let mut chapters: Vec<ChapterData> = Vec::new();
        let mut chapter_num = 1;
        for (idx, spine_item) in book.spine.iter().enumerate() {
            let content = book
                .resources
                .get(&spine_item.href)
                .map(|r| extract_styled_text_from_xhtml(&r.data, &stylesheet))
                .unwrap_or_default();

            if content.is_empty() {
                continue;
            }

            // Try to get title from TOC, fall back to default
            let title = toc_titles
                .get(spine_item.href.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    // Try to use first text line if it looks like a title
                    content.first().and_then(|first| {
                        if first.text.len() < 100 && !first.text.contains('.') {
                            Some(first.text.clone())
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| format!("Chapter {}", chapter_num));

            let chapter_id = format!("chapter-{}", idx);
            chapters.push(ChapterData {
                id: chapter_id,
                title,
                texts: content,
            });
            chapter_num += 1;
        }

        // 3. Build all fragments
        builder.add_format_capabilities();
        builder.add_metadata(book);
        builder.add_metadata_258(&chapters);
        builder.add_document_data(&chapters);
        builder.add_book_navigation(&chapters);
        builder.add_nav_unit_list();

        // 4. Collect all unique styles and add them as P157 fragments
        builder.add_all_styles(&chapters);

        // Add content fragments for each chapter
        // Track EID base for consistent position IDs across content blocks and position maps
        // Each chapter uses: 1 EID for section content entry + N EIDs for content blocks
        let mut eid_base = 860i64;
        for chapter in &chapters {
            builder.add_text_content(chapter);
            builder.add_content_block(chapter, eid_base);
            builder.add_section(chapter, eid_base);
            builder.add_auxiliary_data(chapter);
            // +1 for section content entry, + texts.len() for content blocks
            eid_base += 1 + chapter.texts.len() as i64;
        }

        // Add position/location maps
        builder.add_position_map(&chapters);
        builder.add_position_id_map(&chapters);
        builder.add_location_map(&chapters);

        // Add media resources (images and fonts)
        builder.add_resources(book);

        // Add container entity map (must be last content fragment)
        builder.add_container_entity_map();

        // Add required header fragments ($270 container info, $ion_symbol_table)
        // These are extracted during serialization but must exist as fragments
        builder.add_container_info_fragment();
        builder.add_symbol_table_fragment();

        builder
    }

    /// Add $270 container info fragment (required for serialization)
    fn add_container_info_fragment(&mut self) {
        let mut info = HashMap::new();
        info.insert(
            sym::CONTAINER_ID,
            IonValue::String(self.container_id.clone()),
        );
        info.insert(sym::CHUNK_SIZE, IonValue::Int(4096));
        info.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
        info.insert(sym::DRM_SCHEME, IonValue::Int(0));
        info.insert(
            sym::MIN_VERSION,
            IonValue::String(format!("boko-{}", env!("CARGO_PKG_VERSION"))),
        );
        info.insert(
            sym::VERSION,
            IonValue::String(env!("CARGO_PKG_VERSION").to_string()),
        );
        info.insert(sym::FORMAT, IonValue::String("KFX main".to_string()));

        self.fragments.push(KfxFragment::singleton(
            sym::CONTAINER_INFO,
            IonValue::Struct(info),
        ));
    }

    /// Add $ion_symbol_table fragment (required for serialization)
    fn add_symbol_table_fragment(&mut self) {
        let symtab_value = self.symtab.create_import();
        // Use type 3 which is the $ion_symbol_table annotation ID
        self.fragments.push(KfxFragment::new(
            3, // $ion_symbol_table
            "$ion_symbol_table",
            symtab_value,
        ));
    }

    /// Add format capabilities fragment ($585 - old style, as entity)
    fn add_format_capabilities(&mut self) {
        // $585 format capabilities (old style) - goes as an entity
        // This is what the Kindle reader expects to find
        //
        // Structure: { $590: [ { $586: provider, $492: feature, $589: { $5: { $587: min, $588: ver } } }, ... ] }
        // Match reference KFX format capabilities exactly
        let capabilities = [("com.amazon.yjconversion", "reflow-style", 6, 0),
            ("SDK.Marker", "CanonicalFormat", 1, 0),
            ("com.amazon.yjconversion", "yj_hdv", 1, 0)];

        let caps_list: Vec<IonValue> = capabilities
            .iter()
            .map(|(provider, feature, min_version, version)| {
                // Version struct: { $587 (min_version): min, $588 (version): ver }
                let mut ver_struct = HashMap::new();
                ver_struct.insert(sym::MIN_VERSION, IonValue::Int(*min_version));
                ver_struct.insert(sym::VERSION, IonValue::Int(*version));

                // Wrapper with $5 key
                let mut ver_wrapper = HashMap::new();
                ver_wrapper.insert(5, IonValue::Struct(ver_struct)); // $5 wrapper

                // Capability entry
                let mut cap = HashMap::new();
                cap.insert(sym::CAPABILITY_NAME, IonValue::String(provider.to_string()));
                cap.insert(sym::METADATA_KEY, IonValue::String(feature.to_string()));
                cap.insert(sym::CAPABILITY_VERSION, IonValue::Struct(ver_wrapper));
                IonValue::Struct(cap)
            })
            .collect();

        // Wrap in { $590: [...] }
        let mut caps_struct = HashMap::new();
        caps_struct.insert(sym::CAPABILITIES_LIST, IonValue::List(caps_list));

        self.fragments.push(KfxFragment::singleton(
            sym::FORMAT_CAPABILITIES_OLD,
            IonValue::Struct(caps_struct),
        ));
    }

    /// Add metadata fragment ($490)
    fn add_metadata(&mut self, book: &Book) {
        let mut all_groups = Vec::new();

        // 1. kindle_audit_metadata - information about how the file was created
        {
            let mut entries = Vec::new();

            let mut add_entry = |key: &str, value: IonValue| {
                let mut entry = HashMap::new();
                entry.insert(sym::METADATA_KEY, IonValue::String(key.to_string()));
                entry.insert(sym::VALUE, value);
                entries.push(IonValue::Struct(entry));
            };

            add_entry("file_creator", IonValue::String("boko".to_string()));
            add_entry(
                "creator_version",
                IonValue::String(env!("CARGO_PKG_VERSION").to_string()),
            );

            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_audit_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(entries));
            all_groups.push(IonValue::Struct(group));
        }

        // 2. kindle_title_metadata - book title, author, etc.
        {
            let mut entries = Vec::new();

            let mut add_entry = |key: &str, value: IonValue| {
                let mut entry = HashMap::new();
                entry.insert(sym::METADATA_KEY, IonValue::String(key.to_string()));
                entry.insert(sym::VALUE, value);
                entries.push(IonValue::Struct(entry));
            };

            add_entry("title", IonValue::String(book.metadata.title.clone()));

            for author in &book.metadata.authors {
                add_entry("author", IonValue::String(author.clone()));
            }

            if !book.metadata.language.is_empty() {
                add_entry("language", IonValue::String(book.metadata.language.clone()));
            }

            if let Some(ref publisher) = book.metadata.publisher {
                add_entry("publisher", IonValue::String(publisher.clone()));
            }

            if let Some(ref description) = book.metadata.description {
                add_entry("description", IonValue::String(description.clone()));
            }

            // Use a unique content_id for the book (not ASIN - that's for store books)
            let content_id = if !book.metadata.identifier.is_empty() {
                book.metadata.identifier.clone()
            } else {
                // Generate a unique ID based on title/author
                format!("boko_{:x}", {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut h = DefaultHasher::new();
                    book.metadata.title.hash(&mut h);
                    book.metadata.authors.hash(&mut h);
                    h.finish()
                })
            };
            add_entry("content_id", IonValue::String(content_id));

            // PDOC = Personal Document (sideloaded, no store verification)
            // EBOK = store-purchased eBook (triggers store lookup)
            add_entry("cde_content_type", IonValue::String("PDOC".to_string()));

            // Add cover_image reference if available
            if let Some(cover_href) = &book.metadata.cover_image {
                // Find the cover in resources and reference it
                let mut resource_index = 0;
                for (href, resource) in &book.resources {
                    if is_image_media_type(&resource.media_type) {
                        if href == cover_href {
                            add_entry(
                                "cover_image",
                                IonValue::String(format!("rsrc{}", resource_index)),
                            );
                            break;
                        }
                        resource_index += 1;
                    }
                }
            }

            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_title_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(entries));
            all_groups.push(IonValue::Struct(group));
        }

        // 3. kindle_ebook_metadata - ebook feature flags
        {
            let mut entries = Vec::new();

            let mut add_entry = |key: &str, value: IonValue| {
                let mut entry = HashMap::new();
                entry.insert(sym::METADATA_KEY, IonValue::String(key.to_string()));
                entry.insert(sym::VALUE, value);
                entries.push(IonValue::Struct(entry));
            };

            add_entry("selection", IonValue::String("enabled".to_string()));
            add_entry("nested_span", IonValue::String("enabled".to_string()));

            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_ebook_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(entries));
            all_groups.push(IonValue::Struct(group));
        }

        // 4. kindle_capability_metadata - capability flags (can be empty)
        {
            let mut group = HashMap::new();
            group.insert(
                sym::METADATA_GROUP,
                IonValue::String("kindle_capability_metadata".to_string()),
            );
            group.insert(sym::METADATA, IonValue::List(Vec::new()));
            all_groups.push(IonValue::Struct(group));
        }

        let mut root = HashMap::new();
        root.insert(sym::METADATA_ENTRIES, IonValue::List(all_groups));

        self.fragments.push(KfxFragment::singleton(
            sym::KINDLE_METADATA,
            IonValue::Struct(root),
        ));
    }

    /// Add $258 metadata fragment with reading orders (must be called after sections are added)
    fn add_metadata_258(&mut self, chapters: &[ChapterData]) {
        let section_refs: Vec<IonValue> = chapters
            .iter()
            .map(|ch| {
                let section_id = format!("section-{}", ch.id);
                let sym_id = self.symtab.get_or_intern(&section_id);
                IonValue::Symbol(sym_id)
            })
            .collect();

        let mut reading_order = HashMap::new();
        reading_order.insert(
            sym::READING_ORDER_NAME,
            IonValue::Symbol(sym::DEFAULT_READING_ORDER),
        );
        reading_order.insert(sym::SECTIONS_LIST, IonValue::List(section_refs));

        let mut metadata_258 = HashMap::new();
        metadata_258.insert(
            sym::READING_ORDERS,
            IonValue::List(vec![IonValue::Struct(reading_order)]),
        );

        self.fragments.push(KfxFragment::singleton(
            sym::METADATA,
            IonValue::Struct(metadata_258),
        ));
    }

    /// Add document data fragment ($538) with reading orders and document properties
    fn add_document_data(&mut self, chapters: &[ChapterData]) {
        let section_refs: Vec<IonValue> = chapters
            .iter()
            .map(|ch| {
                let section_id = format!("section-{}", ch.id);
                let sym_id = self.symtab.get_or_intern(&section_id);
                IonValue::Symbol(sym_id)
            })
            .collect();

        let mut reading_order = HashMap::new();
        reading_order.insert(
            sym::READING_ORDER_NAME,
            IonValue::Symbol(sym::DEFAULT_READING_ORDER),
        );
        reading_order.insert(sym::SECTIONS_LIST, IonValue::List(section_refs));

        // Calculate total number of content items (not character count)
        // This is used for position calculations
        let total_items: usize = chapters.iter().map(|ch| ch.texts.len()).sum();

        // Build "typed null" struct used for P16 and P42: {$307: null-like, $306: $308}
        // The null-like value is encoded as a 2-byte decimal (0x80 0x01) per Kindle format
        let typed_null = || {
            let mut s = HashMap::new();
            s.insert(307, IonValue::Decimal(vec![0x80, 0x01])); // Null-like decimal
            s.insert(306, IonValue::Symbol(308)); // Type indicator
            IonValue::Struct(s)
        };

        let mut doc_data = HashMap::new();
        // $169 (reading_orders) - list of reading order definitions
        doc_data.insert(
            sym::READING_ORDERS,
            IonValue::List(vec![IonValue::Struct(reading_order)]),
        );
        // $8 - total content item count (for position calculations)
        doc_data.insert(8, IonValue::Int(total_items as i64));
        // $16 - nullable field
        doc_data.insert(16, typed_null());
        // $42 - nullable field
        doc_data.insert(42, typed_null());
        // $112 - direction/layout: $383 (likely "auto" or "ltr")
        doc_data.insert(112, IonValue::Symbol(383));
        // $192 - some mode: $376
        doc_data.insert(192, IonValue::Symbol(376));
        // $436 - some mode: $441
        doc_data.insert(436, IonValue::Symbol(441));
        // $477 - some mode: $56
        doc_data.insert(477, IonValue::Symbol(56));
        // $560 - some mode: $557
        doc_data.insert(560, IonValue::Symbol(557));

        self.fragments.push(KfxFragment::singleton(
            sym::DOCUMENT_DATA,
            IonValue::Struct(doc_data),
        ));
    }

    /// Add book navigation fragment ($389)
    ///
    /// Creates a complete TOC navigation structure with:
    /// - Reading order reference
    /// - Nav container with type=toc
    /// - Nav entries with titles and section targets (using EIDs)
    fn add_book_navigation(&mut self, chapters: &[ChapterData]) {
        // Create nav container ID
        let nav_container_id = "nav-toc";
        let nav_container_sym = self.symtab.get_or_intern(nav_container_id);

        // Build nav entries for each chapter (inline, annotated with $393)
        // Use EIDs that match content blocks and position maps
        let mut nav_entry_values = Vec::new();
        let mut eid_base = 860i64; // Same starting EID as content blocks

        for chapter in chapters.iter() {
            // Nav title: { $244 (text): "Chapter Title" }
            let mut nav_title = HashMap::new();
            nav_title.insert(sym::TEXT, IonValue::String(chapter.title.clone()));

            // Nav target: { $155 (position/eid): eid, $143 (offset): 0 }
            // Use EID of first content block in this chapter (eid_base + 1)
            // eid_base is reserved for section content entry
            let mut nav_target = HashMap::new();
            nav_target.insert(sym::POSITION, IonValue::Int(eid_base + 1));
            nav_target.insert(sym::OFFSET, IonValue::Int(0));

            // Nav entry struct: { $241: nav_title, $246: nav_target }
            let mut nav_entry = HashMap::new();
            nav_entry.insert(sym::NAV_TITLE, IonValue::Struct(nav_title));
            nav_entry.insert(sym::NAV_TARGET, IonValue::Struct(nav_target));

            // Annotate with $393 (nav_definition)
            nav_entry_values.push(IonValue::Annotated(
                vec![sym::NAV_DEFINITION],
                Box::new(IonValue::Struct(nav_entry)),
            ));

            // Advance EID base: +1 for section content entry, + texts.len() for content blocks
            eid_base += 1 + chapter.texts.len() as i64;
        }

        // Nav container: { $235 (nav_type): $212 (toc), $239 (nav_id): nav_container_sym, $247 (nav_entries): [...] }
        let mut nav_container = HashMap::new();
        nav_container.insert(sym::NAV_TYPE, IonValue::Symbol(sym::TOC));
        nav_container.insert(sym::NAV_ID, IonValue::Symbol(nav_container_sym));
        nav_container.insert(sym::NAV_ENTRIES, IonValue::List(nav_entry_values));

        // Annotate nav container with $391 (nav_container_type)
        let annotated_nav_container = IonValue::Annotated(
            vec![sym::NAV_CONTAINER_TYPE],
            Box::new(IonValue::Struct(nav_container)),
        );

        // Book navigation root: { $178 (reading_order_name): $351, $392 (nav_containers): [...] }
        let mut nav = HashMap::new();
        nav.insert(
            sym::READING_ORDER_NAME,
            IonValue::Symbol(sym::DEFAULT_READING_ORDER),
        );
        nav.insert(
            sym::NAV_CONTAINER_REF,
            IonValue::List(vec![annotated_nav_container]),
        );

        self.fragments.push(KfxFragment::singleton(
            sym::BOOK_NAVIGATION,
            IonValue::List(vec![IonValue::Struct(nav)]),
        ));
    }

    /// Add nav unit list fragment ($395)
    /// This is a required placeholder with empty nav entries
    fn add_nav_unit_list(&mut self) {
        let mut nav_unit_list = HashMap::new();
        nav_unit_list.insert(sym::NAV_ENTRIES, IonValue::List(Vec::new()));

        self.fragments.push(KfxFragment::singleton(
            sym::NAV_UNIT_LIST,
            IonValue::Struct(nav_unit_list),
        ));
    }

    /// Add all unique styles found in the book as $157 fragments
    fn add_all_styles(&mut self, chapters: &[ChapterData]) {
        // Collect all unique styles
        let mut unique_styles = std::collections::HashSet::new();
        for chapter in chapters {
            for text in &chapter.texts {
                unique_styles.insert(text.style.clone());
            }
        }

        // Helper to convert CssValue to IonValue
        let css_to_ion = |val: &CssValue| -> Option<IonValue> {
            match val {
                CssValue::Px(v) => {
                    let mut s = HashMap::new();
                    // For Px, use decimal with UNIT_PX ($361)
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PX));
                    Some(IonValue::Struct(s))
                }
                CssValue::Em(v) | CssValue::Rem(v) => {
                    let mut s = HashMap::new();
                    // Kindle uses decimal for em values
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
                    Some(IonValue::Struct(s))
                }
                CssValue::Percent(v) => {
                    let mut s = HashMap::new();
                    s.insert(sym::VALUE, IonValue::Int(*v as i64));
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                    Some(IonValue::Struct(s))
                }
                CssValue::Number(v) => {
                    let mut s = HashMap::new();
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                    // Default to percent for line-height number
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                    Some(IonValue::Struct(s))
                }
                _ => None,
            }
        };

        for (i, style) in unique_styles.into_iter().enumerate() {
            let style_id = format!("style-{}", i);
            let style_sym = self.symtab.get_or_intern(&style_id);

            let mut style_ion = HashMap::new();
            style_ion.insert(sym::STYLE_NAME, IonValue::Symbol(style_sym));

            if let Some(ref family) = style.font_family {
                // Map common generic families to KFX symbols
                let sym = match family.to_lowercase().as_str() {
                    "serif" => sym::FONT_SERIF,
                    _ => sym::FONT_DEFAULT,
                };
                style_ion.insert(sym::FONT_FAMILY, IonValue::Symbol(sym));
            } else {
                style_ion.insert(sym::FONT_FAMILY, IonValue::Symbol(sym::FONT_DEFAULT));
            }

            if let Some(ref size) = style.font_size {
                if let Some(val) = css_to_ion(size) {
                    style_ion.insert(sym::FONT_SIZE, val);
                }
            }

            if let Some(align) = style.text_align {
                let align_sym = match align {
                    TextAlign::Left => sym::ALIGN_LEFT,
                    TextAlign::Right => sym::ALIGN_RIGHT,
                    TextAlign::Center => sym::ALIGN_CENTER,
                    TextAlign::Justify => sym::ALIGN_JUSTIFY,
                };
                style_ion.insert(sym::TEXT_ALIGN, IonValue::Symbol(align_sym));
            }

            if let Some(ref weight) = style.font_weight {
                if weight.is_bold() {
                    style_ion.insert(sym::BOLD, IonValue::Bool(true));
                }
            }

            if let Some(style_type) = style.font_style {
                if matches!(style_type, crate::css::FontStyle::Italic | crate::css::FontStyle::Oblique) {
                    style_ion.insert(sym::ITALIC, IonValue::Bool(true));
                }
            }

            if let Some(ref margin) = style.margin_top {
                if let Some(val) = css_to_ion(margin) {
                    style_ion.insert(sym::MARGIN_TOP, val);
                }
            }
            if let Some(ref margin) = style.margin_bottom {
                if let Some(val) = css_to_ion(margin) {
                    style_ion.insert(sym::MARGIN_BOTTOM, val);
                }
            }
            if let Some(ref margin) = style.margin_left {
                if let Some(val) = css_to_ion(margin) {
                    style_ion.insert(sym::MARGIN_LEFT, val);
                }
            }
            if let Some(ref margin) = style.margin_right {
                if let Some(val) = css_to_ion(margin) {
                    style_ion.insert(sym::MARGIN_RIGHT, val);
                }
            }

            if let Some(ref indent) = style.text_indent {
                if let Some(val) = css_to_ion(indent) {
                    // P48 is text-indent
                    style_ion.insert(48, val);
                }
            }

            if let Some(ref height) = style.line_height {
                if let Some(val) = css_to_ion(height) {
                    style_ion.insert(sym::LINE_HEIGHT, val);
                }
            }

            self.fragments.push(KfxFragment::new(
                sym::STYLE,
                &style_id,
                IonValue::Struct(style_ion),
            ));

            self.style_map.insert(style, style_sym);
        }
    }

    /// Get style symbol for a parsed style
    fn get_style_symbol(&self, style: &ParsedStyle) -> Option<u64> {
        self.style_map.get(style).copied()
    }

    /// Add text content fragment ($145)
    fn add_text_content(&mut self, chapter: &ChapterData) {
        let content_id = format!("content-{}", chapter.id);
        let content_sym = self.symtab.get_or_intern(&content_id);

        let text_values: Vec<IonValue> = chapter
            .texts
            .iter()
            .map(|t| IonValue::String(t.text.clone()))
            .collect();

        let mut content = HashMap::new();
        content.insert(sym::ID, IonValue::Symbol(content_sym));
        content.insert(sym::CONTENT_ARRAY, IonValue::List(text_values));

        self.fragments.push(KfxFragment::new(
            sym::TEXT_CONTENT,
            &content_id,
            IonValue::Struct(content),
        ));
    }

    /// Add content block fragment ($259)
    fn add_content_block(&mut self, chapter: &ChapterData, eid_base: i64) {
        let block_id = format!("block-{}", chapter.id);
        let block_sym = self.symtab.get_or_intern(&block_id);

        let content_id = format!("content-{}", chapter.id);
        let content_sym = self.symtab.get_or_intern(&content_id);

        // Create content items referencing text content
        // Each item gets a unique EID (P155) that matches position maps
        // EIDs start at eid_base + 1 because eid_base is used for section content entry
        let mut content_items = Vec::new();
        for (i, styled_text) in chapter.texts.iter().enumerate() {
            let mut text_ref = HashMap::new();
            text_ref.insert(sym::ID, IonValue::Symbol(content_sym));
            text_ref.insert(sym::TEXT_OFFSET, IonValue::Int(i as i64));

            let mut item = HashMap::new();
            item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH));
            item.insert(sym::TEXT_CONTENT, IonValue::Struct(text_ref));
            // Use consistent EID that matches position maps
            // +1 offset because eid_base is reserved for section content entry
            item.insert(sym::POSITION, IonValue::Int(eid_base + 1 + i as i64));
            // Add style reference
            if let Some(style_sym) = self.get_style_symbol(&styled_text.style) {
                item.insert(sym::STYLE, IonValue::Symbol(style_sym));
            }

            content_items.push(IonValue::Struct(item));
        }

        let mut block = HashMap::new();
        block.insert(sym::CONTENT_NAME, IonValue::Symbol(block_sym));
        block.insert(sym::CONTENT_ARRAY, IonValue::List(content_items));

        self.fragments.push(KfxFragment::new(
            sym::CONTENT_BLOCK,
            &block_id,
            IonValue::Struct(block),
        ));
    }

    /// Add section fragment ($260)
    fn add_section(&mut self, chapter: &ChapterData, eid_base: i64) {
        let section_id = format!("section-{}", chapter.id);
        let section_sym = self.symtab.get_or_intern(&section_id);

        let block_id = format!("block-{}", chapter.id);
        let block_sym = self.symtab.get_or_intern(&block_id);

        let mut content_ref = HashMap::new();
        // Use the first EID of this section
        content_ref.insert(sym::POSITION, IonValue::Int(eid_base));
        content_ref.insert(sym::CONTENT_NAME, IonValue::Symbol(block_sym));

        let mut section = HashMap::new();
        section.insert(sym::SECTION_NAME, IonValue::Symbol(section_sym));
        section.insert(
            sym::SECTION_CONTENT,
            IonValue::List(vec![IonValue::Struct(content_ref)]),
        );

        self.fragments.push(KfxFragment::new(
            sym::SECTION,
            &section_id,
            IonValue::Struct(section),
        ));
    }

    /// Add auxiliary data fragment ($597) for section metadata
    fn add_auxiliary_data(&mut self, chapter: &ChapterData) {
        let aux_id = format!("section-{}-ad", chapter.id);
        let section_id = format!("section-{}", chapter.id);
        let section_sym = self.symtab.get_or_intern(&section_id);

        let mut meta_entry = HashMap::new();
        meta_entry.insert(
            sym::METADATA_KEY,
            IonValue::String("IS_TARGET_SECTION".to_string()),
        );
        meta_entry.insert(sym::VALUE, IonValue::Bool(true));

        let mut aux = HashMap::new();
        aux.insert(sym::AUX_DATA_REF, IonValue::Symbol(section_sym));
        aux.insert(
            sym::METADATA,
            IonValue::List(vec![IonValue::Struct(meta_entry)]),
        );

        self.fragments.push(KfxFragment::new(
            sym::AUXILIARY_DATA,
            &aux_id,
            IonValue::Struct(aux),
        ));
    }

    /// Add position map fragment ($264)
    /// Maps each section to the list of EIDs it contains
    /// Structure: [ {$181: (section_eid, content_eid1, content_eid2, ...), $174: section_sym}, ... ]
    fn add_position_map(&mut self, chapters: &[ChapterData]) {
        let mut entries = Vec::new();
        let mut eid_base = 860i64; // Start EIDs after local symbol range

        for chapter in chapters {
            let section_id = format!("section-{}", chapter.id);
            let section_sym = self.symtab.get_or_intern(&section_id);

            // Generate list of EIDs for this section
            // Section content entry EID first, then content block EIDs
            let mut eids = Vec::new();
            eids.push(IonValue::Int(eid_base)); // Section content entry EID
            for i in 0..chapter.texts.len() {
                eids.push(IonValue::Int(eid_base + 1 + i as i64)); // Content block EIDs
            }

            let mut entry = HashMap::new();
            entry.insert(sym::ENTITY_ID_LIST, IonValue::List(eids));
            entry.insert(sym::SECTION_NAME, IonValue::Symbol(section_sym));
            entries.push(IonValue::Struct(entry));

            // +1 for section content entry, + texts.len() for content blocks
            eid_base += 1 + chapter.texts.len() as i64;
        }

        self.fragments.push(KfxFragment::singleton(
            sym::POSITION_MAP,
            IonValue::List(entries),
        ));
    }

    /// Add position ID map fragment ($265)
    /// Maps character offsets to EIDs
    /// Structure: [ {$184: char_offset, $185: eid}, ..., {$184: total_chars, $185: 0} ]
    /// Includes section content entry at position 0 (1 char), then content block entries
    fn add_position_id_map(&mut self, chapters: &[ChapterData]) {
        let mut entries = Vec::new();
        let mut char_offset = 0i64;
        let mut eid_base = 860i64;

        for chapter in chapters {
            // Section content entry at current position (1 char)
            let section_eid = eid_base;
            let mut section_entry = HashMap::new();
            section_entry.insert(sym::EID_INDEX, IonValue::Int(char_offset));
            section_entry.insert(sym::EID_VALUE, IonValue::Int(section_eid));
            entries.push(IonValue::Struct(section_entry));
            char_offset += 1; // Section content entry takes 1 char

            // Content block entries
            for (i, styled_text) in chapter.texts.iter().enumerate() {
                let content_eid = eid_base + 1 + i as i64;

                let mut entry = HashMap::new();
                entry.insert(sym::EID_INDEX, IonValue::Int(char_offset));
                entry.insert(sym::EID_VALUE, IonValue::Int(content_eid));
                entries.push(IonValue::Struct(entry));

                // Add text length
                char_offset += styled_text.text.len() as i64;
            }

            // +1 for section content entry, + texts.len() for content blocks
            eid_base += 1 + chapter.texts.len() as i64;
        }

        // Add end marker with EID 0
        let mut end_entry = HashMap::new();
        end_entry.insert(sym::EID_INDEX, IonValue::Int(char_offset));
        end_entry.insert(sym::EID_VALUE, IonValue::Int(0));
        entries.push(IonValue::Struct(end_entry));

        self.fragments.push(KfxFragment::singleton(
            sym::POSITION_ID_MAP,
            IonValue::List(entries),
        ));
    }

    /// Add location map fragment ($550)
    /// This creates the "virtual pages" for reading progress - needs granular entries
    fn add_location_map(&mut self, chapters: &[ChapterData]) {
        // Location map structure: [ { P182: ( {P155: eid, P143: offset}, ... ) } ]
        // Include entries for each content block to enable granular reading positions
        let mut location_entries = Vec::new();
        let mut eid_base = 860i64;

        for chapter in chapters {
            // Section content entry EID at offset 0
            let section_eid = eid_base;
            let mut section_entry = HashMap::new();
            section_entry.insert(sym::POSITION, IonValue::Int(section_eid));
            section_entry.insert(sym::OFFSET, IonValue::Int(0));
            location_entries.push(IonValue::Struct(section_entry));

            // Content block entries - each paragraph gets its own location entry
            // This provides granular reading position tracking
            let mut char_offset = 0i64;
            for (i, styled_text) in chapter.texts.iter().enumerate() {
                let content_eid = eid_base + 1 + i as i64;

                let mut entry = HashMap::new();
                entry.insert(sym::POSITION, IonValue::Int(content_eid));
                entry.insert(sym::OFFSET, IonValue::Int(char_offset));
                location_entries.push(IonValue::Struct(entry));

                char_offset += styled_text.text.len() as i64;
            }

            // +1 for section content entry, + texts.len() for content blocks
            eid_base += 1 + chapter.texts.len() as i64;
        }

        // Wrap in { P182: entries }
        let mut wrapper = HashMap::new();
        wrapper.insert(sym::LOCATION_ENTRIES, IonValue::List(location_entries));

        self.fragments.push(KfxFragment::singleton(
            sym::LOCATION_MAP,
            IonValue::List(vec![IonValue::Struct(wrapper)]),
        ));
    }

    /// Add media resources (images and fonts) from the book
    /// Creates P164 (resource metadata) and P417 (raw media) fragments
    fn add_resources(&mut self, book: &Book) {
        let mut resource_index = 0;

        for resource in book.resources.values() {
            let is_image = is_image_media_type(&resource.media_type);
            let is_font = is_font_media_type(&resource.media_type);

            if !is_image && !is_font {
                continue;
            }

            let resource_id = format!("rsrc{}", resource_index);
            let resource_sym = self.symtab.get_or_intern(&resource_id);

            // Create P164 resource fragment
            let mut res_meta = HashMap::new();
            res_meta.insert(sym::RESOURCE_NAME, IonValue::Symbol(resource_sym));
            res_meta.insert(
                sym::MIME_TYPE,
                IonValue::String(resource.media_type.clone()),
            );
            res_meta.insert(
                sym::LOCATION,
                IonValue::String(format!("resource/{}", resource_id)),
            );

            if is_image {
                res_meta.insert(sym::FORMAT, IonValue::Symbol(sym::IMAGE_FORMAT));
                // Get image dimensions
                let (width, height) = get_image_dimensions(&resource.data).unwrap_or((800, 600));
                res_meta.insert(sym::WIDTH, IonValue::Int(width as i64));
                res_meta.insert(sym::HEIGHT, IonValue::Int(height as i64));
            } else if is_font {
                res_meta.insert(sym::FORMAT, IonValue::Symbol(sym::FONT_FORMAT));
            }

            self.fragments.push(KfxFragment::new(
                sym::RESOURCE,
                &resource_id,
                IonValue::Struct(res_meta),
            ));

            // Create P417 raw media fragment with actual data
            let media_id = format!("{}-media", resource_id);
            self.fragments.push(KfxFragment::new(
                sym::RAW_MEDIA,
                &media_id,
                IonValue::Blob(resource.data.clone()),
            ));

            resource_index += 1;
        }
    }

    /// Add container entity map fragment ($419)
    fn add_container_entity_map(&mut self) {
        // Collect all fragment IDs
        let entity_ids: Vec<IonValue> = self
            .fragments
            .iter()
            .filter(|f| !f.is_singleton())
            .map(|f| {
                let sym_id = self.symtab.get_or_intern(&f.fid);
                IonValue::Symbol(sym_id)
            })
            .collect();

        // P155 should be a STRING containing the container ID, not a symbol
        let mut container_contents = HashMap::new();
        container_contents.insert(sym::POSITION, IonValue::String(self.container_id.clone()));
        container_contents.insert(sym::ENTITY_LIST, IonValue::List(entity_ids));

        let mut entity_map = HashMap::new();
        entity_map.insert(
            sym::CONTAINER_CONTENTS,
            IonValue::List(vec![IonValue::Struct(container_contents)]),
        );

        self.fragments.push(KfxFragment::singleton(
            sym::CONTAINER_ENTITY_MAP,
            IonValue::Struct(entity_map),
        ));
    }

    /// Build and serialize to bytes
    pub fn build(self) -> Vec<u8> {
        // CONTAINER_FRAGMENT_TYPES that go in header, not as entities:
        // - $270 (container info) - used to extract container_id, versions
        // - $593 (format capabilities) - serialized to header section
        // - $ion_symbol_table (type 3) - serialized to header section
        // Exception: $419 (container entity map) IS serialized as an entity

        // Extract symbol table from fragment (type 3 = $ion_symbol_table)
        let symtab_ion = self
            .fragments
            .iter()
            .find(|f| f.ftype == 3)
            .map(|f| serialize_annotated_ion(3, &f.value))
            .unwrap_or_else(|| {
                // Fallback: generate inline
                let symtab_value = self.symtab.create_import();
                serialize_annotated_ion(3, &symtab_value)
            });

        // Generate format capabilities ($593) for header
        // This is separate from the $585 entity and goes in the container header
        let format_caps_ion = {
            // { $492: "kfxgen.textBlock", $5: 1 }
            let mut cap = HashMap::new();
            cap.insert(
                sym::METADATA_KEY,
                IonValue::String("kfxgen.textBlock".to_string()),
            );
            cap.insert(5, IonValue::Int(1));

            let caps_list = IonValue::List(vec![IonValue::Struct(cap)]);
            serialize_annotated_ion(sym::FORMAT_CAPABILITIES, &caps_list)
        };

        // Serialize content entities
        // Skip: $270 (container info), $593 (format caps), $ion_symbol_table (type 3)
        // Include: $419 (container entity map) and all other fragments
        let mut entities: Vec<SerializedEntity> = Vec::new();

        for frag in &self.fragments {
            // Skip header-only fragments
            if frag.ftype == sym::FORMAT_CAPABILITIES
                || frag.ftype == sym::CONTAINER_INFO
                || frag.ftype == 3
            // $ion_symbol_table
            {
                continue;
            }

            let entity_id = if frag.is_singleton() {
                sym::SINGLETON_ID as u32
            } else {
                self.symtab.get(&frag.fid).unwrap_or(sym::SINGLETON_ID) as u32
            };

            // Raw media (P417) fragments should use raw bytes, not ION-encoded
            let data = if frag.ftype == sym::RAW_MEDIA {
                if let IonValue::Blob(bytes) = &frag.value {
                    create_raw_media_data(bytes)
                } else {
                    create_entity_data(&frag.value)
                }
            } else {
                create_entity_data(&frag.value)
            };

            entities.push(SerializedEntity {
                id: entity_id,
                entity_type: frag.ftype as u32,
                data,
            });
        }

        // Build container with proper header structure
        serialize_container_v2(&self.container_id, &entities, &symtab_ion, &format_caps_ion)
    }
}

impl Default for KfxBookBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Chapter Data Helper
// =============================================================================

struct ChapterData {
    id: String,
    title: String,
    texts: Vec<StyledText>,
}

/// Text content with associated style
#[derive(Debug, Clone)]
struct StyledText {
    /// The actual text content
    text: String,
    /// The computed CSS style
    style: ParsedStyle,
}

// =============================================================================
// Serialization
// =============================================================================

struct SerializedEntity {
    id: u32,
    entity_type: u32,
    data: Vec<u8>,
}

/// Container magic bytes
const CONTAINER_MAGIC: &[u8; 4] = b"CONT";

/// Entity magic bytes
const ENTITY_MAGIC: &[u8; 4] = b"ENTY";

/// Serialize a complete KFX container with proper header structure
///
/// Container layout:
/// - Header: CONT magic + version + header_len + ci_offset + ci_len
/// - Entity table (at offset 18, indexed by $413/$414)
/// - Doc symbols ION (indexed by $415/$416)
/// - Format capabilities ION (indexed by $594/$595)
/// - Container info ION
/// - kfxgen_info JSON
/// - Entity payloads (after header_len)
fn serialize_container_v2(
    container_id: &str,
    entities: &[SerializedEntity],
    symtab_ion: &[u8],
    format_caps_ion: &[u8],
) -> Vec<u8> {
    // Build entity table and calculate payload offsets
    let mut entity_table = Vec::new();
    let mut current_offset = 0u64;

    for entity in entities {
        entity_table.extend_from_slice(&entity.id.to_le_bytes());
        entity_table.extend_from_slice(&entity.entity_type.to_le_bytes());
        entity_table.extend_from_slice(&current_offset.to_le_bytes());
        entity_table.extend_from_slice(&(entity.data.len() as u64).to_le_bytes());
        current_offset += entity.data.len() as u64;
    }

    // Calculate SHA1 of entity payloads for kfxgen_info
    let mut entity_data = Vec::new();
    for entity in entities {
        entity_data.extend_from_slice(&entity.data);
    }
    let payload_sha1 = sha1_hex(&entity_data);

    // Header is 18 bytes: magic(4) + version(2) + header_len(4) + ci_offset(4) + ci_len(4)
    const HEADER_SIZE: usize = 18;

    // Calculate offsets within the header section (after the 18-byte fixed header)
    let entity_table_offset = HEADER_SIZE;
    let symtab_offset = entity_table_offset + entity_table.len();
    let format_caps_offset = symtab_offset + symtab_ion.len();

    // Build container info with all the offset pointers
    let mut container_info = HashMap::new();
    container_info.insert(
        sym::CONTAINER_ID,
        IonValue::String(container_id.to_string()),
    );
    container_info.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
    container_info.insert(sym::DRM_SCHEME, IonValue::Int(0));
    container_info.insert(sym::CHUNK_SIZE, IonValue::Int(4096));
    container_info.insert(
        sym::INDEX_TABLE_OFFSET,
        IonValue::Int(entity_table_offset as i64),
    );
    container_info.insert(
        sym::INDEX_TABLE_LENGTH,
        IonValue::Int(entity_table.len() as i64),
    );
    container_info.insert(
        sym::SYMBOL_TABLE_OFFSET,
        IonValue::Int(symtab_offset as i64),
    );
    container_info.insert(
        sym::SYMBOL_TABLE_LENGTH,
        IonValue::Int(symtab_ion.len() as i64),
    );

    // Only include format capabilities offset if we have them
    if !format_caps_ion.is_empty() {
        container_info.insert(sym::FC_OFFSET, IonValue::Int(format_caps_offset as i64));
        container_info.insert(sym::FC_LENGTH, IonValue::Int(format_caps_ion.len() as i64));
    }

    let mut ion_writer = IonWriter::new();
    ion_writer.write_bvm();
    ion_writer.write_value(&IonValue::Struct(container_info));
    let container_info_data = ion_writer.into_bytes();

    let container_info_offset = format_caps_offset + format_caps_ion.len();

    // kfxgen info JSON (matches Amazon's format)
    let kfxgen_info = format!(
        r#"[{{key:kfxgen_package_version,value:boko-{}}},{{key:kfxgen_application_version,value:boko}},{{key:kfxgen_payload_sha1,value:{}}},{{key:kfxgen_acr,value:{}}}]"#,
        env!("CARGO_PKG_VERSION"),
        payload_sha1,
        container_id
    );

    let header_len = container_info_offset + container_info_data.len() + kfxgen_info.len();

    // Build output
    let mut output = Vec::new();

    // Fixed header (18 bytes)
    output.extend_from_slice(CONTAINER_MAGIC);
    output.extend_from_slice(&2u16.to_le_bytes()); // version
    output.extend_from_slice(&(header_len as u32).to_le_bytes());
    output.extend_from_slice(&(container_info_offset as u32).to_le_bytes());
    output.extend_from_slice(&(container_info_data.len() as u32).to_le_bytes());

    // Entity table
    output.extend_from_slice(&entity_table);

    // Doc symbols (symbol table)
    output.extend_from_slice(symtab_ion);

    // Format capabilities
    output.extend_from_slice(format_caps_ion);

    // Container info
    output.extend_from_slice(&container_info_data);

    // kfxgen info JSON
    output.extend_from_slice(kfxgen_info.as_bytes());

    // Entity payloads (after header)
    output.extend_from_slice(&entity_data);

    output
}

/// Compute SHA1 hash as hex string
fn sha1_hex(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Simple hash for now (real implementation would use SHA1)
    // This is just for the kfxgen_info field which is informational
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let hash = hasher.finish();
    format!(
        "{:016x}{:016x}{:08x}",
        hash,
        hash.rotate_left(32),
        (hash >> 32) as u32
    )
}

/// Create raw media entity data (for P417)
/// Raw media stores image bytes directly without ION encoding
fn create_raw_media_data(raw_bytes: &[u8]) -> Vec<u8> {
    // Entity header ION: {$410:0, $411:0}
    let mut header_fields = HashMap::new();
    header_fields.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
    header_fields.insert(sym::DRM_SCHEME, IonValue::Int(0));

    let mut header_writer = IonWriter::new();
    header_writer.write_bvm();
    header_writer.write_value(&IonValue::Struct(header_fields));
    let header_ion = header_writer.into_bytes();

    // ENTY header: magic(4) + version(2) + header_len(4) = 10
    let header_len = 10 + header_ion.len();

    let mut data = Vec::new();
    data.extend_from_slice(ENTITY_MAGIC);
    data.extend_from_slice(&1u16.to_le_bytes()); // version
    data.extend_from_slice(&(header_len as u32).to_le_bytes());
    data.extend_from_slice(&header_ion);
    // Raw bytes directly, not ION-encoded
    data.extend_from_slice(raw_bytes);

    data
}

/// Create entity data with ENTY header
fn create_entity_data(value: &IonValue) -> Vec<u8> {
    // Entity header ION: {$410:0, $411:0}
    let mut header_fields = HashMap::new();
    header_fields.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
    header_fields.insert(sym::DRM_SCHEME, IonValue::Int(0));

    let mut header_writer = IonWriter::new();
    header_writer.write_bvm();
    header_writer.write_value(&IonValue::Struct(header_fields));
    let header_ion = header_writer.into_bytes();

    // Content ION
    let mut content_writer = IonWriter::new();
    content_writer.write_bvm();
    content_writer.write_value(value);
    let content_ion = content_writer.into_bytes();

    // ENTY header: magic(4) + version(2) + header_len(4) = 10
    let header_len = 10 + header_ion.len();

    let mut data = Vec::new();
    data.extend_from_slice(ENTITY_MAGIC);
    data.extend_from_slice(&1u16.to_le_bytes()); // version
    data.extend_from_slice(&(header_len as u32).to_le_bytes());
    data.extend_from_slice(&header_ion);
    data.extend_from_slice(&content_ion);

    data
}

/// Serialize an annotated ION value (for $ion_symbol_table and $593)
fn serialize_annotated_ion(annotation_id: u64, value: &IonValue) -> Vec<u8> {
    let annotated = IonValue::Annotated(vec![annotation_id], Box::new(value.clone()));

    let mut writer = IonWriter::new();
    writer.write_bvm();
    writer.write_value(&annotated);
    writer.into_bytes()
}

/// Generate a unique container ID
fn generate_container_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let mut state = seed;
    let chars: Vec<char> = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().collect();
    let mut id = String::from("CR!");

    for _ in 0..28 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = ((state >> 56) as usize) % chars.len();
        id.push(chars[idx]);
    }

    id
}

// =============================================================================
// Public API
// =============================================================================

/// Write a KFX file from a Book
pub fn write_kfx(book: &Book, path: impl AsRef<Path>) -> io::Result<()> {
    let file = File::create(path)?;
    write_kfx_to_writer(book, BufWriter::new(file))
}

/// Write KFX to any writer
pub fn write_kfx_to_writer<W: Write>(book: &Book, mut writer: W) -> io::Result<()> {
    let builder = KfxBookBuilder::from_book(book);
    let data = builder.build();
    writer.write_all(&data)
}

// =============================================================================
// Text Extraction
// =============================================================================

/// Extract styled text content from XHTML, preserving styles from CSS
fn extract_styled_text_from_xhtml(data: &[u8], stylesheet: &Stylesheet) -> Vec<StyledText> {
    let html = String::from_utf8_lossy(data);
    let mut result = Vec::new();

    let mut reader = Reader::from_str(&html);
    reader.config_mut().trim_text(true);

    let mut current_text = String::new();

    // We need to track the current computed style based on inheritance
    let mut style_stack: Vec<ParsedStyle> = vec![ParsedStyle::default()];

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();

                // Skip non-content tags
                if matches!(tag_name.as_str(), "script" | "style" | "head" | "title" | "svg") {
                    reader.read_to_end_into(e.name(), &mut Vec::new()).ok();
                    continue;
                }

                // Push current text if any
                if !current_text.is_empty() {
                    let text = clean_text(&current_text);
                    if !text.is_empty() {
                        result.push(StyledText {
                            text,
                            style: style_stack.last().cloned().unwrap_or_default(),
                        });
                    }
                    current_text.clear();
                }

                // Extract class and id
                let mut classes = Vec::new();
                let mut id = None;
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"class" {
                        let class_val = String::from_utf8_lossy(&attr.value).to_string();
                        classes.extend(class_val.split_whitespace().map(|s| s.to_string()));
                    } else if attr.key.as_ref() == b"id" {
                        id = Some(String::from_utf8_lossy(&attr.value).to_string());
                    }
                }

                // Compute style for this element
                let class_refs: Vec<&str> = classes.iter().map(|s| s.as_str()).collect();
                let element_style =
                    stylesheet.compute_style(&tag_name, &class_refs, id.as_deref());

                // Merge with parent style for inheritance
                let mut inherited_style = style_stack.last().cloned().unwrap_or_default();
                inherited_style.merge(&element_style);
                style_stack.push(inherited_style);
            }
            Ok(Event::End(_)) => {
                // Push current text if any
                if !current_text.is_empty() {
                    let text = clean_text(&current_text);
                    if !text.is_empty() {
                        result.push(StyledText {
                            text,
                            style: style_stack.last().cloned().unwrap_or_default(),
                        });
                    }
                    current_text.clear();
                }
                if style_stack.len() > 1 {
                    style_stack.pop();
                }
            }
            Ok(Event::Empty(e)) => {
                // Handle empty tags like <br/>
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_lowercase();
                if tag_name == "br" {
                    if !current_text.is_empty() {
                        let text = clean_text(&current_text);
                        if !text.is_empty() {
                            result.push(StyledText {
                                text,
                                style: style_stack.last().cloned().unwrap_or_default(),
                            });
                        }
                        current_text.clear();
                    }
                }
            }
            Ok(Event::Text(e)) => {
                current_text.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }

    // Save any remaining text
    let text = clean_text(&current_text);
    if !text.is_empty() {
        result.push(StyledText {
            text,
            style: style_stack.last().cloned().unwrap_or_default(),
        });
    }

    result
}

/// Clean up text by normalizing whitespace
fn clean_text(text: &str) -> String {
    let decoded = decode_html_entities(text);
    let mut cleaned = String::new();
    let mut last_was_space = true;

    for ch in decoded.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                cleaned.push(' ');
                last_was_space = true;
            }
        } else {
            cleaned.push(ch);
            last_was_space = false;
        }
    }

    cleaned.trim().to_string()
}

/// Decode common HTML entities
fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#8217;", "'")
        .replace("&#8220;", "\"")
        .replace("&#8221;", "\"")
        .replace("&#160;", " ")
        .replace("&nbsp;", " ")
}

// =============================================================================
// Image Handling
// =============================================================================

/// Check if a media type is an image
fn is_image_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/jpeg" | "image/jpg" | "image/png" | "image/gif" | "image/webp"
    )
}

/// Check if a media type is a font
fn is_font_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "font/ttf"
            | "font/otf"
            | "font/woff"
            | "font/woff2"
            | "application/font-sfnt"
            | "application/x-font-ttf"
            | "application/x-font-otf"
            | "application/font-woff"
            | "application/font-woff2"
            | "application/vnd.ms-opentype"
    )
}

/// Get image dimensions from raw bytes (basic parsing for JPEG/PNG)
fn get_image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 {
        return None;
    }

    // PNG: starts with 89 50 4E 47 0D 0A 1A 0A
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        // IHDR chunk at offset 8, width at 16, height at 20 (big-endian)
        if data.len() >= 24 {
            let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
            let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
            return Some((width, height));
        }
    }

    // JPEG: starts with FF D8 FF
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        // Parse JPEG markers to find SOF0/SOF2 frame
        let mut pos = 2;
        while pos + 4 < data.len() {
            if data[pos] != 0xFF {
                pos += 1;
                continue;
            }
            let marker = data[pos + 1];
            if marker == 0xD9 {
                break; // End of image
            }
            if marker == 0xC0 || marker == 0xC2 {
                // SOF0 or SOF2 - contains dimensions
                if pos + 9 < data.len() {
                    let height = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;
                    let width = u16::from_be_bytes([data[pos + 7], data[pos + 8]]) as u32;
                    return Some((width, height));
                }
            }
            // Skip to next marker
            if pos + 3 < data.len() {
                let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
                pos += 2 + len;
            } else {
                break;
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table() {
        let mut symtab = SymbolTable::new();

        // Shared symbols should return their ID
        assert_eq!(symtab.get_or_intern("$260"), 260);
        assert_eq!(symtab.get_or_intern("$145"), 145);

        // Local symbols should get new IDs
        let id1 = symtab.get_or_intern("section-1");
        let id2 = symtab.get_or_intern("section-2");
        assert!(id1 >= SymbolTable::LOCAL_MIN_ID);
        assert_eq!(id2, id1 + 1);

        // Same symbol should return same ID
        assert_eq!(symtab.get_or_intern("section-1"), id1);
    }

    #[test]
    fn test_fragment_singleton() {
        let frag = KfxFragment::singleton(sym::DOCUMENT_DATA, IonValue::Null);
        assert!(frag.is_singleton());
        assert_eq!(frag.fid, "$538");
    }

    #[test]
    fn test_container_id_format() {
        let id = generate_container_id();
        assert!(id.starts_with("CR!"));
        assert_eq!(id.len(), 31); // CR! + 28 chars
    }
}
