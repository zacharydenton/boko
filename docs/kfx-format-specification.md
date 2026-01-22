# KFX Format Specification

This document provides a comprehensive specification of Amazon's KFX format, reverse-engineered from the kfxinput library (part of calibre's KFX Output plugin) and analysis of real KFX files.

**Version**: Based on KFX container version 2, YJ_symbols version 10

## Table of Contents

1. [Overview](#1-overview)
2. [Container Format](#2-container-format)
3. [Ion Binary Encoding](#3-ion-binary-encoding)
4. [Symbol Tables](#4-symbol-tables)
5. [Fragment Types](#5-fragment-types)
6. [Content Structure](#6-content-structure)
7. [Style System](#7-style-system)
8. [Resources](#8-resources)
9. [Navigation](#9-navigation)
10. [Metadata](#10-metadata)
11. [Symbol Reference](#11-symbol-reference)

---

## 1. Overview

KFX (Kindle Format X) is Amazon's proprietary ebook format, introduced around 2015. It uses Amazon's Ion binary format for data serialization and supports advanced typography, enhanced typesetting, and features like Page Flip and X-Ray.

### Key Characteristics

- **Binary format**: Uses Ion binary encoding (Amazon's data serialization format)
- **Fragment-based**: Content organized into typed fragments
- **Symbol-heavy**: Uses numeric symbols ($XXX) for field names and values
- **Supports**: Reflowable text, fixed-layout, comics, magazines, print replicas

### File Extensions

| Extension | Description |
|-----------|-------------|
| `.kfx` | Main KFX container |
| `.kfx-zip` | ZIP archive containing multiple KFX containers |
| `.azw` | Can contain KFX (modern Kindle format) |

---

## 2. Container Format

### 2.1 Container Header

KFX files start with a container header:

```
Offset  Size  Field
------  ----  -----
0       4     Signature: "CONT" (0x434F4E54)
4       2     Version (uint16 LE): 1 or 2
6       4     Header length (uint32 LE)
10      4     Container info offset (uint32 LE)
14      4     Container info length (uint32 LE)
```

### 2.2 Container Info Structure

The container info is an Ion struct containing:

| Symbol | Field | Description |
|--------|-------|-------------|
| `$409` | container_id | Unique container identifier (ACR) |
| `$410` | compression_type | Compression (0 = none) |
| `$411` | drm_scheme | DRM scheme (0 = none) |
| `$412` | chunk_size | Default: 4096 |
| `$413` | index_table_offset | Offset to entity index |
| `$414` | index_table_length | Length of entity index |
| `$415` | doc_symbol_offset | Symbol table offset |
| `$416` | doc_symbol_length | Symbol table length |
| `$594` | format_capabilities_offset | (v2 only) |
| `$595` | format_capabilities_length | (v2 only) |

### 2.3 Entity Index Table

After the header, an entity index table lists all entities:

```
Per entity (24 bytes):
  4 bytes: ID symbol number (uint32 LE)
  4 bytes: Type symbol number (uint32 LE)
  8 bytes: Offset from header end (uint64 LE)
  8 bytes: Length (uint64 LE)
```

### 2.4 Entity Structure

Each entity (ENTY) has its own header:

```
Offset  Size  Field
------  ----  -----
0       4     Signature: "ENTY" (0x454E5459)
4       2     Version (uint16 LE): 1
6       4     Header length (uint32 LE)
```

Entity info follows (Ion struct):
- `$410`: compression_type (0 = none)
- `$411`: drm_scheme (0 = none)

The entity payload follows the header. For most entity types, this is Ion-encoded data. For raw media (`$417`), it's raw binary (images, fonts).

### 2.5 Container Types

KFX books consist of multiple containers, identified by the fragment types they contain:

| Type | Identifying Fragment IDs | Purpose |
|------|-------------------------|---------|
| KFX-main | 259, 260, 538 | Book content (sections, storylines, document structure) |
| KFX-metadata | 258, 419, 490, 585 | Metadata, symbol tables, entity maps |
| KFX-attachable | 417 | Binary resources (images, fonts, raw media) |

**Container type detection logic**:
1. If entity types include any of {259, 260, 538} → KFX-main
2. Else if types include any of {258, 419, 490, 585} OR has doc_symbols → KFX-metadata
3. Else if types include {417} → KFX-attachable

**Required fragments** (for valid books):
- `$ion_symbol_table` - Symbol table import
- `$270` - Container info
- `$490` or `$258` - Metadata (one of these)
- `$389` - Book navigation
- `$419` - Container entity map
- `$538` - Document data
- `$550` - Story timeline
- `$265` - Maximum EID
- `$264` - Format capabilities

### 2.6 Generator Info

After container info, JSON generator info appears:

```json
[
  {"key": "kfxgen_package_version", "value": "..."},
  {"key": "kfxgen_application_version", "value": "..."},
  {"key": "kfxgen_payload_sha1", "value": "..."},
  {"key": "kfxgen_acr", "value": "..."}
]
```

---

## 3. Ion Binary Encoding

Amazon Ion is a richly-typed, self-describing binary format. KFX uses Ion binary version 1.0.

### 3.1 Ion Binary Signature

All Ion binary data starts with:
```
0xE0 0x01 0x00 0xEA   (version marker: Ion 1.0)
```

### 3.2 Ion Type Codes

| Type Code | Hex | Name | Description |
|-----------|-----|------|-------------|
| 0 | 0x0 | null | Null value |
| 1 | 0x1 | bool | Boolean (flag=0 for false, 1 for true) |
| 2 | 0x2 | posint | Positive integer |
| 3 | 0x3 | negint | Negative integer |
| 4 | 0x4 | float | IEEE 754 float (0/4/8 bytes) |
| 5 | 0x5 | decimal | Arbitrary-precision decimal |
| 6 | 0x6 | timestamp | Date/time |
| 7 | 0x7 | symbol | Symbol reference (ID) |
| 8 | 0x8 | string | UTF-8 string |
| 9 | 0x9 | clob | Character LOB |
| 10 | 0xA | blob | Binary LOB |
| 11 | 0xB | list | Ordered collection |
| 12 | 0xC | sexp | S-expression |
| 13 | 0xD | struct | Unordered key-value pairs |
| 14 | 0xE | annotation | Annotated value wrapper |

### 3.3 Value Encoding Format

Each value is encoded as a type descriptor byte followed by data:
```
Type descriptor: [type:4bits][length/flag:4bits]

If flag == 14 (0xE): length follows as VarUInt
If flag == 15 (0xF): value is null of this type
Otherwise: flag is the length
```

**Special cases**:
- Boolean: flag is the value (0=false, 1=true), no length bytes
- Struct with flag=1: sorted struct (error condition in KFX)
- Float: flag=0 for 0.0, flag=4 for 32-bit, flag=8 for 64-bit

### 3.4 VarUInt and VarInt Encoding

**VarUInt** (Variable-length unsigned integer):
- MSB of each byte indicates continuation (1=more bytes, 0=last byte)
- Lower 7 bits contain data, big-endian

**VarInt** (Variable-length signed integer):
- Same as VarUInt, but first data byte's MSB is sign bit

```
Examples:
  0 → 0x00
  127 → 0x7F
  128 → 0x81 0x00
  16383 → 0xFF 0x7F
```

### 3.5 Decimal Encoding

Decimals are encoded as: `coefficient × 10^exponent`

```
Structure:
  - VarInt exponent (negative for fractional values)
  - SignedInt coefficient (magnitude bytes, sign in high bit)

Example: 0.833333
  exponent: -6 (encoded as VarInt)
  coefficient: 833333 (encoded as signed int)
  Result: 833333 × 10^(-6) = 0.833333

Example: 1.5
  exponent: -1
  coefficient: 15
  Result: 15 × 10^(-1) = 1.5
```

### 3.6 Struct Encoding

Structs are sequences of field-name/value pairs:
```
For each field:
  - VarUInt: symbol ID of field name
  - Ion value: field value

The struct length includes all field bytes.
```

### 3.7 Annotation Encoding

Annotated values wrap another value with symbol annotations:
```
Type descriptor: 0xE_ (where _ is length flag)
Length: total bytes of annotation data + wrapped value
Annotation length: VarUInt (bytes of annotation IDs)
Annotation IDs: sequence of VarUInt symbol IDs
Wrapped value: the actual Ion value
```

**KFX Fragment format**:
```
E7 <len>          // annotation wrapper
  82              // 2 bytes of annotations
  <fid_sym>       // fragment ID symbol
  <ftype_sym>     // fragment type symbol
  <value>         // fragment value (struct, blob, etc.)
```

### 3.8 Symbol References

Symbols are stored as VarUInt IDs referencing the symbol table:
```
Type descriptor: 0x7_ (where _ is length)
Symbol ID: unsigned integer (big-endian)

Examples:
  0x71 0x0A       → Symbol ID 10 ($10)
  0x72 0x01 0x9B  → Symbol ID 411 ($411)
```

---

## 4. Symbol Tables

### 4.1 System Symbol Table ($ion)

The base Ion symbol table (IDs 1-9). These symbols are always available:

| ID | Symbol | Purpose |
|----|--------|---------|
| 1  | `$ion` | Ion marker |
| 2  | `$ion_1_0` | Version marker |
| 3  | `$ion_symbol_table` | Symbol table annotation |
| 4  | `name` | Name field |
| 5  | `version` | Version field |
| 6  | `imports` | Imports list |
| 7  | `symbols` | Local symbols list |
| 8  | `max_id` | Maximum symbol ID |
| 9  | `$ion_shared_symbol_table` | Shared table annotation |

### 4.2 YJ_symbols Shared Table

Amazon's shared symbol table for KFX:
- **Name**: "YJ_symbols"
- **Version**: 10 (current)
- **Symbol range**: $10 through approximately $851
- **Total symbols**: ~842 (varies by version)

Symbols are defined in order, so:
- $10 is the first YJ symbol
- Symbol ID = 10 + (position in symbols list - 1)

**Symbol naming convention**:
- Symbols ending with `?` in the catalog are deprecated/unknown
- Property symbols start at $10
- Value symbols are interspersed throughout

### 4.3 Local Symbols

Book-specific symbols are added after shared symbols. Common patterns:

| Pattern | Purpose | Example |
|---------|---------|---------|
| `V_X_Y_*` | Styles | `V_1_0_PARA-1_0_abc123_5` |
| `rsrcN` | Resources | `rsrc0`, `rsrc1` |
| `resource/rsrcN` | Media locations | `resource/rsrc0` |
| `section-*` | Sections | `section-1_0_abc123_1` |
| `story-*` | Storylines | `story-1_0_abc123_1` |
| `anchor-*` | Anchors | `anchor-1_0_abc123_1` |
| `navContainer*` | Navigation | `navContainer1_0_abc123_1` |
| `navUnit*` | Navigation entries | `navUnit1_0_abc123_1` |
| `content_N` | Text content | `content_0`, `content_1` |
| `CR!*` | Container IDs | `CR!ABC123...` (28 chars) |

**Symbol classification types**:
- COMMON: Known special names (`content_N`, UUIDs, `yj.*` patterns)
- DICTIONARY: Dictionary-specific (`G*`, `yj.dictionary.*`)
- ORIGINAL: Original source patterns (KindleGen generated)
- BASE64: Base64-encoded IDs (22+ chars)
- SHORT: Short encoded IDs (`rsrcN`, single prefix + alphanumeric)

### 4.4 Symbol Table Import

Each container includes a symbol table import annotation:

```
$ion_symbol_table::{
  imports: [
    {name: "YJ_symbols", version: 10, max_id: 842}
  ],
  symbols: ["local_symbol_1", "local_symbol_2", ...]
}
```

**Important**: The `max_id` in the import includes system symbols (add 9 to the YJ_symbols count). When reading, subtract 9 to get the actual YJ_symbols max_id.

### 4.5 Symbol ID Calculation

For a symbol to resolve to its ID:
```
If symbol in system table (1-9): ID = position
If symbol in YJ_symbols: ID = 10 + position_in_YJ_symbols - 1
If symbol is local: ID = local_min_id + position_in_local_list
```

Where `local_min_id` = 10 + len(YJ_symbols) = typically 852

---

## 5. Fragment Types

### 5.1 Root Fragment Types

These exist once per book and have fid == ftype:

| Symbol | Name | Purpose |
|--------|------|---------|
| `$258` | metadata | Book metadata |
| `$262` | reading_orders | Reading order definition |
| `$264` | format_capabilities | Format capability list |
| `$265` | max_id | Maximum element ID |
| `$389` | book_navigation | Navigation containers |
| `$395` | resource_path | Resource location info |
| `$419` | container_entity_map | Container contents |
| `$490` | book_metadata | Extended metadata |
| `$538` | document_data | Document structure |
| `$550` | story_timeline | Story timing info |
| `$585` | cde_features | Content delivery features |
| `$593` | format_capabilities | Format version info |
| `$611` | dictionary_index | Dictionary specific |

### 5.2 Content Fragment Types

| Symbol | Name | Purpose |
|--------|------|---------|
| `$145` | text_content | Raw text data |
| `$157` | style | Style definitions |
| `$164` | external_resource | Resource metadata |
| `$259` | storyline | Content sequence |
| `$260` | section | Book section/chapter |
| `$266` | anchor | Position anchor |
| `$267` | periodical_data | Magazine metadata |
| `$391` | nav_container | Navigation container |
| `$393` | nav_unit | Navigation entry |
| `$417` | raw_media | Raw binary data |
| `$597` | auxiliary_data | Additional metadata |
| `$608` | page_template | Page layout info |
| `$609` | section_position_map | Position mapping |

### 5.3 Singleton vs Multiple

**Singleton types** (one per book):
- `$258`, `$262`, `$389`, `$395`, `$419`, `$490`, `$538`, `$550`, `$585`

**Multiple instances allowed**:
- `$145`, `$157`, `$164`, `$259`, `$260`, `$266`, `$391`, `$393`, `$417`, `$593`, `$597`, `$608`, `$609`

### 5.4 Fragment ID Keys

Each fragment type has a specific field that contains its identifier. This mapping is crucial for correctly extracting fragment IDs from their values:

| Fragment Type | ID Key Field(s) | Description |
|---------------|-----------------|-------------|
| `$145` | `name` | Text content name |
| `$157` | `$173` | Style name (self-reference) |
| `$164` | `$175` | Resource name |
| `$259` | `$176` | Storyline name |
| `$260` | `$174` | Section name |
| `$266` | `$180` | Anchor name |
| `$267` | `$174` | Periodical section name |
| `$387` | `$174` | Section metadata name |
| `$391` | `$239` | Navigation container name |
| `$394` | `$240` | Navigation unit name |
| `$417` | `$165` | Raw media location |
| `$418` | `$165` | Font resource location |
| `$597` | `$174`, `$598` | Auxiliary data (section or EID) |
| `$608` | `$598` | Page template EID |
| `$609` | `$174` | Section position map |
| `$610` | `$602` | EID bucket index |
| `$692` | `name` | Named content reference |
| `$756` | `$757` | Dictionary entry |

### 5.5 Fragment Reference Relationships

Fragments reference other fragments through specific fields. This mapping shows which field references which fragment type:

| Field | References Fragment Type | Description |
|-------|-------------------------|-------------|
| `$145` | `$145` | Text content |
| `$146` | `$608` | Page template children |
| `$157` | `$157` | Style reference |
| `$165` | `$417` | Raw media location |
| `$167` | `$164` | Resource reference |
| `$170` | `$260` | Section list |
| `$173` | `$157` | Style self-reference |
| `$174` | `$260` | Section reference |
| `$175` | `$164` | Resource name |
| `$176` | `$259` | Storyline reference |
| `$179` | `$266` | Anchor reference (links) |
| `$214` | `$164` | Page list resource |
| `$245` | `$164` | Image resource |
| `$247` | `$394` | Navigation unit children |
| `$266` | `$266` | Anchor self-reference |
| `$392` | `$391` | Navigation container list |
| `$429` | `$157` | Inline style reference |
| `$479` | `$164` | Background image |
| `$528` | `$164` | Background image alt |
| `$597` | `$597` | Auxiliary data reference |
| `$635` | `$164` | Optional resource |
| `$636` | `$417` | Tile media reference |
| `$749` | `$259` | Storyline reference |
| `$757` | `$756` | Dictionary reference |

### 5.6 Auxiliary Data Structure ($597)

Auxiliary data fragments mark sections for navigation targeting. Each section (including cover) has a corresponding auxiliary data fragment.

**Structure:**
```
auxiliary_data = {
  $598: aux_id,              // Self-reference symbol
  $258: [                    // Metadata array
    {
      $307: true,            // VALUE = true
      $492: "IS_TARGET_SECTION"  // METADATA_KEY
    }
  ]
}
```

**Key Fields:**
| Symbol | Name | Description |
|--------|------|-------------|
| `$598` | aux_data_ref | Self-reference symbol ID |
| `$258` | metadata | Array of metadata entries |
| `$307` | value | Boolean value (true) |
| `$492` | metadata_key | Key string ("IS_TARGET_SECTION") |

The `IS_TARGET_SECTION` flag indicates that this section can be a navigation target (for TOC jumps, bookmarks, location tracking). Every section in the book should have a corresponding auxiliary data fragment with this flag.

**Example:**
```
$597::aux-cover = {
  $598: $aux-cover,
  $258: [{ $307: true, $492: "IS_TARGET_SECTION" }]
}
```

---

## 6. Content Structure

### 6.1 Document Hierarchy

```
document_data ($538)
  └── reading_orders ($169)
        └── reading_order
              ├── $178: name (e.g., "$351" for main)
              └── $170: [section_ids...]

section ($260)
  ├── $174: section_name (self-reference)
  └── $141: [page_templates...]
        └── page_template
              ├── $159: $270 (container type)
              ├── $156: layout ($323=block, $326=scale_fit)
              └── $176: storyline_reference

storyline ($259)
  ├── $176: story_name (self-reference)
  └── $146: [content_items...]
```

### 6.2 Content Item Types ($159)

| Symbol | Name | HTML Equivalent | Notes |
|--------|------|-----------------|-------|
| `$269` | BLOCK_CONTAINER | div, p, blockquote | Block-level container |
| `$270` | PAGE_TEMPLATE | page/section | Section/page container |
| `$271` | IMAGE | img | Image content |
| `$272` | PLUGIN | object, embed | Embedded plugin (KVG vector graphics) |
| `$273` | INLINE_CONTAINER | span | Inline container |
| `$274` | SVG | svg | Scalable vector graphics |
| `$276` | LIST | ul, ol | List container |
| `$277` | LIST_ITEM | li | List item |
| `$278` | TABLE | table | Table container |
| `$279` | TABLE_ROW | tr | Table row |
| `$280` | TABLE_CELL | td | Table cell (unconfirmed - marked with ?) |
| `$439` | HIDDEN_CONTAINER | display:none | Hidden/non-rendered content |
| `$151` | TABLE_HEADER | thead | Table header section (also used for oeb-page-head position) |
| `$454` | TABLE_BODY | tbody | Table body section |
| `$455` | TABLE_FOOTER | tfoot | Table footer section (also used for oeb-page-foot position) |
| `$596` | HORIZONTAL_RULE | hr | Horizontal rule |
| `$764` | RUBY | ruby | Ruby annotation base (unconfirmed - marked with ?) |
| `$765` | RUBY_TEXT | rt | Ruby text |
| `$766` | RUBY_CONTAINER | rp | Ruby parenthesis/container |

**Note**: Symbols marked with `?` in the YJ_symbols catalog are unconfirmed or rarely used.

### 6.3 Content Item Structure

```
content_item = {
  $159: content_type,      // Type symbol
  $155: element_id,        // Position/EID
  $157: style_reference,   // Style name symbol
  $145: text_content,      // Text reference (for text items)
  $146: children,          // Child items (for containers)
  $175: resource_name,     // Resource reference (for images)
  $584: alt_text,          // Alt text (for images)
}
```

### 6.4 Inline Style Runs ($142)

Content items can have inline styling applied to ranges of text through the `$142` field. This is used for:
- Links (noteref references)
- Drop caps
- Ruby annotations
- Inline style changes

**Structure:**
```
content_item = {
  $159: $269,              // BLOCK_CONTAINER
  $146: ["text content"],  // Text content
  $142: [                  // Inline style runs (list)
    {
      $143: start_offset,  // Start character offset (0-based)
      $144: length,        // Number of characters affected
      $179: "anchor-id",   // Link target (optional)
      $616: $617,          // noteref marker (optional)
      $157: style_ref,     // Style override (optional)
      $429: inline_style,  // Inline style struct (optional)
    },
    // ... more style runs
  ]
}
```

**Key Fields:**
| Symbol | Name | Description |
|--------|------|-------------|
| `$143` | start | Character offset where run starts |
| `$144` | length | Number of characters in run |
| `$179` | anchor_ref | Target anchor for links |
| `$616` | epub_type | `$617` for noteref |
| `$157` | style | Reference to a style fragment |
| `$429` | inline_style | Inline style properties |
| `$125` | dropcap_lines | Number of lines for drop cap |
| `$758` | ruby_id | Ruby annotation reference |
| `$759` | ruby_list | List of ruby annotations |

**Drop Cap Example:**
```
{
  $143: 0,           // Start at first character
  $144: 1,           // Affect one character
  $125: 3,           // Span 3 lines
  $173: "dropcap_style"
}
```

### 6.5 Text Content ($145)

Text is stored in separate fragments for efficiency. Each text_content fragment can contain multiple text chunks:

```
text_content_fragment = {
  name: "text_id",           // Fragment ID (string field, not symbol)
  $146: [                    // Children - list of text strings
    "First paragraph text...",
    "Second paragraph text...",
    "Third paragraph text...",
    ""                        // Empty string as terminator
  ]
}
```

**Maximum fragment size**: 8192 bytes (not counting the final empty string)

Text is referenced from storylines using offset:
```
{
  $145: {
    name: "text_id",    // References the text_content fragment
    $403: chunk_index   // Index into $146 array (0-based)
  }
}
```

**Text reference structure**:
- `name`: String referencing the text_content fragment's `name` field
- `$403` (TEXT_OFFSET): Integer index into the `$146` array

**Chunking behavior**:
- Text is split across chunks when it exceeds the maximum size
- Each storyline content item references a specific chunk by index
- The chunk index increments as content progresses through the book

### 6.5 Inline Style Runs ($142)

For inline formatting within text:

```
$142: [
  {
    $143: offset,           // Character offset
    $144: length,           // Character count
    $157: inline_style,     // Style reference
    $179: anchor_ref,       // Link destination (optional)
  },
  ...
]
```

### 6.6 Page Layouts ($156)

| Symbol | Name | Description |
|--------|------|-------------|
| `$322` | HORIZONTAL | Horizontal flow |
| `$323` | VERTICAL_BLOCK | Vertical block layout |
| `$324` | FIXED | Fixed positioning |
| `$325` | FULL_PAGE_IMAGE | Full-page image |
| `$326` | SCALE_FIT | Scale to fit |
| `$437` | PAGE_SPREAD | Two-page spread |
| `$438` | FACING_PAGE | Facing pages |

---

## 7. Style System

### 7.1 Style Fragment Structure

```
style_fragment ($157) = {
  $173: style_name,         // Self-reference
  $583: base_style,         // Inheritance reference

  // Typography
  $11: font_family,         // String or symbol
  $12: font_style,          // $350=normal, $382=italic
  $13: font_weight,         // $350=normal, $361=bold, etc.
  $15: font_stretch,        // $350=normal, $365=condensed
  $16: font_size,           // Value struct or symbol
  $42: line_height,         // Value struct or symbol

  // Text formatting
  $23: text_decoration_underline,
  $27: text_decoration_strikethrough,
  $34: text_align,          // $320=center, $321=justify, etc.
  $35: text_align_last,
  $36: text_indent,
  $41: text_transform,

  // Spacing
  $46: margin,
  $47: margin_top,
  $48: margin_left,
  $49: margin_bottom,
  $50: margin_right,
  $51: padding,
  $52: padding_top,
  $53: padding_left,
  $54: padding_bottom,
  $55: padding_right,

  // Dimensions
  $56: width,
  $57: height,
  $62: min_height,
  $63: min_width,
  $64: max_height,
  $65: max_width,

  // Positioning
  $58: top,
  $59: left,
  $60: bottom,
  $61: right,
  $183: position,

  // Borders
  $83: border_color,
  $88: border_style,
  $93: border_width,
  // Individual sides: $84-$87, $89-$92, $94-$97

  // Background
  $21: background_color,
  $479: background_image,

  // Display
  $68: visibility,
  $127: display/block_type,
  $140: float,
  $476: overflow,

  // Tables
  $148: colspan,
  $149: rowspan,
  $150: border_collapse,
  $633: vertical_align,

  // Images
  $546: box_sizing/image_fit,
  $580: box_align/image_layout,

  // Other
  $10: language,
  $560: writing_mode,
  $761: layout_hints,
  $790: heading_level,
}
```

### 7.2 Value Encoding

Dimensional values use a struct format:

```
{
  $306: unit_symbol,    // UNIT field
  $307: decimal_value   // VALUE field
}
```

### 7.3 Unit Symbols ($306)

Complete mapping of unit symbols to CSS units:

| Symbol | CSS Unit | Notes |
|--------|----------|-------|
| `$308` | em | Relative to font-size |
| `$309` | ex | Relative to x-height |
| `$310` | lh | Line-height multiplier |
| `$311` | vw | Viewport width |
| `$312` | vh | Viewport height |
| `$313` | vmin | Viewport minimum |
| `$314` | % | Percentage |
| `$315` | cm | Centimeters |
| `$316` | mm | Millimeters |
| `$317` | in | Inches |
| `$318` | pt | Points (1pt = 1/72 inch) |
| `$319` | px | Pixels |
| `$505` | rem | Root em |
| `$506` | ch | Character width |
| `$507` | vmax | Viewport maximum |

**Note**: When converting from KFX to CSS, values in `pt` units where `magnitude * 1000 % 225 == 0` are often originally `px` values (converted as `px = pt * 1000 / 450`)

### 7.4 Common Value Symbols

Direct symbols for common values:

| Symbol | Meaning | Context |
|--------|---------|---------|
| `$320` | center | text-align, box-align |
| `$321` | justify | text-align |
| `$322` | horizontal | layout |
| `$323` | vertical-block | layout |
| `$324` | absolute/fixed | position |
| `$328` | solid | border-style |
| `$329` | double | border-style |
| `$330` | dashed | border-style |
| `$331` | dotted | border-style |
| `$334` | groove | border-style |
| `$335` | ridge | border-style |
| `$336` | inset | border-style/box-shadow |
| `$337` | outset | border-style |
| `$349` | none | general |
| `$350` | normal | font-style, font-weight, etc. |
| `$352` | always | page-break |
| `$353` | avoid | page-break |
| `$361` | bold | font-weight |
| `$369` | small-caps | font-variant |
| `$372` | uppercase | text-transform |
| `$373` | lowercase | text-transform |
| `$374` | capitalize | text-transform |
| `$375` | rtl | direction |
| `$376` | ltr | direction |
| `$377` | content-box | box-sizing, background-origin |
| `$378` | border-box | box-sizing, background-clip |
| `$379` | padding-box | box-sizing, background-origin |
| `$381` | oblique | font-style |
| `$382` | italic | font-style |
| `$383` | auto | various |
| `$384` | manual | hyphens |
| `$488` | relative | position |
| `$489` | fixed | position |

### 7.5 Border Styles

| Symbol | CSS border-style |
|--------|------------------|
| `$349` | none |
| `$328` | solid |
| `$329` | double |
| `$330` | dashed |
| `$331` | dotted |
| `$334` | groove |
| `$335` | ridge |
| `$336` | inset |
| `$337` | outset |

### 7.6 Position Values ($183)

| Symbol | CSS position |
|--------|--------------|
| `$324` | absolute |
| `$455` | oeb-page-foot |
| `$151` | oeb-page-head |
| `$488` | relative |
| `$489` | fixed |

### 7.7 Background Properties

| Symbol | CSS Property |
|--------|--------------|
| `$479` | background-image |
| `$480` | background-position-x |
| `$481` | background-position-y |
| `$482` | background-size-x |
| `$483` | background-size-y |
| `$484` | background-repeat |
| `$547` | background-origin |
| `$73` | background-clip |

**background-repeat ($484) values:**
- `$487` = no-repeat
- `$485` = repeat-x
- `$486` = repeat-y

**background-origin/clip values:**
- `$377` = content-box
- `$378` = border-box
- `$379` = padding-box

### 7.8 Color Encoding

Colors are encoded as ARGB integers (32-bit):
```
color = (alpha << 24) | (red << 16) | (green << 8) | blue
```

| Hex Value | Color |
|-----------|-------|
| `0xff000000` | Opaque black |
| `0xffffffff` | Opaque white |
| `0x00000000` | Transparent |
| `0xff0000ff` | Opaque blue |
| `0x80ff0000` | 50% transparent red |

**Alpha mask**: `0xff000000`

**Color properties** (use ARGB encoding):
- `$19` - color (text color)
- `$21` - background-color
- `$83`-`$87` - border colors
- `$105` - outline-color
- `$116` - column-rule-color
- `$24`, `$28` - text-decoration-color
- `$75` - text-stroke-color
- `$70` - fill-color
- `$555` - overline text-decoration-color
- `$718` - text-emphasis-color

### 7.9 Font Weight Symbols ($13)

| Symbol | CSS Weight | Name |
|--------|------------|------|
| `$350` | 400 | normal |
| `$355` | 100 | thin |
| `$356` | 200 | extra-light |
| `$357` | 300 | light |
| `$359` | 500 | medium |
| `$360` | 600 | semi-bold |
| `$361` | 700 | bold |
| `$362` | 800 | extra-bold |
| `$363` | 900 | black |

### 7.10 Font Style Symbols ($12)

| Symbol | CSS font-style |
|--------|----------------|
| `$350` | normal |
| `$382` | italic |
| `$381` | oblique |

### 7.11 Font Stretch Symbols ($15)

| Symbol | CSS font-stretch |
|--------|------------------|
| `$350` | normal |
| `$365` | condensed |
| `$366` | semi-condensed |
| `$367` | semi-expanded |
| `$368` | expanded |

### 7.12 Style Inheritance

Styles can inherit via `$583` (base_style):
```
child_style = {
  $173: "child",
  $583: "parent_style",  // Inherit from parent
  $34: $321,             // Override text-align
}
```

**Note**: $583 has dual meaning depending on context:
- As symbol value: font-variant (`$349`=normal, `$369`=small-caps)
- As string referencing another style: base style for inheritance

### 7.13 Heritable Properties

These CSS properties are inherited by child elements (with their default values):

| Property | Default | KFX Symbol |
|----------|---------|------------|
| color | (inherited) | $19 |
| direction | ltr | $192, $682 |
| font-family | serif | $11 |
| font-size | 1rem | $16 |
| font-stretch | normal | $15 |
| font-style | normal | $12 |
| font-weight | normal | $13 |
| letter-spacing | normal | $32 |
| line-break | auto | $780 |
| line-height | normal | $42 |
| list-style-type | disc | $100 |
| orphans | 2 | (via $785) |
| text-align | (inherited) | $34 |
| text-align-last | auto | $35 |
| text-indent | 0 | $36 |
| text-transform | none | $41 |
| visibility | visible | $68 |
| white-space | normal | $45 |
| widows | 2 | (via $785) |
| word-break | normal | $569 |
| word-spacing | normal | $33 |
| writing-mode | horizontal-tb | $560 |

### 7.14 Non-Heritable Property Defaults

| Property | Default | KFX Symbol |
|----------|---------|------------|
| background-color | transparent | $21 |
| box-sizing | content-box | $546 |
| float | none | $140 |
| margin-* | 0 | $46-$50 |
| overflow | visible | $476 |
| padding-* | 0 | $51-$55 |
| page-break-* | auto | $133-$135 |
| position | static | $183 |
| text-decoration | none | $23, $27, $554 |
| vertical-align | baseline | $44 |

### 7.15 Ruby Properties

Ruby annotations (furigana) for CJK text use these properties:

| Symbol | CSS Property | Values |
|--------|--------------|--------|
| `$762` | ruby-position (horizontal) | `$58`=over, `$60`=under |
| `$763` | ruby-position (vertical) | `$59`=under, `$61`=over |
| `$764` | ruby-merge | `$772`=collapse, `$771`=separate |
| `$765` | ruby-align | `$320`=center, `$773`=space-around, `$774`=space-between, `$680`=start |
| `$766` | ruby-align (alt) | Same values as $765 |

### 7.16 Text Emphasis Properties

Text emphasis marks (used in CJK typography):

| Symbol | CSS Property | Values |
|--------|--------------|--------|
| `$717` | text-emphasis-style | See below |
| `$718` | text-emphasis-color | ARGB integer |
| `$719` | -kfx-text-emphasis-position-horizontal | `$58`=over, `$60`=under |
| `$720` | -kfx-text-emphasis-position-vertical | `$59`=left, `$61`=right |

**text-emphasis-style ($717) values:**

| Symbol | Value |
|--------|-------|
| `$724` | filled |
| `$725` | open |
| `$726` | filled dot |
| `$727` | open dot |
| `$728` | filled circle |
| `$729` | open circle |
| `$730` | filled double-circle |
| `$731` | open double-circle |
| `$732` | filled triangle |
| `$733` | open triangle |
| `$734` | filled sesame |
| `$735` | open sesame |

### 7.17 Direction and Bidi Properties

| Symbol | CSS Property | Values |
|--------|--------------|--------|
| `$192` | direction | `$376`=ltr, `$375`=rtl |
| `$682` | direction (alt) | `$376`=ltr, `$375`=rtl |
| `$674` | unicode-bidi | `$675`=embed, `$676`=isolate, `$678`=isolate-override, `$350`=normal, `$677`=bidi-override, `$679`=plaintext |

### 7.18 Line Break and Word Break

| Symbol | CSS Property | Values |
|--------|--------------|--------|
| `$780` | line-break | `$783`=anywhere, `$383`=auto, `$781`=loose, `$350`=normal, `$782`=strict |
| `$569` | word-break | `$570`=break-all, `$350`=normal |

### 7.19 Text Orientation (Vertical Writing)

| Symbol | CSS Property | Values |
|--------|--------------|--------|
| `$706` | text-orientation | `$383`=mixed, `$778`=sideways, `$779`=upright |
| `$707` | text-combine-upright | `$573`=all |

### 7.20 Modern vs Legacy Page Break Symbols

KFX uses two sets of symbols for page-break properties:

**Legacy symbols** (used by kfxinput):
| Symbol | Property |
|--------|----------|
| `$133` | page-break-after |
| `$134` | page-break-before |
| `$135` | page-break-inside |

**Modern symbols** (also valid):
| Symbol | Property |
|--------|----------|
| `$788` | page-break-after |
| `$789` | page-break-before |

Both sets use the same values: `$352`=always, `$383`=auto, `$353`=avoid

### 7.21 Special Value Constants

**Line Height Defaults**:
- Normal line-height (`$383`) in KFX corresponds to approximately 1.2em
- `LINE_HEIGHT_SCALE_FACTOR = 1.2`
- `MINIMUM_LINE_HEIGHT = 1.0` (as multiplier)

**Default Document Properties**:
- `DEFAULT_DOCUMENT_FONT_FAMILY = "serif"`
- `DEFAULT_DOCUMENT_LINE_HEIGHT = "normal"` (or "1.2em")
- `DEFAULT_DOCUMENT_FONT_SIZE = "1em"`

**Pixel to Percent Conversion**:
- `PX_PER_PERCENT = 8.534`
- 100% ≈ 853.4px

**Points to Pixels Conversion**:
When reading KFX, if a `pt` value has `magnitude * 1000 % 225 == 0`, it may have been converted from pixels:
```
original_px = pt_value * 1000 / 450
```

---

## 8. Resources

### 8.1 External Resource ($164)

```
resource_fragment = {
  $175: resource_name,      // Self-reference
  $161: format,             // Format symbol
  $162: mime_type,          // MIME string (optional)
  $165: location,           // Raw media reference
  $422: width,              // Pixel width
  $423: height,             // Pixel height
}
```

### 8.2 Format Symbols ($161)

| Symbol | Format |
|--------|--------|
| `$284` | PNG |
| `$285` | JPEG |
| `$286` | GIF |
| `$287` | Plugin object |
| `$548` | JPEG-XR |
| `$565` | PDF |
| `$599` | BMP |
| `$600` | TIFF |
| `$612` | BPG |

### 8.3 Font Format Symbol

| Symbol | Format |
|--------|--------|
| `$418` | Font (TTF/OTF/WOFF) |

### 8.4 Raw Media ($417)

Raw media fragments store binary data directly (not Ion-encoded):
- Fragment ID matches `$165` location from resource
- Content is raw bytes (JPEG, PNG, font data, etc.)

### 8.5 Image in Content

Images appear in storylines as:
```
{
  $159: $271,                // IMAGE_CONTENT type
  $155: element_id,
  $157: image_style,
  $175: resource_reference,  // Points to $164 fragment
  $584: alt_text,            // Accessibility text
}
```

### 8.6 Container Entity Map ($419)

The container entity map tracks all entities and their dependencies:

```
container_entity_map = {
  $252: [                           // Container contents list
    {
      $155: container_id,           // Container identifier
      $181: [fragment_id, ...]      // Fragment IDs in this container
    },
    ...
  ],
  $253: [                           // Entity dependencies
    {
      $155: dependent_id,           // The fragment that has dependencies
      $254: [dependency_id, ...],   // Mandatory dependencies
      $255: [optional_id, ...]      // Optional dependencies (for fallbacks)
    },
    ...
  ]
}
```

**Dependency types**:
- `$254` (mandatory): Required for proper rendering (e.g., images for sections)
- `$255` (optional): Fallback resources that may not be present

**Common dependency relationships**:
- Section (`$260`) → Resources (`$164`) → Raw media (`$417`)
- This allows the Kindle to prefetch required resources before displaying a section

---

## 9. Navigation

### 9.1 Book Navigation ($389)

```
book_navigation = [
  {
    $178: reading_order_name,
    $392: [nav_container_references...]
  }
]
```

### 9.2 Navigation Container ($391)

```
nav_container = {
  $235: nav_type,           // Type symbol
  $239: container_name,
  $247: [nav_unit_references...]
}
```

### 9.3 Navigation Types ($235)

| Symbol | Type | Purpose |
|--------|------|---------|
| `$212` | TOC | Table of contents (main navigation) |
| `$213` | SECTION_TOC | Section-specific table of contents |
| `$214` | PAGE_LIST_TOC | Page list navigation |
| `$236` | LANDMARKS | Landmarks (cover, body, toc references) |
| `$237` | PAGE_LIST | Page number list |
| `$798` | HEADINGS | Heading-based navigation |

**Navigation Container Processing:**

kfxinput validates that nav_type is one of: `$212`, `$236`, `$237`, `$213`, `$214`, `$798`. Other values generate an error.

For `$212` (TOC) and `$798` (HEADINGS) types, nested hierarchical entries are supported via `$247` child units.

### 9.3.1 Section Navigation ($390)

Section-specific navigation links containers to sections:

```
section_navigation = {
  $174: section_name,        // Section reference
  $392: [nav_container_ids...] // Navigation containers for this section
}
```

### 9.4 Navigation Unit ($393)

```
nav_unit = {
  $240: unit_name,
  $241: representation,     // Label info
  $246: target_position,    // {$155: eid, $143: offset}
  $247: [child_units...],   // Nested entries
  $238: landmark_type,      // For landmarks
}
```

### 9.5 Landmark Types ($238)

| Symbol | Type | EPUB type |
|--------|------|-----------|
| `$233` | cover | cover |
| `$396` | text (body) | bodymatter |
| `$212` | toc | toc |

### 9.5.1 Popup Footnotes

KFX supports popup footnotes that display in an overlay window instead of navigating to the footnote location. This requires two components:

**1. Classification ($615)** - Marks the footnote content:

| Symbol | Classification | EPUB epub:type | Description |
|--------|----------------|----------------|-------------|
| `$618` | footnote | footnote | Inline footnote content |
| `$619` | endnote | endnote | End-of-chapter/book note |
| `$281` | footnote | footnote | Alternative footnote marker |
| `$688` | math | - | Mathematical content |
| `$689` | (unknown) | - | Internal use |
| `$453` | caption | - | Table caption |

**Detection from EPUB**: kfxinput detects footnotes via:
- `epub:type="footnote"` → `$618`
- `epub:type="endnote"` → `$619`
- `role="doc-footnote"` → `$618`
- `role="doc-endnote"` → `$619`

**2. Noteref Type ($616)** - Marks the link that triggers the popup:

| Symbol | epub:type |
|--------|-----------|
| `$617` | noteref |

**How Popup Footnotes Work:**

1. The footnote content container has `$615: $618` (or `$619` for endnote)
2. The link pointing to the footnote has `$616: $617` in its style events
3. When the Kindle reader detects a `noteref` link pointing to a `footnote`/`endnote` container, it displays the content in a popup overlay instead of navigating

**Style Event Structure for Noteref:**
```
{
  $142: [  // Inline style runs
    {
      $143: start_offset,   // Character offset where noteref starts
      $144: end_offset,     // Character offset where noteref ends
      $616: $617,           // Mark as noteref type
      $179: "anchor-id"     // Link target (anchor reference)
    }
  ]
}
```

**Footnote Container Structure:**
```
{
  $159: $269,              // BLOCK_CONTAINER
  $615: $618,              // Classification = footnote
  $146: [...],             // Content
  // ... other properties
}
```

### 9.5.2 EPUB Type Attributes ($649)

Additional EPUB semantic types for images:

| Symbol | epub:type | Description |
|--------|-----------|-------------|
| `$441` | amzn:not-decorative | Image is meaningful content |
| `$442` | amzn:decorative | Image is decorative (ignored by accessibility) |

### 9.6 Anchors ($266)

```
anchor_fragment = {
  $180: anchor_name,
  $183: {$155: target_eid, $143: offset},  // Position
  // OR
  $186: external_uri,                       // External link
}
```

### 9.7 Heading Levels ($790 for content)

| Symbol | Level |
|--------|-------|
| `$799` | h1 |
| `$800` | h2 |
| `$801` | h3 |
| `$802` | h4 |
| `$803` | h5 |
| `$804` | h6 |

Used in styles to indicate semantic heading level.

### 9.8 Layout Hints ($761)

Layout hints provide semantic information about content:

| Symbol | Meaning |
|--------|---------|
| `$282` | figure |
| `$453` | caption |
| `$760` | heading |

Layout hints are stored as a list of symbols in the `$761` property.

### 9.9 Content Role ($790)

Content role values used in storylines:

| Value | Meaning |
|-------|---------|
| 2 | First content item in section |
| 3 | Normal content item |

This helps readers identify the start of new sections.

---

## 10. Metadata

### 10.1 Metadata Fragment ($258)

```
metadata = {
  $153: title,
  $154: description,
  $222: author,
  $224: ASIN,
  $232: publisher,
  $219: issue_date,
  $10: language,
  $215: orientation,
  $217: support_portrait,
  $218: support_landscape,
  $251: cde_content_type,
  $424: cover_image,        // Resource reference
  $169: [reading_orders...],
}
```

### 10.2 Book Metadata ($490)

Extended metadata structure:
```
{
  $491: [category_metadata...]
}
```

Each category:
```
{
  $495: category_name,      // e.g., "kindle_title_metadata"
  $258: [
    {$492: key, $307: value},
    ...
  ]
}
```

Common keys:
- `author`, `title`, `ASIN`, `content_id`
- `cde_content_type` (EBOK, PDOC, MAGZ, etc.)
- `cover_image`, `description`, `language`, `publisher`

### 10.3 CDE Content Types

| Code | Type |
|------|------|
| EBOK | Standard ebook |
| PDOC | Personal document |
| MAGZ | Magazine |
| NEWS | Newspaper |
| EBSP | Sample |

### 10.4 Format Capabilities ($593)

```
format_capabilities = [
  {$492: feature_name, version: version_number},
  ...
]
```

Common features:
- `kfxgen.textBlock` - Text block support
- `kfxgen.positionMaps` - Position mapping
- `yj_hdv` - High-resolution images
- `yj_fixed_layout` - Fixed layout support

### 10.5 CDE Features ($585)

```
{
  $590: [
    {
      $586: namespace,      // e.g., "com.amazon.yjconversion"
      $492: feature_name,
      $589: {version: {$587: major, $588: minor}}
    },
    ...
  ]
}
```

### 10.6 Position Map ($264)

The position map tracks which EIDs (element IDs) belong to which sections, enabling efficient navigation:

```
position_map = [
  {
    $181: [eid1, eid2, ...],  // List of EIDs in this section
    $174: section_name        // Section reference
  },
  ...
]
```

**Key Fields:**
| Symbol | Name | Description |
|--------|------|-------------|
| `$181` | entity_ids | List of EID values in this section |
| `$174` | section_name | Reference to section fragment |

### 10.7 Position ID Map ($265)

Maps position IDs (PIDs) to EIDs for location tracking:

```
position_id_map = [
  {
    $184: pid,           // Position ID (cumulative character count)
    $185: eid,           // Element ID (symbol reference)
    $143: offset         // Optional: character offset within EID
  },
  ...
  { $184: max_pid, $185: 0 }  // Terminator entry
]
```

**Key Fields:**
| Symbol | Name | Description |
|--------|------|-------------|
| `$184` | position_id | Cumulative position (character count) |
| `$185` | eid | Target element ID |
| `$143` | offset | Optional character offset within the element |

The last entry has `$185: 0` as a terminator, with `$184` containing the total character count.

### 10.8 Location Map ($550)

The location map provides locations (page-like markers) throughout the book:

```
location_map = [
  {
    $182: [                  // Location entries list
      { $155: eid, $143: offset },
      ...
    ]
  }
]
```

**Key Fields:**
| Symbol | Name | Description |
|--------|------|-------------|
| `$182` | locations | List of location entries |
| `$155` | eid | Element ID at this location |
| `$143` | offset | Character offset within the element |

Each entry represents approximately one "location" (similar to a page number) in the book.

---

## 11. Symbol Reference

### 11.1 Structure Symbols

| Symbol | Name | Description |
|--------|------|-------------|
| `$141` | page_templates | Section page templates |
| `$142` | inline_style_runs | Inline formatting |
| `$143` | offset | Position offset |
| `$144` | length | Run length |
| `$145` | text_content | Text reference |
| `$146` | children | Child items |
| `$153` | title | Book title |
| `$154` | description | Description |
| `$155` | eid | Element ID |
| `$156` | layout | Page layout type |
| `$157` | style | Style reference |
| `$159` | content_type | Content type symbol |
| `$161` | format | Resource format |
| `$162` | mime_type | MIME type string |
| `$163` | target | Link target |
| `$164` | external_resource | Resource fragment |
| `$165` | location | Raw media location |
| `$169` | reading_orders | Reading order list |
| `$170` | sections | Section list |
| `$173` | style_name | Style self-reference |
| `$174` | section_name | Section self-reference |
| `$175` | resource_name | Resource reference |
| `$176` | story_name | Storyline reference |
| `$178` | reading_order_name | Reading order ID |
| `$179` | anchor_ref | Link anchor |
| `$180` | anchor_name | Anchor self-reference |
| `$181` | entity_ids | Container entities |
| `$183` | position | Position reference |

### 11.2 Style Property Symbols

| Symbol | CSS Property |
|--------|--------------|
| `$10` | xml:lang |
| `$11` | font-family |
| `$12` | font-style |
| `$13` | font-weight |
| `$15` | font-stretch |
| `$16` | font-size |
| `$19` | color |
| `$21` | background-color |
| `$23` | text-decoration (underline) |
| `$27` | text-decoration (strikethrough) |
| `$31` | baseline-shift |
| `$32` | letter-spacing |
| `$33` | word-spacing |
| `$34` | text-align |
| `$35` | text-align-last |
| `$36` | text-indent |
| `$41` | text-transform |
| `$42` | line-height |
| `$44` | baseline-style |
| `$45` | white-space |
| `$46` | margin |
| `$47` | margin-top |
| `$48` | margin-left |
| `$49` | margin-bottom |
| `$50` | margin-right |
| `$51` | padding |
| `$52` | padding-top |
| `$53` | padding-left |
| `$54` | padding-bottom |
| `$55` | padding-right |
| `$56` | width |
| `$57` | height |
| `$58` | top |
| `$59` | left |
| `$60` | bottom |
| `$61` | right |
| `$62` | min-height |
| `$63` | min-width |
| `$64` | max-height |
| `$65` | max-width |
| `$66` | fixed-width |
| `$67` | fixed-height |
| `$68` | visibility |
| `$83`-`$97` | border properties |
| `$98` | transform |
| `$100` | list-style-type |
| `$127` | display/hyphens |
| `$133` | page-break-after |
| `$134` | page-break-before |
| `$135` | page-break-inside |
| `$140` | float |
| `$148` | colspan |
| `$149` | rowspan |
| `$150` | border-collapse |
| `$183` | position |
| `$476` | overflow |
| `$546` | box-sizing |
| `$560` | writing-mode |
| `$577` | link-color |
| `$576` | visited-color |
| `$580` | box-align |
| `$583` | base-style |
| `$628` | clear |
| `$633` | vertical-align (table) |
| `$761` | layout-hints |
| `$790` | heading-level |

### 11.3 Container Symbols

| Symbol | Name |
|--------|------|
| `$270` | container |
| `$409` | container_id |
| `$410` | compression_type |
| `$411` | drm_scheme |
| `$412` | chunk_size |
| `$413` | index_table_offset |
| `$414` | index_table_length |
| `$415` | doc_symbol_offset |
| `$416` | doc_symbol_length |
| `$417` | raw_media |
| `$419` | container_entity_map |
| `$587` | kfxgen_application_version |
| `$588` | kfxgen_package_version |
| `$594` | format_capabilities_offset |
| `$595` | format_capabilities_length |

### 11.4 List Style Symbols ($100)

| Symbol | CSS list-style-type |
|--------|---------------------|
| `$340` | disc |
| `$341` | square |
| `$342` | circle |
| `$343` | decimal |
| `$344` | lower-roman |
| `$345` | upper-roman |
| `$346` | lower-alpha |
| `$347` | upper-alpha |
| `$349` | none |
| `$736` | cjk-ideographic |
| `$737` | cjk-earthly-branch |
| `$738` | cjk-heavenly-stem |
| `$739` | hiragana |
| `$740` | hiragana-iroha |
| `$741` | katakana |
| `$742` | katakana-iroha |
| `$743` | japanese-formal |
| `$744` | japanese-informal |
| `$745` | simp-chinese-informal |
| `$746` | simp-chinese-formal |
| `$747` | trad-chinese-informal |
| `$748` | trad-chinese-formal |
| `$791` | lower-greek |
| `$792` | upper-greek |
| `$793` | lower-armenian |
| `$794` | upper-armenian |
| `$795` | georgian |
| `$796` | decimal-leading-zero |

**List Type to HTML Element Mapping:**

kfxinput maps list-style-type symbols to HTML list elements:

| Symbol | HTML Element |
|--------|--------------|
| `$340` (disc) | ul |
| `$341` (square) | ul |
| `$342` (circle) | ul |
| `$349` (none) | ul |
| `$271` (image) | ul |
| `$343` (decimal) | ol |
| `$344` (lower-roman) | ol |
| `$345` (upper-roman) | ol |
| `$346` (lower-alpha) | ol |
| `$347` (upper-alpha) | ol |

All other list types default to `ol` (ordered list).

### 11.4.1 Text Decoration Properties

**Underline ($23)**:
| Symbol | Style |
|--------|-------|
| `$328` | underline |
| `$329` | underline double |
| `$330` | underline dashed |
| `$331` | underline dotted |
| `$349` | none |

**Strikethrough ($27)**:
| Symbol | Style |
|--------|-------|
| `$328` | line-through |
| `$329` | line-through double |
| `$330` | line-through dashed |
| `$331` | line-through dotted |
| `$349` | none |

**Overline ($554)**:
| Symbol | Style |
|--------|-------|
| `$328` | overline |
| `$329` | overline double |
| `$330` | overline dashed |
| `$331` | overline dotted |
| `$349` | none |

### 11.5 Text Alignment Symbols ($34)

| Symbol | CSS text-align |
|--------|----------------|
| `$59` | left |
| `$61` | right |
| `$320` | center |
| `$321` | justify |

### 11.6 Border Style Symbols ($88-$92)

| Symbol | CSS border-style |
|--------|------------------|
| `$328` | solid |
| `$329` | double |
| `$330` | dashed |
| `$331` | dotted |
| `$334` | groove |
| `$335` | ridge |
| `$336` | inset |
| `$337` | outset |
| `$349` | none |

### 11.7 Writing Mode Symbols ($560)

| Symbol | CSS writing-mode |
|--------|------------------|
| `$557` | horizontal-tb |
| `$558` | vertical-lr |
| `$559` | vertical-rl |

### 11.8 Hyphens Symbols ($127)

| Symbol | CSS hyphens |
|--------|-------------|
| `$383` | auto |
| `$384` | manual |
| `$349` | none |

### 11.9 Float Symbols ($140)

| Symbol | CSS float |
|--------|-----------|
| `$59` | left |
| `$61` | right |
| `$786` | snap-block |

### 11.10 Clear Symbols ($628)

| Symbol | CSS clear |
|--------|-----------|
| `$59` | left |
| `$61` | right |
| `$421` | both |
| `$349` | none |

### 11.11 Overflow Symbols ($476)

| Value | CSS overflow |
|-------|--------------|
| `false` | visible |
| `true` | hidden |

### 11.12 Visibility Symbols ($68)

| Value | CSS visibility |
|-------|----------------|
| `false` | hidden |
| `true` | visible |

---

## Appendix A: Binary Examples

### A.1 Simple Style Fragment

```
Style: margin-top: 1em, text-align: justify

Ion binary (hex):
D6                    // struct, 6 bytes
  87 C1 B0            // $47 (margin-top): struct
    86 C1 36          // $306 (unit): $308 (em)
    87 C1 01          // $307 (value): 1
  82 C1 41            // $34 (text-align): $321 (justify)
```

### A.2 Text Content Reference

```
Text reference: name="t1", offset=0

Ion binary:
D4                    // struct
  84 "t1"             // name: "t1"
  93 01 00            // $403: 0
```

### A.3 Content Item

```
Block container with text

D8                    // struct
  9F 01 0D            // $159: $269 (BLOCK_CONTAINER)
  9D 01 "s1"          // $157: "s1" (style)
  91 01 ...           // $145: text_content_ref
```

---

## Appendix B: Conversion Notes

### B.1 EPUB to KFX Mapping

| EPUB | KFX |
|------|-----|
| spine | reading_order ($169) |
| manifest item | external_resource ($164) |
| nav doc | book_navigation ($389) |
| CSS | style fragments ($157) |
| HTML content | storylines ($259) |
| metadata | book_metadata ($490) |

### B.2 CSS to KFX Property Mapping

Most CSS properties map 1:1 to KFX symbols. Key differences:
- KFX uses symbol IDs instead of property names
- Values may use value symbols instead of strings
- Units are encoded in value structs
- Some properties have different inheritance behavior

### B.3 Kindle-Specific Features

KFX supports features not in EPUB:
- Region magnification (comics)
- Enhanced typesetting
- X-Ray character/location detection
- Word-wise (reading aid)
- Vocabulary builder integration
- Page flip view

---

## References

- kfxlib (calibre KFX Output plugin) - Primary reference implementation
- Amazon Ion specification - Binary format documentation
- KPF format - Intermediate Kindle publishing format
