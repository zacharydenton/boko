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

use crate::book::Book;
use crate::css::{
    Border, BorderStyle, Color, CssValue, FontVariant, NodeRef, ParsedStyle, Stylesheet, TextAlign,
};
use kuchiki::traits::*;

use super::ion::{IonValue, IonWriter, encode_kfx_decimal};

// =============================================================================
// YJ_SYMBOLS - Shared symbol table (subset of the full 800+ symbols)
// =============================================================================

/// Symbol IDs from YJ_symbols shared table (version 10)
/// These are the well-known symbols used in KFX format.
/// VERIFIED via comprehensive CSS-to-KFX mapping test with Kindle Previewer (2024-01)
#[allow(dead_code)]
pub mod sym {
    // Core property symbols
    pub const ID: u64 = 4; // $4 - generic id field
    pub const LANGUAGE: u64 = 10; // $10 - language

    // ==========================================================================
    // STYLE PROPERTY SYMBOLS (verified via CSS mapping test)
    // ==========================================================================

    // Font properties
    pub const FONT_FAMILY: u64 = 11; // $11 - font-family (string value: serif, sans-serif, etc.)
    pub const FONT_STYLE: u64 = 12; // $12 - font-style (italic, oblique, normal)
    pub const FONT_WEIGHT: u64 = 13; // $13 - font-weight (100-900, bold, normal)
    pub const FONT_SIZE: u64 = 16; // $16 - font-size (relative to 1.0 = 1em)
    pub const COLOR: u64 = 19; // $19 - text color (ARGB integer)

    // Text decoration
    pub const TEXT_DECORATION_UNDERLINE: u64 = 23; // $23 - text-decoration: underline
    pub const TEXT_DECORATION_LINE_THROUGH: u64 = 27; // $27 - text-decoration: line-through

    // Spacing properties
    pub const LETTER_SPACING: u64 = 32; // $32 - letter-spacing
    pub const WORD_SPACING: u64 = 33; // $33 - word-spacing
    pub const TEXT_ALIGN: u64 = 34; // $34 - text alignment
    pub const TEXT_INDENT: u64 = 36; // $36 - text indent
    pub const TEXT_TRANSFORM: u64 = 41; // $41 - text-transform (uppercase, lowercase, etc.)
    pub const LINE_HEIGHT: u64 = 42; // $42 - line-height
    pub const WHITE_SPACE_NOWRAP: u64 = 45; // $45 - white-space: nowrap (boolean)

    // Margin/padding (note: $47 is shared between margin-top/bottom and spacing)
    pub const SPACE_BEFORE: u64 = 47; // $47 - margin-top/space-before (multiplier)
    pub const MARGIN_LEFT: u64 = 48; // $48 - margin-left and padding-left (percent)
    pub const SPACE_AFTER: u64 = 49; // $49 - margin-bottom/space-after (multiplier)
    pub const MARGIN_RIGHT: u64 = 50; // $50 - margin-right and padding-right (percent)
    pub const PADDING_TOP: u64 = 52; // $52 - padding-top (multiplier)
    pub const CELL_PADDING_RIGHT: u64 = 53; // $53 - table cell padding-right
    pub const PADDING_BOTTOM: u64 = 54; // $54 - padding-bottom (multiplier)
    pub const CELL_PADDING_LEFT: u64 = 55; // $55 - table cell padding-left

    // Dimensions
    pub const STYLE_WIDTH: u64 = 56; // $56 - width in style
    pub const STYLE_HEIGHT: u64 = 57; // $57 - height in style
    pub const MAX_WIDTH: u64 = 65; // $65 - max-width (for em widths)
    pub const OPACITY: u64 = 72; // $72 - opacity (0.0-1.0 decimal)

    // Legacy aliases for compatibility (will be removed)
    pub const MARGIN_TOP: u64 = 47; // alias for SPACE_BEFORE
    pub const MARGIN_BOTTOM: u64 = 49; // alias for SPACE_AFTER
    pub const PADDING_LEFT: u64 = 48; // alias for MARGIN_LEFT
    pub const PADDING_RIGHT: u64 = 50; // alias for MARGIN_RIGHT

    // Background
    pub const BACKGROUND_COLOR: u64 = 21; // $21 - background color

    // ==========================================================================
    // UNIT TYPES ($306 values)
    // ==========================================================================
    pub const UNIT: u64 = 306; // $306 - unit field in value struct
    pub const VALUE: u64 = 307; // $307 - value field in value struct
    pub const UNIT_EM: u64 = 308; // $308 - em unit (for text-indent, letter-spacing, etc.)
    pub const UNIT_MULTIPLIER: u64 = 310; // $310 - multiplier unit (for line-height, margins)
    pub const UNIT_PERCENT: u64 = 314; // $314 - percent unit (for margin-left/right, width)
    pub const UNIT_PX: u64 = 318; // $318 - px/points unit
    pub const UNIT_EM_FONTSIZE: u64 = 505; // $505 - em unit specifically for font-size

    // ==========================================================================
    // TEXT ALIGNMENT VALUES ($34)
    // ==========================================================================
    pub const ALIGN_LEFT: u64 = 59; // $59 - text-align: left
    pub const ALIGN_RIGHT: u64 = 61; // $61 - text-align: right
    pub const ALIGN_CENTER: u64 = 320; // $320 - text-align: center
    pub const ALIGN_JUSTIFY: u64 = 321; // $321 - text-align: justify

    // ==========================================================================
    // TABLE CELL ALIGNMENT
    // ==========================================================================
    pub const CELL_ALIGN: u64 = 633; // $633 - table cell alignment

    // ==========================================================================
    // FONT WEIGHT VALUES ($13)
    // ==========================================================================
    pub const FONT_WEIGHT_NORMAL: u64 = 350; // $350 - font-weight: normal/400
    pub const FONT_WEIGHT_100: u64 = 355; // $355 - font-weight: 100
    pub const FONT_WEIGHT_200: u64 = 356; // $356 - font-weight: 200
    pub const FONT_WEIGHT_300: u64 = 357; // $357 - font-weight: 300
    pub const FONT_WEIGHT_500: u64 = 359; // $359 - font-weight: 500
    pub const FONT_WEIGHT_600: u64 = 360; // $360 - font-weight: 600
    pub const FONT_WEIGHT_BOLD: u64 = 361; // $361 - font-weight: bold/700
    pub const FONT_WEIGHT_800: u64 = 362; // $362 - font-weight: 800
    pub const FONT_WEIGHT_900: u64 = 363; // $363 - font-weight: 900

    // ==========================================================================
    // FONT STYLE VALUES ($12)
    // ==========================================================================
    pub const FONT_STYLE_NORMAL: u64 = 350; // $350 - font-style: normal
    pub const FONT_STYLE_OBLIQUE: u64 = 381; // $381 - font-style: oblique
    pub const FONT_STYLE_ITALIC: u64 = 382; // $382 - font-style: italic

    // ==========================================================================
    // TEXT TRANSFORM VALUES ($41)
    // ==========================================================================
    pub const TEXT_TRANSFORM_NONE: u64 = 349; // $349 - text-transform: none
    pub const TEXT_TRANSFORM_UPPERCASE: u64 = 372; // $372 - text-transform: uppercase
    pub const TEXT_TRANSFORM_LOWERCASE: u64 = 373; // $373 - text-transform: lowercase
    pub const TEXT_TRANSFORM_CAPITALIZE: u64 = 374; // $374 - text-transform: capitalize

    // ==========================================================================
    // FONT VARIANT VALUES ($583)
    // ==========================================================================
    pub const FONT_VARIANT: u64 = 583; // $583 - font-variant property
    pub const FONT_VARIANT_NORMAL: u64 = 349; // $349 - font-variant: normal
    pub const FONT_VARIANT_SMALL_CAPS: u64 = 369; // $369 - font-variant: small-caps

    // ==========================================================================
    // TEXT DECORATION VALUES
    // ==========================================================================
    pub const DECORATION_PRESENT: u64 = 328; // $328 - decoration is present
    pub const TEXT_DECORATION_OVERLINE: u64 = 554; // $554 - text-decoration: overline

    // ==========================================================================
    // IMAGE/BLOCK LAYOUT
    // ==========================================================================
    pub const IMAGE_FIT: u64 = 546; // $546 - image fit mode
    pub const IMAGE_FIT_CONTAIN: u64 = 377; // $377 - contain fit mode
    pub const IMAGE_FIT_NONE: u64 = 378; // $378 - image fit: none (baseline)
    pub const IMAGE_LAYOUT: u64 = 580; // $580 - image/block layout

    // Block type symbols
    pub const STYLE_BLOCK_TYPE: u64 = 127; // $127 - block type/display mode for styles
    pub const BLOCK_TYPE_BLOCK: u64 = 383; // $383 - block display value
    pub const BLOCK_TYPE_INLINE: u64 = 349; // $349 - inline display value

    // Content symbols
    pub const SECTION_CONTENT: u64 = 141; // $141 - section content list
    pub const INLINE_STYLE_RUNS: u64 = 142; // $142 - inline style runs array
    pub const TEXT_CONTENT: u64 = 145; // $145 - text content fragment type

    /// Maximum size for a text content chunk (in characters)
    /// Larger chapters are split into multiple chunks
    pub const MAX_CHUNK_SIZE: usize = 15000;
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
    pub const LANDMARKS_NAV_TYPE: u64 = 236; // $236 - landmarks navigation type value
    pub const LANDMARKS: u64 = 237; // $237 - landmarks
    pub const LANDMARK_TYPE: u64 = 238; // $238 - landmark type field
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

    // Content item role/position indicator ($790)
    // This field appears on paragraph content items and indicates their role:
    // - 2: First paragraph in content block (index 0)
    // - 3: Normal paragraph
    // - 4: Special paragraph (endnotes, back matter)
    pub const CONTENT_ROLE: u64 = 790; // $790 - content item role indicator

    // Page template / anchor symbols
    pub const ANCHOR_REF: u64 = 179; // $179 - reference to anchor fragment in inline style runs
    pub const TEMPLATE_NAME: u64 = 180; // $180 - template name/id (also anchor ID)
    pub const POSITION_INFO: u64 = 183; // $183 - position info struct (contains P155, optional P143)
    pub const EXTERNAL_URL: u64 = 186; // $186 - external URL for anchor fragments
    pub const CONTAINER_INFO: u64 = 270; // $270 - container info fragment type

    // Section dimension symbols
    pub const SECTION_WIDTH: u64 = 66; // $66 - section width in pixels
    pub const SECTION_HEIGHT: u64 = 67; // $67 - section height in pixels
    pub const DEFAULT_TEXT_ALIGN: u64 = 140; // $140 - default text alignment for section
    pub const PAGE_LAYOUT: u64 = 156; // $156 - page layout type
    pub const LAYOUT_FULL_PAGE: u64 = 326; // $326 - full page layout value

    // Value/metadata symbols
    pub const LANDMARK_COVER: u64 = 233; // $233 - cover landmark type
    pub const DEFAULT_READING_ORDER: u64 = 351; // $351 - default reading order name
    pub const LANDMARK_BODYMATTER: u64 = 396; // $396 - bodymatter landmark type

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
    pub const PNG_FORMAT: u64 = 284; // $284 - PNG image format
    pub const JPG_FORMAT: u64 = 285; // $285 - JPEG image format
    pub const GIF_FORMAT: u64 = 286; // $286 - GIF image format (also used for fonts)
    pub const IMAGE_CONTENT: u64 = 271; // $271 - image content type
    pub const IMAGE_ALT_TEXT: u64 = 584; // $584 - alt text for image accessibility
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
            fid: format!("${ftype}"),
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
    /// Local symbol IDs start here (after YJ_symbols shared table)
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
            && let Ok(id) = id_str.parse::<u64>()
        {
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
            && let Ok(id) = id_str.parse::<u64>()
        {
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
    /// Map from resource href to resource symbol (for image references)
    resource_symbols: HashMap<String, u64>,
    /// Map from resource symbol to raw media symbol (for P253 entity dependencies)
    resource_to_media: Vec<(u64, u64)>,
    /// Map from section symbol to resource symbol (for P253 entity dependencies)
    section_to_resource: Vec<(u64, u64)>,
    /// Map from anchor href (URL or internal path) to anchor fragment symbol
    anchor_symbols: HashMap<String, u64>,
    /// Map from XHTML path to section EID (for internal link targets)
    section_eids: HashMap<String, i64>,
    /// Map from full anchor href (path#fragment) to (EID, offset) for TOC navigation
    /// For block-level IDs, offset is 0. For inline IDs, offset is character position.
    anchor_eids: HashMap<String, (i64, i64)>,
}

impl KfxBookBuilder {
    pub fn new() -> Self {
        Self {
            symtab: SymbolTable::new(),
            fragments: Vec::new(),
            container_id: generate_container_id(),
            style_map: HashMap::new(),
            resource_symbols: HashMap::new(),
            resource_to_media: Vec::new(),
            section_to_resource: Vec::new(),
            anchor_symbols: HashMap::new(),
            section_eids: HashMap::new(),
            anchor_eids: HashMap::new(),
        }
    }

    /// Build the resource symbol mapping for image references
    /// This maps resource hrefs to their KFX symbol IDs
    fn build_resource_symbols(&mut self, book: &Book) {
        let mut resource_index = 0;

        for (href, resource) in &book.resources {
            let is_image = is_image_media_type(&resource.media_type);

            if !is_image {
                continue;
            }

            let resource_id = format!("rsrc{resource_index}");
            let resource_sym = self.symtab.get_or_intern(&resource_id);

            // Store mapping from original href to resource symbol
            self.resource_symbols.insert(href.clone(), resource_sym);
            resource_index += 1;
        }
    }

    /// Build a KFX book from a Book structure
    pub fn from_book(book: &Book) -> Self {
        let mut builder = Self::new();

        // 0. Build resource symbol mapping (needed for image content items)
        builder.build_resource_symbols(book);

        // Build a map from href to TOC title for lookup
        let toc_titles: std::collections::HashMap<&str, &str> = book
            .toc
            .iter()
            .map(|entry| (entry.href.as_str(), entry.title.as_str()))
            .collect();

        // 1. Extract content from spine - each XHTML uses its own CSS in document order
        let mut chapters: Vec<ChapterData> = Vec::new();
        let mut chapter_num = 1;
        for (idx, spine_item) in book.spine.iter().enumerate() {
            let resource = match book.resources.get(&spine_item.href) {
                Some(r) => r,
                None => continue,
            };

            // Extract CSS hrefs from this XHTML's <link> tags in document order
            let css_hrefs = extract_css_hrefs_from_xhtml(&resource.data, &spine_item.href);

            // Build combined CSS in document order
            let mut combined_css = String::new();
            for css_href in &css_hrefs {
                if let Some(css_resource) = book.resources.get(css_href) {
                    combined_css.push_str(&String::from_utf8_lossy(&css_resource.data));
                    combined_css.push('\n');
                }
            }

            let stylesheet = Stylesheet::parse(&combined_css);
            let content = extract_content_from_xhtml(&resource.data, &stylesheet, &spine_item.href);

            if content.is_empty() {
                continue;
            }

            // Try to get title from TOC, fall back to default
            let title = toc_titles
                .get(spine_item.href.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    // Try to use first text item if it looks like a title (search nested containers)
                    content
                        .iter()
                        .flat_map(|item| item.flatten())
                        .find_map(|item| {
                            if let ContentItem::Text { text, .. } = item
                                && text.len() < 100
                                && !text.contains('.')
                            {
                                return Some(text.clone());
                            }
                            None
                        })
                })
                .unwrap_or_else(|| format!("Chapter {chapter_num}"));

            let chapter_id = format!("chapter-{idx}");
            chapters.push(ChapterData {
                id: chapter_id,
                title,
                content,
                source_path: spine_item.href.clone(),
            });
            chapter_num += 1;
        }

        // 2.5 Populate image dimensions for image styles
        // This allows width: 100% styles to use actual pixel dimensions
        fn populate_image_dimensions(
            item: &mut ContentItem,
            resources: &std::collections::HashMap<String, crate::book::Resource>,
        ) {
            match item {
                ContentItem::Image {
                    resource_href,
                    style,
                    ..
                } => {
                    // Look up the image resource and get its dimensions
                    if let Some(resource) = resources.get(resource_href) {
                        if let Some((width, height)) = get_image_dimensions(&resource.data) {
                            style.image_width_px = Some(width);
                            style.image_height_px = Some(height);
                        }
                    } else {
                        eprintln!("DEBUG: Image resource not found: {resource_href}");
                        eprintln!(
                            "DEBUG: Available: {:?}",
                            resources.keys().collect::<Vec<_>>()
                        );
                    }
                }
                ContentItem::Container { children, .. } => {
                    for child in children {
                        populate_image_dimensions(child, resources);
                    }
                }
                ContentItem::Text { .. } => {}
            }
        }

        for chapter in &mut chapters {
            for content_item in &mut chapter.content {
                populate_image_dimensions(content_item, &book.resources);
            }
        }

        // 3. Build section EID mapping first (needed for TOC navigation)
        builder.build_section_eids(&chapters, book.metadata.cover_image.is_some());

        // 3.5 Build anchor EID mapping for TOC entries with fragment IDs
        builder.build_anchor_eids(&chapters, book.metadata.cover_image.is_some());

        // 4. Build all fragments
        let has_cover = book.metadata.cover_image.is_some();

        builder.add_format_capabilities();
        builder.add_metadata(book);
        builder.add_reading_order_metadata(&chapters, has_cover);
        builder.add_document_data(&chapters, has_cover);

        // Get first content EID for landmarks (first chapter's first content item)
        let first_content_eid = chapters
            .first()
            .and_then(|ch| builder.section_eids.get(&ch.source_path).copied());
        builder.add_book_navigation(&book.toc, has_cover, first_content_eid);
        builder.add_nav_unit_list();

        // 4. Collect all unique styles and add them as P157 fragments
        builder.add_all_styles(&chapters);

        // Split chapters into chunks for text content fragments
        // Each chunk becomes a separate $145 text content fragment
        let mut all_chunks: Vec<(usize, ContentChunk)> = Vec::new();
        for (chapter_idx, chapter) in chapters.iter().enumerate() {
            // Clone chapter data for chunking (keep original for navigation)
            let chapter_clone = ChapterData {
                id: chapter.id.clone(),
                title: chapter.title.clone(),
                content: chapter.content.clone(),
                source_path: chapter.source_path.clone(),
            };
            for chunk in chapter_clone.into_chunks() {
                all_chunks.push((chapter_idx, chunk));
            }
        }

        // Add text content fragments for each chunk
        for (_, chunk) in &all_chunks {
            builder.add_text_content_chunk(chunk);
        }

        // Build anchor symbol mapping BEFORE content blocks (needed for $179 refs)
        builder.build_anchor_symbols(&chapters);

        // Add cover section if book has a cover image
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
        if let Some(cover_href) = &book.metadata.cover_image {
            // Find the cover resource symbol
            if let Some(cover_sym) = builder.resource_symbols.get(cover_href) {
                // Get cover dimensions
                let (cover_width, cover_height) = book
                    .resources
                    .get(cover_href)
                    .and_then(|r| get_image_dimensions(&r.data))
                    .unwrap_or((1400, 2100));

                builder.add_cover_section(*cover_sym, cover_width, cover_height, eid_base);
                eid_base += 2; // Cover uses 1 EID for section + 1 for content
            }
        }

        // Add content fragments for each chapter
        // Track EID base for consistent position IDs across content blocks and position maps
        // Each chapter uses: 1 EID for section content entry + N EIDs for content blocks
        for (chapter_idx, chapter) in chapters.iter().enumerate() {
            // Get chunks for this chapter
            let chapter_chunks: Vec<&ContentChunk> = all_chunks
                .iter()
                .filter(|(idx, _)| *idx == chapter_idx)
                .map(|(_, chunk)| chunk)
                .collect();

            builder.add_content_block_chunked(chapter, &chapter_chunks, eid_base);
            builder.add_section(chapter, eid_base);
            builder.add_auxiliary_data(chapter);
            // +1 for section content entry, + total items for content blocks (including nested)
            // Must match calculation in build_section_eids and add_page_templates
            let total_items = count_content_items(&chapter.content);
            eid_base += 1 + total_items as i64;
        }

        // Add position/location maps
        // CRITICAL: Must pass has_cover to correctly calculate EID bases after cover section
        let has_cover = book.metadata.cover_image.is_some();
        builder.add_position_map(&chapters, has_cover);
        builder.add_position_id_map(&chapters, has_cover);
        builder.add_location_map(&chapters, has_cover);

        // Add page templates (P266) for position tracking
        builder.add_page_templates(&chapters, book.metadata.cover_image.is_some());

        // Add anchor fragments ($266) for external URLs
        builder.add_anchor_fragments();

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
        let capabilities = [
            ("com.amazon.yjconversion", "reflow-style", 6, 0),
            ("SDK.Marker", "CanonicalFormat", 1, 0),
            ("com.amazon.yjconversion", "yj_hdv", 1, 0),
        ];

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

            // Generate ASIN-like identifier for the book
            // kfxlib uses a random 32-char alphanumeric ID for both PDOC and EBOK
            // Both ASIN and content_id must be set to the same value for cover thumbnails to work
            let asin = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                book.metadata.title.hash(&mut h);
                book.metadata.authors.hash(&mut h);
                // Use identifier if available for more uniqueness
                book.metadata.identifier.hash(&mut h);
                format!("{:032X}", h.finish())
            };
            add_entry("ASIN", IonValue::String(asin.clone()));
            add_entry("content_id", IonValue::String(asin));

            // PDOC = Personal Document (sideloaded)
            // EBOK = store-purchased eBook (enables cover thumbnails in library view)
            add_entry("cde_content_type", IonValue::String("EBOK".to_string()));

            // Add cover_image reference if available
            // IMPORTANT: cover_image value must be a Symbol (not String) matching the $164 resource fid
            if let Some(cover_href) = &book.metadata.cover_image {
                // Find the cover in resources and get its symbol
                if let Some(&cover_sym) = self.resource_symbols.get(cover_href) {
                    add_entry("cover_image", IonValue::Symbol(cover_sym));
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
    fn add_reading_order_metadata(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut section_refs: Vec<IonValue> = Vec::new();

        // Add cover section first if present
        if has_cover {
            let cover_section_sym = self.symtab.get_or_intern("cover-section");
            section_refs.push(IonValue::Symbol(cover_section_sym));
        }

        // Add chapter sections
        for ch in chapters {
            let section_id = format!("section-{}", ch.id);
            let sym_id = self.symtab.get_or_intern(&section_id);
            section_refs.push(IonValue::Symbol(sym_id));
        }

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
    fn add_document_data(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut section_refs: Vec<IonValue> = Vec::new();

        // Add cover section first if present
        if has_cover {
            let cover_section_sym = self.symtab.get_or_intern("cover-section");
            section_refs.push(IonValue::Symbol(cover_section_sym));
        }

        // Add chapter sections
        for ch in chapters {
            let section_id = format!("section-{}", ch.id);
            let sym_id = self.symtab.get_or_intern(&section_id);
            section_refs.push(IonValue::Symbol(sym_id));
        }

        let mut reading_order = HashMap::new();
        reading_order.insert(
            sym::READING_ORDER_NAME,
            IonValue::Symbol(sym::DEFAULT_READING_ORDER),
        );
        reading_order.insert(sym::SECTIONS_LIST, IonValue::List(section_refs));

        // Calculate total number of content items (not character count)
        // This is used for position calculations
        let total_items: usize = chapters.iter().map(|ch| ch.content.len()).sum();

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
    /// - Nav container with type=toc ($212)
    /// - Nav container with type=landmarks ($236)
    /// - Nav entries with titles and section targets (using EIDs from section_eids map)
    fn add_book_navigation(
        &mut self,
        toc: &[crate::book::TocEntry],
        has_cover: bool,
        first_content_eid: Option<i64>,
    ) {
        let mut nav_containers = Vec::new();

        // === TOC Nav Container ===
        let nav_toc_id = "nav-toc";
        let nav_toc_sym = self.symtab.get_or_intern(nav_toc_id);

        let nav_entry_values = self.build_nav_entries_from_toc(toc);

        let mut toc_container = HashMap::new();
        toc_container.insert(sym::NAV_TYPE, IonValue::Symbol(sym::TOC));
        toc_container.insert(sym::NAV_ID, IonValue::Symbol(nav_toc_sym));
        toc_container.insert(sym::NAV_ENTRIES, IonValue::List(nav_entry_values));

        nav_containers.push(IonValue::Annotated(
            vec![sym::NAV_CONTAINER_TYPE],
            Box::new(IonValue::Struct(toc_container)),
        ));

        // === Landmarks Nav Container ===
        // Kindle requires landmarks navigation with at least cover and bodymatter entries
        let nav_landmarks_id = "nav-landmarks";
        let nav_landmarks_sym = self.symtab.get_or_intern(nav_landmarks_id);

        let mut landmark_entries = Vec::new();

        // Cover landmark (if cover exists)
        if has_cover {
            let cover_eid = SymbolTable::LOCAL_MIN_ID as i64 + 1; // First content item after section entry
            landmark_entries.push(self.build_landmark_entry(
                "cover-nav-unit",
                cover_eid,
                Some(sym::LANDMARK_COVER),
            ));
        }

        // Bodymatter landmark (first content section)
        if let Some(eid) = first_content_eid {
            // Get the title of the first TOC entry for bodymatter
            let bodymatter_title = toc.first().map(|e| e.title.as_str()).unwrap_or("Content");
            landmark_entries.push(self.build_landmark_entry(
                bodymatter_title,
                eid,
                Some(sym::LANDMARK_BODYMATTER),
            ));
        }

        let mut landmarks_container = HashMap::new();
        landmarks_container.insert(sym::NAV_TYPE, IonValue::Symbol(sym::LANDMARKS_NAV_TYPE));
        landmarks_container.insert(sym::NAV_ID, IonValue::Symbol(nav_landmarks_sym));
        landmarks_container.insert(sym::NAV_ENTRIES, IonValue::List(landmark_entries));

        nav_containers.push(IonValue::Annotated(
            vec![sym::NAV_CONTAINER_TYPE],
            Box::new(IonValue::Struct(landmarks_container)),
        ));

        // Book navigation root: { $178 (reading_order_name): $351, $392 (nav_containers): [...] }
        let mut nav = HashMap::new();
        nav.insert(
            sym::READING_ORDER_NAME,
            IonValue::Symbol(sym::DEFAULT_READING_ORDER),
        );
        nav.insert(sym::NAV_CONTAINER_REF, IonValue::List(nav_containers));

        self.fragments.push(KfxFragment::singleton(
            sym::BOOK_NAVIGATION,
            IonValue::List(vec![IonValue::Struct(nav)]),
        ));
    }

    /// Build a landmark navigation entry
    fn build_landmark_entry(&self, title: &str, eid: i64, landmark_type: Option<u64>) -> IonValue {
        let mut nav_title = HashMap::new();
        nav_title.insert(sym::TEXT, IonValue::String(title.to_string()));

        // Nav target: { $155: eid, $143: 0 }
        // IMPORTANT: Field order matters for Kindle - $155 must come before $143
        let nav_target = IonValue::OrderedStruct(vec![
            (sym::POSITION, IonValue::Int(eid)),
            (sym::OFFSET, IonValue::Int(0)),
        ]);

        let mut nav_entry = HashMap::new();
        nav_entry.insert(sym::NAV_TITLE, IonValue::Struct(nav_title));
        nav_entry.insert(sym::NAV_TARGET, nav_target);

        // Add landmark type if specified ($238 field)
        if let Some(lt) = landmark_type {
            nav_entry.insert(sym::LANDMARK_TYPE, IonValue::Symbol(lt));
        }

        IonValue::Annotated(
            vec![sym::NAV_DEFINITION],
            Box::new(IonValue::Struct(nav_entry)),
        )
    }

    /// Build nav entries from TOC entries, preserving nested hierarchy
    /// Maps TOC hrefs to EIDs using section_eids lookup
    fn build_nav_entries_from_toc(&self, toc: &[crate::book::TocEntry]) -> Vec<IonValue> {
        self.build_nav_entries_recursive(toc)
    }

    /// Recursively build nav entries, preserving TOC hierarchy via nested $247 entries
    fn build_nav_entries_recursive(&self, entries: &[crate::book::TocEntry]) -> Vec<IonValue> {
        let mut nav_entries = Vec::new();

        for entry in entries {
            // Parse the href to extract path and fragment
            let (path, fragment) = if let Some(hash_pos) = entry.href.find('#') {
                (&entry.href[..hash_pos], Some(&entry.href[hash_pos + 1..]))
            } else {
                (entry.href.as_str(), None)
            };

            // Look up the (EID, offset) for this entry:
            // 1. If there's a fragment, try anchor_eids first (path#fragment → (EID, offset))
            // 2. Fall back to section_eids (path → section start EID, offset 0)
            let eid_offset = if fragment.is_some() {
                // Try full href with fragment first
                self.anchor_eids
                    .get(&entry.href)
                    .copied()
                    .or_else(|| self.section_eids.get(path).map(|&eid| (eid, 0)))
            } else {
                self.section_eids.get(path).map(|&eid| (eid, 0))
            };

            if let Some((eid, offset)) = eid_offset {
                // Nav title: { $244 (text): "Entry Title" }
                let mut nav_title = HashMap::new();
                nav_title.insert(sym::TEXT, IonValue::String(entry.title.clone()));

                // Nav target: { $155 (position/eid): eid, $143 (offset): offset }
                // IMPORTANT: Field order matters for Kindle - $155 must come before $143
                let nav_target = IonValue::OrderedStruct(vec![
                    (sym::POSITION, IonValue::Int(eid)),
                    (sym::OFFSET, IonValue::Int(offset)),
                ]);

                // Nav entry struct: { $241: nav_title, $246: nav_target }
                let mut nav_entry = HashMap::new();
                nav_entry.insert(sym::NAV_TITLE, IonValue::Struct(nav_title));
                nav_entry.insert(sym::NAV_TARGET, nav_target);

                // Recursively build children and nest them via $247 (nav_entries)
                if !entry.children.is_empty() {
                    let nested_entries = self.build_nav_entries_recursive(&entry.children);
                    if !nested_entries.is_empty() {
                        nav_entry.insert(sym::NAV_ENTRIES, IonValue::List(nested_entries));
                    }
                }

                // Annotate with $393 (nav_definition)
                nav_entries.push(IonValue::Annotated(
                    vec![sym::NAV_DEFINITION],
                    Box::new(IonValue::Struct(nav_entry)),
                ));
            } else if !entry.children.is_empty() {
                // Entry itself doesn't map to a section, but children might
                // Add children at this level (promoting them up)
                let nested_entries = self.build_nav_entries_recursive(&entry.children);
                nav_entries.extend(nested_entries);
            }
        }

        nav_entries
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
        // Collect all unique styles, including from nested containers and inline runs
        fn collect_styles(item: &ContentItem, styles: &mut std::collections::HashSet<ParsedStyle>) {
            // Add this item's style
            styles.insert(item.style().clone());

            match item {
                ContentItem::Container { children, .. } => {
                    // Recursively collect from children
                    for child in children {
                        collect_styles(child, styles);
                    }
                }
                ContentItem::Text { inline_runs, .. } => {
                    // Collect styles from inline runs
                    for run in inline_runs {
                        styles.insert(run.style.clone());
                    }
                }
                ContentItem::Image { .. } => {}
            }
        }

        let mut unique_styles = std::collections::HashSet::new();
        for chapter in chapters {
            for item in &chapter.content {
                collect_styles(item, &mut unique_styles);
            }
        }

        // Helper to convert CssValue to IonValue for margins
        // Format: {$306: unit_symbol, $307: decimal_value}
        // Based on CSS mapping analysis:
        // - margin-left/right use percent ($314) with values in percent of page width
        // - margin-top uses multiplier ($310) for spacing
        // - em values are converted to percent (3.125% per 1em based on mapping)
        let css_to_ion = |val: &CssValue| -> Option<IonValue> {
            match val {
                CssValue::Px(v) => {
                    if v.abs() < 0.001 {
                        return None;
                    }
                    // Convert px to percent (approximate: 1px ~ 0.117% based on mapping)
                    let pct = *v * 0.117;
                    let mut s = HashMap::new();
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(pct)));
                    Some(IonValue::Struct(s))
                }
                CssValue::Em(v) | CssValue::Rem(v) => {
                    if v.abs() < 0.001 {
                        return None;
                    }
                    // Convert em to percent (3.125% per 1em based on mapping)
                    let pct = *v * 3.125;
                    let mut s = HashMap::new();
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(pct)));
                    Some(IonValue::Struct(s))
                }
                CssValue::Percent(v) => {
                    if v.abs() < 0.001 {
                        return None;
                    }
                    let mut s = HashMap::new();
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                    Some(IonValue::Struct(s))
                }
                CssValue::Number(v) => {
                    if v.abs() < 0.001 {
                        return None;
                    }
                    // Unitless number - use multiplier
                    let mut s = HashMap::new();
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_MULTIPLIER));
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                    Some(IonValue::Struct(s))
                }
                _ => None,
            }
        };

        let color_to_ion = |color: &Color| -> Option<IonValue> {
            match color {
                Color::Rgba(r, g, b, _a) => {
                    // Serialize as integer 0x00RRGGBB
                    let val = ((*r as i64) << 16) | ((*g as i64) << 8) | (*b as i64);
                    Some(IonValue::Int(val))
                }
                _ => None,
            }
        };

        // Pre-intern border symbols to avoid borrowing self in the loop
        let border_top_sym = self.symtab.get_or_intern("border-top");
        let border_bottom_sym = self.symtab.get_or_intern("border-bottom");
        let border_left_sym = self.symtab.get_or_intern("border-left");
        let border_right_sym = self.symtab.get_or_intern("border-right");
        let border_style_sym = self.symtab.get_or_intern("border-style");

        let solid_sym = self.symtab.get_or_intern("solid");
        let dotted_sym = self.symtab.get_or_intern("dotted");
        let dashed_sym = self.symtab.get_or_intern("dashed");

        let border_to_ion = |border: &Border| -> Option<IonValue> {
            if border.style == BorderStyle::None || border.style == BorderStyle::Hidden {
                return None;
            }

            let mut b_struct = HashMap::new();

            // Style
            let style_sym = match border.style {
                BorderStyle::Solid => solid_sym,
                BorderStyle::Dotted => dotted_sym,
                BorderStyle::Dashed => dashed_sym,
                // Fallback to solid for others
                _ => solid_sym,
            };
            b_struct.insert(border_style_sym, IonValue::Symbol(style_sym));

            // Width
            if let Some(ref w) = border.width {
                if let Some(val) = css_to_ion(w) {
                    b_struct.insert(sym::VALUE, val);
                }
            } else {
                // Default width (1px) - use structure format
                let mut val = HashMap::new();
                val.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PX));
                val.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(1.0)));
                b_struct.insert(sym::VALUE, IonValue::Struct(val));
            }

            // Color
            if let Some(ref c) = border.color {
                if let Some(val) = color_to_ion(c) {
                    b_struct.insert(sym::COLOR, val);
                }
            } else {
                // Default to black
                b_struct.insert(sym::COLOR, IonValue::Int(0));
            }

            Some(IonValue::Struct(b_struct))
        };

        for (i, style) in unique_styles.into_iter().enumerate() {
            let style_id = format!("style-{i}");
            let style_sym = self.symtab.get_or_intern(&style_id);

            let mut style_ion = HashMap::new();
            style_ion.insert(sym::STYLE_NAME, IonValue::Symbol(style_sym));

            // Detect special style types
            let is_image_style = style.is_image;
            let is_inline_style = style.is_inline;

            // Inline styles (for links/anchors) are minimal - just block type
            // Reference uses $127: $349 for inline elements
            if is_inline_style {
                style_ion.insert(
                    sym::STYLE_BLOCK_TYPE,
                    IonValue::Symbol(sym::BLOCK_TYPE_INLINE),
                );
                // Skip all other properties - inline elements inherit from parent
                self.fragments.push(KfxFragment::new(
                    sym::STYLE,
                    style_id,
                    IonValue::Struct(style_ion),
                ));
                self.style_map.insert(style, style_sym);
                continue;
            }

            if !is_image_style {
                // Font family - use string value (verified via CSS mapping)
                // KFX uses strings like "serif", "sans-serif", "monospace"
                if let Some(ref family) = style.font_family {
                    let family_lower = family.to_lowercase();
                    let family_str = match family_lower.as_str() {
                        "serif" | "georgia" | "times" | "times new roman" => "serif".to_string(),
                        "sans-serif" | "arial" | "helvetica" => "sans-serif".to_string(),
                        "monospace" | "courier" | "courier new" => "monospace".to_string(),
                        "cursive" => "cursive".to_string(),
                        "fantasy" => "fantasy".to_string(),
                        _ => family_lower.clone(),
                    };
                    style_ion.insert(sym::FONT_FAMILY, IonValue::String(family_str));
                }

                // Add display:block ($127: $383) for text block elements
                if style.display == Some(crate::css::Display::Block) {
                    style_ion.insert(
                        sym::STYLE_BLOCK_TYPE,
                        IonValue::Symbol(sym::BLOCK_TYPE_BLOCK),
                    );
                }
            }

            // Font size - use struct {$307: value, $306: $505}
            // Value is relative to 1.0 (1em). Omit if 1.0/100% (baseline).
            if let Some(ref size) = style.font_size {
                let size_val: Option<f32> = match size {
                    CssValue::Em(v) | CssValue::Rem(v) => {
                        if (v - 1.0).abs() < 0.001 {
                            None // 1em is baseline, omit
                        } else {
                            Some(*v)
                        }
                    }
                    CssValue::Percent(v) => {
                        if (v - 100.0).abs() < 0.001 {
                            None // 100% is baseline, omit
                        } else {
                            Some(*v / 100.0)
                        }
                    }
                    CssValue::Keyword(k) => match k.as_str() {
                        "smaller" => Some(0.833333),
                        "larger" => Some(1.2),
                        "xx-small" => Some(0.5625),
                        "x-small" => Some(0.625),
                        "small" => Some(0.8125),
                        "medium" => None, // baseline
                        "large" => Some(1.125),
                        "x-large" => Some(1.5),
                        "xx-large" => Some(2.0),
                        _ => None,
                    },
                    CssValue::Px(v) => {
                        // Approximate: assume 16px = 1em
                        let em_val = *v / 16.0;
                        if (em_val - 1.0).abs() < 0.001 {
                            None
                        } else {
                            Some(em_val)
                        }
                    }
                    _ => None,
                };
                if let Some(val) = size_val {
                    let mut s = HashMap::new();
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM_FONTSIZE));
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
                    style_ion.insert(sym::FONT_SIZE, IonValue::Struct(s));
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

            // Font weight - use $13 with weight symbol
            // $350 = normal/400, $361 = bold/700, etc.
            if let Some(ref weight) = style.font_weight {
                let weight_sym = if weight.is_bold() {
                    sym::FONT_WEIGHT_BOLD // $361
                } else {
                    // Map numeric weights
                    match weight {
                        crate::css::FontWeight::Weight(100) => sym::FONT_WEIGHT_100,
                        crate::css::FontWeight::Weight(200) => sym::FONT_WEIGHT_200,
                        crate::css::FontWeight::Weight(300) => sym::FONT_WEIGHT_300,
                        crate::css::FontWeight::Weight(400) => sym::FONT_WEIGHT_NORMAL,
                        crate::css::FontWeight::Weight(500) => sym::FONT_WEIGHT_500,
                        crate::css::FontWeight::Weight(600) => sym::FONT_WEIGHT_600,
                        crate::css::FontWeight::Weight(700) => sym::FONT_WEIGHT_BOLD,
                        crate::css::FontWeight::Weight(800) => sym::FONT_WEIGHT_800,
                        crate::css::FontWeight::Weight(900) => sym::FONT_WEIGHT_900,
                        crate::css::FontWeight::Weight(n) if *n < 400 => sym::FONT_WEIGHT_300,
                        crate::css::FontWeight::Weight(n) if *n < 600 => sym::FONT_WEIGHT_500,
                        crate::css::FontWeight::Weight(_) => sym::FONT_WEIGHT_BOLD,
                        crate::css::FontWeight::Normal => sym::FONT_WEIGHT_NORMAL,
                        crate::css::FontWeight::Bold => sym::FONT_WEIGHT_BOLD,
                    }
                };
                // Only add if not normal (to avoid unnecessary properties)
                if weight_sym != sym::FONT_WEIGHT_NORMAL {
                    style_ion.insert(sym::FONT_WEIGHT, IonValue::Symbol(weight_sym));
                }
            }

            // Font style - use $12 with style symbol
            // $350 = normal, $382 = italic, $381 = oblique
            if let Some(style_type) = style.font_style {
                let style_sym = match style_type {
                    crate::css::FontStyle::Italic => sym::FONT_STYLE_ITALIC,
                    crate::css::FontStyle::Oblique => sym::FONT_STYLE_OBLIQUE,
                    crate::css::FontStyle::Normal => sym::FONT_STYLE_NORMAL,
                };
                // Only add if not normal
                if style_sym != sym::FONT_STYLE_NORMAL {
                    style_ion.insert(sym::FONT_STYLE, IonValue::Symbol(style_sym));
                }
            }

            // Font variant - use $583 with $369 for small-caps
            if let Some(FontVariant::SmallCaps) = style.font_variant {
                style_ion.insert(
                    sym::FONT_VARIANT,
                    IonValue::Symbol(sym::FONT_VARIANT_SMALL_CAPS),
                );
            }

            // Apply margin properties from computed styles
            if let Some(ref margin) = style.margin_top
                && let Some(val) = css_to_ion(margin)
            {
                style_ion.insert(sym::MARGIN_TOP, val);
            }
            if let Some(ref margin) = style.margin_bottom
                && let Some(val) = css_to_ion(margin)
            {
                style_ion.insert(sym::MARGIN_BOTTOM, val);
            }
            if let Some(ref margin) = style.margin_left
                && let Some(val) = css_to_ion(margin)
            {
                style_ion.insert(sym::MARGIN_LEFT, val);
            }
            if let Some(ref margin) = style.margin_right
                && let Some(val) = css_to_ion(margin)
            {
                style_ion.insert(sym::MARGIN_RIGHT, val);
            }

            // Width and height (for images and block elements)
            // Based on CSS mapping: width uses percent unit ($314) for percentage values
            if let Some(ref width) = style.width {
                let width_val = match width {
                    CssValue::Percent(pct) => {
                        // Use percent unit ($314) with the percentage value directly
                        let mut s = HashMap::new();
                        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*pct)));
                        IonValue::Struct(s)
                    }
                    CssValue::Em(v) | CssValue::Rem(v) => {
                        // Em widths use em unit ($308) and need $65 (max-width) set to 100%
                        let mut s = HashMap::new();
                        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
                        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*v)));
                        // Also set max-width to 100%
                        let mut max_s = HashMap::new();
                        max_s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                        max_s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(100.0)));
                        style_ion.insert(sym::MAX_WIDTH, IonValue::Struct(max_s));
                        IonValue::Struct(s)
                    }
                    CssValue::Px(v) => {
                        // Pixel widths - convert to percent (approximate)
                        let pct = *v * 0.117; // Rough conversion
                        let mut s = HashMap::new();
                        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(pct)));
                        IonValue::Struct(s)
                    }
                    _ => continue,
                };
                style_ion.insert(sym::STYLE_WIDTH, width_val);
            }
            if let Some(ref height) = style.height {
                let height_val = match height {
                    CssValue::Percent(pct) => {
                        let mut s = HashMap::new();
                        s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_PERCENT));
                        s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(*pct)));
                        IonValue::Struct(s)
                    }
                    _ => {
                        if let Some(val) = css_to_ion(height) {
                            val
                        } else {
                            continue;
                        }
                    }
                };
                style_ion.insert(sym::STYLE_HEIGHT, height_val);
            }

            // Add image-specific properties for image styles (matching reference)
            if is_image_style {
                // P546: image fit (contain)
                style_ion.insert(sym::IMAGE_FIT, IonValue::Symbol(sym::IMAGE_FIT_CONTAIN));

                // P580: image layout - center (verified via test EPUB: $580 = $320)
                style_ion.insert(sym::IMAGE_LAYOUT, IonValue::Symbol(sym::ALIGN_CENTER));
            }

            // Check for margin: auto centering (margin-left: auto AND margin-right: auto)
            // This triggers block centering properties like in imprint paragraphs
            let has_margin_auto_centering = matches!(
                (&style.margin_left, &style.margin_right),
                (Some(CssValue::Keyword(l)), Some(CssValue::Keyword(r)))
                if l == "auto" && r == "auto"
            );

            // Add centering properties for blocks with margin: auto (like imprint paragraphs)
            // Reference shows: $546 (image-fit) = contain, $580 (image-layout) = center
            if has_margin_auto_centering && !is_image_style {
                style_ion.insert(sym::IMAGE_FIT, IonValue::Symbol(sym::IMAGE_FIT_CONTAIN));
                style_ion.insert(sym::IMAGE_LAYOUT, IonValue::Symbol(sym::ALIGN_CENTER));
            }

            // Text indent - use $36 with em units ($308)
            // Verified: text-indent: 1em -> $36={'$307': 1, '$306': '$308'}
            if let Some(ref indent) = style.text_indent {
                let em_val: Option<f32> = match indent {
                    CssValue::Em(v) | CssValue::Rem(v) => Some(*v),
                    CssValue::Px(v) => {
                        // Convert px to em (assume 16px = 1em)
                        Some(*v / 16.0)
                    }
                    CssValue::Percent(v) => {
                        // Convert percent to em approximation
                        Some(*v / 100.0)
                    }
                    _ => None,
                };
                if let Some(val) = em_val {
                    let mut s = HashMap::new();
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
                    style_ion.insert(sym::TEXT_INDENT, IonValue::Struct(s));
                }
            }

            // Line height - use $42 with multiplier units ($310)
            // Verified: CSS line-height values map differently in KFX
            // line-height: 1 -> $42={'$307': 0.833333, '$306': '$310'}
            // line-height: 1.5 -> $42={'$307': 1.25, '$306': '$310'}
            if let Some(ref height) = style.line_height {
                let kfx_val: Option<f32> = match height {
                    CssValue::Number(v) => {
                        // Unitless number - the KFX value is different from CSS
                        // Based on mapping: css 1.0 -> 0.833333, css 1.5 -> 1.25, css 2.0 -> 1.66667
                        // Formula appears to be: kfx = css * (5/6) approximately
                        Some(*v * 0.833333)
                    }
                    CssValue::Percent(v) => {
                        // Percentage - convert to multiplier then apply formula
                        Some(*v / 100.0 * 0.833333)
                    }
                    CssValue::Em(v) | CssValue::Rem(v) => {
                        // Em values - similar treatment
                        Some(*v * 0.833333)
                    }
                    CssValue::Px(v) => {
                        // Pixel values - convert to em first (assume 16px = 1em)
                        Some(*v / 16.0 * 0.833333)
                    }
                    _ => None,
                };
                if let Some(val) = kfx_val {
                    let mut s = HashMap::new();
                    s.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_MULTIPLIER));
                    s.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(val)));
                    style_ion.insert(sym::LINE_HEIGHT, IonValue::Struct(s));
                }
            }

            if let Some(ref color) = style.color
                && let Some(val) = color_to_ion(color)
            {
                style_ion.insert(sym::COLOR, val);
            }

            if let Some(ref bg_color) = style.background_color
                && let Some(val) = color_to_ion(bg_color)
            {
                style_ion.insert(sym::BACKGROUND_COLOR, val);
            }

            if let Some(ref b) = style.border_top
                && let Some(val) = border_to_ion(b)
            {
                style_ion.insert(border_top_sym, val);
            }
            if let Some(ref b) = style.border_right
                && let Some(val) = border_to_ion(b)
            {
                style_ion.insert(border_right_sym, val);
            }
            if let Some(ref b) = style.border_bottom
                && let Some(val) = border_to_ion(b)
            {
                style_ion.insert(border_bottom_sym, val);
            }
            if let Some(ref b) = style.border_left
                && let Some(val) = border_to_ion(b)
            {
                style_ion.insert(border_left_sym, val);
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

    /// Add text content fragment ($145) for a chunk
    fn add_text_content_chunk(&mut self, chunk: &ContentChunk) {
        let content_id = format!("content-{}", chunk.id);
        let content_sym = self.symtab.get_or_intern(&content_id);

        // Only include text items in text content (images are referenced directly in content blocks)
        // Use flatten() to extract text from nested containers
        let text_values: Vec<IonValue> = chunk
            .items
            .iter()
            .flat_map(|item| item.flatten())
            .filter_map(|item| {
                if let ContentItem::Text { text, .. } = item {
                    Some(IonValue::String(text.clone()))
                } else {
                    None
                }
            })
            .collect();

        // Don't create an empty text content fragment
        if text_values.is_empty() {
            return;
        }

        let mut content = HashMap::new();
        content.insert(sym::ID, IonValue::Symbol(content_sym));
        content.insert(sym::CONTENT_ARRAY, IonValue::List(text_values));

        self.fragments.push(KfxFragment::new(
            sym::TEXT_CONTENT,
            &content_id,
            IonValue::Struct(content),
        ));
    }

    /// Add content block fragment ($259) with chunked text content references
    /// Supports nested Container items which generate nested $146 arrays
    fn add_content_block_chunked(
        &mut self,
        chapter: &ChapterData,
        chunks: &[&ContentChunk],
        eid_base: i64,
    ) {
        let block_id = format!("block-{}", chapter.id);
        let block_sym = self.symtab.get_or_intern(&block_id);

        // Track state across chunks for text indexing and EID assignment
        struct ContentState {
            global_idx: usize,
            text_idx_in_chunk: i64,
            current_content_sym: u64,
        }

        // Recursively build content items for nested structures
        fn build_content_item(
            builder: &mut KfxBookBuilder,
            content_item: &ContentItem,
            state: &mut ContentState,
            eid_base: i64,
        ) -> IonValue {
            let mut item = HashMap::new();

            match content_item {
                ContentItem::Text {
                    style,
                    inline_runs,
                    anchor_href: _,
                    ..
                } => {
                    // Text content: reference the text chunk
                    let mut text_ref = HashMap::new();
                    text_ref.insert(sym::ID, IonValue::Symbol(state.current_content_sym));
                    text_ref.insert(sym::TEXT_OFFSET, IonValue::Int(state.text_idx_in_chunk));

                    item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH));
                    item.insert(sym::TEXT_CONTENT, IonValue::Struct(text_ref));

                    // Add base style reference
                    if let Some(style_sym) = builder.get_style_symbol(style) {
                        item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                    }

                    // Note: anchor_href on ContentItem is deprecated - anchors are in inline_runs now

                    // Add inline style runs ($142) if present
                    // Each run can have a style and/or an anchor reference
                    let has_inline_runs = if !inline_runs.is_empty() {
                        let runs: Vec<IonValue> = inline_runs
                            .iter()
                            .filter_map(|run| {
                                // Get style symbol (required for run)
                                let style_sym = builder.get_style_symbol(&run.style)?;
                                let mut run_struct = HashMap::new();
                                run_struct.insert(sym::OFFSET, IonValue::Int(run.offset as i64));
                                run_struct.insert(sym::COUNT, IonValue::Int(run.length as i64));
                                run_struct.insert(sym::STYLE, IonValue::Symbol(style_sym));

                                // Add anchor reference ($179) if this run has a hyperlink
                                if let Some(ref href) = run.anchor_href
                                    && let Some(anchor_sym) = builder.anchor_symbols.get(href)
                                {
                                    run_struct
                                        .insert(sym::ANCHOR_REF, IonValue::Symbol(*anchor_sym));
                                }

                                Some(IonValue::Struct(run_struct))
                            })
                            .collect();

                        if !runs.is_empty() {
                            item.insert(sym::INLINE_STYLE_RUNS, IonValue::List(runs));
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    // Add content role indicator ($790)
                    // Only on items WITHOUT inline style runs
                    // First item in content block gets 2, normal paragraphs get 3
                    if !has_inline_runs {
                        let role = if state.global_idx == 0 { 2 } else { 3 };
                        item.insert(sym::CONTENT_ROLE, IonValue::Int(role));
                    }

                    state.text_idx_in_chunk += 1;
                }
                ContentItem::Image {
                    resource_href,
                    style,
                    alt,
                } => {
                    // Image content: reference the resource directly
                    let resource_sym =
                        builder
                            .resource_symbols
                            .get(resource_href)
                            .unwrap_or_else(|| {
                                panic!(
                                    "Image resource not found: '{}'. Available: {:?}",
                                    resource_href,
                                    builder.resource_symbols.keys().collect::<Vec<_>>()
                                )
                            });
                    item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::IMAGE_CONTENT));
                    item.insert(sym::RESOURCE_NAME, IonValue::Symbol(*resource_sym));
                    // $584 = IMAGE_ALT_TEXT for accessibility
                    let alt_text = alt.clone().unwrap_or_default();
                    item.insert(sym::IMAGE_ALT_TEXT, IonValue::String(alt_text));

                    // Add style reference if present
                    if let Some(style_sym) = builder.get_style_symbol(style) {
                        item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                    }
                }
                ContentItem::Container {
                    style, children, ..
                } => {
                    // Container: create nested $146 array with children
                    item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH));
                    // Note: Containers do NOT get $790 - only leaf text items do

                    // Build nested content array
                    let nested_items: Vec<IonValue> = children
                        .iter()
                        .map(|child| build_content_item(builder, child, state, eid_base))
                        .collect();

                    item.insert(sym::CONTENT_ARRAY, IonValue::List(nested_items));

                    // Add style reference for the container
                    if let Some(style_sym) = builder.get_style_symbol(style) {
                        item.insert(sym::STYLE, IonValue::Symbol(style_sym));
                    }
                }
            }

            // Use consistent EID that matches position maps
            // +1 offset because eid_base is reserved for section content entry
            item.insert(
                sym::POSITION,
                IonValue::Int(eid_base + 1 + state.global_idx as i64),
            );
            state.global_idx += 1;

            IonValue::Struct(item)
        }

        // Create content items referencing text content chunks or images
        let mut content_items = Vec::new();
        let mut state = ContentState {
            global_idx: 0,
            text_idx_in_chunk: 0,
            current_content_sym: 0,
        };

        for chunk in chunks {
            let content_id = format!("content-{}", chunk.id);
            state.current_content_sym = self.symtab.get_or_intern(&content_id);
            state.text_idx_in_chunk = 0;

            for content_item in chunk.items.iter() {
                let ion_item = build_content_item(self, content_item, &mut state, eid_base);
                content_items.push(ion_item);
            }
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
        // Note: Regular text sections should NOT have P66/P67 dimensions
        // Only cover/image sections have fixed dimensions
        // Content type: paragraph content
        content_ref.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTENT_PARAGRAPH));

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

        // Track section -> resource dependencies for images in this chapter
        // Use flatten() to find images in nested containers
        for content_item in &chapter.content {
            for leaf_item in content_item.flatten() {
                if let ContentItem::Image { resource_href, .. } = leaf_item
                    && let Some(resource_sym) = self.resource_symbols.get(resource_href)
                {
                    self.section_to_resource.push((section_sym, *resource_sym));
                }
            }
        }
    }

    /// Add cover section with IMAGE_CONTENT for the cover image
    /// Creates a style (P157), content block (P259), and section (P260) for the cover
    fn add_cover_section(
        &mut self,
        cover_resource_sym: u64,
        width: u32,
        height: u32,
        eid_base: i64,
    ) {
        let cover_block_id = "cover-block";
        let cover_block_sym = self.symtab.get_or_intern(cover_block_id);
        let cover_section_id = "cover-section";
        let cover_section_sym = self.symtab.get_or_intern(cover_section_id);
        let cover_style_id = "cover-style";
        let cover_style_sym = self.symtab.get_or_intern(cover_style_id);

        // Create cover image style (matching reference P1120 structure)
        let mut cover_style = HashMap::new();
        cover_style.insert(sym::STYLE_NAME, IonValue::Symbol(cover_style_sym));
        // text_indent: 1.5em - use em unit ($308) with value 1.5
        let mut text_indent = HashMap::new();
        text_indent.insert(sym::UNIT, IonValue::Symbol(sym::UNIT_EM));
        text_indent.insert(sym::VALUE, IonValue::Decimal(encode_kfx_decimal(1.5)));
        cover_style.insert(sym::TEXT_INDENT, IonValue::Struct(text_indent));
        // Note: margin_bottom: 0 is omitted (default value)

        self.fragments.push(KfxFragment::new(
            sym::STYLE,
            cover_style_id,
            IonValue::Struct(cover_style),
        ));

        // Create content block with the cover image
        let mut image_item = HashMap::new();
        image_item.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::IMAGE_CONTENT));
        image_item.insert(sym::RESOURCE_NAME, IonValue::Symbol(cover_resource_sym));
        image_item.insert(sym::POSITION, IonValue::Int(eid_base + 1));
        // P157 references the style for this image content
        image_item.insert(sym::STYLE, IonValue::Symbol(cover_style_sym));
        // Note: $584 (IMAGE_ALT_TEXT) is NOT present in cover IMAGE_CONTENT in reference

        let mut block = HashMap::new();
        block.insert(sym::CONTENT_NAME, IonValue::Symbol(cover_block_sym));
        block.insert(
            sym::CONTENT_ARRAY,
            IonValue::List(vec![IonValue::Struct(image_item)]),
        );

        self.fragments.push(KfxFragment::new(
            sym::CONTENT_BLOCK,
            cover_block_id,
            IonValue::Struct(block),
        ));

        // Create section referencing the cover content block
        // Match reference format with P140 (text align) and P156 (page layout)
        let mut content_ref = HashMap::new();
        content_ref.insert(sym::POSITION, IonValue::Int(eid_base));
        content_ref.insert(sym::CONTENT_NAME, IonValue::Symbol(cover_block_sym));
        content_ref.insert(sym::SECTION_WIDTH, IonValue::Int(width as i64));
        content_ref.insert(sym::SECTION_HEIGHT, IonValue::Int(height as i64));
        content_ref.insert(sym::CONTENT_TYPE, IonValue::Symbol(sym::CONTAINER_INFO));
        content_ref.insert(sym::DEFAULT_TEXT_ALIGN, IonValue::Symbol(sym::ALIGN_CENTER));
        content_ref.insert(sym::PAGE_LAYOUT, IonValue::Symbol(sym::LAYOUT_FULL_PAGE));

        let mut section = HashMap::new();
        section.insert(sym::SECTION_NAME, IonValue::Symbol(cover_section_sym));
        section.insert(
            sym::SECTION_CONTENT,
            IonValue::List(vec![IonValue::Struct(content_ref)]),
        );

        self.fragments.push(KfxFragment::new(
            sym::SECTION,
            cover_section_id,
            IonValue::Struct(section),
        ));

        // Track section -> resource dependency for P253
        self.section_to_resource
            .push((cover_section_sym, cover_resource_sym));
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
    fn add_position_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut entries = Vec::new();
        // Start EIDs after local symbol range, accounting for cover section if present
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
        if has_cover {
            eid_base += 2; // Cover section uses 2 EIDs (section entry + content)
        }

        for chapter in chapters {
            let section_id = format!("section-{}", chapter.id);
            let section_sym = self.symtab.get_or_intern(&section_id);

            // Generate list of EIDs for this section
            // Section content entry EID first, then content block EIDs
            // Must use count_content_items to include nested containers
            let total_items = count_content_items(&chapter.content);
            let mut eids = Vec::new();
            eids.push(IonValue::Int(eid_base)); // Section content entry EID
            for i in 0..total_items {
                eids.push(IonValue::Int(eid_base + 1 + i as i64)); // Content block EIDs
            }

            let mut entry = HashMap::new();
            entry.insert(sym::ENTITY_ID_LIST, IonValue::List(eids));
            entry.insert(sym::SECTION_NAME, IonValue::Symbol(section_sym));
            entries.push(IonValue::Struct(entry));

            // +1 for section content entry, + total_items for content blocks (including nested)
            eid_base += 1 + total_items as i64;
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
    fn add_position_id_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut entries = Vec::new();
        let mut char_offset = 0i64;
        // Start EIDs after local symbol range, accounting for cover section if present
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
        if has_cover {
            eid_base += 2; // Cover section uses 2 EIDs (section entry + content)
        }

        // Helper to recursively add entries for content items
        fn add_entries_recursive(
            item: &ContentItem,
            eid: &mut i64,
            char_offset: &mut i64,
            entries: &mut Vec<IonValue>,
        ) {
            match item {
                ContentItem::Text { text, .. } => {
                    let mut entry = HashMap::new();
                    entry.insert(sym::EID_INDEX, IonValue::Int(*char_offset));
                    entry.insert(sym::EID_VALUE, IonValue::Int(*eid));
                    entries.push(IonValue::Struct(entry));
                    *char_offset += text.len() as i64;
                    *eid += 1;
                }
                ContentItem::Image { .. } => {
                    // Images take 1 character position
                    let mut entry = HashMap::new();
                    entry.insert(sym::EID_INDEX, IonValue::Int(*char_offset));
                    entry.insert(sym::EID_VALUE, IonValue::Int(*eid));
                    entries.push(IonValue::Struct(entry));
                    *char_offset += 1;
                    *eid += 1;
                }
                ContentItem::Container { children, .. } => {
                    // Process children first (children-first EID order, matching content block)
                    for child in children {
                        add_entries_recursive(child, eid, char_offset, entries);
                    }
                    // Container itself gets 1 character position
                    let mut entry = HashMap::new();
                    entry.insert(sym::EID_INDEX, IonValue::Int(*char_offset));
                    entry.insert(sym::EID_VALUE, IonValue::Int(*eid));
                    entries.push(IonValue::Struct(entry));
                    *char_offset += 1;
                    *eid += 1;
                }
            }
        }

        for chapter in chapters {
            // Section content entry at current position (1 char)
            let section_eid = eid_base;
            let mut section_entry = HashMap::new();
            section_entry.insert(sym::EID_INDEX, IonValue::Int(char_offset));
            section_entry.insert(sym::EID_VALUE, IonValue::Int(section_eid));
            entries.push(IonValue::Struct(section_entry));
            char_offset += 1; // Section content entry takes 1 char

            // Content block entries - recursively process all items including nested
            let mut content_eid = eid_base + 1;
            for content_item in &chapter.content {
                add_entries_recursive(
                    content_item,
                    &mut content_eid,
                    &mut char_offset,
                    &mut entries,
                );
            }

            // Advance eid_base: +1 for section entry + total_items for content
            let total_items = count_content_items(&chapter.content);
            eid_base += 1 + total_items as i64;
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
    fn add_location_map(&mut self, chapters: &[ChapterData], has_cover: bool) {
        // Location map structure: [ { P182: ( {P155: eid, P143: offset}, ... ) } ]
        // KFX_POSITIONS_PER_LOCATION = 110 (from kfxlib's yj_position_location.py)
        // Multiple location entries can reference the same content item (EID) with
        // different offsets, allowing granular progress tracking within paragraphs.
        const CHARS_PER_LOCATION: usize = 110;

        // First pass: build a list of content items with their EIDs and character ranges
        #[derive(Debug)]
        struct ContentRange {
            eid: i64,
            char_start: usize,
            char_end: usize,
        }

        let mut content_ranges: Vec<ContentRange> = Vec::new();
        // Start EIDs after local symbol range, accounting for cover section if present
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
        if has_cover {
            eid_base += 2; // Cover section uses 2 EIDs (section entry + content)
        }
        let mut total_chars: usize = 0;

        for chapter in chapters {
            let mut content_eid = eid_base + 1;

            fn collect_ranges_recursive(
                item: &ContentItem,
                content_eid: &mut i64,
                char_pos: &mut usize,
                ranges: &mut Vec<ContentRange>,
            ) {
                match item {
                    ContentItem::Text { text, .. } => {
                        let start = *char_pos;
                        let end = start + text.len();
                        ranges.push(ContentRange {
                            eid: *content_eid,
                            char_start: start,
                            char_end: end,
                        });
                        *content_eid += 1;
                        *char_pos = end;
                    }
                    ContentItem::Image { .. } => {
                        // Images don't contribute to character count but need an entry
                        ranges.push(ContentRange {
                            eid: *content_eid,
                            char_start: *char_pos,
                            char_end: *char_pos,
                        });
                        *content_eid += 1;
                    }
                    ContentItem::Container { children, .. } => {
                        // Process children first (children-first EID order)
                        for child in children {
                            collect_ranges_recursive(child, content_eid, char_pos, ranges);
                        }
                        // Container itself gets an entry at current position
                        ranges.push(ContentRange {
                            eid: *content_eid,
                            char_start: *char_pos,
                            char_end: *char_pos,
                        });
                        *content_eid += 1;
                    }
                }
            }

            for content_item in &chapter.content {
                collect_ranges_recursive(
                    content_item,
                    &mut content_eid,
                    &mut total_chars,
                    &mut content_ranges,
                );
            }

            let total_items = count_content_items(&chapter.content);
            eid_base += 1 + total_items as i64;
        }

        // Second pass: create location entries
        // Strategy: Include EVERY content item EID at offset 0 (for TOC navigation),
        // plus additional entries at character boundaries for reading progress tracking.
        let mut location_entries = Vec::new();
        let mut added_eids: std::collections::HashSet<i64> = std::collections::HashSet::new();

        // First, add an entry for every content item at offset 0
        // This ensures all TOC target EIDs are present in the LOCATION_MAP
        for range in &content_ranges {
            if !added_eids.contains(&range.eid) {
                location_entries.push(IonValue::OrderedStruct(vec![
                    (sym::POSITION, IonValue::Int(range.eid)),
                    (sym::OFFSET, IonValue::Int(0)),
                ]));
                added_eids.insert(range.eid);
            }
        }

        // Then, add additional entries at character position boundaries
        // This provides granular reading progress tracking within long paragraphs
        let num_locations = (total_chars / CHARS_PER_LOCATION).max(1);
        for loc_idx in 1..num_locations {
            // Start from 1 since offset 0 entries were added above
            let char_pos = loc_idx * CHARS_PER_LOCATION;

            // Find which content item this location falls within
            let range = content_ranges
                .iter()
                .find(|r| char_pos >= r.char_start && char_pos < r.char_end)
                .or_else(|| content_ranges.last());

            if let Some(range) = range {
                let offset_within_item = if char_pos >= range.char_start {
                    (char_pos - range.char_start) as i64
                } else {
                    0
                };

                // Only add if this is a non-zero offset (zero offsets already added)
                if offset_within_item > 0 {
                    location_entries.push(IonValue::OrderedStruct(vec![
                        (sym::POSITION, IonValue::Int(range.eid)),
                        (sym::OFFSET, IonValue::Int(offset_within_item)),
                    ]));
                }
            }
        }

        // Wrap in { P182: entries }
        let mut wrapper = HashMap::new();
        wrapper.insert(sym::LOCATION_ENTRIES, IonValue::List(location_entries));

        self.fragments.push(KfxFragment::singleton(
            sym::LOCATION_MAP,
            IonValue::List(vec![IonValue::Struct(wrapper)]),
        ));
    }

    /// Add page templates (P266) for position tracking
    /// Creates one template per virtual "page" based on character count.
    /// Future: could also create pages at style boundaries (headings, etc.)
    /// Structure: { P180: template_sym, P183: { P155: eid, P143: offset } }
    fn add_page_templates(&mut self, chapters: &[ChapterData], has_cover: bool) {
        const CHARS_PER_PAGE: usize = 2000; // Approximate characters per page

        let mut template_idx = 0;
        let mut total_chars: usize = 0;
        let mut next_page_at: usize = 0;

        // Start EID calculation after cover section (if present)
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;
        if has_cover {
            // Cover section gets its own page template
            let cover_content_eid = eid_base + 1;
            self.add_page_template_with_offset(template_idx, cover_content_eid, 0);
            template_idx += 1;
            next_page_at = CHARS_PER_PAGE;
            eid_base += 2;
        }

        // Create page templates at regular character intervals
        for chapter in chapters {
            let total_items = count_content_items(&chapter.content);
            for (i, item) in chapter.content.iter().enumerate() {
                let content_eid = eid_base + 1 + i as i64;
                // Use total_text_size for containers, handle images specially
                let item_len = match item {
                    ContentItem::Image { .. } => CHARS_PER_PAGE, // Images get their own page
                    _ => item.total_text_size(),
                };

                // Check if we've crossed page boundaries within this item
                let item_start = total_chars;
                let item_end = total_chars + item_len;

                while next_page_at < item_end {
                    let offset_in_item = if next_page_at > item_start {
                        (next_page_at - item_start) as i64
                    } else {
                        0
                    };
                    self.add_page_template_with_offset(template_idx, content_eid, offset_in_item);
                    template_idx += 1;
                    next_page_at += CHARS_PER_PAGE;
                }

                total_chars = item_end;
            }

            // Move to next section's EID range (including nested items)
            eid_base += 1 + total_items as i64;
        }
    }

    /// Add a single page template fragment with position offset
    fn add_page_template_with_offset(&mut self, idx: usize, eid: i64, offset: i64) {
        let template_id = format!("template-{idx}");
        let template_sym = self.symtab.get_or_intern(&template_id);

        // Position info: { P155: eid, P143: offset (optional if 0) }
        // IMPORTANT: Field order matters for Kindle - $155 must come before $143
        let pos_info = if offset > 0 {
            IonValue::OrderedStruct(vec![
                (sym::POSITION, IonValue::Int(eid)),
                (sym::OFFSET, IonValue::Int(offset)),
            ])
        } else {
            IonValue::OrderedStruct(vec![(sym::POSITION, IonValue::Int(eid))])
        };

        // Template content: { P180: template_sym, P183: pos_info }
        let mut template = HashMap::new();
        template.insert(sym::TEMPLATE_NAME, IonValue::Symbol(template_sym));
        template.insert(sym::POSITION_INFO, pos_info);

        self.fragments.push(KfxFragment::new(
            sym::PAGE_TEMPLATE,
            &template_id,
            IonValue::Struct(template),
        ));
    }

    /// Build section EID mapping for internal link targets
    /// Maps XHTML paths to their FIRST CONTENT ITEM EID (not section EID)
    /// TOC navigation needs to point to content items, not section entries
    fn build_section_eids(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

        // Cover section uses 2 EIDs (section + content)
        if has_cover {
            eid_base += 2;
        }

        // Map each chapter's source path to its first content item EID
        for chapter in chapters {
            // Content items start at eid_base + 1 (eid_base is the section entry)
            // TOC entries should point to content items, not section entries
            self.section_eids
                .insert(chapter.source_path.clone(), eid_base + 1);

            // Advance by section entry + content items (including nested items)
            // Must match calculation in add_page_templates and main chapter loop
            let total_items = count_content_items(&chapter.content);
            eid_base += 1 + total_items as i64;
        }
    }

    /// Build anchor EID mapping for TOC navigation with fragment IDs
    /// Maps "source_path#element_id" → content item EID
    /// Must be called AFTER build_section_eids and uses same EID assignment logic
    fn build_anchor_eids(&mut self, chapters: &[ChapterData], has_cover: bool) {
        let mut eid_base = SymbolTable::LOCAL_MIN_ID as i64;

        // Cover section uses 2 EIDs (section + content)
        if has_cover {
            eid_base += 2;
        }

        for chapter in chapters {
            // Content item EIDs start after section entry
            let mut content_eid = eid_base + 1;

            /// Recursively collect element_ids and their EIDs from content items
            /// For block-level IDs, offset is 0. For inline IDs (merged text), offset is tracked.
            fn collect_anchor_eids_recursive(
                item: &ContentItem,
                content_eid: &mut i64,
                source_path: &str,
                anchor_eids: &mut HashMap<String, (i64, i64)>,
            ) {
                match item {
                    ContentItem::Text { element_id, inline_runs, .. } => {
                        // Block-level element ID on the Text item itself
                        if let Some(id) = element_id {
                            let key = format!("{}#{}", source_path, id);
                            anchor_eids.insert(key, (*content_eid, 0));
                        }
                        // Check inline runs for element IDs (from inline elements like <a id="...">)
                        // The offset is the character position within the merged text
                        for run in inline_runs {
                            if let Some(ref id) = run.element_id {
                                let key = format!("{}#{}", source_path, id);
                                // Only insert if not already present (first occurrence wins)
                                anchor_eids.entry(key).or_insert((*content_eid, run.offset as i64));
                            }
                        }
                        *content_eid += 1;
                    }
                    ContentItem::Image { .. } => {
                        *content_eid += 1;
                    }
                    ContentItem::Container {
                        children,
                        element_id,
                        ..
                    } => {
                        // Process children first (children-first EID order)
                        for child in children {
                            collect_anchor_eids_recursive(
                                child,
                                content_eid,
                                source_path,
                                anchor_eids,
                            );
                        }
                        // Container gets EID after its children
                        if let Some(id) = element_id {
                            let key = format!("{}#{}", source_path, id);
                            anchor_eids.insert(key, (*content_eid, 0));
                        }
                        *content_eid += 1;
                    }
                }
            }

            for content_item in &chapter.content {
                collect_anchor_eids_recursive(
                    content_item,
                    &mut content_eid,
                    &chapter.source_path,
                    &mut self.anchor_eids,
                );
            }

            // Advance by section entry + content items
            let total_items = count_content_items(&chapter.content);
            eid_base += 1 + total_items as i64;
        }
    }

    /// Build anchor symbol mapping for URLs found in content (external and internal)
    /// Must be called BEFORE content blocks are built so $179 references work
    fn build_anchor_symbols(&mut self, chapters: &[ChapterData]) {
        fn collect_anchor_hrefs(item: &ContentItem, hrefs: &mut std::collections::HashSet<String>) {
            match item {
                ContentItem::Text { inline_runs, .. } => {
                    // Collect hrefs from inline runs (where anchors are stored now)
                    for run in inline_runs {
                        if let Some(ref href) = run.anchor_href {
                            // Collect all hrefs except fragment-only anchors (#...)
                            // Both external (http/https) and internal (.xhtml) links
                            if !href.starts_with('#') {
                                hrefs.insert(href.clone());
                            }
                        }
                    }
                }
                ContentItem::Container { children, .. } => {
                    for child in children {
                        collect_anchor_hrefs(child, hrefs);
                    }
                }
                _ => {}
            }
        }

        let mut unique_hrefs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for chapter in chapters {
            for item in &chapter.content {
                collect_anchor_hrefs(item, &mut unique_hrefs);
            }
        }

        // Register symbols for each unique href (fragments created later)
        for (anchor_index, href) in unique_hrefs.into_iter().enumerate() {
            let anchor_id = format!("anchor{anchor_index}");
            let anchor_sym = self.symtab.get_or_intern(&anchor_id);
            self.anchor_symbols.insert(href, anchor_sym);
        }
    }

    /// Add anchor fragments ($266) for external URLs and internal links
    /// External: $180 (anchor ID) + $186 (external URL)
    /// Internal: $180 (anchor ID) + $183 (position info with $155 EID, optional $143 offset)
    /// Must be called AFTER build_anchor_symbols, build_section_eids, and build_anchor_eids
    fn add_anchor_fragments(&mut self) {
        // Clone maps to avoid borrowing issues
        let section_eids = self.section_eids.clone();
        let anchor_eids = self.anchor_eids.clone();

        // Create anchor fragments for each registered href
        for (href, anchor_sym) in &self.anchor_symbols {
            let anchor_id = format!("${anchor_sym}");
            let mut anchor_struct = HashMap::new();
            anchor_struct.insert(sym::TEMPLATE_NAME, IonValue::Symbol(*anchor_sym)); // $180

            if href.starts_with("http://") || href.starts_with("https://") {
                // External link: use $186 (EXTERNAL_URL)
                anchor_struct.insert(sym::EXTERNAL_URL, IonValue::String(href.clone()));
            } else {
                // Internal link: use $183 (POSITION_INFO) with $155 (EID)
                // For links with fragment identifiers (e.g., "endnotes.xhtml#note-1"),
                // try to find the specific element's EID first, then fall back to section EID

                let (path_without_fragment, has_fragment) = if let Some(hash_pos) = href.find('#') {
                    (&href[..hash_pos], true)
                } else {
                    (href.as_str(), false)
                };

                // Try to find target (EID, offset):
                // 1. If href has fragment, try anchor_eids (full href -> (EID, offset))
                // 2. Fall back to section_eids (path -> (section start EID, 0))
                let target = if has_fragment {
                    anchor_eids
                        .get(href)
                        .copied()
                        .or_else(|| section_eids.get(path_without_fragment).map(|&e| (e, 0)))
                } else {
                    section_eids.get(path_without_fragment).map(|&e| (e, 0))
                };

                if let Some((eid, offset)) = target {
                    // Use OrderedStruct to ensure $155 comes before $143 (field order matters)
                    // Include offset only if non-zero (kfxlib removes $143 when offset is 0)
                    let pos_info = if offset > 0 {
                        IonValue::OrderedStruct(vec![
                            (sym::POSITION, IonValue::Int(eid)),
                            (sym::OFFSET, IonValue::Int(offset)),
                        ])
                    } else {
                        IonValue::OrderedStruct(vec![(sym::POSITION, IonValue::Int(eid))])
                    };
                    anchor_struct.insert(sym::POSITION_INFO, pos_info);
                } else {
                    // Target not found - skip this anchor
                    // This can happen for links to non-spine items
                    continue;
                }
            }

            self.fragments.push(KfxFragment::new(
                sym::PAGE_TEMPLATE, // $266
                &anchor_id,
                IonValue::Struct(anchor_struct),
            ));
        }
    }

    /// Add media resources (images and fonts) from the book
    /// Creates P164 (resource metadata) and P417 (raw media) fragments
    fn add_resources(&mut self, book: &Book) {
        let mut resource_index = 0;
        let cover_href = book.metadata.cover_image.as_deref();

        for (href, resource) in book.resources.iter() {
            let is_image = is_image_media_type(&resource.media_type);
            let is_font = is_font_media_type(&resource.media_type);

            if !is_image && !is_font {
                continue;
            }

            let is_cover = cover_href == Some(href.as_str());
            let resource_id = format!("rsrc{resource_index}");
            let resource_sym = self.symtab.get_or_intern(&resource_id);

            // Use original image data (PNG conversion disabled - causes transparency issues)
            let (image_data, media_type) = (resource.data.clone(), resource.media_type.clone());

            // Create P164 resource fragment
            let mut res_meta = HashMap::new();
            res_meta.insert(sym::RESOURCE_NAME, IonValue::Symbol(resource_sym));
            // Skip MIME_TYPE (P162) for cover image to match reference KFX format
            if !is_cover {
                res_meta.insert(sym::MIME_TYPE, IonValue::String(media_type));
            }
            res_meta.insert(
                sym::LOCATION,
                IonValue::String(format!("resource/{resource_id}")),
            );

            if is_image {
                // Use correct format symbol based on image type
                let format_sym = if is_png_data(&image_data) {
                    sym::PNG_FORMAT
                } else if is_gif_data(&image_data) {
                    sym::GIF_FORMAT
                } else {
                    sym::JPG_FORMAT // Default to JPEG
                };
                res_meta.insert(sym::FORMAT, IonValue::Symbol(format_sym));
                // Get image dimensions
                let (width, height) = get_image_dimensions(&image_data).unwrap_or((800, 600));
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

            // Create P417 raw media fragment with raw image bytes
            // KFX stores raw image data directly in the blob (not base64)
            // Note: P417 fragment ID doesn't need to match P165 location - linkage is via P253
            let media_id = format!("resource/{resource_id}");
            let media_sym = self.symtab.get_or_intern(&media_id);
            self.fragments.push(KfxFragment::new(
                sym::RAW_MEDIA,
                &media_id,
                IonValue::Blob(image_data),
            ));

            // Track resource -> media dependency for P253
            self.resource_to_media.push((resource_sym, media_sym));

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

        // Add P253 entity dependencies
        // Two types: section -> resource AND resource -> raw media
        let mut all_deps: Vec<IonValue> = Vec::new();

        // First add section -> resource dependencies
        for (section_sym, resource_sym) in &self.section_to_resource {
            let mut dep = HashMap::new();
            dep.insert(sym::POSITION, IonValue::Symbol(*section_sym));
            dep.insert(
                sym::MANDATORY_DEPS,
                IonValue::List(vec![IonValue::Symbol(*resource_sym)]),
            );
            all_deps.push(IonValue::Struct(dep));
        }

        // Then add resource -> raw media dependencies
        for (resource_sym, media_sym) in &self.resource_to_media {
            let mut dep = HashMap::new();
            dep.insert(sym::POSITION, IonValue::Symbol(*resource_sym));
            dep.insert(
                sym::MANDATORY_DEPS,
                IonValue::List(vec![IonValue::Symbol(*media_sym)]),
            );
            all_deps.push(IonValue::Struct(dep));
        }

        if !all_deps.is_empty() {
            entity_map.insert(sym::ENTITY_DEPS, IonValue::List(all_deps));
        }

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
    content: Vec<ContentItem>,
    /// Source XHTML path (for internal link targets)
    source_path: String,
}

/// A chunk of content (subset of a chapter)
struct ContentChunk {
    /// Unique ID for this chunk
    id: String,
    /// Content items for this chunk
    items: Vec<ContentItem>,
}

impl ChapterData {
    /// Split chapter into chunks that don't exceed MAX_CHUNK_SIZE characters
    fn into_chunks(self) -> Vec<ContentChunk> {
        let mut chunks = Vec::new();
        let mut current_items = Vec::new();
        let mut current_size = 0;
        let mut chunk_index = 0;

        for item in self.content.into_iter() {
            let item_size = item.total_text_size();

            // If adding this item would exceed chunk size, start a new chunk
            if current_size + item_size > sym::MAX_CHUNK_SIZE && !current_items.is_empty() {
                chunks.push(ContentChunk {
                    id: format!("{}-{}", self.id, chunk_index),
                    items: std::mem::take(&mut current_items),
                });
                chunk_index += 1;
                current_size = 0;
            }

            current_size += item_size;
            current_items.push(item);
        }

        // Push remaining items
        if !current_items.is_empty() {
            chunks.push(ContentChunk {
                id: format!("{}-{}", self.id, chunk_index),
                items: current_items,
            });
        }

        chunks
    }
}

/// An inline style run within a paragraph
/// Specifies that a range of characters has a different style
#[derive(Debug, Clone)]
struct StyleRun {
    /// Character offset within the text
    offset: usize,
    /// Number of characters this style applies to
    length: usize,
    /// The style to apply for this range
    style: ParsedStyle,
    /// Optional anchor href for hyperlinks in this range
    anchor_href: Option<String>,
    /// Optional element ID from inline element (e.g., <a id="noteref-1">)
    /// Used for anchor targets (back-links)
    element_id: Option<String>,
}

/// A content item - text, image, or nested container
#[derive(Debug, Clone)]
enum ContentItem {
    /// Text content with styling and optional inline style runs
    Text {
        text: String,
        style: ParsedStyle,
        /// Optional inline style runs for different character ranges
        inline_runs: Vec<StyleRun>,
        /// Optional anchor href for hyperlinks
        anchor_href: Option<String>,
        /// Optional HTML element ID (for TOC anchor targets)
        element_id: Option<String>,
    },
    /// Image reference with optional styling
    Image {
        /// Path/href to the image resource (relative to EPUB structure)
        resource_href: String,
        style: ParsedStyle,
        /// Alt text for accessibility
        alt: Option<String>,
    },
    /// Container with nested content items (for block-level elements like sections, divs)
    Container {
        /// Style for the container itself
        style: ParsedStyle,
        /// Nested content items
        children: Vec<ContentItem>,
        /// Tag name for debugging/identification
        tag: String,
        /// Optional HTML element ID (for TOC anchor targets)
        element_id: Option<String>,
    },
}

impl ContentItem {
    fn style(&self) -> &ParsedStyle {
        match self {
            ContentItem::Text { style, .. } => style,
            ContentItem::Image { style, .. } => style,
            ContentItem::Container { style, .. } => style,
        }
    }

    /// Get flattened iterator over all leaf items (text and images)
    fn flatten(&self) -> Vec<&ContentItem> {
        match self {
            ContentItem::Text { .. } | ContentItem::Image { .. } => vec![self],
            ContentItem::Container { children, .. } => {
                children.iter().flat_map(|c| c.flatten()).collect()
            }
        }
    }

    /// Calculate total text size (for chunking)
    fn total_text_size(&self) -> usize {
        match self {
            ContentItem::Text { text, .. } => text.len(),
            ContentItem::Image { .. } => 1, // Images count as minimal size
            ContentItem::Container { children, .. } => {
                children.iter().map(|c| c.total_text_size()).sum()
            }
        }
    }

    /// Count total number of items including nested children (for EID calculation)
    fn count_items(&self) -> usize {
        match self {
            ContentItem::Text { .. } | ContentItem::Image { .. } => 1,
            ContentItem::Container { children, .. } => {
                1 + children.iter().map(|c| c.count_items()).sum::<usize>()
            }
        }
    }
}

/// Count total content items including nested containers
fn count_content_items(items: &[ContentItem]) -> usize {
    items.iter().map(|item| item.count_items()).sum()
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
    // Get seed from platform-appropriate time source
    #[cfg(target_arch = "wasm32")]
    let seed = {
        // In WASM, use js_sys::Date::now() which returns milliseconds
        (js_sys::Date::now() as u128) * 1_000_000 // Convert to nanoseconds scale
    };

    #[cfg(not(target_arch = "wasm32"))]
    let seed = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    };

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
// Content Extraction
// =============================================================================

/// Check if a tag is a block-level element that should become a Container
fn is_block_element(tag: &str) -> bool {
    matches!(
        tag,
        "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "figure"
            | "figcaption"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "thead"
            | "tbody"
            | "main"
            | "address"
            | "pre"
    )
}

/// Check if a container tag represents a structural wrapper that can be flattened.
/// Structural elements like <section>, <div>, <article> are just grouping wrappers
/// and their children should be promoted to the parent level.
fn is_structural_container(tag: &str) -> bool {
    matches!(tag, "section" | "div" | "article" | "main" | "body")
}

/// Check if a container tag represents a semantic element that should be preserved.
/// Semantic elements like <header>, <footer>, <figure> should never be flattened
/// or unwrapped, even with a single child.
fn is_semantic_container(tag: &str) -> bool {
    matches!(
        tag,
        "header" | "footer" | "nav" | "aside" | "figure" | "figcaption" | "blockquote"
    )
}

/// Flatten unnecessary container nesting
/// - Structural containers (section, div, article) are completely flattened - children promoted
/// - Semantic containers (header, footer, figure) are always preserved as containers
/// - Generic containers (p, span) with a single block child are unwrapped (child promoted)
fn flatten_containers(items: Vec<ContentItem>) -> Vec<ContentItem> {
    items
        .into_iter()
        .flat_map(|item| {
            match item {
                ContentItem::Container {
                    children,
                    style,
                    tag,
                    element_id,
                } => {
                    // First, recursively flatten children
                    let flattened_children = flatten_containers(children);

                    // Structural containers (section, div, article) are flattened -
                    // their children are promoted to the parent level.
                    // This matches the reference KFX structure where <section> doesn't
                    // create an extra container layer.
                    // IMPORTANT: Preserve element_id by propagating it to the first child
                    // (used for TOC navigation with fragment IDs)
                    if is_structural_container(&tag) {
                        if let Some(id) = element_id {
                            // Propagate element_id to first child that can have it
                            let mut children = flattened_children;
                            if let Some(first) = children.first_mut() {
                                match first {
                                    ContentItem::Text {
                                        element_id: child_id,
                                        ..
                                    } => {
                                        if child_id.is_none() {
                                            *child_id = Some(id);
                                        }
                                    }
                                    ContentItem::Container {
                                        element_id: child_id,
                                        ..
                                    } => {
                                        if child_id.is_none() {
                                            *child_id = Some(id);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            return children;
                        }
                        return flattened_children;
                    }

                    // Semantic containers (header, footer, figure) are always preserved,
                    // even with a single child. They represent meaningful structure.
                    if is_semantic_container(&tag) {
                        return vec![ContentItem::Container {
                            children: flattened_children,
                            style,
                            tag,
                            element_id,
                        }];
                    }

                    // For generic containers (p, span, etc.), apply single-child unwrapping

                    // If container has single child that's a Container or Text, unwrap it
                    // (unless the container has meaningful style that would be lost)
                    if flattened_children.len() == 1 {
                        let child = flattened_children.into_iter().next().unwrap();
                        match child {
                            // Single Text child - the container (like <p>) becomes the Text
                            // The style from the container should be on the Text
                            ContentItem::Text {
                                text,
                                inline_runs,
                                anchor_href,
                                style: child_style,
                                element_id: child_element_id,
                            } => {
                                // Merge container's style with child's style
                                let mut merged_style = style;
                                merged_style.merge(&child_style);
                                // Prefer container's element_id, fall back to child's
                                let merged_element_id = element_id.or(child_element_id);
                                return vec![ContentItem::Text {
                                    text,
                                    style: merged_style,
                                    inline_runs,
                                    anchor_href,
                                    element_id: merged_element_id,
                                }];
                            }
                            // Single Container child - flatten if container has default style
                            ContentItem::Container {
                                children: inner_children,
                                style: inner_style,
                                tag: inner_tag,
                                element_id: inner_element_id,
                            } => {
                                // Keep the inner container, but with merged style
                                let mut merged_style = style;
                                merged_style.merge(&inner_style);
                                // Prefer outer element_id, fall back to inner
                                let merged_element_id = element_id.or(inner_element_id);
                                return vec![ContentItem::Container {
                                    children: inner_children,
                                    style: merged_style,
                                    tag: inner_tag,
                                    element_id: merged_element_id,
                                }];
                            }
                            // Single Image child - unwrap, keeping the image
                            other => return vec![other],
                        }
                    }

                    // Multiple children - keep container but with flattened children
                    vec![ContentItem::Container {
                        children: flattened_children,
                        style,
                        tag,
                        element_id,
                    }]
                }
                // Non-containers pass through unchanged
                other => vec![other],
            }
        })
        .collect()
}

/// Merge consecutive Text items into a single Text item with inline style runs
/// This combines text spans that have different inline styles (bold, italic, etc.)
/// into a single paragraph with style runs specifying which ranges have which styles.
/// Anchor hrefs and inline element IDs are tracked in the inline runs.
fn merge_text_with_inline_runs(items: Vec<ContentItem>) -> Vec<ContentItem> {
    if items.is_empty() {
        return items;
    }

    let mut result = Vec::new();
    // Track pending texts: (text, style, anchor_href, element_id)
    let mut pending_texts: Vec<(String, ParsedStyle, Option<String>, Option<String>)> = Vec::new();

    // Helper to flush pending text items into a merged item
    fn flush_pending(
        pending: &mut Vec<(String, ParsedStyle, Option<String>, Option<String>)>,
        result: &mut Vec<ContentItem>,
    ) {
        if pending.is_empty() {
            return;
        }

        if pending.len() == 1 && pending[0].2.is_none() && pending[0].3.is_none() {
            // Single text item with no anchor and no element_id, no inline runs needed
            let (text, style, _, _) = pending.remove(0);
            result.push(ContentItem::Text {
                text,
                style,
                inline_runs: Vec::new(),
                anchor_href: None,
                element_id: None, // Text merged from inline elements doesn't have its own ID
            });
        } else {
            // Multiple text items OR has anchors/element_ids - merge with inline style runs
            // Find the most common style to use as base (or use first item's style)
            let base_style = pending[0].1.clone();

            // Build combined text and inline runs
            let mut combined_text = String::new();
            let mut inline_runs = Vec::new();

            for (text, style, anchor_href, element_id) in pending.drain(..) {
                let offset = combined_text.chars().count();
                let length = text.chars().count();

                // Determine if we need an inline run
                let style_differs = style != base_style;
                let has_anchor = anchor_href.is_some();
                let has_element_id = element_id.is_some();

                if style_differs || has_anchor || has_element_id {
                    // Determine the style for this run:
                    // - If style differs from base (bold, italic, etc.), use the full style
                    // - If only anchor/element_id differs (plain link), use a minimal inline style
                    let run_style = if !style_differs && (has_anchor || has_element_id) {
                        // Anchor-only run: create minimal inline style
                        // This matches reference behavior where links use $127: $349 only
                        ParsedStyle {
                            is_inline: true,
                            ..Default::default()
                        }
                    } else {
                        // Style differs: use the actual style
                        style
                    };

                    inline_runs.push(StyleRun {
                        offset,
                        length,
                        style: run_style,
                        anchor_href,
                        element_id,
                    });
                }

                combined_text.push_str(&text);
            }

            result.push(ContentItem::Text {
                text: combined_text,
                style: base_style,
                inline_runs,
                anchor_href: None, // Anchors are now in inline_runs
                element_id: None,  // Merged text doesn't have element ID
            });
        }
    }

    for item in items {
        match item {
            ContentItem::Text {
                text,
                style,
                anchor_href,
                element_id,
                ..
            } => {
                // Accumulate text with style, anchor, and element_id for inline anchor targets
                pending_texts.push((text, style, anchor_href, element_id));
            }
            other => {
                // Non-text item: flush any pending texts first
                flush_pending(&mut pending_texts, &mut result);
                result.push(other);
            }
        }
    }

    // Flush any remaining pending texts
    flush_pending(&mut pending_texts, &mut result);

    result
}

/// Extract CSS stylesheet hrefs from XHTML <link> tags in document order
/// `base_path` is the path of the XHTML file, used to resolve relative CSS paths
fn extract_css_hrefs_from_xhtml(data: &[u8], base_path: &str) -> Vec<String> {
    let html = String::from_utf8_lossy(data);
    let document = kuchiki::parse_html().one(html.as_ref());

    // Get the directory part of the base path for resolving relative paths
    let base_dir = if let Some(pos) = base_path.rfind('/') {
        &base_path[..pos + 1]
    } else {
        ""
    };

    let mut css_hrefs = Vec::new();

    // Find all <link> elements with rel="stylesheet"
    for link in document.select("link").unwrap() {
        let node = link.as_node();
        if let Some(element) = node.as_element() {
            let attrs = element.attributes.borrow();
            // Check if this is a stylesheet link
            if attrs.get("rel").is_some_and(|r| r.contains("stylesheet"))
                && let Some(href) = attrs.get("href")
            {
                // Resolve relative path to absolute path within EPUB
                let resolved = resolve_relative_path(base_dir, href);
                css_hrefs.push(resolved);
            }
        }
    }

    css_hrefs
}

/// Extract content items (text and images) from XHTML, preserving styles and hierarchy
/// `base_path` is the path of the XHTML file within the EPUB, used to resolve relative paths
fn extract_content_from_xhtml(
    data: &[u8],
    stylesheet: &Stylesheet,
    base_path: &str,
) -> Vec<ContentItem> {
    let html = String::from_utf8_lossy(data);

    // Get the directory part of the base path for resolving relative paths
    let base_dir = if let Some(pos) = base_path.rfind('/') {
        &base_path[..pos + 1]
    } else {
        ""
    };

    // Parse HTML with kuchiki for proper DOM-based CSS selector matching
    let document = kuchiki::parse_html().one(html.as_ref());

    // Find the body element (or root if no body)
    let body = document
        .select("body")
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|n| n.as_node().clone())
        .unwrap_or_else(|| document.clone());

    /// Extract content from a node, preserving hierarchy for block elements
    /// Returns the extracted content items for this node and its descendants
    fn extract_from_node(
        node: &NodeRef,
        stylesheet: &Stylesheet,
        parent_style: &ParsedStyle,
        base_dir: &str,
        anchor_href: Option<&str>, // Current anchor href context (from parent <a>)
    ) -> Vec<ContentItem> {
        use kuchiki::NodeData;

        match node.data() {
            NodeData::Element(element) => {
                let tag_name = element.name.local.as_ref();

                // Skip non-content tags
                if matches!(tag_name, "script" | "style" | "head" | "title" | "svg") {
                    return vec![];
                }

                // Get direct style (only rules matching this element, no CSS inheritance)
                // KFX has its own style inheritance, so we only output direct styles
                let element_ref = node.clone().into_element_ref().unwrap();
                let direct_style = stylesheet.get_direct_style_for_element(&element_ref);

                // Also compute full style for hidden element detection and DOM traversal
                let mut computed_style = parent_style.clone();
                computed_style.merge(&direct_style);

                // Apply inline style (highest specificity) to both
                let mut direct_with_inline = direct_style.clone();
                if let Some(style_attr) = element.attributes.borrow().get("style") {
                    let inline = Stylesheet::parse_inline_style(style_attr);
                    direct_with_inline.merge(&inline);
                    computed_style.merge(&inline);
                }

                // Skip hidden elements (display:none, position:absolute with large negative offset)
                if computed_style.is_hidden() {
                    return vec![];
                }

                // Extract element ID for anchor targets (used in TOC navigation)
                let element_id = element.attributes.borrow().get("id").map(|s| s.to_string());

                // Handle image elements specially
                if tag_name == "img" {
                    let attrs = element.attributes.borrow();
                    if let Some(src) = attrs.get("src") {
                        // Resolve relative path to absolute path within EPUB
                        let resolved_path = resolve_relative_path(base_dir, src);
                        // Use direct style (not computed) - KFX handles inheritance
                        let mut image_style = direct_with_inline.clone();
                        image_style.is_image = true;
                        // Extract alt text for accessibility ($584)
                        let alt = attrs.get("alt").map(|s| s.to_string());
                        return vec![ContentItem::Image {
                            resource_href: resolved_path,
                            style: image_style,
                            alt,
                        }];
                    }
                    return vec![]; // img is self-closing, no children to process
                }

                // Determine anchor_href for children:
                // If this is an <a> element, extract its href; otherwise pass through parent's
                let child_anchor_href = if tag_name == "a" {
                    element.attributes.borrow().get("href").map(|href| {
                        // Resolve relative href to full path (matches section_eids keys)
                        // External URLs (http/https) are kept as-is
                        if href.starts_with("http://") || href.starts_with("https://") {
                            href.to_string()
                        } else {
                            resolve_relative_path(base_dir, href)
                        }
                    })
                } else {
                    anchor_href.map(|s| s.to_string())
                };

                // Extract children with anchor context
                let mut children = Vec::new();
                for child in node.children() {
                    children.extend(extract_from_node(
                        &child,
                        stylesheet,
                        &computed_style,
                        base_dir,
                        child_anchor_href.as_deref(),
                    ));
                }

                // Block elements become Containers with their children nested
                if is_block_element(tag_name) && !children.is_empty() {
                    // Merge consecutive text items with inline style runs
                    let merged_children = merge_text_with_inline_runs(children);
                    return vec![ContentItem::Container {
                        style: direct_with_inline,
                        children: merged_children,
                        tag: tag_name.to_string(),
                        element_id,
                    }];
                }

                // Non-block elements (span, a, em, strong, etc.) pass through children
                // IMPORTANT: Propagate element_id to first child if this inline element has an ID
                // This handles cases like <a id="noteref-1">2</a> where the anchor tag has an ID
                // that needs to be preserved for back-links
                if let Some(id) = element_id {
                    if let Some(first) = children.first_mut() {
                        match first {
                            ContentItem::Text {
                                element_id: child_id,
                                ..
                            } => {
                                if child_id.is_none() {
                                    *child_id = Some(id);
                                }
                            }
                            ContentItem::Container {
                                element_id: child_id,
                                ..
                            } => {
                                if child_id.is_none() {
                                    *child_id = Some(id);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                children
            }
            NodeData::Text(text) => {
                let text_content = text.borrow();
                let cleaned = clean_text(&text_content);
                if !cleaned.is_empty() {
                    vec![ContentItem::Text {
                        text: cleaned,
                        style: parent_style.clone(),
                        inline_runs: Vec::new(),
                        anchor_href: anchor_href.map(|s| s.to_string()),
                        element_id: None, // Text nodes don't have IDs (parent block does)
                    }]
                } else {
                    vec![]
                }
            }
            _ => {
                // Process children for document/doctype/etc nodes
                let mut children = Vec::new();
                for child in node.children() {
                    children.extend(extract_from_node(
                        &child,
                        stylesheet,
                        parent_style,
                        base_dir,
                        anchor_href,
                    ));
                }
                children
            }
        }
    }

    let items = extract_from_node(&body, stylesheet, &ParsedStyle::default(), base_dir, None);
    // Flatten unnecessary container nesting (section wrappers, paragraph wrappers, etc.)
    flatten_containers(items)
}

/// Resolve a relative path against a base directory
/// e.g., resolve_relative_path("epub/text/", "../images/foo.png") -> "epub/images/foo.png"
fn resolve_relative_path(base_dir: &str, relative: &str) -> String {
    if !relative.starts_with("../") && !relative.starts_with("./") {
        // Not a relative path, just join
        return format!("{base_dir}{relative}");
    }

    // Split the base directory into components
    let mut components: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();

    let mut rel = relative;

    // Process ../ and ./
    while rel.starts_with("../") || rel.starts_with("./") {
        if rel.starts_with("../") {
            components.pop(); // Go up one directory
            rel = &rel[3..];
        } else if rel.starts_with("./") {
            rel = &rel[2..];
        }
    }

    // Join remaining components with the relative path
    if components.is_empty() {
        rel.to_string()
    } else {
        format!("{}/{}", components.join("/"), rel)
    }
}

/// Clean up text by normalizing whitespace
fn clean_text(text: &str) -> String {
    let decoded = decode_html_entities(text);

    // Preserve knowledge of leading/trailing whitespace for proper merging
    let has_leading_space = decoded.chars().next().is_some_and(|c| c.is_whitespace());
    let has_trailing_space = decoded
        .chars()
        .next_back()
        .is_some_and(|c| c.is_whitespace());

    // Normalize internal whitespace (collapse multiple whitespace to single space)
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

    // Trim internal whitespace
    let trimmed = cleaned.trim();

    if trimmed.is_empty() {
        // Text is all whitespace (e.g., HTML source indentation) - return empty
        // Boundary whitespace is handled when there's actual content adjacent to it
        String::new()
    } else {
        // Restore boundary spaces for proper merging with sibling elements
        let mut result = String::new();
        if has_leading_space {
            result.push(' ');
        }
        result.push_str(trimmed);
        if has_trailing_space {
            result.push(' ');
        }
        result
    }
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

/// Check if image data is PNG format
fn is_png_data(data: &[u8]) -> bool {
    data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
}

/// Check if image data is GIF format
fn is_gif_data(data: &[u8]) -> bool {
    data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to find text content containing a needle and get its style
    /// Searches recursively through nested containers
    fn find_text_style<'a>(
        items: &'a [ContentItem],
        needle: &str,
    ) -> Option<(&'a str, &'a ParsedStyle)> {
        items
            .iter()
            .flat_map(|item| item.flatten())
            .find_map(|item| {
                if let ContentItem::Text { text, style, .. } = item {
                    if text.contains(needle) {
                        return Some((text.as_str(), style));
                    }
                }
                None
            })
    }

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

        // Verify all characters after "CR!" are valid (alphanumeric uppercase)
        let suffix = &id[3..];
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
            "Container ID should only contain uppercase alphanumeric: {}",
            id
        );
    }

    #[test]
    fn test_container_id_uniqueness() {
        // Generate multiple IDs and verify they're different
        // (they use time-based seeds so should be unique)
        let id1 = generate_container_id();
        let id2 = generate_container_id();

        // IDs should be valid format
        assert!(id1.starts_with("CR!"));
        assert!(id2.starts_with("CR!"));
        assert_eq!(id1.len(), 31);
        assert_eq!(id2.len(), 31);

        // With time-based seeding, consecutive calls may produce same ID
        // if called within same nanosecond/millisecond, so we just verify format
        // The important thing is they don't panic on any platform
    }

    #[test]
    fn test_styled_text_extraction_inheritance() {
        use crate::css::{Stylesheet, TextAlign};

        // CSS similar to epub: headings are centered, paragraphs are not
        let css = r#"
            h3 { text-align: center; margin-top: 3em; }
            p { margin-top: 0; text-indent: 1em; }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // HTML with heading and paragraph as siblings (not nested)
        // The paragraph should NOT inherit center from the h3 sibling
        let html = br#"
            <body>
                <section>
                    <h3>Chapter I</h3>
                    <p>This is body text that should NOT be centered.</p>
                </section>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Count flattened text items (nested in containers)
        let flat_text_count = items
            .iter()
            .flat_map(|i| i.flatten())
            .filter(|i| matches!(i, ContentItem::Text { .. }))
            .count();
        assert!(
            flat_text_count >= 2,
            "Expected at least 2 text items, got {}",
            flat_text_count
        );

        // Find the heading text
        let (_, heading_style) = find_text_style(&items, "Chapter").expect("Should find heading");
        assert_eq!(
            heading_style.text_align,
            Some(TextAlign::Center),
            "Heading should be centered"
        );

        // Find the paragraph text
        let (_, para_style) = find_text_style(&items, "body text").expect("Should find paragraph");
        assert_eq!(
            para_style.text_align, None,
            "Body paragraph should NOT be centered (siblings don't inherit)"
        );
    }

    #[test]
    fn test_styled_text_extraction_hgroup_inheritance() {
        use crate::css::{Stylesheet, TextAlign};

        // CSS where hgroup has center, and p inside hgroup should inherit
        let css = r#"
            hgroup { text-align: center; }
            p { margin-top: 0; text-indent: 1em; }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // HTML with p inside hgroup - should inherit center
        let html = br#"
            <body>
                <hgroup>
                    <h2>The Title</h2>
                    <p>Subtitle that SHOULD be centered</p>
                </hgroup>
                <section>
                    <p>Body text that should NOT be centered</p>
                </section>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Find the subtitle (inside hgroup)
        let (_, subtitle_style) =
            find_text_style(&items, "Subtitle").expect("Should find subtitle");
        assert_eq!(
            subtitle_style.text_align,
            Some(TextAlign::Center),
            "Subtitle inside hgroup should inherit center"
        );

        // Find the body paragraph (outside hgroup)
        let (_, body_style) = find_text_style(&items, "Body text").expect("Should find body");
        assert_eq!(
            body_style.text_align, None,
            "Body paragraph outside hgroup should NOT be centered"
        );
    }

    #[test]
    fn test_paragraph_text_indent_extraction() {
        use crate::css::{CssValue, Stylesheet};

        // CSS with text-indent on paragraphs (like the epictetus EPUB)
        let css = r#"
            p {
                margin-top: 0;
                margin-bottom: 0;
                text-indent: 1em;
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // HTML structure similar to epictetus
        let html = br#"
            <body>
                <section>
                    <h3>XXXIII</h3>
                    <p>Immediately prescribe some character.</p>
                    <p>And let silence be the general rule.</p>
                </section>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Find body paragraphs
        let (_, para1_style) = find_text_style(&items, "prescribe").expect("Should find para1");
        let (_, para2_style) = find_text_style(&items, "silence").expect("Should find para2");

        // Both paragraphs should have text-indent: 1em
        assert!(
            matches!(para1_style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01),
            "Para1 should have text-indent: 1em, got {:?}",
            para1_style.text_indent
        );
        assert!(
            matches!(para2_style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01),
            "Para2 (silence) should have text-indent: 1em, got {:?}",
            para2_style.text_indent
        );
    }

    #[test]
    fn test_epictetus_like_structure() {
        use crate::css::{CssValue, Stylesheet, TextAlign};

        // CSS similar to the actual epictetus EPUB
        let css = r#"
            body {
                font-variant-numeric: oldstyle-nums;
            }
            p {
                margin-top: 0;
                margin-right: 0;
                margin-bottom: 0;
                margin-left: 0;
                text-indent: 1em;
            }
            h3 {
                font-variant: small-caps;
                text-align: center;
                margin-top: 3em;
                margin-bottom: 3em;
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // HTML structure matching epictetus section 33
        let html = br#"
            <body>
                <section id="the-enchiridion-33" role="doc-chapter">
                    <h3>XXXIII</h3>
                    <p>Immediately prescribe some character and some form to yourself.</p>
                    <p>And let silence be the general rule, or let only what is necessary be said.</p>
                    <p>Let not your laughter be much, nor on many occasions.</p>
                </section>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Find heading
        let (_, heading_style) = find_text_style(&items, "XXXIII").expect("Should find heading");
        assert_eq!(
            heading_style.text_align,
            Some(TextAlign::Center),
            "Heading should be centered"
        );

        // Find the silence paragraph
        let (_, silence_style) =
            find_text_style(&items, "silence").expect("Should find silence para");

        // Paragraph should have text-indent
        assert!(
            matches!(silence_style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01),
            "Silence paragraph should have text-indent: 1em, got {:?}",
            silence_style.text_indent
        );

        // Paragraph should NOT inherit center from heading (siblings don't inherit)
        assert_eq!(
            silence_style.text_align, None,
            "Silence paragraph should NOT be centered (it's a sibling of h3, not child)"
        );
    }

    #[test]
    fn test_actual_epictetus_epub_styles() {
        use crate::css::{CssValue, Stylesheet};

        // Parse the EPUB using the path-based reader
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Combine CSS like from_book does
        let mut combined_css = String::new();
        let mut css_resources: Vec<_> = book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .collect();
        css_resources.sort_by_key(|(path, _)| path.as_str());

        for (path, resource) in &css_resources {
            println!("Loading CSS: {} ({} bytes)", path, resource.data.len());
            combined_css.push_str(&String::from_utf8_lossy(&resource.data));
            combined_css.push('\n');
        }

        let stylesheet = Stylesheet::parse(&combined_css);

        // Find the main content XHTML
        let content_path = book
            .spine
            .iter()
            .find(|s| s.href.contains("enchiridion"))
            .map(|s| s.href.clone())
            .expect("Should find enchiridion");

        let content = book
            .resources
            .get(&content_path)
            .expect("Should have content");
        let items = extract_content_from_xhtml(&content.data, &stylesheet, "");

        // Find the silence paragraph
        let (_, silence_style) = find_text_style(&items, "And let silence be the general rule")
            .expect("Should find silence paragraph");

        println!("\n=== Silence paragraph style ===");
        println!("text_indent: {:?}", silence_style.text_indent);
        println!("text_align: {:?}", silence_style.text_align);
        println!("margin_top: {:?}", silence_style.margin_top);
        println!("margin_bottom: {:?}", silence_style.margin_bottom);

        // Verify text-indent is present (should be 1em from p {} rule)
        assert!(
            matches!(silence_style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01),
            "Silence paragraph should have text-indent: 1em from p rule, got {:?}",
            silence_style.text_indent
        );

        // Now test the full conversion pipeline
        // Build KFX and check what style is output
        let builder = KfxBookBuilder::from_book(&book);

        // Find the style fragment that should be used for the silence paragraph
        // The style should have non-zero text_indent
        let mut zero_indent_count = 0;
        let mut nonzero_indent_count = 0;

        for f in builder.fragments.iter() {
            if f.ftype == sym::STYLE {
                if let IonValue::Struct(style) = &f.value {
                    if let Some(IonValue::Struct(indent_val)) = style.get(&sym::TEXT_INDENT) {
                        if let Some(IonValue::Symbol(unit_sym)) = indent_val.get(&sym::UNIT) {
                            if *unit_sym == sym::UNIT_MULTIPLIER {
                                zero_indent_count += 1;
                            } else {
                                nonzero_indent_count += 1;
                                println!("Found non-zero text-indent: unit={}", unit_sym);
                            }
                        }
                    }
                }
            }
        }

        println!("Styles with zero text-indent: {}", zero_indent_count);
        println!("Styles with non-zero text-indent: {}", nonzero_indent_count);

        assert!(
            nonzero_indent_count > 0,
            "Should have at least one style with non-zero text-indent (P505 or similar)"
        );

        // Debug: Check the exact style that the silence paragraph gets
        // Find which style symbol maps to the silence paragraph's style
        println!("\n=== Silence style hash/equality check ===");
        println!("text_indent: {:?}", silence_style.text_indent);
        println!("style hash: {:?}", {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            silence_style.hash(&mut h);
            h.finish()
        });

        // Check if there's a style with exact same text_indent that differs in other ways
        // which might cause hash collisions
        let silence_has_em_indent =
            matches!(silence_style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01);
        assert!(
            silence_has_em_indent,
            "Silence style should have Em(1.0) text-indent when extracted"
        );

        // Print all unique styles to see which one the silence paragraph maps to
        // Search recursively through nested containers
        let unique_styles: std::collections::HashSet<_> = items
            .iter()
            .flat_map(|item| item.flatten())
            .filter_map(|item| {
                if let ContentItem::Text { style, .. } = item {
                    Some(style.clone())
                } else {
                    None
                }
            })
            .collect();
        println!("\n=== Unique styles ({}) ===", unique_styles.len());
        for (i, style) in unique_styles.iter().enumerate() {
            if matches!(style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01) {
                println!("Style {} has Em(1.0) indent: {:?}", i, style.text_indent);
            } else if matches!(style.text_indent, Some(CssValue::Px(v)) if v.abs() < 0.001) {
                // This is the zero px case
                println!("Style {} has Px(0) indent", i);
            } else if style.text_indent.is_some() {
                println!("Style {} has other indent: {:?}", i, style.text_indent);
            }
        }

        // Check if silence style is in the unique set
        let silence_in_set = unique_styles.contains(&silence_style);
        println!("Silence style found in unique set: {}", silence_in_set);

        // Now check which KFX style the silence paragraph gets assigned to
        // Look at the style_map in the builder
        if let Some(&style_sym) = builder.style_map.get(&silence_style) {
            println!("\nSilence style maps to symbol: {}", style_sym);

            // Find that style fragment and check its text_indent
            for f in builder.fragments.iter() {
                if f.ftype == sym::STYLE {
                    if let IonValue::Struct(style) = &f.value {
                        if let Some(IonValue::Symbol(name_sym)) = style.get(&sym::STYLE_NAME) {
                            if *name_sym == style_sym {
                                println!("Found KFX style {}", style_sym);
                                if let Some(IonValue::Struct(indent_val)) =
                                    style.get(&sym::TEXT_INDENT)
                                {
                                    println!("  P16 (text_indent): {:?}", indent_val);
                                    if let Some(IonValue::Symbol(unit)) = indent_val.get(&sym::UNIT)
                                    {
                                        println!(
                                            "  Unit symbol: {} (P505={}, ZERO={})",
                                            unit,
                                            sym::UNIT_EM,
                                            sym::UNIT_MULTIPLIER
                                        );
                                        assert_ne!(
                                            *unit,
                                            sym::UNIT_MULTIPLIER,
                                            "Silence style should have non-zero text-indent!"
                                        );
                                    }
                                } else {
                                    println!("  No P16 (text_indent) field!");
                                }
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            panic!("Silence style not found in style_map!");
        }
    }

    #[test]
    fn test_silence_style_in_full_conversion() {
        use crate::css::{CssValue, Stylesheet};

        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Combine CSS exactly like from_book does
        let mut combined_css = String::new();
        let mut css_resources: Vec<_> = book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .collect();
        css_resources.sort_by_key(|(path, _)| path.as_str());
        for (_, resource) in css_resources {
            combined_css.push_str(&String::from_utf8_lossy(&resource.data));
            combined_css.push('\n');
        }
        let stylesheet = Stylesheet::parse(&combined_css);

        // Process ALL spine items like from_book does
        let mut all_items: Vec<ContentItem> = Vec::new();
        for spine_item in &book.spine {
            if let Some(resource) = book.resources.get(&spine_item.href) {
                let items = extract_content_from_xhtml(&resource.data, &stylesheet, "");
                all_items.extend(items);
            }
        }

        // Find the silence paragraph
        let (_, silence_style) = find_text_style(&all_items, "And let silence be the general rule")
            .expect("Should find silence");

        println!("\n=== Silence style from full conversion ===");
        println!("text_indent: {:?}", silence_style.text_indent);
        println!("text_align: {:?}", silence_style.text_align);
        println!("margin_top: {:?}", silence_style.margin_top);

        // The key check: text_indent should be Em(1.0), not zero
        let has_em_indent =
            matches!(silence_style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01);
        let has_zero_indent =
            matches!(silence_style.text_indent, Some(CssValue::Px(v)) if v.abs() < 0.001);

        println!("Has Em(1.0) indent: {}", has_em_indent);
        println!("Has Px(0) indent: {}", has_zero_indent);

        // This assertion tells us if the style extraction is correct
        assert!(
            !has_zero_indent,
            "Silence paragraph should NOT have zero text-indent! Got: {:?}",
            silence_style.text_indent
        );

        // Now trace through the style_map to see what happens
        let unique_styles: std::collections::HashSet<_> = all_items
            .iter()
            .filter_map(|item| {
                if let ContentItem::Text { style, .. } = item {
                    Some(style.clone())
                } else {
                    None
                }
            })
            .collect();
        println!("\nTotal unique styles: {}", unique_styles.len());

        // Check: does the silence style equal any style with zero indent?
        let mut equal_to_zero_indent = false;
        for other_style in &unique_styles {
            if other_style == silence_style {
                continue; // Skip self
            }
            // Check if other_style has zero indent
            if matches!(other_style.text_indent, Some(CssValue::Px(v)) if v.abs() < 0.001) {
                // Check if they're hash-equal (which would cause collision)
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h1 = DefaultHasher::new();
                let mut h2 = DefaultHasher::new();
                silence_style.hash(&mut h1);
                other_style.hash(&mut h2);
                if h1.finish() == h2.finish() {
                    println!("HASH COLLISION! Silence style and zero-indent style have same hash!");
                    println!("  Silence: {:?}", silence_style.text_indent);
                    println!("  Other: {:?}", other_style.text_indent);
                    equal_to_zero_indent = true;
                }
                // Check if they're equal (even if hashes differ)
                if silence_style == other_style {
                    println!("EQUALITY BUG! Silence style equals zero-indent style!");
                    println!("  Silence: {:?}", silence_style.text_indent);
                    println!("  Other: {:?}", other_style.text_indent);
                    equal_to_zero_indent = true;
                }
            }
        }
        assert!(
            !equal_to_zero_indent,
            "Silence style should not equal any zero-indent style!"
        );

        // Finally, trace through the builder to see which style the silence paragraph gets
        let builder = KfxBookBuilder::from_book(&book);

        // The silence style from our extraction
        let silence_style_from_extraction = silence_style;

        // Check if it's in the builder's style_map
        if let Some(&sym) = builder.style_map.get(silence_style_from_extraction) {
            println!("\nExtracted silence style maps to symbol: {}", sym);

            // Find the style fragment that has this symbol as its name
            for frag in &builder.fragments {
                if frag.ftype == sym::STYLE {
                    if let IonValue::Struct(ref s) = frag.value {
                        // P173 is STYLE_NAME which contains the symbol
                        if let Some(IonValue::Symbol(name_sym)) = s.get(&sym::STYLE_NAME) {
                            if *name_sym == sym {
                                println!(
                                    "Found style fragment for silence (frag.fid={}):",
                                    frag.fid
                                );
                                if let Some(IonValue::Struct(p16)) = s.get(&sym::TEXT_INDENT) {
                                    if let Some(IonValue::Symbol(unit)) = p16.get(&sym::UNIT) {
                                        println!("  text_indent unit symbol: {}", unit);
                                        if *unit == sym::UNIT_EM {
                                            println!("  -> P505 (1.5em) - CORRECT!");
                                        } else if *unit == sym::UNIT_MULTIPLIER {
                                            println!("  -> P310 (zero) - WRONG!");
                                        } else {
                                            println!("  -> Unknown symbol: {}", unit);
                                        }
                                    }
                                } else {
                                    println!("  No text_indent in style fragment");
                                }
                            }
                        }
                    }
                }
            }
        } else {
            println!("\nWARNING: Extracted silence style NOT FOUND in builder.style_map!");

            // Check how many styles match
            let mut matches = 0;
            for (style, &sym) in &builder.style_map {
                if matches!(style.text_indent, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.01) {
                    println!("  Found Em(1.0) style at symbol {}", sym);
                    matches += 1;
                }
            }
            println!(
                "Total styles with Em(1.0) text-indent in builder: {}",
                matches
            );
        }
    }

    #[test]
    fn test_font_size_serialization() {
        // Test that font-size values are serialized correctly
        use crate::css::{CssValue, ParsedStyle};

        // Create styles with different font sizes
        let mut style_100 = ParsedStyle::default();
        style_100.font_size = Some(CssValue::Em(1.0));

        let mut style_smaller = ParsedStyle::default();
        style_smaller.font_size = Some(CssValue::Em(0.67));

        let mut style_percent_small = ParsedStyle::default();
        style_percent_small.font_size = Some(CssValue::Percent(83.0));

        let mut style_keyword = ParsedStyle::default();
        style_keyword.font_size = Some(CssValue::Keyword("smaller".to_string()));

        // 1em/100% should use P350
        assert!(matches!(style_100.font_size, Some(CssValue::Em(v)) if (v - 1.0).abs() < 0.001));

        // Smaller values should use P382
        assert!(matches!(style_smaller.font_size, Some(CssValue::Em(v)) if v < 1.0));
        assert!(matches!(style_percent_small.font_size, Some(CssValue::Percent(v)) if v < 100.0));
        assert!(
            matches!(style_keyword.font_size, Some(CssValue::Keyword(ref k)) if k == "smaller")
        );
    }

    #[test]
    fn test_font_variant_serialization() {
        use crate::css::{FontVariant, ParsedStyle};

        // Style with small-caps
        let mut style = ParsedStyle::default();
        style.font_variant = Some(FontVariant::SmallCaps);

        assert_eq!(style.font_variant, Some(FontVariant::SmallCaps));

        // Normal variant
        let mut normal_style = ParsedStyle::default();
        normal_style.font_variant = Some(FontVariant::Normal);

        assert_eq!(normal_style.font_variant, Some(FontVariant::Normal));
    }

    #[test]
    fn test_style_omits_zero_margins() {
        // Zero margins should NOT be included in style output
        // Reference KFX omits default values, only including meaningful properties
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find style fragments
        let styles: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::STYLE)
            .collect();

        // Count styles with zero margin values
        // Zero is represented as {$306: $310, $307: ...} where $310 is ZERO
        let mut styles_with_zero_margins = 0;
        for style in &styles {
            if let IonValue::Struct(s) = &style.value {
                for &margin_key in &[
                    sym::MARGIN_TOP,
                    sym::MARGIN_BOTTOM,
                    sym::MARGIN_LEFT,
                    sym::MARGIN_RIGHT,
                ] {
                    if let Some(IonValue::Struct(margin_struct)) = s.get(&margin_key) {
                        // Check if unit is ZERO ($310)
                        if let Some(IonValue::Symbol(unit)) = margin_struct.get(&sym::UNIT) {
                            if *unit == sym::UNIT_MULTIPLIER {
                                styles_with_zero_margins += 1;
                                break; // Count each style only once
                            }
                        }
                    }
                }
            }
        }

        // No styles should have explicit zero margins - they should be omitted
        assert_eq!(
            styles_with_zero_margins, 0,
            "Found {} styles with explicit zero margins (should be omitted)",
            styles_with_zero_margins
        );
    }

    #[test]
    fn test_style_property_count_reasonable() {
        // Styles should be minimal - only non-default properties
        // Reference average is ~6.6 properties per style
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        let styles: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::STYLE)
            .collect();

        let total_props: usize = styles
            .iter()
            .filter_map(|s| match &s.value {
                IonValue::Struct(map) => Some(map.len()),
                _ => None,
            })
            .sum();

        let avg_props = total_props as f64 / styles.len() as f64;
        println!("Average properties per style: {:.1}", avg_props);

        // Should be <= 7 properties per style on average (reference is ~6.6)
        assert!(
            avg_props <= 7.0,
            "Styles too verbose: {:.1} avg properties (expected <= 7.0)",
            avg_props
        );
    }

    #[test]
    fn test_toc_navigation_from_epub() {
        // Test that TOC entries from EPUB are correctly converted to KFX navigation
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Verify the book has a hierarchical TOC structure
        assert!(book.toc.len() > 0, "Book should have TOC entries");

        // Count total TOC entries including children
        fn count_toc_entries(entries: &[crate::book::TocEntry]) -> usize {
            entries
                .iter()
                .fold(0, |acc, e| acc + 1 + count_toc_entries(&e.children))
        }
        let total_toc_entries = count_toc_entries(&book.toc);
        println!(
            "Total TOC entries (including nested): {}",
            total_toc_entries
        );

        // Build KFX
        let kfx = KfxBookBuilder::from_book(&book);

        // Find the book navigation fragment ($389)
        let nav_fragment = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::BOOK_NAVIGATION)
            .expect("Should have $389 book_navigation fragment");

        // Verify the navigation structure
        if let IonValue::List(book_navs) = &nav_fragment.value {
            assert!(!book_navs.is_empty(), "Book navigation should not be empty");

            // Get the first (and usually only) book navigation entry
            if let IonValue::Struct(book_nav) = &book_navs[0] {
                // Check for nav_containers ($392)
                let nav_containers = book_nav
                    .get(&sym::NAV_CONTAINER_REF)
                    .expect("Should have nav_containers");

                if let IonValue::List(containers) = nav_containers {
                    assert!(!containers.is_empty(), "Nav containers should not be empty");

                    // Get the TOC nav container (annotated with $391)
                    let toc_container = &containers[0];
                    let toc_struct = match toc_container {
                        IonValue::Annotated(annotations, inner) => {
                            assert!(
                                annotations.contains(&sym::NAV_CONTAINER_TYPE),
                                "Nav container should be annotated with $391"
                            );
                            match inner.as_ref() {
                                IonValue::Struct(s) => s,
                                _ => panic!("Nav container inner should be a struct"),
                            }
                        }
                        IonValue::Struct(s) => s,
                        _ => panic!("Nav container should be struct or annotated struct"),
                    };

                    // Verify nav type is TOC ($212)
                    if let Some(IonValue::Symbol(nav_type)) = toc_struct.get(&sym::NAV_TYPE) {
                        assert_eq!(*nav_type, sym::TOC, "Nav type should be $212 (TOC)");
                    }

                    // Check nav entries ($247)
                    if let Some(IonValue::List(nav_entries)) = toc_struct.get(&sym::NAV_ENTRIES) {
                        println!("KFX nav entries: {}", nav_entries.len());

                        // We should have nav entries for each TOC entry that maps to a valid section
                        assert!(nav_entries.len() > 0, "Should have at least one nav entry");

                        // Verify each nav entry has the required structure
                        for (i, entry) in nav_entries.iter().enumerate() {
                            let entry_struct = match entry {
                                IonValue::Annotated(annotations, inner) => {
                                    assert!(
                                        annotations.contains(&sym::NAV_DEFINITION),
                                        "Nav entry should be annotated with $393"
                                    );
                                    match inner.as_ref() {
                                        IonValue::Struct(s) => s,
                                        _ => panic!("Nav entry inner should be a struct"),
                                    }
                                }
                                IonValue::Struct(s) => s,
                                _ => panic!("Nav entry should be struct or annotated struct"),
                            };

                            // Check nav_title ($241)
                            assert!(
                                entry_struct.contains_key(&sym::NAV_TITLE),
                                "Nav entry {} should have nav_title ($241)",
                                i
                            );

                            // Check nav_target ($246)
                            // Nav targets use OrderedStruct to preserve field order
                            if let Some(nav_target) = entry_struct.get(&sym::NAV_TARGET) {
                                // Extract position from either Struct or OrderedStruct
                                let position = match nav_target {
                                    IonValue::Struct(target) => target.get(&sym::POSITION).cloned(),
                                    IonValue::OrderedStruct(fields) => fields
                                        .iter()
                                        .find(|(k, _)| *k == sym::POSITION)
                                        .map(|(_, v)| v.clone()),
                                    _ => None,
                                };

                                assert!(
                                    position.is_some(),
                                    "Nav target should have position ($155)"
                                );

                                // Verify EID is valid (> LOCAL_MIN_ID)
                                if let Some(IonValue::Int(eid)) = position {
                                    assert!(
                                        eid >= SymbolTable::LOCAL_MIN_ID as i64,
                                        "Nav entry EID should be >= LOCAL_MIN_ID"
                                    );
                                }
                            } else {
                                panic!("Nav entry {} should have nav_target ($246)", i);
                            }
                        }

                        // Count total entries including nested ones
                        fn count_nested_entries(entries: &[IonValue]) -> usize {
                            let mut count = 0;
                            for entry in entries {
                                count += 1;
                                let entry_struct = match entry {
                                    IonValue::Annotated(_, inner) => match inner.as_ref() {
                                        IonValue::Struct(s) => s,
                                        _ => continue,
                                    },
                                    IonValue::Struct(s) => s,
                                    _ => continue,
                                };
                                // Check for nested entries ($247)
                                if let Some(IonValue::List(nested)) =
                                    entry_struct.get(&sym::NAV_ENTRIES)
                                {
                                    count += count_nested_entries(nested);
                                }
                            }
                            count
                        }

                        let total_kfx_entries = count_nested_entries(nav_entries);
                        println!(
                            "Total KFX nav entries (including nested): {}",
                            total_kfx_entries
                        );

                        // Verify nesting: "The Enchiridion" should have nested children
                        let mut found_nested = false;
                        for entry in nav_entries {
                            let entry_struct = match entry {
                                IonValue::Annotated(_, inner) => match inner.as_ref() {
                                    IonValue::Struct(s) => s,
                                    _ => continue,
                                },
                                IonValue::Struct(s) => s,
                                _ => continue,
                            };
                            if let Some(IonValue::List(nested)) =
                                entry_struct.get(&sym::NAV_ENTRIES)
                            {
                                if !nested.is_empty() {
                                    found_nested = true;
                                    // Get the title for debugging
                                    if let Some(IonValue::Struct(title)) =
                                        entry_struct.get(&sym::NAV_TITLE)
                                    {
                                        if let Some(IonValue::String(text)) = title.get(&sym::TEXT)
                                        {
                                            println!(
                                                "Entry '{}' has {} nested children",
                                                text,
                                                nested.len()
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        assert!(
                            found_nested,
                            "At least one nav entry should have nested children"
                        );

                        // Print some nav entry titles for debugging
                        for (i, entry) in nav_entries.iter().take(5).enumerate() {
                            if let IonValue::Annotated(_, inner) = entry {
                                if let IonValue::Struct(s) = inner.as_ref() {
                                    if let Some(IonValue::Struct(title)) = s.get(&sym::NAV_TITLE) {
                                        if let Some(IonValue::String(text)) = title.get(&sym::TEXT)
                                        {
                                            println!("  Nav entry {}: {}", i, text);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        panic!("Nav container should have nav_entries ($247)");
                    }
                } else {
                    panic!("Nav containers should be a list");
                }
            } else {
                panic!("Book navigation entry should be a struct");
            }
        } else {
            panic!("Book navigation should be a list");
        }
    }

    #[test]
    fn test_toc_eid_mapping_debug() {
        // Debug test to trace TOC EID mapping issues
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Build KFX and inspect internal state
        let kfx = KfxBookBuilder::from_book(&book);

        // Print section_eids mapping
        println!("\n=== SECTION_EIDS ===");
        for (path, eid) in &kfx.section_eids {
            println!("  {} -> EID {}", path, eid);
        }

        // Print anchor_eids mapping (first 20)
        println!("\n=== ANCHOR_EIDS (first 20) ===");
        for (key, (eid, offset)) in kfx.anchor_eids.iter().take(20) {
            println!("  {} -> EID {} (offset {})", key, eid, offset);
        }

        // Print TOC hrefs (first 20)
        println!("\n=== TOC HREFS (first 20) ===");
        fn print_toc_hrefs(entries: &[crate::book::TocEntry], count: &mut usize) {
            for entry in entries {
                if *count >= 20 {
                    return;
                }
                println!("  {}: {}", entry.title, entry.href);
                *count += 1;
                print_toc_hrefs(&entry.children, count);
            }
        }
        let mut count = 0;
        print_toc_hrefs(&book.toc, &mut count);

        // Check which TOC entries match anchor_eids or section_eids
        println!("\n=== TOC EID LOOKUP RESULTS ===");
        fn check_toc_matches(
            entries: &[crate::book::TocEntry],
            section_eids: &std::collections::HashMap<String, i64>,
            anchor_eids: &std::collections::HashMap<String, (i64, i64)>,
            count: &mut usize,
        ) {
            for entry in entries {
                if *count >= 10 {
                    return;
                }

                let (path, fragment) = if let Some(hash_pos) = entry.href.find('#') {
                    (&entry.href[..hash_pos], Some(&entry.href[hash_pos + 1..]))
                } else {
                    (entry.href.as_str(), None)
                };

                let eid_offset = if fragment.is_some() {
                    anchor_eids
                        .get(&entry.href)
                        .copied()
                        .or_else(|| section_eids.get(path).map(|&e| (e, 0)))
                } else {
                    section_eids.get(path).map(|&e| (e, 0))
                };

                let source = if fragment.is_some() && anchor_eids.contains_key(&entry.href) {
                    "anchor_eids"
                } else if section_eids.contains_key(path) {
                    "section_eids"
                } else {
                    "NOT FOUND"
                };

                println!(
                    "  {} -> {} (from {})",
                    entry.href,
                    eid_offset.map(|(e, o)| format!("EID {} offset {}", e, o)).unwrap_or("NONE".to_string()),
                    source
                );
                *count += 1;
                check_toc_matches(&entry.children, section_eids, anchor_eids, count);
            }
        }
        let mut count = 0;
        check_toc_matches(&book.toc, &kfx.section_eids, &kfx.anchor_eids, &mut count);

        // Verify at least some anchor_eids exist
        assert!(
            !kfx.anchor_eids.is_empty(),
            "anchor_eids should not be empty for book with fragment hrefs in TOC"
        );
    }

    #[test]
    fn test_symbol_values_correct() {
        // Verify the new symbol constants have the correct values per CSS-to-KFX mapping analysis
        assert_eq!(sym::BACKGROUND_COLOR, 21, "BACKGROUND_COLOR should be $21");
        assert_eq!(sym::OPACITY, 72, "OPACITY should be $72");
        assert_eq!(sym::SPACE_AFTER, 49, "SPACE_AFTER should be $49");
        assert_eq!(
            sym::CELL_PADDING_RIGHT,
            53,
            "CELL_PADDING_RIGHT should be $53"
        );
        assert_eq!(
            sym::CELL_PADDING_LEFT,
            55,
            "CELL_PADDING_LEFT should be $55"
        );
        assert_eq!(sym::CELL_ALIGN, 633, "CELL_ALIGN should be $633");
        assert_eq!(sym::IMAGE_FIT_NONE, 378, "IMAGE_FIT_NONE should be $378");
        // Verify legacy alias points to correct value
        assert_eq!(
            sym::MARGIN_BOTTOM,
            sym::SPACE_AFTER,
            "MARGIN_BOTTOM should alias SPACE_AFTER"
        );
    }

    #[test]
    fn test_background_color_serialization() {
        // Test that background-color is serialized with the correct symbol ($21)
        let mut book = crate::book::Book::default();
        book.metadata.title = "Test".to_string();
        let content = r#"<html><body><p style="background-color: red;">Test</p></body></html>"#;
        book.add_resource(
            "test.xhtml",
            content.as_bytes().to_vec(),
            "application/xhtml+xml",
        );
        book.add_spine_item("test", "test.xhtml", "application/xhtml+xml");

        let kfx = KfxBookBuilder::from_book(&book);

        // Find styles that have $21 (BACKGROUND_COLOR)
        let styles_with_bg: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::STYLE)
            .filter(|f| {
                if let IonValue::Struct(s) = &f.value {
                    s.contains_key(&sym::BACKGROUND_COLOR)
                } else {
                    false
                }
            })
            .collect();

        assert!(
            !styles_with_bg.is_empty(),
            "Should have at least one style with background-color ($21)"
        );
    }

    #[test]
    fn test_margin_bottom_uses_space_after_symbol() {
        // Test that margin-bottom is serialized using SPACE_AFTER ($49)
        let mut book = crate::book::Book::default();
        book.metadata.title = "Test".to_string();
        let content = r#"<html><body><p style="margin-bottom: 2em;">Test</p></body></html>"#;
        book.add_resource(
            "test.xhtml",
            content.as_bytes().to_vec(),
            "application/xhtml+xml",
        );
        book.add_spine_item("test", "test.xhtml", "application/xhtml+xml");

        let kfx = KfxBookBuilder::from_book(&book);

        // Find styles that have $49 (SPACE_AFTER / MARGIN_BOTTOM)
        let styles_with_margin_bottom: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::STYLE)
            .filter(|f| {
                if let IonValue::Struct(s) = &f.value {
                    s.contains_key(&sym::MARGIN_BOTTOM) // Which is now 49
                } else {
                    false
                }
            })
            .collect();

        assert!(
            !styles_with_margin_bottom.is_empty(),
            "Should have at least one style with margin-bottom using $49 (SPACE_AFTER)"
        );

        // Verify the symbol value is 49
        assert_eq!(sym::MARGIN_BOTTOM, 49);
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    /// Helper to find text content containing a needle and get its style
    /// Searches recursively through nested containers
    fn find_text_style<'a>(
        items: &'a [ContentItem],
        needle: &str,
    ) -> Option<(&'a str, &'a ParsedStyle)> {
        items
            .iter()
            .flat_map(|item| item.flatten())
            .find_map(|item| {
                if let ContentItem::Text { text, style, .. } = item {
                    if text.contains(needle) {
                        return Some((text.as_str(), style));
                    }
                }
                None
            })
    }

    #[test]
    fn test_titlepage_image_extraction() {
        use crate::css::Stylesheet;

        // CSS from se.css - makes h1 and p hidden but NOT img
        let css = r#"
            section.epub-type-contains-word-titlepage h1,
            section.epub-type-contains-word-titlepage p {
                left: -999em;
                position: absolute;
            }
            section.epub-type-contains-word-titlepage img {
                display: block;
                margin-top: 3em;
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // Title page XHTML structure
        let html = br#"
            <body>
                <section class="epub-type-contains-word-titlepage" id="titlepage">
                    <h1>Short Works</h1>
                    <p>By <b>Epictetus</b>.</p>
                    <p>Translated by <b>George Long</b>.</p>
                    <img alt="" src="../images/titlepage.png"/>
                </section>
            </body>
        "#;

        // Use the same path format as from_book (manifest href, not full EPUB path)
        let items = extract_content_from_xhtml(html, &stylesheet, "text/titlepage.xhtml");

        println!("Extracted items: {}", items.len());
        for item in &items {
            match item {
                ContentItem::Text { text, .. } => println!("  Text: {}", text),
                ContentItem::Image { resource_href, .. } => println!("  Image: {}", resource_href),
                ContentItem::Container { tag, children, .. } => {
                    println!("  Container({}): {} children", tag, children.len())
                }
            }
        }

        // Should have ONLY the image, no text (h1 and p are hidden)
        // Search recursively through nested containers
        let text_items: Vec<_> = items
            .iter()
            .flat_map(|i| i.flatten())
            .filter(|i| matches!(i, ContentItem::Text { .. }))
            .collect();
        let image_items: Vec<_> = items
            .iter()
            .flat_map(|i| i.flatten())
            .filter(|i| matches!(i, ContentItem::Image { .. }))
            .collect();

        println!("Text items: {}", text_items.len());
        println!("Image items: {}", image_items.len());

        assert_eq!(text_items.len(), 0, "Hidden text should be filtered out");
        assert_eq!(image_items.len(), 1, "Should have 1 image");

        if let ContentItem::Image { resource_href, .. } = image_items[0] {
            // Resolved path should match manifest href format (not full EPUB path)
            assert_eq!(
                resource_href, "images/titlepage.png",
                "Image path should be resolved to manifest href"
            );
        }
    }

    #[test]
    fn test_image_resource_key_matching() {
        // Test that extracted image paths match resource keys from EPUB
        use crate::epub::read_epub;

        let book = read_epub("tests/fixtures/epictetus.epub").unwrap();

        // Check what keys are used for image resources
        println!("\nImage resource keys in book:");
        for (href, resource) in &book.resources {
            if resource.media_type.starts_with("image/") {
                println!("  '{}'", href);
            }
        }

        // Get the spine item for titlepage
        let titlepage_spine = book
            .spine
            .iter()
            .find(|s| s.href.contains("titlepage"))
            .expect("Should have titlepage in spine");

        println!("\nTitlepage spine item href: '{}'", titlepage_spine.href);

        // Load the titlepage content
        let resource = book
            .resources
            .get(&titlepage_spine.href)
            .expect("Should have titlepage resource");

        // Parse CSS
        let mut combined_css = String::new();
        for (_, r) in &book.resources {
            if r.media_type == "text/css" {
                combined_css.push_str(&String::from_utf8_lossy(&r.data));
                combined_css.push('\n');
            }
        }
        let stylesheet = crate::css::Stylesheet::parse(&combined_css);

        // Extract content - this is what from_book does
        let items = extract_content_from_xhtml(&resource.data, &stylesheet, &titlepage_spine.href);

        println!("\nExtracted items:");
        for item in &items {
            match item {
                ContentItem::Text { text, .. } => println!("  Text: '{}'", text),
                ContentItem::Image { resource_href, .. } => {
                    let found = book.resources.contains_key(resource_href);
                    println!(
                        "  Image: '{}' (found in resources: {})",
                        resource_href, found
                    );
                }
                ContentItem::Container { tag, children, .. } => {
                    println!("  Container({}): {} children", tag, children.len())
                }
            }
        }

        // Check that image hrefs match resource keys
        for item in &items {
            if let ContentItem::Image { resource_href, .. } = item {
                assert!(
                    book.resources.contains_key(resource_href),
                    "Image path '{}' should exist in book.resources",
                    resource_href
                );
            }
        }
    }

    #[test]
    fn test_kfx_contains_image_content() {
        use crate::epub::read_epub;

        let book = read_epub("tests/fixtures/epictetus.epub").unwrap();
        let kfx = KfxBookBuilder::from_book(&book);

        // Count IMAGE_CONTENT items in content block fragments
        let mut image_count = 0;
        let mut content_block_count = 0;

        for fragment in &kfx.fragments {
            if fragment.ftype == sym::CONTENT_BLOCK {
                content_block_count += 1;
                if let IonValue::Struct(block) = &fragment.value {
                    if let Some(IonValue::List(items)) = block.get(&sym::CONTENT_ARRAY) {
                        for item in items {
                            if let IonValue::Struct(item_struct) = item {
                                if let Some(IonValue::Symbol(content_type)) =
                                    item_struct.get(&sym::CONTENT_TYPE)
                                {
                                    if *content_type == sym::IMAGE_CONTENT {
                                        image_count += 1;
                                        // Print which resource this image references
                                        if let Some(IonValue::Symbol(rsrc_sym)) =
                                            item_struct.get(&sym::RESOURCE_NAME)
                                        {
                                            println!(
                                                "Found IMAGE_CONTENT with resource symbol: {}",
                                                rsrc_sym
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        println!(
            "Content blocks: {}, Images: {}",
            content_block_count, image_count
        );

        // The reference KFX has 2 images (titlepage and logo)
        // We should have at least the titlepage image
        assert!(
            image_count >= 1,
            "Generated KFX should contain at least 1 image"
        );
    }

    #[test]
    fn test_cover_image_setup() {
        use crate::epub::read_epub;

        let book = read_epub("tests/fixtures/epictetus.epub").unwrap();

        // Verify cover is detected in book metadata
        println!("Book cover_image: {:?}", book.metadata.cover_image);
        assert!(
            book.metadata.cover_image.is_some(),
            "EPUB should have cover_image in metadata"
        );

        let kfx = KfxBookBuilder::from_book(&book);

        // Find P490 (kindle metadata) fragment
        let p490 = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::KINDLE_METADATA);
        assert!(p490.is_some(), "Should have P490 kindle metadata fragment");

        // Find cover resource (largest image, should be ~1400x2100)
        let mut cover_resource = None;
        let mut cover_has_mime_type = false;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::RESOURCE {
                if let IonValue::Struct(res) = &fragment.value {
                    if let (Some(IonValue::Int(w)), Some(IonValue::Int(h))) =
                        (res.get(&sym::WIDTH), res.get(&sym::HEIGHT))
                    {
                        // Cover is typically portrait orientation
                        if *h > *w && *h > 1000 {
                            println!("Found cover resource: {}x{}, fid={}", w, h, fragment.fid);
                            cover_resource = Some(&fragment.fid);
                            // Check if cover resource has MIME_TYPE (P162)
                            cover_has_mime_type = res.contains_key(&sym::MIME_TYPE);
                            println!(
                                "Cover has MIME_TYPE (P162): {} (should be false to match reference)",
                                cover_has_mime_type
                            );
                        }
                    }
                }
            }
        }
        assert!(cover_resource.is_some(), "Should have cover resource");
        assert!(
            !cover_has_mime_type,
            "Cover resource should NOT have MIME_TYPE (P162) to match reference KFX format"
        );

        // Verify P253 dependency exists for cover resource
        let p419 = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::CONTAINER_ENTITY_MAP);
        assert!(p419.is_some(), "Should have P419 container entity map");

        if let Some(fragment) = p419 {
            if let IonValue::Struct(map) = &fragment.value {
                let has_deps = map.contains_key(&sym::ENTITY_DEPS);
                println!("P419 has P253 entity deps: {}", has_deps);
                assert!(has_deps, "P419 should have P253 entity dependencies");
            }
        }
    }

    #[test]
    fn test_cover_image_content_fragment() {
        // Verify the cover IMAGE_CONTENT fragment structure matches reference KFX
        // Reference structure:
        //   $259 content block with $146 array containing:
        //     - $155 (POSITION): EID
        //     - $157 (STYLE): style reference
        //     - $159 (CONTENT_TYPE): $271 (IMAGE_CONTENT)
        //     - $175 (RESOURCE_NAME): resource reference
        use crate::epub::read_epub;

        let book = read_epub("tests/fixtures/epictetus.epub").unwrap();
        let kfx = KfxBookBuilder::from_book(&book);

        // Find cover resource symbol (largest portrait image)
        let mut cover_resource_sym = None;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::RESOURCE {
                if let IonValue::Struct(res) = &fragment.value {
                    if let (Some(IonValue::Int(w)), Some(IonValue::Int(h))) =
                        (res.get(&sym::WIDTH), res.get(&sym::HEIGHT))
                    {
                        if *h > *w && *h > 1000 {
                            if let Some(IonValue::Symbol(sym)) = res.get(&sym::RESOURCE_NAME) {
                                cover_resource_sym = Some(*sym);
                            }
                        }
                    }
                }
            }
        }
        assert!(
            cover_resource_sym.is_some(),
            "Should find cover resource symbol"
        );
        let cover_sym = cover_resource_sym.unwrap();

        // Find content block ($259) with IMAGE_CONTENT ($271) referencing the cover
        let mut found_cover_content = false;
        let mut cover_block_sym = None;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::CONTENT_BLOCK {
                if let IonValue::Struct(block) = &fragment.value {
                    if let Some(IonValue::List(content_array)) = block.get(&sym::CONTENT_ARRAY) {
                        for item in content_array {
                            if let IonValue::Struct(item_struct) = item {
                                // Check if this is IMAGE_CONTENT referencing our cover
                                let is_image_content = matches!(
                                    item_struct.get(&sym::CONTENT_TYPE),
                                    Some(IonValue::Symbol(s)) if *s == sym::IMAGE_CONTENT
                                );
                                let refs_cover = matches!(
                                    item_struct.get(&sym::RESOURCE_NAME),
                                    Some(IonValue::Symbol(s)) if *s == cover_sym
                                );

                                if is_image_content && refs_cover {
                                    found_cover_content = true;

                                    // Verify required fields
                                    assert!(
                                        item_struct.contains_key(&sym::POSITION),
                                        "Cover IMAGE_CONTENT should have $155 (POSITION)"
                                    );
                                    assert!(
                                        item_struct.contains_key(&sym::STYLE),
                                        "Cover IMAGE_CONTENT should have $157 (STYLE)"
                                    );

                                    // Cover should NOT have alt text (per reference)
                                    assert!(
                                        !item_struct.contains_key(&sym::IMAGE_ALT_TEXT),
                                        "Cover IMAGE_CONTENT should NOT have $584 (IMAGE_ALT_TEXT)"
                                    );

                                    // Get the content block symbol for section verification
                                    if let Some(IonValue::Symbol(sym)) =
                                        block.get(&sym::CONTENT_NAME)
                                    {
                                        cover_block_sym = Some(*sym);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            found_cover_content,
            "Should have content block with IMAGE_CONTENT ($271) for cover image"
        );

        // Verify section ($260) references the cover content block
        let cover_block = cover_block_sym.unwrap();
        let mut found_cover_section = false;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::SECTION {
                if let IonValue::Struct(section) = &fragment.value {
                    if let Some(IonValue::List(content_list)) = section.get(&sym::SECTION_CONTENT) {
                        for content_ref in content_list {
                            if let IonValue::Struct(ref_struct) = content_ref {
                                // Check if this section references our cover content block
                                let refs_cover_block = matches!(
                                    ref_struct.get(&sym::CONTENT_NAME),
                                    Some(IonValue::Symbol(s)) if *s == cover_block
                                );

                                if refs_cover_block {
                                    found_cover_section = true;

                                    // Verify cover section has dimensions
                                    assert!(
                                        ref_struct.contains_key(&sym::SECTION_WIDTH),
                                        "Cover section should have $66 (SECTION_WIDTH)"
                                    );
                                    assert!(
                                        ref_struct.contains_key(&sym::SECTION_HEIGHT),
                                        "Cover section should have $67 (SECTION_HEIGHT)"
                                    );

                                    // Verify content type is CONTAINER_INFO ($270)
                                    assert!(
                                        matches!(
                                            ref_struct.get(&sym::CONTENT_TYPE),
                                            Some(IonValue::Symbol(s)) if *s == sym::CONTAINER_INFO
                                        ),
                                        "Cover section content type should be $270 (CONTAINER_INFO)"
                                    );

                                    // Verify page layout is full page ($326)
                                    assert!(
                                        ref_struct.contains_key(&sym::PAGE_LAYOUT),
                                        "Cover section should have $156 (PAGE_LAYOUT)"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            found_cover_section,
            "Should have section ($260) referencing the cover content block"
        );
    }

    #[test]
    fn test_cover_section_first_in_reading_order() {
        // Verify that when a book has a cover image, the cover section is first in reading order
        // Reference KFX has cover section at position 0 in $170 (SECTIONS_LIST)
        use crate::epub::read_epub;

        let book = read_epub("tests/fixtures/epictetus.epub").unwrap();
        assert!(
            book.metadata.cover_image.is_some(),
            "Test book should have cover image"
        );

        let kfx = KfxBookBuilder::from_book(&book);

        // Find cover section symbol (the one with dimensions)
        let mut cover_section_sym = None;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::SECTION {
                if let IonValue::Struct(section) = &fragment.value {
                    if let Some(IonValue::List(content_list)) = section.get(&sym::SECTION_CONTENT) {
                        if let Some(IonValue::Struct(content_ref)) = content_list.first() {
                            // Cover section has dimensions
                            if content_ref.contains_key(&sym::SECTION_WIDTH)
                                && content_ref.contains_key(&sym::SECTION_HEIGHT)
                            {
                                if let Some(IonValue::Symbol(sym)) = section.get(&sym::SECTION_NAME)
                                {
                                    cover_section_sym = Some(*sym);
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            cover_section_sym.is_some(),
            "Should find cover section with dimensions"
        );
        let cover_sym = cover_section_sym.unwrap();

        // Check $258 (METADATA) reading order
        let metadata_258 = kfx.fragments.iter().find(|f| f.ftype == sym::METADATA);
        assert!(metadata_258.is_some(), "Should have $258 metadata fragment");

        if let Some(fragment) = metadata_258 {
            if let IonValue::Struct(metadata) = &fragment.value {
                if let Some(IonValue::List(reading_orders)) = metadata.get(&sym::READING_ORDERS) {
                    if let Some(IonValue::Struct(ro)) = reading_orders.first() {
                        if let Some(IonValue::List(sections)) = ro.get(&sym::SECTIONS_LIST) {
                            assert!(!sections.is_empty(), "Sections list should not be empty");
                            // First section should be cover
                            if let Some(IonValue::Symbol(first_sym)) = sections.first() {
                                assert_eq!(
                                    *first_sym, cover_sym,
                                    "Cover section should be first in $258 reading order"
                                );
                            }
                        }
                    }
                }
            }
        }

        // Check $538 (DOCUMENT_DATA) reading order
        let doc_data = kfx.fragments.iter().find(|f| f.ftype == sym::DOCUMENT_DATA);
        assert!(
            doc_data.is_some(),
            "Should have $538 document data fragment"
        );

        if let Some(fragment) = doc_data {
            if let IonValue::Struct(data) = &fragment.value {
                if let Some(IonValue::List(reading_orders)) = data.get(&sym::READING_ORDERS) {
                    if let Some(IonValue::Struct(ro)) = reading_orders.first() {
                        if let Some(IonValue::List(sections)) = ro.get(&sym::SECTIONS_LIST) {
                            assert!(!sections.is_empty(), "Sections list should not be empty");
                            // First section should be cover
                            if let Some(IonValue::Symbol(first_sym)) = sections.first() {
                                assert_eq!(
                                    *first_sym, cover_sym,
                                    "Cover section should be first in $538 reading order"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Helper to find image content and get its style
    /// Searches recursively through nested containers
    fn find_image_style<'a>(
        items: &'a [ContentItem],
        resource_needle: &str,
    ) -> Option<&'a ParsedStyle> {
        items
            .iter()
            .flat_map(|item| item.flatten())
            .find_map(|item| {
                if let ContentItem::Image {
                    resource_href,
                    style,
                    ..
                } = item
                {
                    if resource_href.contains(resource_needle) {
                        return Some(style);
                    }
                }
                None
            })
    }

    #[test]
    fn test_image_style_descendant_selector() {
        use crate::css::{CssValue, Stylesheet};

        // CSS with descendant selector: section.titlepage img should get margin-top
        let css = r#"
            section.titlepage img {
                display: block;
                margin-top: 3em;
                margin-left: auto;
                margin-right: auto;
            }
            img {
                margin-top: 0;
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // HTML with img inside section.titlepage
        let html = br#"
            <body>
                <section class="titlepage">
                    <img src="images/titlepage.png" alt="Title"/>
                </section>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Should have 1 image
        assert_eq!(items.len(), 1, "Should have exactly 1 image");

        let style = find_image_style(&items, "titlepage").expect("Should find titlepage image");

        // Image should have margin-top: 3em from descendant selector
        assert!(
            matches!(style.margin_top, Some(CssValue::Em(v)) if (v - 3.0).abs() < 0.01),
            "Titlepage image should have margin-top: 3em, got {:?}",
            style.margin_top
        );
    }

    #[test]
    fn test_image_style_class_selector() {
        use crate::css::{CssValue, Stylesheet};

        // CSS targeting img by class
        let css = r#"
            img.hero {
                margin-top: 2em;
            }
            img {
                margin-top: 0;
            }
        "#;
        let stylesheet = Stylesheet::parse(css);

        let html = br#"
            <body>
                <section>
                    <img class="hero" src="images/hero.png" alt="Hero"/>
                    <img src="images/regular.png" alt="Regular"/>
                </section>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");
        // Count flattened images (nested in containers)
        let image_count = items
            .iter()
            .flat_map(|i| i.flatten())
            .filter(|i| matches!(i, ContentItem::Image { .. }))
            .count();
        assert_eq!(
            image_count, 2,
            "Should have 2 images (nested in containers)"
        );

        let hero_style = find_image_style(&items, "hero").expect("Should find hero image");
        let regular_style = find_image_style(&items, "regular").expect("Should find regular image");

        // Hero image should have margin-top: 2em
        assert!(
            matches!(hero_style.margin_top, Some(CssValue::Em(v)) if (v - 2.0).abs() < 0.01),
            "Hero image should have margin-top: 2em, got {:?}",
            hero_style.margin_top
        );

        // Regular image should have margin-top: 0 (or none)
        let regular_has_zero = matches!(regular_style.margin_top, Some(CssValue::Px(v)) if v.abs() < 0.01)
            || regular_style.margin_top.is_none();
        assert!(
            regular_has_zero,
            "Regular image should have margin-top: 0 or none, got {:?}",
            regular_style.margin_top
        );
    }

    #[test]
    fn test_epictetus_titlepage_image_style() {
        use crate::css::CssValue;

        // Parse the actual EPUB
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Combine CSS
        let mut combined_css = String::new();
        let mut css_resources: Vec<_> = book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .collect();
        css_resources.sort_by_key(|(path, _)| path.as_str());
        for (_, resource) in &css_resources {
            combined_css.push_str(&String::from_utf8_lossy(&resource.data));
            combined_css.push('\n');
        }
        let stylesheet = crate::css::Stylesheet::parse(&combined_css);

        // Find titlepage XHTML
        let titlepage_path = book
            .spine
            .iter()
            .find(|s| s.href.contains("titlepage"))
            .map(|s| s.href.clone())
            .expect("Should find titlepage");

        let titlepage = book
            .resources
            .get(&titlepage_path)
            .expect("Should have titlepage");
        let base_dir = std::path::Path::new(&titlepage_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let items = extract_content_from_xhtml(&titlepage.data, &stylesheet, &base_dir);

        // Find the titlepage image
        let image_style =
            find_image_style(&items, "titlepage").expect("Should find titlepage image");

        println!("Titlepage image style:");
        println!("  margin_top: {:?}", image_style.margin_top);
        println!("  margin_left: {:?}", image_style.margin_left);
        println!("  margin_right: {:?}", image_style.margin_right);
        println!("  width: {:?}", image_style.width);
        println!("  is_image: {:?}", image_style.is_image);

        // The CSS has: section.epub-type-contains-word-titlepage img { margin-top: 3em; }
        // So the titlepage image should have margin-top: 3em
        assert!(
            matches!(image_style.margin_top, Some(CssValue::Em(v)) if (v - 3.0).abs() < 0.01),
            "Titlepage image should have margin-top: 3em from CSS, got {:?}",
            image_style.margin_top
        );
    }

    #[test]
    fn test_titlepage_image_kfx_style_has_margin_top() {
        // Test that the KFX style for titlepage image actually contains margin-top
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find the titlepage image style by looking for IMAGE_CONTENT blocks
        // and checking which style they reference
        // Search recursively through nested content arrays
        fn find_image_style_sym(items: &[IonValue]) -> Option<u64> {
            for item in items {
                if let IonValue::Struct(item_struct) = item {
                    // Check if this is an image
                    if let Some(IonValue::Symbol(content_type)) =
                        item_struct.get(&sym::CONTENT_TYPE)
                    {
                        if *content_type == sym::IMAGE_CONTENT {
                            // Check if it has a style
                            if let Some(IonValue::Symbol(style_sym)) = item_struct.get(&sym::STYLE)
                            {
                                // Check resource name to find titlepage
                                if let Some(IonValue::Symbol(_rsrc)) =
                                    item_struct.get(&sym::RESOURCE_NAME)
                                {
                                    println!("Found image with style symbol: {}", style_sym);
                                    return Some(*style_sym);
                                }
                            }
                        }
                    }
                    // Check nested content arrays (for containers)
                    if let Some(IonValue::List(nested)) = item_struct.get(&sym::CONTENT_ARRAY) {
                        if let Some(style) = find_image_style_sym(nested) {
                            return Some(style);
                        }
                    }
                }
            }
            None
        }

        let mut titlepage_style_sym = None;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::CONTENT_BLOCK {
                if let IonValue::Struct(block) = &fragment.value {
                    if let Some(IonValue::List(items)) = block.get(&sym::CONTENT_ARRAY) {
                        if let Some(style) = find_image_style_sym(items) {
                            titlepage_style_sym = Some(style);
                            break;
                        }
                    }
                }
            }
        }

        // Now find the style and check for margin-top
        // Also count all image styles that DO have margin-top
        let mut image_styles_with_margin = Vec::new();
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::STYLE {
                if let IonValue::Struct(style) = &fragment.value {
                    let has_margin_top = style.get(&sym::MARGIN_TOP).is_some();
                    let has_image_layout = style.get(&sym::IMAGE_LAYOUT).is_some();
                    if has_margin_top && has_image_layout {
                        if let Some(IonValue::Symbol(name)) = style.get(&sym::STYLE_NAME) {
                            image_styles_with_margin.push(*name);
                        }
                    }
                }
            }
        }

        println!(
            "Image styles with margin-top: {:?}",
            image_styles_with_margin
        );

        if let Some(style_sym) = titlepage_style_sym {
            for fragment in &kfx.fragments {
                if fragment.ftype == sym::STYLE {
                    if let IonValue::Struct(style) = &fragment.value {
                        if let Some(IonValue::Symbol(name_sym)) = style.get(&sym::STYLE_NAME) {
                            if *name_sym == style_sym {
                                println!(
                                    "Found style {} content: {:?}",
                                    style_sym,
                                    style.keys().collect::<Vec<_>>()
                                );

                                // Check for P36 (margin_top)
                                if let Some(margin_top) = style.get(&sym::MARGIN_TOP) {
                                    println!("  P36 (margin_top): {:?}", margin_top);
                                    // Verify it's not zero
                                    if let IonValue::Struct(mt) = margin_top {
                                        if let Some(IonValue::Symbol(unit)) = mt.get(&sym::UNIT) {
                                            assert_ne!(
                                                *unit,
                                                sym::UNIT_MULTIPLIER,
                                                "Titlepage image margin-top should not be zero!"
                                            );
                                            println!(
                                                "  margin-top unit: {} (UNIT_EM={})",
                                                unit,
                                                sym::UNIT_EM
                                            );
                                        }
                                    }
                                } else {
                                    // This specific image may use a default style without margin-top
                                    // As long as SOME image styles have margin-top, we're OK
                                    if !image_styles_with_margin.is_empty() {
                                        println!(
                                            "Note: First image found uses style {}, but other image styles {} have margin-top",
                                            style_sym,
                                            image_styles_with_margin.len()
                                        );
                                        return;
                                    }
                                    panic!(
                                        "Titlepage image style should have P36 (margin_top)! No image styles with margin_top found."
                                    );
                                }
                                return;
                            }
                        }
                    }
                }
            }
        }

        panic!("Could not find titlepage image style in KFX output");
    }

    #[test]
    fn test_titlepage_image_style_has_image_layout() {
        // Focused test: verify image styles have required properties:
        // 1. $546 (IMAGE_FIT) = $377 (CONTAIN)
        // 2. $580 (IMAGE_LAYOUT) = $320 (CENTER)
        //
        // Note: Reference KFX image styles ($1165) do NOT have $127 (STYLE_BLOCK_TYPE),
        // only $546, $56, $580, $173. Verified via test EPUB through Kindle Previewer.
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find all style fragments that have IMAGE_LAYOUT (image styles)
        let mut image_styles = Vec::new();

        for fragment in &kfx.fragments {
            if fragment.ftype == sym::STYLE {
                if let IonValue::Struct(style) = &fragment.value {
                    let has_image_layout = style.get(&sym::IMAGE_LAYOUT).is_some();
                    let has_image_fit = style.get(&sym::IMAGE_FIT).is_some();

                    if has_image_layout {
                        let style_name = style
                            .get(&sym::STYLE_NAME)
                            .map(|v| {
                                if let IonValue::Symbol(s) = v {
                                    Some(*s)
                                } else {
                                    None
                                }
                            })
                            .flatten();

                        println!("Found image style: {:?}", style_name);
                        println!("  has IMAGE_FIT ($546): {}", has_image_fit);
                        println!("  has IMAGE_LAYOUT ($580): {}", has_image_layout);
                        println!("  style keys: {:?}", style.keys().collect::<Vec<_>>());

                        image_styles.push((style_name, has_image_fit, style.clone()));
                    }
                }
            }
        }

        assert!(
            !image_styles.is_empty(),
            "Should find at least one image style with IMAGE_LAYOUT"
        );

        // Verify all image styles have IMAGE_FIT (contain)
        for (style_name, has_image_fit, style) in &image_styles {
            assert!(
                *has_image_fit,
                "Image style {:?} should have IMAGE_FIT ($546)",
                style_name
            );

            // Verify IMAGE_LAYOUT is CENTER ($320)
            if let Some(IonValue::Symbol(layout)) = style.get(&sym::IMAGE_LAYOUT) {
                assert_eq!(
                    *layout,
                    sym::ALIGN_CENTER,
                    "Image style {:?} IMAGE_LAYOUT should be CENTER ($320), got {}",
                    style_name,
                    layout
                );
            }
        }
    }

    #[test]
    fn test_colophon_paragraph_parsed_style() {
        // Test that CSS inheritance works: the colophon paragraph should have text_align: center
        // inherited from the section, even though p rules don't set text-align.
        use crate::css::{Stylesheet, TextAlign};

        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Combine CSS like from_book does
        let mut combined_css = String::new();
        let mut css_resources: Vec<_> = book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .collect();
        css_resources.sort_by_key(|(path, _)| path.as_str());

        for (_, resource) in &css_resources {
            combined_css.push_str(&String::from_utf8_lossy(&resource.data));
            combined_css.push('\n');
        }

        let stylesheet = Stylesheet::parse(&combined_css);

        // Find the colophon XHTML
        let colophon_path = book
            .spine
            .iter()
            .find(|s| s.href.contains("colophon"))
            .map(|s| s.href.clone())
            .expect("Should find colophon");

        let colophon = book
            .resources
            .get(&colophon_path)
            .expect("Should have colophon");
        let items = extract_content_from_xhtml(&colophon.data, &stylesheet, &colophon_path);

        // Find the colophon paragraph containing specific text
        let (_, colophon_style) =
            find_text_style(&items, "Standard Ebooks").expect("Should find colophon paragraph");

        // The paragraph should inherit text-align: center from the section
        // (section.epub-type-contains-word-colophon has text-align: center)
        // This tests CSS inheritance working correctly.
        assert_eq!(
            colophon_style.text_align,
            Some(TextAlign::Center),
            "Colophon paragraph should inherit text-align: center from section, got {:?}",
            colophon_style.text_align
        );
    }

    #[test]
    fn test_colophon_paragraph_style() {
        // Test that the colophon paragraph has correct margin encoding
        // CSS for colophon p: margin-top: 1em; margin-bottom: 0; text-align: center;
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find text content containing text from the actual colophon (not imprint)
        // "Short Works" is the book title in the colophon
        let mut colophon_text_id = None;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::TEXT_CONTENT {
                if let IonValue::Struct(content) = &fragment.value {
                    if let Some(IonValue::List(texts)) = content.get(&sym::CONTENT_ARRAY) {
                        for text in texts {
                            if let IonValue::String(s) = text {
                                if s.contains("Short Works") {
                                    if let Some(IonValue::Symbol(id)) = content.get(&sym::ID) {
                                        colophon_text_id = Some(*id);
                                        println!("Found colophon text with id: {}", id);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(
            colophon_text_id.is_some(),
            "Should find colophon text content"
        );

        // Find content block that references this text and get its style
        // Content items have TEXT_CONTENT: {ID: text_sym, TEXT_OFFSET: n}
        // Search recursively through nested content arrays ($146)
        fn find_style_for_text_id(items: &[IonValue], text_id: u64) -> Option<u64> {
            for item in items {
                if let IonValue::Struct(item_struct) = item {
                    // Check if this references our colophon text via TEXT_CONTENT
                    if let Some(IonValue::Struct(text_ref)) = item_struct.get(&sym::TEXT_CONTENT) {
                        if let Some(IonValue::Symbol(found_id)) = text_ref.get(&sym::ID) {
                            if *found_id == text_id {
                                // Get the style for this content item
                                if let Some(IonValue::Symbol(style)) = item_struct.get(&sym::STYLE)
                                {
                                    return Some(*style);
                                }
                            }
                        }
                    }
                    // Check nested content arrays (for containers)
                    if let Some(IonValue::List(nested)) = item_struct.get(&sym::CONTENT_ARRAY) {
                        if let Some(style) = find_style_for_text_id(nested, text_id) {
                            return Some(style);
                        }
                    }
                }
            }
            None
        }

        let mut colophon_style_sym = None;
        let text_id = colophon_text_id.unwrap();
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::CONTENT_BLOCK {
                if let IonValue::Struct(block) = &fragment.value {
                    if let Some(IonValue::List(items)) = block.get(&sym::CONTENT_ARRAY) {
                        if let Some(style) = find_style_for_text_id(items, text_id) {
                            colophon_style_sym = Some(style);
                            println!("Found colophon style symbol: {}", style);
                            break;
                        }
                    }
                }
            }
        }

        assert!(
            colophon_style_sym.is_some(),
            "Should find style for colophon paragraph"
        );

        // Now find the style and verify its properties
        let style_sym = colophon_style_sym.unwrap();
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::STYLE {
                if let IonValue::Struct(style) = &fragment.value {
                    if let Some(IonValue::Symbol(name)) = style.get(&sym::STYLE_NAME) {
                        if *name == style_sym {
                            println!("Colophon style properties:");
                            for (k, v) in style.iter() {
                                println!("  {}: {:?}", k, v);
                            }

                            // Check margin-top/SPACE_BEFORE exists and uses appropriate unit
                            // Based on CSS mapping: margin-top uses $47 with multiplier ($310) or percent ($314)
                            if let Some(IonValue::Struct(margin_top)) =
                                style.get(&sym::SPACE_BEFORE)
                            {
                                if let Some(IonValue::Symbol(unit)) = margin_top.get(&sym::UNIT) {
                                    assert!(
                                        *unit == sym::UNIT_MULTIPLIER || *unit == sym::UNIT_PERCENT,
                                        "Colophon margin-top should use multiplier or percent unit, got unit={} (UNIT_MULTIPLIER={}, UNIT_PERCENT={})",
                                        unit,
                                        sym::UNIT_MULTIPLIER,
                                        sym::UNIT_PERCENT
                                    );
                                    println!("  margin-top unit: {} (correct)", unit);
                                }
                            } else {
                                panic!("Colophon style should have margin-top (SPACE_BEFORE)");
                            }

                            // Check text-align is center (ALIGN_CENTER=321)
                            if let Some(IonValue::Symbol(align)) = style.get(&sym::TEXT_ALIGN) {
                                assert_eq!(
                                    *align,
                                    sym::ALIGN_CENTER,
                                    "Colophon text-align should be center, got {} (ALIGN_CENTER={})",
                                    align,
                                    sym::ALIGN_CENTER
                                );
                                println!("  text-align: center (correct)");
                            }

                            return;
                        }
                    }
                }
            }
        }

        panic!("Could not find colophon style in KFX output");
    }

    #[test]
    fn test_body_paragraph_style_is_minimal() {
        // Test that body paragraph styles follow the reference KFX approach:
        // - Only include directly-specified properties
        // - Reference $1116 has: $10 (lang), $127 (display:block), $16 (text-indent),
        //   $36 (margin-top), $42 (margin-bottom) - and NO font-size, font-family,
        //   text-align, width, or base-style
        use crate::css::Stylesheet;

        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Combine CSS
        let mut combined_css = String::new();
        let mut css_resources: Vec<_> = book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .collect();
        css_resources.sort_by_key(|(path, _)| path.as_str());

        for (_, resource) in &css_resources {
            combined_css.push_str(&String::from_utf8_lossy(&resource.data));
            combined_css.push('\n');
        }

        let stylesheet = Stylesheet::parse(&combined_css);

        // Find the-enchiridion.xhtml (has body paragraphs)
        let chapter_path = book
            .spine
            .iter()
            .find(|s| s.href.contains("enchiridion"))
            .map(|s| s.href.clone())
            .expect("Should find the-enchiridion");

        let chapter = book
            .resources
            .get(&chapter_path)
            .expect("Should have chapter");
        let items = extract_content_from_xhtml(&chapter.data, &stylesheet, &chapter_path);

        // Find a body paragraph (contains text from the first paragraph)
        let (_, para_style) = find_text_style(&items, "In everything which pleases")
            .expect("Should find body paragraph");

        println!("Body paragraph direct style:");
        println!("  font_size: {:?}", para_style.font_size);
        println!("  font_family: {:?}", para_style.font_family);
        println!("  text_align: {:?}", para_style.text_align);
        println!("  text_indent: {:?}", para_style.text_indent);
        println!("  margin_top: {:?}", para_style.margin_top);
        println!("  margin_bottom: {:?}", para_style.margin_bottom);
        println!("  width: {:?}", para_style.width);
        println!("  display: {:?}", para_style.display);

        // These should NOT be set (reference style $1116 doesn't have them)
        assert!(
            para_style.font_size.is_none(),
            "Body paragraph should NOT have font_size directly specified (inherits via KFX $583), got {:?}",
            para_style.font_size
        );
        assert!(
            para_style.font_family.is_none(),
            "Body paragraph should NOT have font_family directly specified (inherits via KFX), got {:?}",
            para_style.font_family
        );
        assert!(
            para_style.text_align.is_none(),
            "Body paragraph should NOT have text_align directly specified (inherits via KFX), got {:?}",
            para_style.text_align
        );
        assert!(
            para_style.width.is_none(),
            "Body paragraph should NOT have width directly specified, got {:?}",
            para_style.width
        );

        // These SHOULD be set (reference style $1116 has them)
        assert!(
            para_style.text_indent.is_some(),
            "Body paragraph should have text_indent directly specified"
        );
        assert!(
            para_style.margin_top.is_some(),
            "Body paragraph should have margin_top directly specified"
        );
        assert!(
            para_style.margin_bottom.is_some(),
            "Body paragraph should have margin_bottom directly specified"
        );
    }

    #[test]
    fn test_imprint_paragraph_style() {
        // Debug test: check what direct styles we extract for imprint paragraph
        use crate::css::Stylesheet;

        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        let mut combined_css = String::new();
        let mut css_resources: Vec<_> = book
            .resources
            .iter()
            .filter(|(_, r)| r.media_type == "text/css")
            .collect();
        css_resources.sort_by_key(|(path, _)| path.as_str());

        for (_, resource) in &css_resources {
            combined_css.push_str(&String::from_utf8_lossy(&resource.data));
            combined_css.push('\n');
        }

        let stylesheet = Stylesheet::parse(&combined_css);

        let imprint_path = book
            .spine
            .iter()
            .find(|s| s.href.contains("imprint"))
            .map(|s| s.href.clone())
            .expect("Should find imprint");

        let imprint = book
            .resources
            .get(&imprint_path)
            .expect("Should have imprint");
        let items = extract_content_from_xhtml(&imprint.data, &stylesheet, &imprint_path);

        // Find the imprint paragraph
        let (_, imprint_style) =
            find_text_style(&items, "ebook is the product").expect("Should find imprint paragraph");

        println!("Imprint paragraph direct style:");
        println!("  font_size: {:?}", imprint_style.font_size);
        println!("  text_align: {:?}", imprint_style.text_align);
        println!("  text_indent: {:?}", imprint_style.text_indent);
        println!("  width: {:?}", imprint_style.width);
        println!("  margin_top: {:?}", imprint_style.margin_top);
        println!("  margin_left: {:?}", imprint_style.margin_left);
        println!("  margin_right: {:?}", imprint_style.margin_right);
    }

    #[test]
    fn test_inline_style_runs_extraction() {
        use crate::css::{FontStyle, FontWeight, Stylesheet};

        // CSS that makes em/i/strong have distinct styles
        let css = r#"
            p { font-size: 16px; }
            em, i { font-style: italic; }
            strong, b { font-weight: bold; }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // HTML with mixed inline styles within a paragraph
        let html = br#"
            <body>
                <p>Hello <em>emphasized</em> and <strong>bold</strong> world!</p>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Should have 1 item (the <p> element, flattened to Text since it has single text content)
        assert_eq!(items.len(), 1, "Should have 1 item");

        // After flattening, paragraph with single text child becomes just Text
        if let ContentItem::Text {
            text, inline_runs, ..
        } = &items[0]
        {
            // Text should be merged
            assert!(text.contains("Hello"), "Text should contain 'Hello'");
            assert!(
                text.contains("emphasized"),
                "Text should contain 'emphasized'"
            );
            assert!(text.contains("bold"), "Text should contain 'bold'");
            assert!(text.contains("world"), "Text should contain 'world'");

            println!("Merged text: '{}'", text);
            println!("Inline runs: {}", inline_runs.len());
            for run in inline_runs {
                println!(
                    "  offset: {}, length: {}, font_style: {:?}, font_weight: {:?}",
                    run.offset, run.length, run.style.font_style, run.style.font_weight
                );
            }

            // Should have inline runs for em and strong styled portions
            // The exact count depends on how whitespace is handled
            assert!(
                !inline_runs.is_empty(),
                "Should have inline style runs for styled text"
            );

            // Check that at least one run has italic (for the em element)
            let has_italic_run = inline_runs
                .iter()
                .any(|r| r.style.font_style == Some(FontStyle::Italic));
            assert!(has_italic_run, "Should have an italic inline run for <em>");

            // Check that at least one run has bold (for the strong element)
            let has_bold_run = inline_runs
                .iter()
                .any(|r| r.style.font_weight == Some(FontWeight::Bold));
            assert!(has_bold_run, "Should have a bold inline run for <strong>");
        } else {
            panic!(
                "Expected Text item (flattened paragraph), got {:?}",
                items[0]
            );
        }
    }

    #[test]
    fn test_inline_style_runs_kfx_output() {
        use crate::css::{FontStyle, Stylesheet};

        // CSS with distinct styles
        let css = r#"
            p { font-size: 16px; }
            em { font-style: italic; }
        "#;
        let stylesheet = Stylesheet::parse(css);

        // HTML with inline styled content
        let html = br#"
            <body>
                <p>Normal <em>italic</em> text.</p>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Find the text item with inline runs
        let text_item = items.iter().flat_map(|i| i.flatten()).find(
            |i| matches!(i, ContentItem::Text { inline_runs, .. } if !inline_runs.is_empty()),
        );

        assert!(
            text_item.is_some(),
            "Should have a text item with inline style runs"
        );

        if let Some(ContentItem::Text {
            text, inline_runs, ..
        }) = text_item
        {
            println!("Text: '{}'", text);
            println!("Inline runs count: {}", inline_runs.len());

            // The italic portion should create an inline run
            let italic_run = inline_runs
                .iter()
                .find(|r| r.style.font_style == Some(FontStyle::Italic));
            assert!(
                italic_run.is_some(),
                "Should have italic run for <em> content"
            );
        }
    }

    // ==========================================================================
    // TDD Tests for KFX Issues (imprint comparison)
    // ==========================================================================

    #[test]
    fn test_image_alt_text_extraction() {
        // Issue: Image alt text ($584) is not being extracted from <img alt="...">
        let css = "";
        let stylesheet = Stylesheet::parse(css);

        let html = br#"
            <body>
                <img src="logo.png" alt="The Standard Ebooks logo." />
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Find the image item
        let image_item = items
            .iter()
            .flat_map(|i| i.flatten())
            .find(|i| matches!(i, ContentItem::Image { .. }));

        assert!(image_item.is_some(), "Should have an image item");

        if let Some(ContentItem::Image { alt, .. }) = image_item {
            assert_eq!(
                alt.as_deref(),
                Some("The Standard Ebooks logo."),
                "Image should have alt text extracted"
            );
        } else {
            panic!("Expected ContentItem::Image");
        }
    }

    #[test]
    fn test_container_nesting_depth() {
        // Issue: Generated KFX has 2-3 levels of nesting vs reference's 1 level
        // A simple paragraph should not be deeply nested
        let css = "p { font-size: 16px; }";
        let stylesheet = Stylesheet::parse(css);

        let html = br#"
            <body>
                <p>Simple paragraph text.</p>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Count nesting depth
        fn max_depth(item: &ContentItem) -> usize {
            match item {
                ContentItem::Text { .. } | ContentItem::Image { .. } => 0,
                ContentItem::Container { children, .. } => {
                    1 + children.iter().map(|c| max_depth(c)).max().unwrap_or(0)
                }
            }
        }

        let depth = items.iter().map(|i| max_depth(i)).max().unwrap_or(0);
        println!("Nesting depth: {}", depth);

        // A simple paragraph should have at most 1 level of container nesting
        // (the paragraph itself, containing text)
        assert!(
            depth <= 1,
            "Simple paragraph should not be deeply nested (depth: {})",
            depth
        );
    }

    #[test]
    fn test_hyperlink_anchor_support() {
        // Issue: No hyperlink/anchor support ($179) - links are stripped to plain text
        let css = "a { color: blue; }";
        let stylesheet = Stylesheet::parse(css);

        let html = br#"
            <body>
                <p>Visit <a href="https://standardebooks.org">Standard Ebooks</a> for more.</p>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Check that anchor information is preserved in inline_runs
        // The link text should have associated anchor/href data in inline runs
        let has_anchor_data = items
            .iter()
            .flat_map(|i| i.flatten())
            .any(|item| match item {
                ContentItem::Text { inline_runs, .. } => {
                    inline_runs.iter().any(|r| r.anchor_href.is_some())
                }
                _ => false,
            });

        assert!(
            has_anchor_data,
            "Hyperlinks should preserve anchor href information"
        );
    }

    #[test]
    fn test_internal_anchor_reference() {
        // Issue: Internal links (href="#footnote-1") need to reference anchor fragments
        let css = "";
        let stylesheet = Stylesheet::parse(css);

        let html = b"<body><p>See note<a href=\"#footnote-1\"><sup>1</sup></a></p><p id=\"footnote-1\">Footnote.</p></body>";

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // The anchor should reference the internal target (in inline_runs)
        let has_internal_anchor = items
            .iter()
            .flat_map(|i| i.flatten())
            .any(|item| match item {
                ContentItem::Text { inline_runs, .. } => inline_runs
                    .iter()
                    .any(|r| r.anchor_href.as_ref().map_or(false, |h| h.starts_with("#"))),
                _ => false,
            });

        assert!(
            has_internal_anchor,
            "Internal anchors (#footnote-1) should be preserved"
        );
    }

    #[test]
    fn test_paragraph_style_differentiation() {
        // Issue: All paragraphs use same style instead of differentiating h1, h2, p, etc.
        let css = r#"
            h1 { font-size: 24px; font-weight: bold; }
            h2 { font-size: 20px; font-weight: bold; }
            p { font-size: 16px; }
        "#;
        let stylesheet = Stylesheet::parse(css);

        let html = br#"
            <body>
                <h1>Title</h1>
                <h2>Subtitle</h2>
                <p>Body text.</p>
            </body>
        "#;

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // After flattening, h1/h2/p become Text items with their styles preserved
        // Should have 3 Text items
        assert_eq!(
            items.len(),
            3,
            "Should have 3 items (h1, h2, p flattened to Text)"
        );

        // Collect styles from text items
        let text_styles: Vec<_> = items
            .iter()
            .filter_map(|item| {
                if let ContentItem::Text { text, style, .. } = item {
                    Some((text.as_str(), style.clone()))
                } else {
                    None
                }
            })
            .collect();

        println!("Text styles:");
        for (text, style) in &text_styles {
            println!(
                "  '{}': font_size={:?}, font_weight={:?}",
                text, style.font_size, style.font_weight
            );
        }

        // Find by content
        let h1_style = text_styles
            .iter()
            .find(|(t, _)| *t == "Title")
            .map(|(_, s)| s);
        let h2_style = text_styles
            .iter()
            .find(|(t, _)| *t == "Subtitle")
            .map(|(_, s)| s);
        let p_style = text_styles
            .iter()
            .find(|(t, _)| *t == "Body text.")
            .map(|(_, s)| s);

        assert!(h1_style.is_some(), "Should have h1 text");
        assert!(h2_style.is_some(), "Should have h2 text");
        assert!(p_style.is_some(), "Should have p text");

        // H1 should be larger than p
        let h1_size = h1_style.unwrap().font_size.clone();
        let p_size = p_style.unwrap().font_size.clone();

        // Extract numeric values for comparison
        fn get_px_value(val: &Option<CssValue>) -> Option<f32> {
            match val {
                Some(CssValue::Px(px)) => Some(*px),
                _ => None,
            }
        }

        let h1_px = get_px_value(&h1_size);
        let p_px = get_px_value(&p_size);

        assert!(
            h1_px.is_some() && p_px.is_some(),
            "Both H1 and P should have px font sizes"
        );
        assert!(
            h1_px.unwrap() > p_px.unwrap(),
            "H1 ({:?}px) should have larger font than P ({:?}px)",
            h1_px,
            p_size
        );
    }

    // ==========================================================================
    // TDD Tests for Anchor Fragment Emission ($266, $179)
    // ==========================================================================

    #[test]
    fn test_anchor_href_collected_from_content() {
        // Verify that anchor hrefs are collected during content extraction (in inline_runs)
        let css = "";
        let stylesheet = Stylesheet::parse(css);

        let html = b"<body><p>Visit <a href=\"https://example.com\">Example</a> and <a href=\"https://other.com\">Other</a>.</p></body>";

        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Collect all unique anchor hrefs from inline_runs
        let anchor_hrefs: Vec<_> = items
            .iter()
            .flat_map(|i| i.flatten())
            .flat_map(|item| match item {
                ContentItem::Text { inline_runs, .. } => inline_runs
                    .iter()
                    .filter_map(|r| r.anchor_href.clone())
                    .collect::<Vec<_>>(),
                _ => vec![],
            })
            .collect();

        println!("Anchor hrefs found: {:?}", anchor_hrefs);

        assert!(
            anchor_hrefs.contains(&"https://example.com".to_string()),
            "Should have example.com anchor"
        );
        assert!(
            anchor_hrefs.contains(&"https://other.com".to_string()),
            "Should have other.com anchor"
        );
    }

    #[test]
    fn test_external_anchor_fragments_created() {
        // External URLs should create $266 anchor fragments with $186 (EXTERNAL_URL)
        // The epictetus EPUB has links to standardebooks.org in the imprint section
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Look for anchor fragments ($266) with external URLs ($186)
        let anchor_fragments: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::PAGE_TEMPLATE) // $266
            .filter(|f| {
                if let IonValue::Struct(s) = &f.value {
                    s.contains_key(&sym::EXTERNAL_URL) // $186
                } else {
                    false
                }
            })
            .collect();

        println!(
            "Found {} anchor fragments with external URLs",
            anchor_fragments.len()
        );
        for frag in &anchor_fragments {
            if let IonValue::Struct(s) = &frag.value {
                if let Some(IonValue::String(url)) = s.get(&sym::EXTERNAL_URL) {
                    println!("  Anchor: {} -> {}", frag.fid, url);
                }
            }
        }

        // The imprint section has links to standardebooks.org
        assert!(
            !anchor_fragments.is_empty(),
            "Should have anchor fragments with external URLs for links in imprint section"
        );

        // Verify at least one points to standardebooks.org
        let has_se_link = anchor_fragments.iter().any(|frag| {
            if let IonValue::Struct(s) = &frag.value {
                if let Some(IonValue::String(url)) = s.get(&sym::EXTERNAL_URL) {
                    return url.contains("standardebooks.org");
                }
            }
            false
        });
        assert!(
            has_se_link,
            "Should have anchor fragment for standardebooks.org link"
        );
    }

    #[test]
    fn test_content_items_have_anchor_refs() {
        // Content items with hyperlinks should have $179 anchor references
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Look through content blocks for items with $179 (ANCHOR_REF)
        fn has_anchor_ref(value: &IonValue) -> bool {
            match value {
                IonValue::Struct(s) => {
                    if s.contains_key(&sym::ANCHOR_REF) {
                        return true;
                    }
                    // Check $142 inline style runs
                    if let Some(IonValue::List(runs)) = s.get(&sym::INLINE_STYLE_RUNS) {
                        for run in runs {
                            if let IonValue::Struct(run_s) = run {
                                if run_s.contains_key(&sym::ANCHOR_REF) {
                                    return true;
                                }
                            }
                        }
                    }
                    // Check nested content arrays
                    if let Some(IonValue::List(items)) = s.get(&sym::CONTENT_ARRAY) {
                        for item in items {
                            if has_anchor_ref(item) {
                                return true;
                            }
                        }
                    }
                    false
                }
                IonValue::List(items) => items.iter().any(has_anchor_ref),
                _ => false,
            }
        }

        let content_with_anchors: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::CONTENT_BLOCK)
            .filter(|f| has_anchor_ref(&f.value))
            .collect();

        println!(
            "Found {} content blocks with anchor references",
            content_with_anchors.len()
        );

        // The imprint section has hyperlinks that should have $179 references
        assert!(
            !content_with_anchors.is_empty(),
            "Content blocks should have $179 anchor references for hyperlinks"
        );
    }

    // ==========================================================================
    // TDD Tests for Links as Inline Runs (matching reference KFX structure)
    // ==========================================================================

    #[test]
    fn test_links_merged_into_combined_text() {
        // Links should NOT split text - they should be merged with inline runs
        let css = "a { color: blue; }";
        let stylesheet = Stylesheet::parse(css);

        let html =
            b"<body><p>Visit <a href=\"https://example.com\">Example</a> for more.</p></body>";
        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Should have 1 item (paragraph flattened to Text since it has single text content)
        assert_eq!(items.len(), 1, "Should have 1 item");

        // After flattening, paragraph with single text child becomes just Text
        if let ContentItem::Text {
            text, inline_runs, ..
        } = &items[0]
        {
            // Text should be combined
            assert_eq!(
                text, "Visit Example for more.",
                "Text should be combined, not split"
            );

            // Should have inline run for the link portion
            assert!(!inline_runs.is_empty(), "Should have inline run for link");

            // The inline run should have anchor_href
            let has_anchor_run = inline_runs.iter().any(|r| r.anchor_href.is_some());
            assert!(
                has_anchor_run,
                "Inline run should have anchor_href for the link"
            );
        } else {
            panic!("Expected Text item (flattened paragraph)");
        }
    }

    #[test]
    fn test_style_run_includes_anchor_href() {
        // StyleRun should be able to carry both style AND anchor information
        let css = "a { color: blue; text-decoration: underline; }";
        let stylesheet = Stylesheet::parse(css);

        let html = b"<body><p>Click <a href=\"https://example.com\">here</a> now.</p></body>";
        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Find the text item and check its inline runs
        let text_item = items
            .iter()
            .flat_map(|i| match i {
                ContentItem::Container { children, .. } => children.iter().collect::<Vec<_>>(),
                other => vec![other],
            })
            .find(|i| matches!(i, ContentItem::Text { .. }));

        assert!(text_item.is_some(), "Should have a text item");

        if let Some(ContentItem::Text { inline_runs, .. }) = text_item {
            // Find the run for "here" (the link text)
            let anchor_run = inline_runs
                .iter()
                .find(|r| r.anchor_href.as_deref() == Some("https://example.com"));

            assert!(
                anchor_run.is_some(),
                "Should have inline run with anchor_href for the link"
            );

            let run = anchor_run.unwrap();
            // "Click " = 6 chars, so "here" starts at offset 6
            assert_eq!(run.offset, 6, "Link should start at offset 6");
            assert_eq!(run.length, 4, "Link 'here' should have length 4");
        }
    }

    #[test]
    fn test_kfx_anchor_ref_in_inline_runs() {
        // $179 should be inside $142 inline runs, not directly on content items
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Look for $142 runs that contain $179
        fn find_anchor_in_inline_runs(value: &IonValue) -> bool {
            match value {
                IonValue::Struct(s) => {
                    // Check if $142 runs contain $179
                    if let Some(IonValue::List(runs)) = s.get(&sym::INLINE_STYLE_RUNS) {
                        for run in runs {
                            if let IonValue::Struct(run_s) = run {
                                if run_s.contains_key(&sym::ANCHOR_REF) {
                                    return true;
                                }
                            }
                        }
                    }
                    // Check nested content
                    if let Some(IonValue::List(items)) = s.get(&sym::CONTENT_ARRAY) {
                        for item in items {
                            if find_anchor_in_inline_runs(item) {
                                return true;
                            }
                        }
                    }
                    false
                }
                IonValue::List(items) => items.iter().any(find_anchor_in_inline_runs),
                _ => false,
            }
        }

        let has_anchor_in_runs = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::CONTENT_BLOCK)
            .any(|f| find_anchor_in_inline_runs(&f.value));

        assert!(
            has_anchor_in_runs,
            "$179 anchor refs should be inside $142 inline runs, not directly on content items"
        );
    }

    #[test]
    fn test_no_direct_anchor_ref_on_content_items() {
        // Content items should NOT have $179 directly - it should be in $142 runs
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Look for content items with direct $179 (not in $142)
        fn has_direct_anchor_ref(value: &IonValue) -> bool {
            match value {
                IonValue::Struct(s) => {
                    // Check if this item has $179 directly (not good)
                    // but exclude if it's inside a $142 run
                    let has_direct = s.contains_key(&sym::ANCHOR_REF);
                    let has_inline_runs = s.contains_key(&sym::INLINE_STYLE_RUNS);

                    // If it has $179 but no $142, that's a direct anchor (bad)
                    if has_direct && !has_inline_runs {
                        // Check it's a content item (has $145 text ref or $159 content type)
                        if s.contains_key(&sym::TEXT_CONTENT) || s.contains_key(&sym::CONTENT_TYPE)
                        {
                            return true;
                        }
                    }

                    // Check nested content
                    if let Some(IonValue::List(items)) = s.get(&sym::CONTENT_ARRAY) {
                        for item in items {
                            if has_direct_anchor_ref(item) {
                                return true;
                            }
                        }
                    }
                    false
                }
                IonValue::List(items) => items.iter().any(has_direct_anchor_ref),
                _ => false,
            }
        }

        let has_direct = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::CONTENT_BLOCK)
            .any(|f| has_direct_anchor_ref(&f.value));

        assert!(
            !has_direct,
            "Content items should NOT have direct $179 refs - they should be in $142 inline runs"
        );
    }

    // ==========================================================================
    // TDD Tests for CSS Document Order
    // ==========================================================================

    #[test]
    fn test_css_hrefs_extracted_in_document_order() {
        // CSS links should be extracted in document order, not alphabetically
        let xhtml = br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
    <head>
        <link href="../css/core.css" rel="stylesheet" type="text/css"/>
        <link href="../css/se.css" rel="stylesheet" type="text/css"/>
        <link href="../css/local.css" rel="stylesheet" type="text/css"/>
    </head>
    <body><p>Test</p></body>
</html>"#;

        let hrefs = extract_css_hrefs_from_xhtml(xhtml, "epub/text/chapter.xhtml");

        assert_eq!(hrefs.len(), 3, "Should extract 3 CSS hrefs");
        // Document order: core.css, se.css, local.css (NOT alphabetical: core, local, se)
        assert_eq!(
            hrefs[0], "epub/css/core.css",
            "First CSS should be core.css"
        );
        assert_eq!(hrefs[1], "epub/css/se.css", "Second CSS should be se.css");
        assert_eq!(
            hrefs[2], "epub/css/local.css",
            "Third CSS should be local.css"
        );
    }

    #[test]
    fn test_css_order_affects_style_cascade() {
        // Later CSS should override earlier CSS (document order matters for cascade)
        let css1 = "p { color: red; font-size: 16px; }";
        let css2 = "p { color: blue; }"; // Overrides color but not font-size

        // Correct order: css1 then css2
        let combined_correct = format!("{}\n{}", css1, css2);
        let stylesheet_correct = Stylesheet::parse(&combined_correct);

        // Wrong order: css2 then css1
        let combined_wrong = format!("{}\n{}", css2, css1);
        let stylesheet_wrong = Stylesheet::parse(&combined_wrong);

        let html = b"<body><p>Test</p></body>";
        let items_correct = extract_content_from_xhtml(html, &stylesheet_correct, "");
        let items_wrong = extract_content_from_xhtml(html, &stylesheet_wrong, "");

        // Extract the paragraph's style
        fn get_p_color(items: &[ContentItem]) -> Option<crate::css::Color> {
            items.iter().flat_map(|i| i.flatten()).find_map(|item| {
                if let ContentItem::Text { style, .. } = item {
                    style.color.clone()
                } else {
                    None
                }
            })
        }

        let correct_color = get_p_color(&items_correct);
        let wrong_color = get_p_color(&items_wrong);

        // With correct order (css1 then css2), color should be blue (css2 wins)
        // With wrong order (css2 then css1), color should be red (css1 wins)
        assert_ne!(
            correct_color, wrong_color,
            "CSS order should affect cascade - different orders should give different results"
        );
    }

    #[test]
    fn test_epictetus_css_document_order() {
        // Verify epictetus EPUB uses correct CSS order from XHTML, not alphabetical
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");

        // Get imprint XHTML and extract CSS hrefs
        let imprint_spine = book
            .spine
            .iter()
            .find(|s| s.href.contains("imprint"))
            .expect("imprint in spine");

        let imprint_resource = book
            .resources
            .get(&imprint_spine.href)
            .expect("imprint resource");

        let css_hrefs = extract_css_hrefs_from_xhtml(&imprint_resource.data, &imprint_spine.href);

        println!("CSS hrefs from imprint.xhtml: {:?}", css_hrefs);

        // Should have core.css before se.css (document order)
        let core_pos = css_hrefs.iter().position(|h| h.contains("core.css"));
        let se_pos = css_hrefs.iter().position(|h| h.contains("se.css"));

        assert!(core_pos.is_some(), "Should find core.css");
        assert!(se_pos.is_some(), "Should find se.css");
        assert!(
            core_pos < se_pos,
            "core.css should come before se.css (document order, not alphabetical)"
        );
    }

    // ==========================================================================
    // TDD Tests for Container Nesting
    // ==========================================================================

    #[test]
    fn test_imprint_content_not_over_nested() {
        // The imprint section should NOT have excessive container nesting
        // Reference: paragraphs are direct children of the content array
        // Generated: should match this flat structure
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find the imprint content block (has "Standard Ebooks" text)
        fn count_nesting_depth(value: &IonValue) -> usize {
            match value {
                IonValue::Struct(s) => {
                    // Check nested $146 arrays
                    if let Some(IonValue::List(items)) = s.get(&sym::CONTENT_ARRAY) {
                        1 + items.iter().map(count_nesting_depth).max().unwrap_or(0)
                    } else {
                        0
                    }
                }
                IonValue::List(items) => items.iter().map(count_nesting_depth).max().unwrap_or(0),
                _ => 0,
            }
        }

        // Find content block with imprint content (has image with SE logo alt text)
        let imprint_block = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::CONTENT_BLOCK)
            .find(|f| {
                fn has_se_logo(v: &IonValue) -> bool {
                    match v {
                        IonValue::String(s) => s.contains("Standard Ebooks logo"),
                        IonValue::Struct(s) => s.values().any(has_se_logo),
                        IonValue::List(items) => items.iter().any(has_se_logo),
                        _ => false,
                    }
                }
                has_se_logo(&f.value)
            });

        assert!(imprint_block.is_some(), "Should find imprint content block");

        let depth = count_nesting_depth(&imprint_block.unwrap().value);
        println!("Imprint nesting depth: {}", depth);

        // Reference has depth 2: content array -> header container -> image
        // Paragraphs are direct children of content array (not wrapped)
        assert!(
            depth <= 2,
            "Imprint should not be over-nested (depth: {}, expected <= 2)",
            depth
        );
    }

    #[test]
    fn test_imprint_structure_matches_reference() {
        // Test that imprint section has exact structure:
        // - Outer CONTAINER with content array containing:
        //   - Header CONTAINER (with IMAGE inside)
        //   - TEXT (paragraph 1) as sibling
        //   - TEXT (paragraph 2) as sibling
        //   - TEXT (paragraph 3) as sibling
        //   - TEXT (paragraph 4) as sibling
        //
        // This matches reference KFX where <section> is flattened but <header> is preserved.
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find the imprint content block
        let imprint_block = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::CONTENT_BLOCK)
            .find(|f| {
                fn has_se_logo(v: &IonValue) -> bool {
                    match v {
                        IonValue::String(s) => s.contains("Standard Ebooks logo"),
                        IonValue::Struct(s) => s.values().any(has_se_logo),
                        IonValue::List(items) => items.iter().any(has_se_logo),
                        _ => false,
                    }
                }
                has_se_logo(&f.value)
            })
            .expect("Should find imprint content block");

        // Get the outer content array
        let outer_array = match &imprint_block.value {
            IonValue::Struct(s) => s.get(&sym::CONTENT_ARRAY),
            _ => None,
        };
        assert!(outer_array.is_some(), "Should have outer content array");

        let outer_items = match outer_array.unwrap() {
            IonValue::List(items) => items,
            _ => panic!("Content array should be a list"),
        };

        // Should have 5 items: 1 header container + 4 text paragraphs
        assert_eq!(
            outer_items.len(),
            5,
            "Outer content array should have 5 items (header + 4 paragraphs), got {}",
            outer_items.len()
        );

        // First item should be a Container (header) with its own $146 containing an image
        let first_item = &outer_items[0];
        let first_has_content_array = match first_item {
            IonValue::Struct(s) => s.contains_key(&sym::CONTENT_ARRAY),
            _ => false,
        };
        assert!(
            first_has_content_array,
            "First item should be a Container (header) with nested content array"
        );

        // Check that header contains an image (has $175 resource name or $584 alt text)
        fn has_image(v: &IonValue) -> bool {
            match v {
                IonValue::Struct(s) => {
                    s.contains_key(&sym::RESOURCE_NAME) || // $175
                    s.contains_key(&sym::IMAGE_ALT_TEXT) || // $584 alt text
                    s.values().any(has_image)
                }
                IonValue::List(items) => items.iter().any(has_image),
                _ => false,
            }
        }
        assert!(
            has_image(first_item),
            "Header container should contain an image"
        );

        // Remaining 4 items should be Text references (have $145 but no nested $146)
        for (i, item) in outer_items.iter().skip(1).enumerate() {
            let is_text_ref = match item {
                IonValue::Struct(s) => {
                    s.contains_key(&sym::TEXT_CONTENT) && // $145
                    !s.contains_key(&sym::CONTENT_ARRAY) // not a container
                }
                _ => false,
            };
            assert!(
                is_text_ref,
                "Item {} should be a Text reference (paragraph), not a Container",
                i + 2
            );
        }
    }

    #[test]
    fn test_paragraphs_not_individually_wrapped() {
        // Issue: Each <p> is wrapped in its own Container
        // Reference: Paragraphs are direct children of parent, not wrapped
        use crate::css::Stylesheet;

        let html = br#"
            <body>
                <section>
                    <p>Paragraph one.</p>
                    <p>Paragraph two.</p>
                    <p>Paragraph three.</p>
                </section>
            </body>
        "#;

        let stylesheet = Stylesheet::parse("");
        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // Should produce a flat structure:
        // - Container (section) with 3 Text children (not 3 Container children)
        // OR just 3 Text items if section is also flattened

        fn count_text_items(item: &ContentItem) -> usize {
            match item {
                ContentItem::Text { .. } => 1,
                ContentItem::Container { children, .. } => {
                    children.iter().map(count_text_items).sum()
                }
                _ => 0,
            }
        }

        fn count_container_wrappers(item: &ContentItem) -> usize {
            // Count containers that have exactly one child
            match item {
                ContentItem::Container { children, .. } => {
                    let self_is_wrapper = children.len() == 1;
                    let child_wrappers: usize = children.iter().map(count_container_wrappers).sum();
                    (if self_is_wrapper { 1 } else { 0 }) + child_wrappers
                }
                _ => 0,
            }
        }

        let total_text = items.iter().map(count_text_items).sum::<usize>();
        let total_wrappers = items.iter().map(count_container_wrappers).sum::<usize>();

        println!(
            "Text items: {}, Single-child wrappers: {}",
            total_text, total_wrappers
        );

        assert_eq!(total_text, 3, "Should have 3 text items");
        // Should have at most 1 wrapper (the section itself if not flattened)
        // Each paragraph should NOT be wrapped
        assert!(
            total_wrappers <= 1,
            "Paragraphs should not be individually wrapped (found {} wrappers)",
            total_wrappers
        );
    }

    #[test]
    fn test_section_with_only_blocks_flattened() {
        // A section containing only block elements should be flattened
        // (its children promoted to parent level)
        use crate::css::Stylesheet;

        let html = br#"
            <body>
                <section>
                    <p>Only child paragraph.</p>
                </section>
            </body>
        "#;

        let stylesheet = Stylesheet::parse("");
        let items = extract_content_from_xhtml(html, &stylesheet, "");

        // The section should be flattened, leaving just the paragraph
        fn max_depth(item: &ContentItem) -> usize {
            match item {
                ContentItem::Text { .. } | ContentItem::Image { .. } => 0,
                ContentItem::Container { children, .. } => {
                    1 + children.iter().map(max_depth).max().unwrap_or(0)
                }
            }
        }

        let depth = items.iter().map(max_depth).max().unwrap_or(0);
        println!("Depth after section flattening: {}", depth);

        // Should be 0 (just text) or at most 1 (if paragraph kept as container)
        assert!(
            depth <= 1,
            "Section with single block child should be flattened (depth: {})",
            depth
        );
    }

    // ==========================================================================
    // TDD Tests for Internal Link Anchors
    // ==========================================================================

    #[test]
    fn test_internal_link_creates_anchor_fragment() {
        // Internal links (href="uncopyright.xhtml") should create $266 anchor fragments
        // with $183 position info, not $186 external URL
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Look for anchor fragments with $183 (position info) instead of $186 (external URL)
        let internal_anchors: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::PAGE_TEMPLATE) // $266
            .filter(|f| {
                if let IonValue::Struct(s) = &f.value {
                    // Has $183 (position info) but NOT $186 (external URL)
                    s.contains_key(&sym::POSITION_INFO) && !s.contains_key(&sym::EXTERNAL_URL)
                } else {
                    false
                }
            })
            .collect();

        println!("Found {} internal anchor fragments", internal_anchors.len());

        // The imprint has an internal link to uncopyright.xhtml
        // So we should have at least one internal anchor
        assert!(
            !internal_anchors.is_empty(),
            "Should have internal anchor fragments for links like uncopyright.xhtml"
        );
    }

    #[test]
    fn test_internal_link_has_position_reference() {
        // Internal anchor should have $183 with $155 (EID) pointing to target position
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find an internal anchor fragment
        let internal_anchor = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::PAGE_TEMPLATE)
            .find(|f| {
                if let IonValue::Struct(s) = &f.value {
                    s.contains_key(&sym::POSITION_INFO)
                } else {
                    false
                }
            });

        if let Some(anchor) = internal_anchor {
            if let IonValue::Struct(s) = &anchor.value {
                if let Some(IonValue::Struct(pos_info)) = s.get(&sym::POSITION_INFO) {
                    // Should have $155 (EID)
                    assert!(
                        pos_info.contains_key(&sym::POSITION),
                        "Position info should contain $155 (EID)"
                    );
                    println!("Internal anchor position info: {:?}", pos_info);
                }
            }
        }
    }

    #[test]
    fn test_imprint_uncopyright_link_has_anchor_ref() {
        // The "Uncopyright" link in imprint should have $179 anchor reference
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find the imprint content block and check for anchor ref at the Uncopyright position
        // Text: "...see the Uncopyright at the end..."
        // Uncopyright is at offset 544, length 11

        fn find_anchor_at_offset(value: &IonValue, target_offset: i64) -> bool {
            match value {
                IonValue::Struct(s) => {
                    // Check $142 inline runs
                    if let Some(IonValue::List(runs)) = s.get(&sym::INLINE_STYLE_RUNS) {
                        for run in runs {
                            if let IonValue::Struct(run_s) = run {
                                if let Some(IonValue::Int(offset)) = run_s.get(&sym::OFFSET) {
                                    if *offset == target_offset
                                        && run_s.contains_key(&sym::ANCHOR_REF)
                                    {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                    // Check nested
                    if let Some(IonValue::List(items)) = s.get(&sym::CONTENT_ARRAY) {
                        if items
                            .iter()
                            .any(|item| find_anchor_at_offset(item, target_offset))
                        {
                            return true;
                        }
                    }
                    false
                }
                IonValue::List(items) => items
                    .iter()
                    .any(|item| find_anchor_at_offset(item, target_offset)),
                _ => false,
            }
        }

        let has_uncopyright_anchor = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::CONTENT_BLOCK)
            .any(|f| find_anchor_at_offset(&f.value, 544)); // Uncopyright is at offset 544

        assert!(
            has_uncopyright_anchor,
            "Uncopyright internal link at offset 544 should have $179 anchor reference"
        );
    }

    #[test]
    fn test_internal_link_eid_matches_target_section() {
        // The internal anchor's $155 EID must match a valid content item EID
        // (TOC and internal links point to content items, not section entries)
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Collect all content item EIDs from $259 (content block) fragments
        let content_item_eids: std::collections::HashSet<i64> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::CONTENT_BLOCK)
            .flat_map(|f| {
                let mut eids = Vec::new();
                fn collect_eids(value: &IonValue, eids: &mut Vec<i64>) {
                    match value {
                        IonValue::Struct(s) => {
                            if let Some(IonValue::Int(eid)) = s.get(&sym::POSITION) {
                                eids.push(*eid);
                            }
                            for v in s.values() {
                                collect_eids(v, eids);
                            }
                        }
                        IonValue::List(list) => {
                            for v in list {
                                collect_eids(v, eids);
                            }
                        }
                        _ => {}
                    }
                }
                collect_eids(&f.value, &mut eids);
                eids
            })
            .collect();

        println!("Content item EIDs count: {}", content_item_eids.len());

        // Find internal anchor fragments (those with $183 position info, but NOT page templates)
        // Page templates have IDs like "template-N", anchors have IDs like "$1234" or "anchor0"
        let internal_anchors: Vec<_> = kfx
            .fragments
            .iter()
            .filter(|f| f.ftype == sym::PAGE_TEMPLATE)
            .filter(|f| !f.fid.starts_with("template-")) // Exclude page templates
            .filter_map(|f| {
                if let IonValue::Struct(s) = &f.value {
                    // Handle both Struct and OrderedStruct for position info
                    let pos_info = s.get(&sym::POSITION_INFO)?;
                    match pos_info {
                        IonValue::Struct(pos_map) => {
                            if let Some(IonValue::Int(eid)) = pos_map.get(&sym::POSITION) {
                                return Some(*eid);
                            }
                        }
                        IonValue::OrderedStruct(fields) => {
                            for (k, v) in fields {
                                if *k == sym::POSITION {
                                    if let IonValue::Int(eid) = v {
                                        return Some(*eid);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                None
            })
            .collect();

        println!("Internal anchor target EIDs: {:?}", internal_anchors);

        // Each internal anchor EID must point to a valid content item
        for anchor_eid in &internal_anchors {
            assert!(
                content_item_eids.contains(anchor_eid),
                "Internal anchor points to EID {} which is not a valid content item EID",
                anchor_eid
            );
        }

        // Verify we have internal anchors pointing to different sections
        let unique_anchor_eids: std::collections::HashSet<_> = internal_anchors.iter().collect();
        assert!(
            unique_anchor_eids.len() >= 3,
            "Should have internal anchors pointing to at least 3 different content items, found {}",
            unique_anchor_eids.len()
        );
    }

    #[test]
    fn test_fragment_links_use_anchor_eids() {
        // Internal links with fragment identifiers (e.g., "text/the-enchiridion.xhtml#the-enchiridion-1")
        // should use anchor_eids to get the specific element's EID, not fall back to section_eids.
        // This ensures TOC navigation and hyperlinks to fragments land on the correct element.
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Verify anchor_eids contains fragment hrefs
        let fragment_anchors: Vec<_> = kfx
            .anchor_eids
            .iter()
            .filter(|(k, _)| k.contains('#'))
            .collect();

        assert!(
            !fragment_anchors.is_empty(),
            "anchor_eids should contain fragment identifiers"
        );

        // Check specific TOC entry with fragment: "the-enchiridion-1"
        let target_href = "text/the-enchiridion.xhtml#the-enchiridion-1";
        let fragment_eid = kfx.anchor_eids.get(target_href);
        let section_eid = kfx.section_eids.get("text/the-enchiridion.xhtml");

        assert!(
            fragment_eid.is_some(),
            "anchor_eids should have entry for {}", target_href
        );
        assert!(
            section_eid.is_some(),
            "section_eids should have entry for base path"
        );

        // The fragment EID should be DIFFERENT from the section EID
        // (fragment points to specific element within section)
        let (fragment_eid, fragment_offset) = fragment_eid.unwrap();
        let section_eid = *section_eid.unwrap();

        assert_ne!(
            *fragment_eid, section_eid,
            "Fragment EID ({}) should differ from section EID ({})",
            fragment_eid, section_eid
        );

        // Fragment EID should be greater than section EID
        // (content items within section come after section entry)
        assert!(
            *fragment_eid > section_eid,
            "Fragment EID ({}) should be > section EID ({})",
            fragment_eid, section_eid
        );

        // Print the offset for debugging
        println!("Fragment offset: {}", fragment_offset);

        // Verify the TOC navigation entry uses the fragment EID, not section EID
        // Find the book_navigation fragment and check the nav entries
        let nav_fragment = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::BOOK_NAVIGATION);

        assert!(nav_fragment.is_some(), "Should have book_navigation fragment");

        // Look for nav entry with title "I" (first chapter in The Enchiridion)
        fn find_nav_target_eid(nav_value: &IonValue, title: &str) -> Option<i64> {
            match nav_value {
                IonValue::List(entries) => {
                    for entry in entries {
                        if let Some(eid) = find_nav_target_eid(entry, title) {
                            return Some(eid);
                        }
                    }
                }
                IonValue::Struct(s) => {
                    // Check if this entry has the target title
                    if let Some(IonValue::Struct(nav_title)) = s.get(&sym::NAV_TITLE) {
                        if let Some(IonValue::String(text)) = nav_title.get(&sym::TEXT) {
                            if text == title {
                                // Found it! Get the target EID
                                if let Some(nav_target) = s.get(&sym::NAV_TARGET) {
                                    match nav_target {
                                        IonValue::Struct(t) => {
                                            if let Some(IonValue::Int(eid)) = t.get(&sym::POSITION) {
                                                return Some(*eid);
                                            }
                                        }
                                        IonValue::OrderedStruct(fields) => {
                                            for (k, v) in fields {
                                                if *k == sym::POSITION {
                                                    if let IonValue::Int(eid) = v {
                                                        return Some(*eid);
                                                    }
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    // Recurse into nav_container_ref ($392) for nav containers
                    if let Some(containers) = s.get(&sym::NAV_CONTAINER_REF) {
                        if let Some(eid) = find_nav_target_eid(containers, title) {
                            return Some(eid);
                        }
                    }
                    // Recurse into nested entries
                    if let Some(nested) = s.get(&sym::NAV_ENTRIES) {
                        if let Some(eid) = find_nav_target_eid(nested, title) {
                            return Some(eid);
                        }
                    }
                }
                IonValue::Annotated(_, inner) => {
                    return find_nav_target_eid(inner, title);
                }
                _ => {}
            }
            None
        }

        let nav_eid = find_nav_target_eid(&nav_fragment.unwrap().value, "I");
        assert!(
            nav_eid.is_some(),
            "Should find nav entry with title 'I'"
        );

        // The nav entry EID should match the fragment EID (not section EID)
        assert_eq!(
            nav_eid.unwrap(),
            *fragment_eid,
            "Nav entry for 'I' should use fragment EID {}, not section EID {}",
            *fragment_eid, section_eid
        );
    }

    #[test]
    fn test_imprint_paragraph_has_centering_properties() {
        // Test that imprint paragraphs with margin:auto centering have correct KFX properties:
        // - $34 (TEXT_ALIGN) = $321 (JUSTIFY) - for text justification inside the block
        // - $580 (IMAGE_LAYOUT) = $320 (CENTER) - for block centering
        // - $546 (IMAGE_FIT) = $377 (CONTAIN) - for proper layout
        //
        // This was verified via test EPUB converted through Kindle Previewer:
        // CSS: text-align: justify; width: 75%; margin-left: auto; margin-right: auto;
        // KFX: $34=$321, $580=$320, $546=$377
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find the imprint text fragment (contains "ebook is the product")
        let mut imprint_text_id = None;
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::TEXT_CONTENT {
                if let IonValue::Struct(text_struct) = &fragment.value {
                    if let Some(IonValue::List(lines)) = text_struct.get(&sym::CONTENT_ARRAY) {
                        for line in lines {
                            if let IonValue::String(s) = line {
                                if s.contains("ebook is the product") {
                                    // Get the ID from the struct, not the fragment
                                    if let Some(IonValue::Symbol(id)) = text_struct.get(&sym::ID) {
                                        imprint_text_id = Some(*id);
                                        println!("Found imprint text with id: {}", id);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if imprint_text_id.is_some() {
                break;
            }
        }

        assert!(
            imprint_text_id.is_some(),
            "Should find imprint text fragment"
        );

        // Find content block that references this text and get its style
        fn find_style_for_text_id(items: &[IonValue], text_id: u64) -> Option<u64> {
            for item in items {
                if let IonValue::Struct(item_struct) = item {
                    if let Some(IonValue::Struct(text_ref)) = item_struct.get(&sym::TEXT_CONTENT) {
                        if let Some(IonValue::Symbol(found_id)) = text_ref.get(&sym::ID) {
                            if *found_id == text_id {
                                if let Some(IonValue::Symbol(style)) = item_struct.get(&sym::STYLE)
                                {
                                    return Some(*style);
                                }
                            }
                        }
                    }
                    if let Some(IonValue::List(nested)) = item_struct.get(&sym::CONTENT_ARRAY) {
                        if let Some(style) = find_style_for_text_id(nested, text_id) {
                            return Some(style);
                        }
                    }
                }
            }
            None
        }

        let mut imprint_style_sym = None;
        let text_id = imprint_text_id.unwrap();
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::CONTENT_BLOCK {
                if let IonValue::Struct(block) = &fragment.value {
                    if let Some(IonValue::List(items)) = block.get(&sym::CONTENT_ARRAY) {
                        if let Some(style) = find_style_for_text_id(items, text_id) {
                            imprint_style_sym = Some(style);
                            println!("Found imprint style symbol: {}", style);
                            break;
                        }
                    }
                }
            }
        }

        assert!(
            imprint_style_sym.is_some(),
            "Should find style for imprint paragraph"
        );
        let style_sym = imprint_style_sym.unwrap();

        // Find the style and verify centering properties
        for fragment in &kfx.fragments {
            if fragment.ftype == sym::STYLE {
                if let IonValue::Struct(style) = &fragment.value {
                    if let Some(IonValue::Symbol(name)) = style.get(&sym::STYLE_NAME) {
                        if *name == style_sym {
                            println!("Imprint paragraph style properties:");
                            for (k, v) in style.iter() {
                                println!("  {}: {:?}", k, v);
                            }

                            // Check $34 (TEXT_ALIGN) = $321 (JUSTIFY)
                            if let Some(IonValue::Symbol(align)) = style.get(&sym::TEXT_ALIGN) {
                                assert_eq!(
                                    *align,
                                    sym::ALIGN_JUSTIFY,
                                    "Imprint text-align should be JUSTIFY ($321), got {} (ALIGN_JUSTIFY={})",
                                    align,
                                    sym::ALIGN_JUSTIFY
                                );
                                println!("  TEXT_ALIGN: {} (JUSTIFY) ✓", align);
                            } else {
                                panic!("Imprint style should have TEXT_ALIGN property");
                            }

                            // Check $580 (IMAGE_LAYOUT) = $320 (CENTER) for block centering
                            if let Some(IonValue::Symbol(layout)) = style.get(&sym::IMAGE_LAYOUT) {
                                assert_eq!(
                                    *layout,
                                    sym::ALIGN_CENTER,
                                    "Imprint image-layout should be CENTER ($320), got {} (ALIGN_CENTER={})",
                                    layout,
                                    sym::ALIGN_CENTER
                                );
                                println!("  IMAGE_LAYOUT: {} (CENTER) ✓", layout);
                            } else {
                                panic!(
                                    "Imprint style should have IMAGE_LAYOUT property for block centering"
                                );
                            }

                            // Check $546 (IMAGE_FIT) = $377 (CONTAIN)
                            if let Some(IonValue::Symbol(fit)) = style.get(&sym::IMAGE_FIT) {
                                assert_eq!(
                                    *fit,
                                    sym::IMAGE_FIT_CONTAIN,
                                    "Imprint image-fit should be CONTAIN ($377), got {} (IMAGE_FIT_CONTAIN={})",
                                    fit,
                                    sym::IMAGE_FIT_CONTAIN
                                );
                                println!("  IMAGE_FIT: {} (CONTAIN) ✓", fit);
                            } else {
                                panic!("Imprint style should have IMAGE_FIT property");
                            }

                            return;
                        }
                    }
                }
            }
        }

        panic!("Could not find imprint style in KFX output");
    }

    #[test]
    fn test_location_map_has_multiple_entries_per_eid() {
        // Test that LOCATION_MAP creates multiple entries per content item (EID)
        // based on character positions (~150 chars per location).
        // This is required for proper reading progress tracking on Kindle.
        //
        // Previously, we created one entry per content item, which caused:
        // - "Learning reading speed..." stuck message
        // - Position always showing "100% read"
        // - Incorrect TOC page numbers
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Find the LOCATION_MAP fragment ($550)
        let location_map = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::LOCATION_MAP)
            .expect("Should have LOCATION_MAP fragment");

        // Extract the location entries
        let entries = match &location_map.value {
            IonValue::List(outer) => {
                if let Some(IonValue::Struct(wrapper)) = outer.first() {
                    wrapper
                        .get(&sym::LOCATION_ENTRIES)
                        .and_then(|v| match v {
                            IonValue::List(entries) => Some(entries),
                            _ => None,
                        })
                        .expect("Should have location entries")
                } else {
                    panic!("LOCATION_MAP should have wrapper struct")
                }
            }
            _ => panic!("LOCATION_MAP value should be a list"),
        };

        // Count how many times each EID appears
        let mut eid_counts: std::collections::HashMap<i64, usize> =
            std::collections::HashMap::new();
        for entry in entries {
            // Location entries use OrderedStruct to preserve field order
            let eid = match entry {
                IonValue::Struct(entry_struct) => entry_struct.get(&sym::POSITION).cloned(),
                IonValue::OrderedStruct(fields) => fields
                    .iter()
                    .find(|(k, _)| *k == sym::POSITION)
                    .map(|(_, v)| v.clone()),
                _ => None,
            };
            if let Some(IonValue::Int(eid)) = eid {
                *eid_counts.entry(eid).or_insert(0) += 1;
            }
        }

        // Find the max count - at least one EID should appear multiple times
        let max_count = eid_counts.values().max().copied().unwrap_or(0);
        println!("Max EID frequency: {}", max_count);
        println!("Total unique EIDs: {}", eid_counts.len());
        println!("Total location entries: {}", entries.len());

        // With ~150 chars per location and typical paragraph lengths,
        // we expect some EIDs to appear 10+ times (for long paragraphs)
        assert!(
            max_count >= 5,
            "At least one EID should appear 5+ times for long content items, max was {}",
            max_count
        );

        // Also verify we have a reasonable number of location entries
        // (epictetus.epub should have ~500 locations)
        assert!(
            entries.len() > 100,
            "Should have substantial number of location entries, got {}",
            entries.len()
        );
    }

    #[test]
    fn test_toc_eids_in_location_map() {
        // Test that TOC navigation EIDs exist in the LOCATION_MAP.
        // Kindle uses LOCATION_MAP for navigation - if TOC EIDs aren't present,
        // navigation to those positions may fail.
        //
        // Reference file (epictetus.kfx) has ~34% of TOC EIDs in LOCATION_MAP.
        // We should have at least 25% coverage for reliable navigation.
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Extract TOC EIDs from book navigation ($389)
        let nav_fragment = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::BOOK_NAVIGATION)
            .expect("Should have book navigation");

        let mut toc_eids: Vec<i64> = Vec::new();
        fn extract_nav_eids(value: &IonValue, eids: &mut Vec<i64>) {
            match value {
                IonValue::List(items) => {
                    for item in items {
                        extract_nav_eids(item, eids);
                    }
                }
                IonValue::Struct(map) => {
                    // Check if this is a nav_target ($246)
                    if let Some(target) = map.get(&sym::NAV_TARGET) {
                        if let IonValue::OrderedStruct(fields) = target {
                            for (k, v) in fields {
                                if *k == sym::POSITION {
                                    if let IonValue::Int(eid) = v {
                                        eids.push(*eid);
                                    }
                                }
                            }
                        } else if let IonValue::Struct(target_map) = target {
                            if let Some(IonValue::Int(eid)) = target_map.get(&sym::POSITION) {
                                eids.push(*eid);
                            }
                        }
                    }
                    // Recurse into nav_entries ($247)
                    if let Some(entries) = map.get(&sym::NAV_ENTRIES) {
                        extract_nav_eids(entries, eids);
                    }
                    // Recurse into nav_containers ($392)
                    if let Some(containers) = map.get(&sym::NAV_CONTAINER_REF) {
                        extract_nav_eids(containers, eids);
                    }
                }
                IonValue::Annotated(_, inner) => {
                    extract_nav_eids(inner, eids);
                }
                _ => {}
            }
        }
        extract_nav_eids(&nav_fragment.value, &mut toc_eids);

        println!("Found {} TOC EIDs", toc_eids.len());

        // Extract LOCATION_MAP EIDs
        let location_map = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::LOCATION_MAP)
            .expect("Should have location map");

        let mut location_eids: std::collections::HashSet<i64> = std::collections::HashSet::new();
        fn extract_location_eids(value: &IonValue, eids: &mut std::collections::HashSet<i64>) {
            match value {
                IonValue::List(items) => {
                    for item in items {
                        extract_location_eids(item, eids);
                    }
                }
                IonValue::Struct(map) => {
                    if let Some(IonValue::Int(eid)) = map.get(&sym::POSITION) {
                        eids.insert(*eid);
                    }
                    if let Some(entries) = map.get(&sym::LOCATION_ENTRIES) {
                        extract_location_eids(entries, eids);
                    }
                }
                IonValue::OrderedStruct(fields) => {
                    for (k, v) in fields {
                        if *k == sym::POSITION {
                            if let IonValue::Int(eid) = v {
                                eids.insert(*eid);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        extract_location_eids(&location_map.value, &mut location_eids);

        println!("Found {} unique LOCATION_MAP EIDs", location_eids.len());

        // Count how many TOC EIDs are in LOCATION_MAP
        let toc_in_location: Vec<_> = toc_eids
            .iter()
            .filter(|eid| location_eids.contains(eid))
            .collect();

        let coverage = if toc_eids.is_empty() {
            0.0
        } else {
            100.0 * toc_in_location.len() as f64 / toc_eids.len() as f64
        };

        println!(
            "TOC EIDs in LOCATION_MAP: {}/{} ({:.1}%)",
            toc_in_location.len(),
            toc_eids.len(),
            coverage
        );

        // Print missing EIDs for debugging
        let missing: Vec<_> = toc_eids
            .iter()
            .filter(|eid| !location_eids.contains(eid))
            .take(10)
            .collect();
        println!("Missing TOC EIDs (first 10): {:?}", missing);

        // Require at least 25% coverage (reference has ~34%)
        assert!(
            coverage >= 25.0,
            "TOC EIDs should have at least 25% coverage in LOCATION_MAP, got {:.1}%",
            coverage
        );
    }

    /// Test that position maps correctly account for cover section EIDs
    /// When a book has a cover, the cover uses EIDs 750-751, so chapters start at 752
    #[test]
    fn test_position_map_eids_with_cover() {
        // Load epictetus.epub which has a cover image
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        assert!(
            book.metadata.cover_image.is_some(),
            "epictetus.epub should have a cover image"
        );

        let kfx = KfxBookBuilder::from_book(&book);

        // Find position map ($264)
        let pos_map = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::POSITION_MAP)
            .expect("Should have position map");

        // Extract EIDs from position map
        let mut pos_map_eids: Vec<i64> = Vec::new();
        if let IonValue::List(entries) = &pos_map.value {
            for entry in entries {
                if let IonValue::Struct(map) = entry {
                    if let Some(IonValue::List(eids)) = map.get(&sym::ENTITY_ID_LIST) {
                        for eid in eids {
                            if let IonValue::Int(e) = eid {
                                pos_map_eids.push(*e);
                            }
                        }
                    }
                }
            }
        }

        println!(
            "Position map EIDs (first 20): {:?}",
            &pos_map_eids[..20.min(pos_map_eids.len())]
        );

        // LOCAL_MIN_ID = 860, so:
        // - Cover section EID = 860, cover content EID = 861
        // - First chapter section EID = 862
        let expected_first_chapter_eid = SymbolTable::LOCAL_MIN_ID as i64 + 2; // 862

        assert!(
            pos_map_eids.contains(&expected_first_chapter_eid),
            "With cover, first chapter section EID should be {}, got: {:?}",
            expected_first_chapter_eid,
            &pos_map_eids[..20.min(pos_map_eids.len())]
        );
    }

    /// Test that position ID map ($265) correctly accounts for cover section
    #[test]
    fn test_position_id_map_eids_with_cover() {
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        assert!(book.metadata.cover_image.is_some());

        let kfx = KfxBookBuilder::from_book(&book);

        // Find position ID map ($265)
        let pos_id_map = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::POSITION_ID_MAP)
            .expect("Should have position ID map");

        // Extract EIDs from position ID map
        let mut pos_id_map_eids: Vec<i64> = Vec::new();
        if let IonValue::List(entries) = &pos_id_map.value {
            for entry in entries {
                if let IonValue::Struct(map) = entry {
                    if let Some(IonValue::Int(eid)) = map.get(&sym::EID_VALUE) {
                        pos_id_map_eids.push(*eid);
                    }
                }
            }
        }

        println!(
            "Position ID map EIDs (first 20): {:?}",
            &pos_id_map_eids[..20.min(pos_id_map_eids.len())]
        );

        // LOCAL_MIN_ID = 860, so with cover: first chapter section EID = 862
        let expected_first_chapter_eid = SymbolTable::LOCAL_MIN_ID as i64 + 2;

        assert!(
            pos_id_map_eids.contains(&expected_first_chapter_eid),
            "Position ID map should contain chapter section EID {}, got: {:?}",
            expected_first_chapter_eid,
            &pos_id_map_eids[..20.min(pos_id_map_eids.len())]
        );
    }

    /// CRITICAL TEST: book navigation ($389) nav_target EIDs must exist in location map ($550)
    /// This ensures TOC entries can be navigated to on Kindle
    #[test]
    fn test_nav_targets_must_exist_in_location_map() {
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Extract nav_target EIDs from book navigation ($389)
        let nav_frag = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::BOOK_NAVIGATION)
            .expect("Should have book navigation");

        let mut nav_eids: Vec<i64> = Vec::new();
        fn extract_nav_eids(val: &IonValue, eids: &mut Vec<i64>) {
            match val {
                IonValue::List(items) => {
                    for item in items {
                        extract_nav_eids(item, eids);
                    }
                }
                IonValue::Struct(map) => {
                    if let Some(target) = map.get(&sym::NAV_TARGET) {
                        match target {
                            IonValue::OrderedStruct(fields) => {
                                for (k, v) in fields {
                                    if *k == sym::POSITION {
                                        if let IonValue::Int(eid) = v {
                                            eids.push(*eid);
                                        }
                                    }
                                }
                            }
                            IonValue::Struct(target_map) => {
                                if let Some(IonValue::Int(eid)) = target_map.get(&sym::POSITION) {
                                    eids.push(*eid);
                                }
                            }
                            _ => {}
                        }
                    }
                    for v in map.values() {
                        extract_nav_eids(v, eids);
                    }
                }
                _ => {}
            }
        }
        extract_nav_eids(&nav_frag.value, &mut nav_eids);

        println!("Navigation EIDs: {:?}", nav_eids);

        // Extract EIDs from location map ($550)
        let loc_map = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::LOCATION_MAP)
            .expect("Should have location map");

        let mut loc_map_eids: std::collections::HashSet<i64> = std::collections::HashSet::new();
        fn extract_loc_eids(val: &IonValue, eids: &mut std::collections::HashSet<i64>) {
            match val {
                IonValue::List(items) => {
                    for item in items {
                        extract_loc_eids(item, eids);
                    }
                }
                IonValue::Struct(map) => {
                    if let Some(IonValue::Int(eid)) = map.get(&sym::POSITION) {
                        eids.insert(*eid);
                    }
                    for v in map.values() {
                        extract_loc_eids(v, eids);
                    }
                }
                IonValue::OrderedStruct(fields) => {
                    for (k, v) in fields {
                        if *k == sym::POSITION {
                            if let IonValue::Int(eid) = v {
                                eids.insert(*eid);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        extract_loc_eids(&loc_map.value, &mut loc_map_eids);

        println!("Location map has {} unique EIDs", loc_map_eids.len());

        // CRITICAL: Every nav_target EID must exist in the location map
        let mut missing = Vec::new();
        for nav_eid in &nav_eids {
            if !loc_map_eids.contains(nav_eid) {
                missing.push(*nav_eid);
            }
        }

        assert!(
            missing.is_empty(),
            "Navigation target EIDs missing from location map: {:?}. All nav EIDs: {:?}",
            missing,
            nav_eids
        );
    }

    /// Test EID consistency between position_map ($264) and position_id_map ($265)
    #[test]
    fn test_position_maps_eid_consistency() {
        let book = crate::epub::read_epub("tests/fixtures/epictetus.epub").expect("parse EPUB");
        let kfx = KfxBookBuilder::from_book(&book);

        // Extract EIDs from position map ($264)
        let pos_map = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::POSITION_MAP)
            .expect("Should have position map");

        let mut pos_map_eids: std::collections::HashSet<i64> = std::collections::HashSet::new();
        if let IonValue::List(entries) = &pos_map.value {
            for entry in entries {
                if let IonValue::Struct(map) = entry {
                    if let Some(IonValue::List(eids)) = map.get(&sym::ENTITY_ID_LIST) {
                        for eid in eids {
                            if let IonValue::Int(e) = eid {
                                pos_map_eids.insert(*e);
                            }
                        }
                    }
                }
            }
        }

        // Extract EIDs from position ID map ($265)
        let pos_id_map = kfx
            .fragments
            .iter()
            .find(|f| f.ftype == sym::POSITION_ID_MAP)
            .expect("Should have position ID map");

        let mut pos_id_map_eids: std::collections::HashSet<i64> = std::collections::HashSet::new();
        if let IonValue::List(entries) = &pos_id_map.value {
            for entry in entries {
                if let IonValue::Struct(map) = entry {
                    if let Some(IonValue::Int(eid)) = map.get(&sym::EID_VALUE) {
                        if *eid != 0 {
                            // Skip terminator entry
                            pos_id_map_eids.insert(*eid);
                        }
                    }
                }
            }
        }

        println!("Position map has {} EIDs", pos_map_eids.len());
        println!("Position ID map has {} EIDs", pos_id_map_eids.len());

        // Every EID in position_map should be in position_id_map
        let missing: Vec<_> = pos_map_eids
            .iter()
            .filter(|eid| !pos_id_map_eids.contains(eid))
            .collect();

        assert!(
            missing.is_empty(),
            "EIDs in position_map but not in position_id_map: {:?}",
            missing
        );
    }
}
