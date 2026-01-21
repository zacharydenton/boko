//! KFX symbol definitions and symbol table management.
//!
//! Contains the YJ_symbols shared table constants and local symbol management.

use std::collections::HashMap;

use crate::kfx::ion::IonValue;

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
    pub const DECORATION_BOX_CLONE: u64 = 99; // $99 - decoration-break: clone

    // ==========================================================================
    // VERTICAL ALIGN VALUES ($44)
    // ==========================================================================
    pub const VERTICAL_ALIGN: u64 = 44; // $44 - vertical-align property
    pub const VERTICAL_TOP: u64 = 58; // $58 - vertical-align: top
    pub const VERTICAL_BOTTOM: u64 = 60; // $60 - vertical-align: bottom
    pub const VERTICAL_SUPER: u64 = 370; // $370 - vertical-align: super
    pub const VERTICAL_SUB: u64 = 371; // $371 - vertical-align: sub
    pub const VERTICAL_TEXT_TOP: u64 = 447; // $447 - vertical-align: text-top
    pub const VERTICAL_TEXT_BOTTOM: u64 = 449; // $449 - vertical-align: text-bottom
    // Note: $350 (FONT_WEIGHT_NORMAL) = baseline, $320 (ALIGN_CENTER) = middle

    // ==========================================================================
    // LAYOUT PROPERTIES
    // ==========================================================================
    pub const MIN_HEIGHT: u64 = 62; // $62 - min-height (also used for height in some contexts)
    pub const MIN_WIDTH: u64 = 63; // $63 - min-width
    pub const VISIBILITY: u64 = 68; // $68 - visibility (boolean: true = visible)
    pub const OVERFLOW_CLIP: u64 = 476; // $476 - overflow: hidden/clip (boolean: true = clip)

    // ==========================================================================
    // CLEAR PROPERTY ($628)
    // ==========================================================================
    pub const CLEAR: u64 = 628; // $628 - clear property
    pub const CLEAR_BOTH: u64 = 421; // $421 - clear: both
    // Note: $349 (TEXT_TRANSFORM_NONE) = none, $59 (ALIGN_LEFT) = left, $61 (ALIGN_RIGHT) = right

    // ==========================================================================
    // WORD BREAK ($569)
    // ==========================================================================
    pub const WORD_BREAK: u64 = 569; // $569 - word-break property
    pub const WORD_BREAK_ALL: u64 = 570; // $570 - word-break: break-all
    // Note: $350 (FONT_WEIGHT_NORMAL) = normal

    // ==========================================================================
    // PAGE BREAK CONTROL
    // ==========================================================================
    pub const BREAK_INSIDE: u64 = 135; // $135 - break-inside property
    pub const BREAK_AFTER: u64 = 788; // $788 - break-after property
    pub const BREAK_BEFORE: u64 = 789; // $789 - break-before property
    pub const BREAK_AVOID: u64 = 353; // $353 - avoid value for break properties
    // Note: $383 (BLOCK_TYPE_BLOCK) = auto

    // ==========================================================================
    // BORDER RADIUS ($459-$462)
    // ==========================================================================
    pub const BORDER_RADIUS_TL: u64 = 459; // $459 - border-top-left-radius
    pub const BORDER_RADIUS_TR: u64 = 460; // $460 - border-top-right-radius
    pub const BORDER_RADIUS_BR: u64 = 461; // $461 - border-bottom-right-radius
    pub const BORDER_RADIUS_BL: u64 = 462; // $462 - border-bottom-left-radius

    // ==========================================================================
    // BORDER PROPERTIES (additional)
    // ==========================================================================
    pub const BORDER_TOP_COLOR: u64 = 83; // $83 - border-top-color
    pub const BORDER_RIGHT_COLOR: u64 = 84; // $84 - border-right-color
    pub const BORDER_BOTTOM_COLOR: u64 = 89; // $89 - border-bottom-color
    pub const BORDER_LEFT_COLOR: u64 = 94; // $94 - border-left-color
    pub const BORDER_TOP_PRESENT: u64 = 88; // $88 - border-top decoration present
    pub const BORDER_TOP_WIDTH: u64 = 93; // $93 - border-top-width

    // ==========================================================================
    // TABLE PROPERTIES
    // ==========================================================================
    pub const CAPTION_SIDE: u64 = 453; // $453 - caption-side property

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

    // List symbols (for ol/ul)
    pub const LIST_TYPE: u64 = 100; // $100 - list type property on container
    pub const LIST_TYPE_DECIMAL: u64 = 343; // $343 - decimal numbered list (ol)
    pub const CONTENT_LIST: u64 = 276; // $276 - content type for list container (ol/ul)
    pub const CONTENT_LIST_ITEM: u64 = 277; // $277 - content type for list item (li)
    // Note: Unordered list (ul) bullet type symbol TBD - needs investigation

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
    pub const LOCAL_MIN_ID: u64 = 860;

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
}
