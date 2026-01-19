# KFX Imprint Section Comparison

Comparison between generated KFX (`/tmp/epictetus-boko.kfx`) and reference KFX (`tests/fixtures/epictetus.kfx`) for the imprint.xhtml section.

## Source XHTML Structure

```xml
<section class="epub-type-contains-word-imprint" id="imprint" epub:type="imprint">
    <header>
        <h2 epub:type="title">Imprint</h2>
        <img alt="The Standard Ebooks logo." src="../images/logo.png"/>
    </header>
    <p>This ebook is the product... <a href="https://standardebooks.org/">Standard Ebooks</a>...</p>
    <p>This particular ebook... <a href="...perseus...">Perseus Digital Library</a>... <a href="...archive.org...">Internet Archive</a>.</p>
    <p>The source text... <a href="...creativecommons...">CC0 1.0</a>... <a href="uncopyright.xhtml">Uncopyright</a>...</p>
    <p>Standard Ebooks is... <a href="https://standardebooks.org/">standardebooks.org</a>.</p>
</section>
```

## Content Comparison

### Text Content ✓ MATCH
| Element | Generated ($941) | Reference ($1071) |
|---------|------------------|-------------------|
| Para 0 | "This ebook is the product..." | "This ebook is the product..." |
| Para 1 | "This particular ebook..." | "This particular ebook..." |
| Para 2 | "The source text and artwork..." | "The source text and artwork..." |
| Para 3 | "Standard Ebooks is a volunteer..." | "Standard Ebooks is a volunteer..." |

### Image ✓ MATCH
| Property | Generated | Reference |
|----------|-----------|-----------|
| Alt text ($584) | "The Standard Ebooks logo." | "The Standard Ebooks logo." |
| Resource ref ($175) | Present | Present |

### Links ✓ MATCH
All link offsets and lengths match exactly:

| Link Target | Offset | Length | Generated Anchor | Reference Anchor |
|-------------|--------|--------|------------------|------------------|
| standardebooks.org | 71 | 15 | $186 (external URL) | $186 (external URL) |
| Perseus Digital Library | 59 | 23 | $186 (external URL) | $186 (external URL) |
| Internet Archive | 113 | 16 | $186 (external URL) | $186 (external URL) |
| CC0 1.0 | 462 | 42 | $186 (external URL) | $186 (external URL) |
| **Uncopyright** | 544 | 11 | **$183 (position info)** | **$183 (position info)** |
| standardebooks.org | 282 | 18 | $186 (external URL) | $186 (external URL) |

## Structure Comparison

### Fragment Type Counts
| Type | Description | Generated | Reference | Diff |
|------|-------------|-----------|-----------|------|
| $145 | Text content | 6 | 13 | -7 |
| $157 | Styles | 62 | 63 | -1 |
| $164 | Resource metadata | 3 | 3 | 0 |
| $258 | Metadata | 1 | 1 | 0 |
| $259 | Content blocks | 8 | 8 | 0 |
| $260 | Sections | 8 | 8 | 0 |
| $266 | Page templates/anchors | 262 | 207 | +55 |
| $597 | ? | 7 | 8 | -1 |

### Container Nesting ($146)
| Metric | Generated | Reference |
|--------|-----------|-----------|
| $146 count in imprint block | 15 | 8 |
| Nesting depth | ~4 levels | ~2 levels |

**Generated structure:**
```
$146 (content array)
  └─ $146 (section container)
       └─ $146 (header container)
            └─ image item
       └─ $146 (paragraph wrapper)
            └─ paragraph item
       └─ $146 (paragraph wrapper)
            └─ paragraph item
       ...
```

**Reference structure:**
```
$146 (content array)
  └─ image item (with header container)
  └─ paragraph item
  └─ paragraph item
  └─ paragraph item
  └─ paragraph item
```

### Style Verbosity
**Generated style ($889):**
```
$12: '$382'           # font-family
$127: '$383'          # display: block
$16: { $306: '$310', $307: 1 }  # line-height
$34: '$320'           # text-align
$36: { $306: '$361', $307: 1 }  # margin
...
```

**Reference style ($1125):**
```
$127: '$349'          # display type only
$173: '$1125'         # style name
```

## Discrepancies to Fix

### 1. Container Over-Nesting (Priority: Medium)
- **Issue:** Generated has ~2x more container wrappers than reference
- **Impact:** Larger file size, potentially slower rendering
- **Root cause:** Each block element in XHTML creates a Container, even when unnecessary
- **Fix:** Flatten container hierarchy when children don't need separate styling

### 2. Text Fragment Consolidation (Priority: Low)
- **Issue:** Reference combines text from multiple sections into fewer $145 fragments
- **Impact:** Minor efficiency difference
- **Root cause:** Generated creates one $145 per section; reference batches them
- **Fix:** Consider batching text fragments (may not be worth the complexity)

### 3. Style Verbosity (Priority: Low)
- **Issue:** Generated styles include many computed CSS properties
- **Impact:** Larger file size
- **Root cause:** All parsed CSS properties are emitted
- **Fix:** Only emit non-default style properties

### 4. Extra Page Templates (Priority: Low)
- **Issue:** 262 vs 207 page templates (+55)
- **Impact:** Minor file size increase
- **Root cause:** Different pagination calculation
- **Fix:** Review page template generation algorithm

## What's Working Correctly

1. ✓ Text content extraction
2. ✓ Image handling with alt text ($584)
3. ✓ External link anchors ($186)
4. ✓ Internal link anchors ($183 with position info)
5. ✓ Link offset/length calculation
6. ✓ Section/content block structure
7. ✓ CSS document order (per-XHTML)
8. ✓ Section EID calculation for internal links
