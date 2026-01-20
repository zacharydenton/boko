# KFX Cover Images and Kindle Thumbnails

This document explains how cover images work in KFX files and how Kindle displays cover thumbnails in the library/browse view.

## Overview

There are two separate systems for cover images on Kindle:

1. **In-book cover** - The cover displayed when opening the book (first page)
2. **Library thumbnail** - The cover shown in the Kindle's browse/library view

These are handled differently and require different approaches.

## In-Book Cover Image

The in-book cover is part of the KFX file structure:

### Required Fragments

1. **$164 (Resource)** - Image metadata fragment
   - `$166` (RESOURCE_NAME): Symbol pointing to this resource
   - `$161` (FORMAT): Image format symbol ($286 for JPEG, $287 for PNG, etc.)
   - `$422` (WIDTH): Image width in pixels
   - `$423` (HEIGHT): Image height in pixels
   - `$165` (LOCATION): Path to raw media (e.g., "resource/rsrc0")

2. **$417 (Raw Media)** - Actual image data as a blob

3. **$260 (Section)** - Cover section in the reading order

4. **$259 (Storyline)** - Content structure referencing the cover image

### Metadata Reference

In the `kindle_title_metadata` group, the `cover_image` key must be an **IonSymbol** (not a String) pointing to the $164 resource fragment ID:

```
{
  $492: "cover_image",      // METADATA_KEY
  $307: rsrc0              // VALUE - must be Symbol, not String!
}
```

**Important**: Using a String instead of Symbol for cover_image will cause it to not be recognized.

## Library Thumbnail (Browse View)

The library thumbnail is **NOT embedded in the KFX file**. It's a separate JPEG file stored on the Kindle's filesystem.

### How Calibre Handles Thumbnails

When Calibre sends a book to Kindle, it:

1. Reads metadata from the KFX file:
   - `content_id` or `ASIN` (uuid)
   - `cde_content_type` (cdetype)

2. Generates thumbnail filename:
   ```
   thumbnail_{uuid}_{cdetype}_portrait.jpg
   ```

3. Uploads the thumbnail to:
   ```
   /system/thumbnails/thumbnail_{uuid}_{cdetype}_portrait.jpg
   ```

### Required Metadata for Thumbnails

The KFX file must have these metadata values in `kindle_title_metadata`:

| Key | Symbol | Description |
|-----|--------|-------------|
| ASIN | $224 | Unique book identifier (32-char alphanumeric) |
| content_id | - | Same value as ASIN |
| cde_content_type | $251 | "EBOK" or "PDOC" |

Example thumbnail filename:
```
thumbnail_ABC123DEF456GHI789JKL012MNO345PQ_EBOK_portrait.jpg
```

### EBOK vs PDOC

- **PDOC** (Personal Document): Sideloaded content, fewer features
- **EBOK** (eBook): Store-purchased book type, enables more features

Both types can display cover thumbnails if the thumbnail file exists in `/system/thumbnails/`.

### Amazon Cover Bug

Amazon firmware sometimes deletes sideloaded book thumbnails. Calibre works around this by:

1. Caching thumbnails in `/amazon-cover-bug/` on the Kindle
2. Restoring them from cache on reconnect

See: https://manual.calibre-ebook.com/faq.html#covers-for-books-i-send-to-my-e-ink-kindle-show-up-momentarily-and-then-are-replaced-by-a-generic-cover

## Implementation in boko

### Current Status

boko generates:
- In-book cover image (embedded in KFX)
- Correct metadata (ASIN, content_id, cde_content_type, cover_image)

boko does NOT generate:
- Separate thumbnail file for library view

### Metadata Generation

```rust
// Generate ASIN-like identifier
let asin = format!("{:032X}", hash_of_title_author_identifier);
add_entry("ASIN", IonValue::String(asin.clone()));
add_entry("content_id", IonValue::String(asin));
add_entry("cde_content_type", IonValue::String("EBOK".to_string()));

// Cover image reference (must be Symbol!)
if let Some(&cover_sym) = self.resource_symbols.get(cover_href) {
    add_entry("cover_image", IonValue::Symbol(cover_sym));
}
```

### To Display Library Thumbnails

Option 1: **Use Calibre** to send books to Kindle (recommended)
- Calibre automatically generates and uploads thumbnails

Option 2: **Manual thumbnail creation**
- Extract cover from KFX or source EPUB
- Resize to ~500px height (portrait orientation)
- Save as JPEG
- Copy to Kindle: `/system/thumbnails/thumbnail_{ASIN}_EBOK_portrait.jpg`

Option 3: **Future boko feature**
- Generate thumbnail file alongside KFX output
- Would require knowing the Kindle mount point

## Symbol Reference

| Symbol | ID | Description |
|--------|-----|-------------|
| $161 | FORMAT | Image format |
| $164 | RESOURCE | Resource fragment type |
| $165 | LOCATION | Path to raw media |
| $166 | RESOURCE_NAME | Resource identifier |
| $224 | ASIN | Amazon identifier |
| $251 | cde_content_type | Content type (EBOK/PDOC) |
| $286 | JPG_FORMAT | JPEG format symbol |
| $287 | PNG_FORMAT | PNG format symbol |
| $417 | RAW_MEDIA | Raw media fragment type |
| $422 | WIDTH | Image width |
| $423 | HEIGHT | Image height |
| $424 | cover_image | Cover image reference |

## References

- Calibre Kindle driver: `calibre/src/calibre/devices/kindle/driver.py`
- Calibre KFX metadata: `calibre/src/calibre/ebooks/metadata/kfx.py`
- kfxlib metadata: `kfxlib/yj_metadata.py`
