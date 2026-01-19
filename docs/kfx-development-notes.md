# KFX Development Notes

## Debugging Tools

### kfxlib (Recommended)
Use `kfxlib` from calibre's KFX Output plugin for accurate parsing. The simple `calibre.ebooks.metadata.kfx.Container` does NOT support decimal values (returns None for them).

```python
import sys
sys.path.insert(0, '/tmp/kfx_output')
from kfxlib.ion_binary import IonBinary
from kfxlib.ion_symbol_table import LocalSymbolTable
from kfxlib.utilities import file_read_binary
import struct

data = file_read_binary('file.kfx')

# Parse container header
version = struct.unpack_from('<H', data, 4)[0]
header_len = struct.unpack_from('<L', data, 6)[0]

# Read entity table
pos = 18
entities = []
while pos + 24 <= len(data) and data[pos:pos+4] != b'\xe0\x01\x00\xea':
    entity_id, entity_type, entity_offset, entity_len = struct.unpack_from('<LLQQ', data, pos)
    entity_start = header_len + entity_offset
    entities.append({'id': entity_id, 'type': entity_type, 'offset': entity_start, 'len': entity_len})
    pos += 24

# Parse entities
symtab = LocalSymbolTable()
ion = IonBinary(symtab)

for entity in entities:
    if entity['type'] == 157:  # STYLE
        ent_data = data[entity['offset']:entity['offset']+entity['len']]
        ent_header_len = struct.unpack_from('<L', ent_data, 6)[0]
        ent_ion = ent_data[ent_header_len:]
        values = ion.deserialize_multiple_values(ent_ion, import_symbols=True)
        for value in values:
            val = value.value if hasattr(value, 'value') else value
            # val is now properly decoded with Decimal values
```

**WARNING**: `calibre.ebooks.metadata.kfx.Container` returns `None` for decimal values because it doesn't implement decimal parsing. Always use `kfxlib` for accurate results.

### KFX Output Plugin
Located at `/tmp/kfx_output/kfxlib/` - this is calibre's KFX Output plugin source.

Key files:
- `yj_symbol_catalog.py` - List of all KFX symbols ($1 through $600+)
- `yj_structure.py` - KFX structure definitions
- `yj_book.py` - Book handling
- `kpf_book.py` - KPF (Kindle Package Format) book creation
- `ion_binary.py` - ION binary encoding/decoding
- `resources.py` - Image format symbols, MIME types

## KFX Symbol Reference

### Entity Types (Fragment Types)
- `$145` (P145) - Text content fragments
- `$157` (P157) - Style fragments
- `$164` (P164) - Resource fragments
- `$258` (P258) - Book metadata
- `$259` (P259) - Content blocks (paragraphs)
- `$260` (P260) - Sections
- `$264` (P264) - Book data
- `$265` (P265) - Book data
- `$266` (P266) - Page templates
- `$389` (P389) - Book navigation
- `$395` (P395) - Reading orders
- `$417` (P417) - Raw media (images, fonts)
- `$490` (P490) - Metadata
- `$538` (P538) - Auxiliary data
- `$585` (P585) - Format capabilities
- `$597` (P597) - Section content references

### Style Properties
- `$12` (P12) - font_size
- `$13` (P13) - line_height
- `$16` (P16) - text_indent
- `$34` (P34) - text_align
- `$36` (P36) - margin_top
- `$42` (P42) - margin_bottom
- `$44` (P44) - font_family
- `$45` (P45) - bold (boolean)
- `$46` (P46) - italic (boolean)
- `$47` (P47) - margin_left
- `$48` (P48) - margin_right
- `$173` (P173) - style_name (self-reference)
- `$583` (P583) - base style reference (inheritance)

### Value Structure Fields
- `$306` (P306) - unit field in value struct
- `$307` (P307) - value field in value struct

### Predefined Value Symbols
- `$310` (P310) - zero (0)
- `$314` (P314) - px unit
- `$308` (P308) - em unit
- `$350` (P350) - 100% (used for font_size, line_height)
- `$361` (P361) - 1em value
- `$505` (P505) - 1.5em value (commonly used for text-indent)
- `$382` (P382) - specific font size value

### Text Alignment Symbols
- `$320` (P320) - justify
- `$321` (P321) - center
- `$322` (P322) - left
- `$323` (P323) - right

### Font Family Symbols
- `$369` (P369) - default font
- `$370` (P370) - serif
- `$371` (P371) - serif bold
- `$372` (P372) - sans-serif
- `$373` (P373) - monospace

## KFX Value Encoding

### Value Structure
KFX uses a consistent structure for dimensional values:
```
{$306 (UNIT): unit_symbol, $307 (VALUE): decimal_value}
```

Example for text-indent 1.5em:
```
{$306: $505, $307: Decimal('1.5')}
```

### Unit Symbols
- `$310` - zero unit (value is multiplied by 0)
- `$308` - em unit
- `$314` - px unit
- `$505` - 1.5em unit (value is multiplied by 1.5em)

### Value Encoding
The reference KFX uses actual decimal values in P307, NOT null or special encodings:
- `Decimal('1')` - value of 1
- `Decimal('100')` - value of 100 (for 100% width)
- `Decimal('2.5')` - value of 2.5

**IMPORTANT**: Earlier versions of these notes mentioned a "null-like decimal" (0x80, 0x01).
This was based on incorrect interpretation from calibre's simple parser which returns None
for decimal types. Use actual decimal values instead.

### Direct Symbol Values
Some properties like font_size and line_height can use direct symbols:
```
P12: P350  (font_size: 100%)
P13: P350  (line_height: 100%)
```

## Reference KFX Comparison

When comparing generated KFX to reference:
1. Check entity type counts match
2. Check P145 (text content) count matches
3. Compare style fragments (P157) structure
4. Verify P259 content block structure
5. Check P260 section fields

Reference sections often include:
- P66: width (e.g., 1400)
- P67: height (e.g., 2100)
- P140: default text_align
- P156: page template reference
- P159: content type

## Common Issues

### Tiny Text
Usually caused by:
1. **Explicit default values instead of inheritance** - The reference KFX omits P12 (font_size) and
   P13 (line_height) on most styles, letting them inherit from Kindle defaults. Setting explicit
   P350 (100%) on every style can cause rendering issues.
2. Wrong P13 (line_height) - P310 is zero, not 100%
3. Missing base style inheritance (P583)

**Key insight**: The reference KFX has most styles with NO font_size/line_height, while only
specific styles (headings, special text) override these values. BOKO should follow this pattern:
- Only include P12/P13 when CSS explicitly specifies them
- Omit these fields otherwise to inherit from Kindle defaults

Reference distribution example:
- Font size: 41 styles with none, 14 with P382, 8 with P350
- Line height: 51 styles with none, 9 with P361, 3 with P350

### CSS Inheritance and Text Alignment
CSS `text-align` inherits from ancestors (not siblings). Elements inside `<hgroup>`, `<header>`,
or `<section class="epub-type-contains-word-colophon">` will inherit center alignment. This is
correct CSS behavior, not a bug.

### Style Parsing Errors
Use actual decimal values for P307. The simple calibre parser shows `None` for decimals
because it doesn't implement decimal parsing - use kfxlib instead.

### Entity Decoding Failures
Raw media entities (P417) don't contain ION data - they have raw image/font bytes.

## Image Styles

Image styles in KFX differ from text styles. They use specific properties for image display.

### Reference Image Style Example ($1139)
```
$1139 (titlepage image style):
  $10: 'en-us'                              # Language
  $127: $383                                # Block type (block display)
  $16: {$306: $505, $307: Decimal('1')}     # margin-top: 1 * 1.5em
  $42: {$306: $310, $307: Decimal('1')}     # margin-bottom: 1 * 0
  $47: {$306: $310, $307: Decimal('2.5')}   # margin-left: 2.5 * 0
  $546: $377                                # image fit (contain)
  $56: {$306: $314, $307: Decimal('100')}   # width: 100 (percent) in px unit
  $580: $320                                # image layout (justify)
```

### Image-Specific Properties
- `$127` - Block type: `$383` (block display mode)
- `$546` - Image fit: `$377` (contain)
- `$580` - Image layout: `$320` (justify)
- `$56` - Width: `{$306: $314 (px), $307: Decimal('100')}` for 100%

### Key Differences from Text Styles
1. Image styles have `$127: $383` (block type), NOT `$127: $349` (used for text blocks)
2. Image styles include `$546` (image fit) and `$580` (image layout)
3. Image styles typically do NOT have `$583` (base style) or font properties
4. Width is encoded as `Decimal('100')` for 100%, not pixel dimensions

## Base Style Inheritance (P583)

The reference KFX uses **P583** (base style reference) to implement style inheritance. Instead of
specifying all properties on every style, styles reference a base style that provides defaults.

### How It Works

1. A base style (typically P369 - the default font symbol) provides default font_size, line_height, etc.
2. Other styles include `P583: P369` to inherit from this base
3. Styles only override properties that differ from the base

### Reference Example

```
P1114 (body paragraph style):
  P583: P369          # Inherits from base style
  P10: en-gb          # Language
  P16: {P306: P505}   # text-indent: 1.5em
  P34: P320           # text-align: justify
  P42: {P306: P310}   # margin-bottom: 0
  P47: {P306: P310}   # margin-left: 0
  # No P12 (font_size) or P13 (line_height) - inherited from P369
```

### BOKO Approach

BOKO omits P12/P13 when not explicitly specified, letting Kindle use its defaults. This achieves
similar results without implementing full P583 support.

## Missing/Unused Symbols

### Symbols Used by Reference but Not BOKO

**Style Properties:**
- `$10` - language (e.g., "en-gb", "en-us")
- `$41` - font_family variant
- `$127` - unknown style property (value P349)
- `$135` - unknown style property (value P353)
- `$583` - base style reference (critical for inheritance)
- `$788` - unknown style property

**Structure/Layout:**
- `$66` - width (section/page width in pixels)
- `$67` - height (section/page height in pixels)
- `$140` - text content field
- `$156` - page template reference
- `$179`, `$180`, `$183` - page template related
- `$253` - entity dependencies
- `$254` - mandatory dependencies

**Value Symbols:**
- `$314` - px unit
- `$349`, `$353` - unknown value constants
- `$382` - specific font size value (larger than 100%)

**Navigation:**
- `$236` - nav type (landmarks variant)
- `$396` - nav unit list related
- `$798`, `$800`, `$801`, `$802` - navigation/TOC related

### Priority for Implementation

1. **High**: P583 (base style) - Would allow proper style inheritance
2. **Medium**: P66/P67 (dimensions) - May affect layout on some devices
3. **Low**: Navigation symbols - Current nav works, these are enhancements

## Symbol Verification

To verify symbols against the official catalog:

```python
import sys
sys.path.insert(0, '/tmp/kfx_output/kfxlib')
from yj_symbol_catalog import YJ_SYMBOLS

# Build lookup: symbol number -> name
for i, sym in enumerate(YJ_SYMBOLS.symbols):
    sym_num = i + 10  # YJ_SYMBOLS starts at $10
    is_valid = not sym.endswith('?')  # ? means unknown/deprecated
    print(f"${sym_num}: {sym} {'✓' if is_valid else '?'}")
```

All symbols in `YJ_SYMBOLS` (version 10) from $10 to ~$851 are shared symbols.
Symbols above that range are local (book-specific) symbols like style names, section IDs, etc.

## Image Handling

### Image Fragment Types

Images in KFX use three interconnected fragment types:

| Fragment | Symbol | Purpose | Key Fields |
|----------|--------|---------|------------|
| External Resource | $164 (P164) | Image metadata | $161 (format), $162 (MIME), $165 (location), $175 (name), $422/$423 (dimensions) |
| Raw Media | $417 (P417) | Binary image data | Fragment ID matches $165 location string |
| Content Block | $259 (P259) | References images via IMAGE_CONTENT | $146 array with $159=P271 entries |

### P164 Resource Structure

```
P164 Fragment (External Resource):
├─ Fragment ID: "image_001"
├─ $175 (name): "image_001"           # Self-reference
├─ $161 (format): "$285"              # Format symbol (JPEG)
├─ $162 (mime): "image/jpg"           # MIME type (optional for cover)
├─ $165 (location): "resource/rsrc0"  # Links to P417 fragment
├─ $422 (width): 1400                 # Width in pixels
└─ $423 (height): 2100                # Height in pixels
```

### P417 Raw Media

Raw media fragments store binary image data directly (not ION-encoded):
- Fragment ID matches the `$165` location string from P164
- Value is raw bytes (JPEG starts with FFD8FF, PNG starts with 89504E47)
- Entity header still has ION structure: `{$410:0, $411:0}`

### IMAGE_CONTENT Structure (P271)

IMAGE_CONTENT appears inside P259 content blocks under `$146` arrays:

```
P259 Content Block:
├─ $176: "content_id"
└─ $146: [
    {
      $155: 1234           # Position/EID
      $157: "style_id"     # Style reference
      $159: P271           # Content type = IMAGE_CONTENT
      $175: "resource_id"  # Reference to P164 resource
      $584: ""             # Alt text (optional, empty for cover)
    }
  ]
```

### Image Format Symbols

From `resources.py`:
```python
FORMAT_SYMBOLS = {
    "bmp": "$599",
    "gif": "$286",
    "jpg": "$285",    # Standard JPEG
    "jxr": "$548",    # JPEG-XR (HD Photo)
    "pbm": "$420",
    "pdf": "$565",    # PDF-backed images
    "png": "$284",
    "pobject": "$287", # Plugin object
    "tiff": "$600",
    "bpg": "$612",
}
```

### P253 Entity Dependencies

The P419 container entity map includes P253 which defines dependency chains for loading:

```
P253 Entry Structure:
{
  $155: "section_id",           # Entity ID
  $254: ["image_resource_id"]   # Mandatory dependencies
}
```

**Required dependency chains:**
1. Section ($260) → Image Resource ($164)
2. Image Resource ($164) → Raw Media ($417)

Example from reference:
```
P253 has 7 entries:
  [0]: P1059 -> ['P1088']      # Section -> Resource
  [1]: P1063 -> ['P1089']      # Section -> Resource
  [2]: P1069 -> ['P1090']      # Section -> Resource
  [3]: P1084 -> ['P1090']      # Section -> Resource
  [4]: P1088 -> ['P1104']      # Resource -> Raw Media
  [5]: P1089 -> ['P1102']      # Resource -> Raw Media
  [6]: P1090 -> ['P1103']      # Resource -> Raw Media
```

### Cover Section Structure

Cover sections have specific structure with P66/P67 dimensions:

```
P260 Cover Section:
├─ $174: "section_id"          # Self-reference
└─ $141: [                      # Section content
    {
      $155: 1234               # Position
      $176: "content_id"       # Reference to P259
      $66: 1400                # Width (cover only)
      $67: 2100                # Height (cover only)
      $156: P326               # Page layout (full page)
      $140: P320               # Default text-align
      $159: P270               # Content type = CONTAINER
    }
  ]
```

### Kindle Display Requirements

**JPEG Requirements:**
- Standard JPEG preferred for covers (JFIF format, starts with FFD8FFE0)
- MIME type should be `image/jpg` (not `image/jpeg`)
- RST markers may be required for certain devices

**Transparency:**
- Transparency only supported in magazines
- Regular books should not have transparent images

**Animation:**
- Animation only supported in WebP format
- Animated GIFs will cause warnings

**High-Resolution (HDV) Images:**
- Images > 1920px in either dimension are "HDV"
- Requires `yj_hdv` feature flag
- Tiled HDV images require `yj_hdv-2` feature

**Cover Lockscreen:**
- Cover must be standard JPEG (JFIF format) for Kindle lockscreen display
- JPEG/Exif, JPEG/SPIFF, JPEG/Adobe variants may not display correctly

### Ion Struct Key Ordering

**Critical**: Ion structs must have keys sorted in **ascending** order by symbol ID.
Calibre's decoder expects this order. Incorrect ordering causes parse failures.

```rust
// CORRECT: ascending order
let mut keys: Vec<_> = fields.keys().collect();
keys.sort();

// WRONG: descending order breaks parsing
keys.sort();
keys.reverse();  // DON'T DO THIS
```

### Debugging Image Issues

Check script to verify image structure:
```python
from calibre.ebooks.metadata.kfx import Container

with open('file.kfx', 'rb') as f:
    c = Container(f.read())

for e in c.decode():
    if e[0] == b'P164':  # Resources
        fid = e[1].decode()
        content = e[2]
        print(f'{fid}: format={content.get(b"P161")}, '
              f'location={content.get(b"P165")}, '
              f'dims={content.get(b"P422")}x{content.get(b"P423")}')

    if e[0] == b'P417':  # Raw media
        fid = e[1].decode()
        data = e[2]
        if hasattr(data, 'tobytes'):
            raw = data.tobytes()
        else:
            raw = data
        magic = raw[:4].hex() if len(raw) >= 4 else 'empty'
        print(f'{fid}: {len(raw)} bytes, magic={magic}')
```

Note: calibre's decode() returns base64-encoded data for display, but the actual
file contains raw bytes. Use `xxd` to verify raw file contents.

## Page Templates (P266)

Page templates are an important feature that BOKO does not currently implement.
Reference KFX files can have 200+ page templates while BOKO generates none.

### What Page Templates Do

Page templates (P266 fragments) provide layout information for the Kindle renderer:
- **P180**: Template ID (self-reference)
- **P183**: Content with position info `{P155: position_id, P143: offset}`

Example from reference:
```
P266 P965:
  P180: P965
  P183: {P155: 1208}

P266 P966:
  P180: P966
  P183: {P155: 1121, P143: 112}
```

### Impact of Missing Page Templates

The absence of P266 page templates may affect:
- Page break rendering
- Reading position tracking
- Virtual page calculations
- Possibly image display on some Kindle devices

### Landmarks Navigation

Reference KFX files include landmarks navigation (P235=P236) with a cover-nav-unit:

```
P389 (book navigation):
  P392: [
    ...
    {
      P235: P236           # Landmarks nav type
      P239: P1099          # Container reference
      P247: [
        {
          P238: P233       # Cover landmark type
          P241: {P244: 'cover-nav-unit'}
          P246: {P143: 0, P155: 1832}
        }
        ...
      ]
    }
  ]
```

BOKO currently only generates TOC navigation (P235=P212), not landmarks.

## CSS vs KFX Style Inheritance

### The Problem

KFX has its own style inheritance mechanism via `$583` (BASE_STYLE). When BOKO outputs fully-computed
CSS values (with all inherited properties), it conflicts with KFX's inheritance system, causing issues
like duplicate margin values or incorrect rendering.

### Reference KFX Style Approach

Reference KFX files use **minimal styles** - they only include properties that are directly specified,
not inherited values. For example:

**Colophon text style comparison:**
```
Reference ($1145):
  $36: {$306: $310, $307: 1}    # margin-top: 0
  (only one property!)

Generated (old approach):
  $12: {$306: $350, $307: 1}    # font-size: 100%
  $34: $321                      # text-align: center
  $36: {$306: $308, $307: 2}    # margin-top: 2em
  $42: {$306: $308, $307: 1}    # margin-bottom: 1em
  $44: $369                      # font-family: default
  $583: $369                     # base-style reference
```

The reference style only has `margin-top: 0` because that's the only direct CSS rule for that
element. All other properties (font-size, text-align, font-family) are inherited via KFX's
own `$583` (base style) mechanism.

### Titlepage Image Style Comparison

Another revealing example - the titlepage image style:

**Reference ($1139):**
```
$127: $383                       # display: block
$42: {$306: $314, $307: 1}      # margin-bottom: 1pt
$47: {$306: $314, $307: 2.5}    # margin-left: 2.5pt
$546: $377                       # image-fit: contain
$56: {$306: $314, $307: 100}    # width: 100%
$580: $320                       # image-layout: justify
```

**Key observation:** Reference has NO margin-top! The margin-bottom and margin-left are minimal
positioning adjustments. BOKO was incorrectly outputting margin-top from CSS inheritance.

### Solution: Direct Styles Only

BOKO now uses `get_direct_style_for_element()` which returns only CSS rules that directly match
an element, WITHOUT CSS inheritance. This produces minimal styles that work correctly with
KFX's own inheritance system.

```rust
// Get direct style (only rules matching this element, no CSS inheritance)
// KFX has its own style inheritance, so we only output direct styles
let element_ref = node.clone().into_element_ref().unwrap();
let direct_style = stylesheet.get_direct_style_for_element(&element_ref);
```

### Two Inheritance Systems

1. **CSS Inheritance** - Properties like font-size, text-align inherit from parent elements in DOM
2. **KFX Inheritance** - Styles reference a base style via `$583` (BASE_STYLE)

BOKO handles CSS inheritance internally for DOM traversal decisions (e.g., detecting hidden elements),
but outputs only direct styles to KFX. KFX then handles its own inheritance via `$583`.

### Summary

| Property | Old Approach | New Approach |
|----------|--------------|--------------|
| CSS inheritance | Included in output | Only used internally |
| Output styles | All computed values | Direct matches only |
| KFX $583 | Added but redundant | Works as intended |
| Style count | High (many properties) | Low (minimal) |

## Known Differences from Reference

### Fragment Counts

| Fragment Type | Reference | BOKO Generated |
|---------------|-----------|----------------|
| P266 (page templates) | 207 | 0 |
| P260 (sections) | 8 | 8 |
| P259 (content blocks) | 8 | 8 |
| P164 (resources) | 3 | 3 |
| P417 (raw media) | 3 | 3 |
| P157 (styles) | 63 | ~40 |

### Content Block Structure

Reference KFX uses **nested containers** with **multiple styles** for complex content like imprint/colophon
sections. BOKO uses a flat structure with fewer styles.

**Reference structure (imprint section):**
```
$259 Content Block:
├─ [0]: Container with nested $146 array
│   ├─ $146: [nested image content]
│   ├─ $157: '$1166'              # Header/container style
│   └─ $159: '$269' (CONTAINER)
├─ [1]: Paragraph
│   ├─ $142: [inline style runs]  # Links, emphasis
│   ├─ $145: {$4: text_id, $403: 0}
│   ├─ $157: '$1167'              # Paragraph style (width: 75%)
│   └─ $159: '$269'
├─ [2]: Paragraph
│   ├─ $157: '$1167'              # Same paragraph style
│   ...
```

**BOKO structure (flat):**
```
$259 Content Block:
├─ [0]: Image
│   ├─ $157: '$933'               # Single image style
│   └─ $159: '$271' (IMAGE)
├─ [1]: Text
│   ├─ $145: {$4: text_id, $403: 0}
│   ├─ $157: '$899'               # Single text style for all
│   └─ $159: '$269'
├─ [2]: Text
│   ├─ $157: '$899'               # Same style repeated
│   ...
```

**Key differences:**
1. Reference uses nested `$146` arrays for container hierarchies
2. Reference uses different styles for different element types (header vs paragraph)
3. Reference includes `$142` arrays for inline style runs (links, emphasis within text)
4. BOKO uses flat structure with single style per content type

**Impact:**
- Reference: 63 styles, complex nesting, inline formatting preserved
- BOKO: ~40 styles, flat structure, simpler but less precise formatting

This structural difference may affect rendering of complex layouts like centered blocks with
`margin: auto` or nested formatting.

### Missing Navigation Features

BOKO does not generate:
- Landmarks navigation (P235=P236)
- Cover nav unit (P238=P233)
- Heading nav units (P235=P798)

## Imprint Section Comparison (2025-01-19)

Detailed comparison of imprint.xhtml between reference KFX and BOKO-generated KFX.

### Original EPUB Structure

```html
<section class="epub-type-contains-word-imprint">
  <header>
    <h2>Imprint</h2>
    <img src="../images/logo.png" alt="The Standard Ebooks logo."/>
  </header>
  <p>This ebook is the product... <a href="...">Standard Ebooks</a>...</p>
  <p>This particular ebook... <a>Perseus</a>...<a>Internet Archive</a>...</p>
  <p>The source text... <a>CC0</a>...<a>Uncopyright</a>...</p>
  <p>Standard Ebooks is... <a>standardebooks.org</a>...</p>
</section>
```

### Reference KFX Structure ($1096)

```
$146: [
  [0]: Header container
    $146: [Image with alt text]
    $157: '$1166' (header style)

  [1-4]: Paragraph items (NOT containers, direct content)
    $142: [inline style runs for links]
      - $143: offset, $144: count
      - $157: link style, $179: anchor reference
    $145: {id: '$1071', index: 0-3}
    $157: '$1167' (paragraph style)
]
```

### Generated KFX Structure ($951)

```
$146: [
  [0]: Section container (EXTRA NESTING)
    $146: [
      [0]: Header container
        $146: [Image, NO alt text]
        $157: '$926'

      [1-4]: Paragraph CONTAINERS (extra nesting)
        $146: [
          [0]: Text reference
            $145: {id: '$941', index: 0-3}
            $157: '$916' (single style)
            // NO $142 inline runs
        ]
        $157: '$892'
    ]
]
```

### Key Differences

| Aspect | Reference | Generated | Issue |
|--------|-----------|-----------|-------|
| Nesting depth | 1 level | 2-3 levels | Over-nesting from block elements |
| Inline runs ($142) | Yes, for links | None | Links not tracked |
| Alt text ($584) | 'The Standard Ebooks logo.' | '' (empty) | Not extracted from img |
| Anchor refs ($179) | Yes, links preserved | None | No hyperlink support |
| Paragraph styles | Multiple ($1167, etc) | Single ($916) | Less style differentiation |
| Content types | Mix of container/text | All containers | Different structure choices |

### Root Causes

1. **Over-nesting**: Every block element becomes a Container, creating unnecessary depth
2. **No link tracking**: `<a>` elements are stripped during extraction, only text preserved
3. **Alt text**: Image alt attribute not being captured in `$584` field
4. **Single paragraph style**: All paragraphs getting the same style instead of differentiated

### Recommended Fixes

1. **Flatten structure**: Don't create containers for every block element, match reference depth
2. **Add anchor support**:
   - Track `<a href>` during extraction
   - Create anchor fragments ($597 or similar)
   - Add $179 to inline style runs
3. **Preserve alt text**: Extract and store img alt attribute in $584
4. **Improve style mapping**: Create distinct styles based on CSS specificity

### Impact on Readers

- Links: Non-functional (text only, no navigation)
- Alt text: Missing for accessibility
- Layout: Generally correct, minor differences in spacing
- Text content: Correct and readable
