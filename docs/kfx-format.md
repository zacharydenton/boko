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
| `$546` | image-fit | Added to all styles with value `$378` (none) |
| `$761` | unknown | List property `['$760']` |
| `$788`, `$135` | unknown | Kindle-specific flags |

### Impact
These differences do not affect rendering. Kindle devices/apps handle both approaches correctly. Style counts now match (63 vs 63 for epictetus.epub).

## Anchor Differences

We generate more internal anchors than Kindle Previewer:
- **Ours**: ~263 anchors (more inline element IDs tracked)
- **Kindle Previewer**: ~207 anchors

This is because we create anchors for all elements with `id` attributes, while Kindle Previewer may optimize these.

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
- Style count (different deduplication strategies)
- Internal anchor count (different ID tracking)
- Symbol IDs (arbitrary, file-specific)
