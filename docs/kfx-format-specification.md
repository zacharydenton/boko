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

KFX books consist of multiple containers:

| Type | Fragment IDs | Purpose |
|------|--------------|---------|
| KFX-main | 259, 260, 538 | Book content |
| KFX-metadata | 258, 419, 490, 585 | Metadata, symbols |
| KFX-attachable | 417 | Resources (images, fonts) |

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

Amazon Ion is a richly-typed, self-describing binary format. KFX uses a subset of Ion.

### 3.1 Ion Type Codes

```
Type    Code  Description
------  ----  -----------
null    0x0   Null value
bool    0x1   Boolean
posint  0x2   Positive integer
negint  0x3   Negative integer
float   0x4   IEEE 754 float
decimal 0x5   Arbitrary-precision decimal
timestamp 0x6 Date/time
symbol  0x7   Symbol reference
string  0x8   UTF-8 string
clob    0x9   Character LOB
blob    0xA   Binary LOB
list    0xB   Ordered collection
sexp    0xC   S-expression
struct  0xD   Unordered key-value pairs
annotation 0xE Annotated value
```

### 3.2 Ion Binary Structure

Each value is encoded as:
```
[type nibble][length nibble][optional length bytes][value bytes]
```

For structs, keys are symbol IDs (VarUInt), values are Ion values.

### 3.3 Decimal Encoding

KFX decimals use Ion's decimal type with custom precision:

```
Encoded as: coefficient * 10^exponent

Structure:
  - VarInt exponent (negative for fractional)
  - VarInt coefficient

Example: 0.833333
  exponent: -6
  coefficient: 833333
```

### 3.4 Symbol References

Symbols are referenced by numeric ID in binary:
```
0xE7 0x81 0x83  → Symbol $131 (YJ_symbols shared symbol)
```

---

## 4. Symbol Tables

### 4.1 System Symbol Table ($ion)

The base Ion symbol table (IDs 1-9):

| ID | Symbol |
|----|--------|
| 1  | `$ion` |
| 2  | `$ion_1_0` |
| 3  | `$ion_symbol_table` |
| 4  | `name` |
| 5  | `version` |
| 6  | `imports` |
| 7  | `symbols` |
| 8  | `max_id` |
| 9  | `$ion_shared_symbol_table` |

### 4.2 YJ_symbols Shared Table

Amazon's shared symbol table for KFX (version 10):
- Name: "YJ_symbols"
- IDs: $10 through ~$851
- Contains all standard KFX property and value symbols

Symbols ending with `?` in the catalog are deprecated/unknown.

### 4.3 Local Symbols

Book-specific symbols are added after shared symbols:
- Style names (e.g., "style_0", "V_1_0_PARA...")
- Section IDs
- Resource IDs
- Anchor names

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

| Symbol | Name | HTML Equivalent |
|--------|------|-----------------|
| `$269` | BLOCK_CONTAINER | div, p, blockquote |
| `$270` | PAGE_TEMPLATE | page/section container |
| `$271` | IMAGE | img |
| `$272` | PLUGIN | embedded object |
| `$274` | SVG | svg |
| `$276` | LIST | ul, ol |
| `$277` | LIST_ITEM | li |
| `$278` | TABLE | table |
| `$279` | TABLE_ROW | tr |
| `$439` | HIDDEN_CONTAINER | display:none |
| `$454` | TABLE_BODY | tbody |
| `$151` | TABLE_HEADER | thead |
| `$455` | TABLE_FOOTER | tfoot |
| `$596` | HORIZONTAL_RULE | hr |

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

### 6.4 Text Content ($145)

Text is stored in separate fragments for efficiency:

```
text_content_fragment = {
  name: "text_id",
  $146: ["chunk1", "chunk2", "chunk3", ...]  // Text chunks
}
```

Referenced from storylines:
```
{
  $145: {name: "text_id", $403: chunk_index}
}
```

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

| Symbol | CSS Unit |
|--------|----------|
| `$308` | em |
| `$309` | ex |
| `$310` | lh (line-height multiplier) |
| `$311` | vw |
| `$312` | vh |
| `$313` | vmin |
| `$314` | % (percent) |
| `$315` | cm |
| `$316` | mm |
| `$317` | in |
| `$318` | pt |
| `$319` | px |
| `$505` | rem |
| `$506` | ch |
| `$507` | vmax |

### 7.4 Common Value Symbols

Direct symbols for common values:

| Symbol | Meaning |
|--------|---------|
| `$310` | 0 / zero / none |
| `$320` | center (text-align) |
| `$321` | justify |
| `$322` | left (in some contexts) |
| `$323` | vertical-block |
| `$328` | solid (border-style) |
| `$349` | none |
| `$350` | normal (font-style, etc.) |
| `$361` | bold |
| `$369` | default-font |
| `$376` | ltr (direction) |
| `$375` | rtl (direction) |
| `$377` | contain (image-fit) |
| `$378` | border-box |
| `$379` | padding-box |
| `$382` | italic |
| `$383` | auto |

### 7.5 Color Encoding

Colors use ARGB integers:
```
color = (alpha << 24) | (red << 16) | (green << 8) | blue
```

Common: `0xff000000` = opaque black, `0xffffffff` = opaque white

### 7.6 Font Weight Symbols ($13)

| Symbol | Weight |
|--------|--------|
| `$355` | 100 |
| `$356` | 200 |
| `$357` | 300 |
| `$359` | 500 |
| `$360` | 600 |
| `$361` | bold/700 |
| `$362` | 800 |
| `$363` | 900 |
| `$350` | normal/400 |

### 7.7 Style Inheritance

Styles can inherit via `$583` (base_style):
```
child_style = {
  $173: "child",
  $583: "parent_style",  // Inherit from parent
  $34: $321,             // Override text-align
}
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

### 8.6 Resource to Media Mapping

The container entity map (`$419`) tracks dependencies:
```
{
  $252: [container_contents...],
  $253: [entity_dependencies...],
  $254: [mandatory_dependencies...],
}
```

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
| `$212` | TOC | Table of contents |
| `$213` | SECTION_TOC | Section navigation |
| `$214` | PAGE_LIST_TOC | Page list |
| `$236` | LANDMARKS | Landmarks (cover, etc.) |
| `$237` | PAGE_LIST | Page numbers |
| `$798` | HEADINGS | Heading navigation |

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

| Symbol | Type |
|--------|------|
| `$233` | cover |
| `$396` | text (body) |
| `$212` | toc |

### 9.6 Anchors ($266)

```
anchor_fragment = {
  $180: anchor_name,
  $183: {$155: target_eid, $143: offset},  // Position
  // OR
  $186: external_uri,                       // External link
}
```

### 9.7 Heading Levels ($238 for nav, $790 for content)

| Symbol | Level |
|--------|-------|
| `$799` | h1 |
| `$800` | h2 |
| `$801` | h3 |
| `$802` | h4 |
| `$803` | h5 |
| `$804` | h6 |

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
