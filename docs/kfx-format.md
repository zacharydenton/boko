# KFX Format Notes

This document describes the KFX format as implemented in boko and differences from Kindle Previewer's output.

## Overview

KFX is Amazon's latest Kindle format (KF10), successor to KF8/AZW3. It uses Amazon's Ion binary format for structured data within a custom container format.

## Container Structure

```
CONT header (18 bytes)
  - Magic: "CONT" (4 bytes)
  - Version: u16 (2 bytes)
  - Header length: u32 (4 bytes)
  - Container info offset: u32 (4 bytes)
  - Container info length: u32 (4 bytes)

Entity index table (24 bytes per entry)
  - Entity ID: u32
  - Entity type: u32
  - Offset: u64
  - Length: u64

Payload data (Ion-encoded entities)
```

## Key Entity Types

| Type | Symbol | Description |
|------|--------|-------------|
| 145 | $145 | TEXT_CONTENT - Raw text strings |
| 157 | $157 | STYLE - CSS-like style definitions |
| 164 | $164 | RESOURCE_INFO - Resource metadata |
| 258 | $258 | DOCUMENT_DATA - Document structure |
| 259 | $259 | STORYLINE - Content block with items |
| 260 | $260 | SECTION - Chapter/section definition |
| 264 | $264 | POSITION_MAP - Reading position data |
| 265 | $265 | LOCATION_MAP - Location calculations |
| 266 | $266 | ANCHOR - Internal/external links |
| 389 | $389 | NAV_UNIT_LIST - Navigation units |
| 395 | $395 | BOOK_NAVIGATION - TOC structure |
| 417 | $417 | RAW_MEDIA - Binary resources (images) |
| 490 | $490 | METADATA - Book metadata |
| 538 | $538 | READING_ORDER - Spine order |
| 585 | $585 | FORMAT_CAPABILITIES - Format version info |
| 597 | $597 | PAGE_TEMPLATE - Page layout templates |

## TEXT_CONTENT Format

Text content entities use `$146` (CONTENT_ARRAY) with a list of paragraph strings:

```ion
$145:: {
  $176: $symbol_id,      // Content name
  $146: [                // Content array (list of strings)
    "First paragraph text...",
    "Second paragraph text...",
  ]
}
```

Note: Earlier versions incorrectly used `$244` (TEXT) with a single concatenated string. The `$146` list format is required for the KFX reader to extract text content.

## Style Differences from Kindle Previewer

Our generated KFX produces valid styles but with some differences from Kindle Previewer:

### What We Generate
- Core CSS properties: font-size, font-weight, font-style, text-align, margins, padding, text-indent, line-height
- Color properties: color, background-color
- Text decorations: underline, line-through
- Display modes: block, inline
- Language tags: `$10` (lang) from `lang` attribute (e.g., `en-us`, `la`, `grc`)
- Font variant: small-caps via `$583`

### What Kindle Previewer Adds
Kindle Previewer generates additional style properties that we don't:

| Symbol | Property | Description |
|--------|----------|-------------|
| `$761` | unknown | List property `['$760']` |
| `$788`, `$135` | unknown | Kindle-specific flags |
| `$353` | unknown | Possibly vertical-align related |

### Impact
These differences do not affect rendering. Kindle devices/apps handle both approaches correctly.

## List Support (ol/ul)

KFX represents HTML ordered and unordered lists using container elements with the `$100` property.

### List Container Structure

When processing an `<ol>` or `<ul>` element, a list container is created with:

```ion
{
  $155: position_id,        // Position
  $100: $343,               // List type (decimal for <ol>)
  $157: style_symbol,       // Style reference
  $159: $276,               // Content type: CONTENT_LIST
  $146: [                   // Children (list items)
    { ... },                // First <li> content item
    { ... },                // Second <li> content item
    ...
  ]
}
```

### Content Types

| Symbol | Value | Description |
|--------|-------|-------------|
| `$276` | CONTENT_LIST | Content type for list containers (ol/ul) |
| `$277` | CONTENT_LIST_ITEM | Content type for list items (li) |

### List Type Values ($100)

| Value | HTML | Description |
|-------|------|-------------|
| `$343` | `<ol>` | Decimal numbered list (1, 2, 3...) |
| TBD | `<ul>` | Bullet/disc list |
| TBD | `list-style-type: upper-roman` | Roman numerals (I, II, III...) |

Note: Only `$343` (decimal) has been confirmed from reference files. Other list types need investigation.

### List Item Structure

Each `<li>` becomes a child content item within the list container with direct text reference:

```ion
{
  $155: position_id,
  $157: style_symbol,       // List item style
  $159: $277,               // Content type: CONTENT_LIST_ITEM
  $145: {                   // Direct text reference (not nested $146)
    $4: $text_content_symbol,
    $403: offset            // Index in TEXT_CONTENT array
  }
}
```

Key difference from regular paragraphs: List items use `$159: $277` (CONTENT_LIST_ITEM) instead of `$159: $269` (CONTENT_PARAGRAPH), and directly contain the `$145` text reference without nested `$146` containers.

### Example: Endnotes List

For an `<ol>` with 98 endnotes:

```ion
// List container
{
  $155: 1234,
  $100: $343,               // Decimal numbered list
  $157: $list_style,
  $159: $269,
  $146: [
    // 98 list items, each referencing text content
    { $155: 1235, $157: $item_style, $145: { version: $1082, $403: 0 } },
    { $155: 1236, $157: $item_style, $145: { version: $1082, $403: 1 } },
    ...
  ]
}
```

### Related Style Properties

List-related styles may include:

| Symbol | Property | Description |
|--------|----------|-------------|
| `$761` | unknown | List marker property, value `[$760]` |
| `$100` | list-type | On content items, not styles |

The `$761: [$760]` property appears on header styles in reference files, possibly related to list numbering reset or marker styling.

## Known Differences from Kindle Previewer

### Structural Counts (epictetus.epub)
These counts match Kindle Previewer output:
- TEXT_CONTENT: 13 vs 13
- SECTIONS: 8 vs 8
- CONTENT_BLOCKS: 8 vs 8
- List structure (ol/ul): Matches reference

### Structural Counts (kotlin_clean.epub - with MathML)
Significant differences due to MathML and pagination handling:

| Category | Boko | Reference | Delta | Notes |
|----------|------|-----------|-------|-------|
| TEXT_CONTENT blocks | 24 | 107 | +83 | Different chunking strategy |
| Total characters | 792,922 | 885,752 | -92,830 | MathML converted to plain text |
| Styles | 334 | 271 | -63 | Extra baseline properties |
| Anchors | 1,099 | 719 | -380 | Pagination anchors not needed |
| Resources | 157 | 75 | -82 | Fonts as images, equation images |
| Sections | 26 | 26 | ✓ | Matches |
| Storylines | 26 | 26 | ✓ | Matches |

### MathML Handling

EPUBs with mathematical content often use dual-format markup:

```html
<span class="epub"><math xmlns="http://www.w3.org/1998/Math/MathML">...</math></span>
<span class="mobi"><img src="../images/eq9-1-2.jpg" alt=""/></span>
```

**Kindle Previewer behavior**: Preserves MathML as raw XML strings in TEXT_CONTENT paragraphs. The entire `<math>` element is serialized as a string and stored as a paragraph.

**Current boko behavior**: Converts MathML to plain Unicode text (e.g., `"ωt+1=ωt−glsinθtδt"`).

**Impact**: ~93k character difference in books with many equations (78 MathML elements = significant loss).

**Decision**: Match Kindle Previewer - preserve MathML as raw XML strings. Since Kindle Previewer outputs MathML this way, modern Kindle devices must support rendering it.

**Conditional content via CSS**:
EPUBs use CSS to show/hide content for different targets:
```css
.epub { display: inline; }  /* Shown in modern readers */
.mobi { display: none; }    /* Hidden - fallback for legacy */
```

**Generic solution**: Skip elements with `display: none` computed style. This handles:
- `class="mobi"` fallback content
- Any other conditionally hidden content
- Already supported via `ParsedStyle::is_hidden()` method

**Exception**: `<br>` elements are NEVER skipped even if CSS sets `display: none`. Standard Ebooks uses CSS like:
```css
.epub-type-contains-word-z3998-verse p > span + br {
    display: none;
}
```
This is to control line breaks for different rendering contexts. Kindle Previewer ignores `display: none` for BR elements, treating them as structural line breaks regardless.

**Implementation**:
1. After computing element style, check `computed_style.is_hidden()`
2. If hidden (`display: none`) AND tag is not `br`, skip element and all children
3. When encountering a `<math>` element, serialize it as raw XML string
4. Store that XML string as a paragraph in TEXT_CONTENT

### Pagination Anchors

Boko generates pagination anchors every 2000 characters via `add_page_templates()` for Kindle location tracking.

**Kindle Previewer behavior**: Does NOT generate these anchors; relies on POSITION_MAP instead.

**Impact**: 380+ extra anchors in typical books.

**Recommended fix**: Remove or disable pagination anchor generation.

### Styles
We generate more styles (~334 vs ~271 in kotlin example) due to:

1. **Unnecessary baseline properties** always included:
   - `$1043-$1046` (border-left/right/top/bottom)
   - `$546: $378` (IMAGE_FIT: IMAGE_FIT_NONE)

2. **Missing properties**:
   - `$10` (LANGUAGE) - should be added from `lang` attribute
   - `$761` - list marker property

3. **Value differences**:
   - Font family: `"noto serif"` should be `"default,serif"`
   - Line height: Using `1.75` where reference uses `1.0`
   - Space after: Using `UNIT_PERCENT` where reference uses `UNIT_MULTIPLIER`

### Anchors
We generate more internal anchors than Kindle Previewer (~1099 vs ~719 in kotlin example):

1. **Pagination anchors** (~380 extra): Generated every 2000 chars, not needed
2. **Element ID anchors**: Created for all elements with `id` attributes
3. **Missing protocols**: `mailto:` URLs not handled as external

### Resources (Images/Fonts)

Current issues:
1. **Fonts treated as images**: `.otf`/`.ttf` files incorrectly added as RAW_MEDIA with 0x0 dimensions
2. **Equation images included**: Reference excludes small equation fallback images when MathML is present
3. **Icon images included**: Small decorative images (<100px) that reference excludes

## Symbol Table

KFX uses a shared symbol table (YJ_symbols) with ~800 predefined symbols. Local symbols start at ID 860 and are defined per-file.

Key symbol ranges:
- 1-100: Core properties (font, color, spacing)
- 100-200: Structure (position, content, style references)
- 200-400: Units and values (em, px, %, alignment constants)
- 400-600: Container and metadata
- 600-800: Advanced features

## Verification

To compare generated KFX against a reference:

```bash
python scripts/kfx_smart_diff.py generated.kfx reference.kfx
```

Key metrics that should match:
- TEXT_CONTENT count (text blocks)
- SECTION count (chapters)
- STORYLINE count (content blocks)
- Total text character count
- External anchor URLs

Acceptable differences:
- Internal anchor count (different ID tracking)
- Image formats (we preserve originals, Kindle may convert)
- Symbol IDs (arbitrary, file-specific)
- Kindle-specific style properties ($760, $761, etc.)
