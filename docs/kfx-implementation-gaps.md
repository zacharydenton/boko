# KFX Implementation Gaps and Remediation Plan

This document identifies gaps between the KFX format specification (`docs/kfx-format-specification.md`) and the current Boko implementation, organized by priority with a TDD-style remediation plan.

## Gap Summary by Category

| Category | Critical | High | Medium | Low |
|----------|----------|------|--------|-----|
| Content Types | 0 | 2 | 1 | 0 |
| Style Properties | 0 | 2 | 6 | 2 |
| Navigation | 0 | 0 | 2 | 1 |
| Resources | 0 | 0 | 0 | 4 |
| Metadata | 0 | 0 | 1 | 2 |
| **Total** | **0** | **4** | **10** | **9** |

---

## Detailed Gap Analysis

### 1. Content Structure Gaps

#### 1.1 SVG Support (HIGH)

**Spec Reference**: Section 6.2 - Content type `$274` for SVG

**Current State**: SVG elements are completely skipped in `extraction.rs:109`:
```rust
if matches!(tag_name, "script" | "style" | "head" | "title" | "svg") {
    return vec![];
}
```

**Impact**: Books with SVG illustrations render with missing content.

**Files to Modify**:
- `src/kfx/writer/content/extraction.rs`
- `src/kfx/writer/symbols.rs`
- `src/kfx/writer/builder/content.rs`

---

#### 1.2 Ruby Content Structure (HIGH)

**Spec Reference**: Section 6.2 - Content types `$764` (RUBY), `$765` (RUBY_TEXT), `$766` (RUBY_CONTAINER)

**Current State**: Ruby CSS properties (position, align, merge) are implemented, but the content structure for `<ruby>`, `<rt>`, `<rp>` elements is not handled. Ruby annotations are critical for Japanese text.

**Impact**: Japanese ebooks with furigana render incorrectly.

**Files to Modify**:
- `src/kfx/writer/content/extraction.rs`
- `src/kfx/writer/symbols.rs`
- `src/kfx/writer/builder/content.rs`

---

#### 1.3 Plugin/KVG Vector Graphics (MEDIUM)

**Spec Reference**: Section 6.2 - Content type `$272` for embedded plugins

**Current State**: Not implemented. KVG (Kindle Vector Graphics) is Amazon's proprietary vector format.

**Impact**: Advanced vector graphics won't render. Low priority since KVG is rarely used in EPUBs.

---

### 2. Style Property Gaps

#### 2.1 CSS Position Coordinates (HIGH)

**Spec Reference**: Section 7.6 - Position values and coordinates ($58-$61)

**Current State**: `CSS_POSITION` symbol ($183) is defined but position coordinates (top, left, right, bottom) are not converted to KFX.

**Impact**: Absolutely/relatively positioned elements don't position correctly.

**Files to Modify**:
- `src/css/style.rs` - Add position coordinate fields
- `src/css/parsing.rs` - Parse position coordinates
- `src/kfx/writer/style/layout.rs` - Convert to Ion

---

#### 2.2 Background Image Properties (HIGH)

**Spec Reference**: Section 7.7 - Background properties ($479-$484, $547, $73)

**Current State**: Not implemented. Background images are common in ebooks for decorative elements.

**Impact**: Elements with background images render without backgrounds.

**Symbols Needed**:
- `$479` - background-image
- `$480` - background-position-x
- `$481` - background-position-y
- `$482` - background-size-x
- `$483` - background-size-y
- `$484` - background-repeat
- `$547` - background-origin
- `$73` - background-clip

**Files to Modify**:
- `src/css/style.rs` - Add background fields
- `src/css/parsing.rs` - Parse background properties
- `src/kfx/writer/symbols.rs` - Add symbols
- `src/kfx/writer/style/conversion.rs` - Convert to Ion

---

#### 2.3 Direction Property (MEDIUM)

**Spec Reference**: Section 7.17 - Direction ($192, $682)

**Current State**: Symbols defined (`DIRECTION`, `DIRECTION_LTR`, `DIRECTION_RTL`) but not used in style conversion.

**Impact**: RTL text (Arabic, Hebrew) may not render correctly.

**Files to Modify**:
- `src/css/style.rs` - Add direction field
- `src/css/parsing.rs` - Parse direction
- `src/kfx/writer/style/conversion.rs` - Add conversion

---

#### 2.4 Text Decoration Color (MEDIUM)

**Spec Reference**: Section 11.4.1 - Text decoration colors ($24, $28, $555)

**Current State**: Symbols defined but not used. Currently only decoration presence/style is output.

**Impact**: Colored underlines/strikethroughs render in default color.

**Files to Modify**:
- `src/css/style.rs` - Add decoration color fields
- `src/css/parsing.rs` - Parse text-decoration-color
- `src/kfx/writer/style/conversion.rs` - Output color values

---

#### 2.5 Outline Properties (MEDIUM)

**Spec Reference**: Section 11.2 - Outline properties ($105-$108)

**Current State**: Symbols defined but not implemented.

**Impact**: CSS outline styles don't render.

**Files to Modify**:
- `src/css/style.rs` - Add outline fields
- `src/css/parsing.rs` - Parse outline-*
- `src/kfx/writer/style/conversion.rs` - Convert to Ion

---

#### 2.6 Box/Text Shadow Structured Format (MEDIUM)

**Spec Reference**: Section 7.1 style properties

**Current State**: Shadows stored as raw CSS strings instead of structured Ion values:
```rust
// Current (incorrect)
style_ion.insert(sym::BOX_SHADOW, IonValue::String(shadow.clone()));

// Should be structured
style_ion.insert(sym::BOX_SHADOW, IonValue::List(shadow_layers));
```

**Impact**: Shadows may not render correctly on all Kindle devices.

**Files to Modify**:
- `src/css/types.rs` - Add Shadow struct
- `src/css/parsing.rs` - Parse shadow values
- `src/kfx/writer/style/conversion.rs` - Output structured format

---

#### 2.7 Multi-Column Layout (MEDIUM)

**Spec Reference**: Section 7.1 - Column properties

**Current State**: `column-count` is implemented, but `column-gap` ($113), `column-rule-*` ($114-$117) are not.

**Impact**: Multi-column layouts may have incorrect spacing/dividers.

---

#### 2.8 Text Emphasis Position (MEDIUM)

**Spec Reference**: Section 7.16 - Text emphasis position ($719, $720)

**Current State**: `text-emphasis-style` and `text-emphasis-color` implemented, but position (horizontal/vertical) is not.

**Files to Modify**:
- `src/css/types.rs` - Add TextEmphasisPosition
- `src/css/parsing.rs` - Parse -webkit-text-emphasis-position
- `src/kfx/writer/style/conversion.rs` - Output $719/$720

---

#### 2.9 Orphans/Widows (LOW)

**Spec Reference**: Section 7.13 - Heritable properties

**Current State**: Not implemented. These control paragraph breaking.

---

#### 2.10 Heading Level Values (LOW)

**Spec Reference**: Section 9.7 - Heading levels ($799-$804)

**Current State**: Layout hints include `$760` (heading) but specific level values ($799 for h1, $800 for h2, etc.) are not output.

---

### 3. Navigation Gaps

#### 3.1 Page List Navigation (MEDIUM)

**Spec Reference**: Section 9.3 - Navigation type `$237`

**Current State**: Not implemented. EPUB3 page-list nav is ignored.

**Impact**: "Go to page" feature won't work for books with page lists.

**Files to Modify**:
- `src/book/mod.rs` - Add page_list field
- `src/book/epub.rs` - Extract page-list nav
- `src/kfx/writer/navigation.rs` - Generate $237 nav container

---

#### 3.2 Headings Navigation (MEDIUM)

**Spec Reference**: Section 9.3 - Navigation type `$798`

**Current State**: Not implemented. This provides heading-based navigation.

**Impact**: "Browse by heading" feature unavailable.

---

#### 3.3 Section Navigation ($390) (LOW)

**Spec Reference**: Section 9.3.1

**Current State**: Not implemented. Links nav containers to specific sections.

---

### 4. Resource Gaps

#### 4.1 Additional Image Formats (LOW)

**Spec Reference**: Section 8.2

| Format | Symbol | Status |
|--------|--------|--------|
| JPEG-XR | $548 | Not implemented |
| PDF | $565 | Not implemented |
| BMP | $599 | Not implemented |
| TIFF | $600 | Not implemented |
| BPG | $612 | Not implemented |

**Impact**: Rare formats won't be included. Most EPUBs use JPEG/PNG/GIF.

---

### 5. Metadata Gaps

#### 5.1 Issue Date (MEDIUM)

**Spec Reference**: Section 10.1 - `$219`

**Current State**: Publication date from EPUB metadata not included.

**Files to Modify**:
- `src/book/mod.rs` - Add publication_date field
- `src/book/epub.rs` - Extract dc:date
- `src/kfx/writer/builder/fragments.rs` - Output in metadata

---

#### 5.2 Orientation Support (LOW)

**Spec Reference**: Section 10.1 - `$215`, `$217`, `$218`

**Current State**: Not implemented. Controls device orientation for fixed-layout.

---

#### 5.3 Generator Info JSON (LOW)

**Spec Reference**: Section 2.6

**Current State**: Generator info JSON block not written after container info.

---

### 6. CJK List Style Types (LOW)

**Spec Reference**: Section 11.4

Many CJK list style types are not implemented:
- `$736` cjk-ideographic
- `$737` cjk-earthly-branch
- `$738` cjk-heavenly-stem
- `$739`-`$742` hiragana/katakana variants
- `$743`-`$748` Japanese/Chinese formal/informal

**Note**: Symbols $736-$740 are currently incorrectly used for text-emphasis in the implementation.

---

## TDD Implementation Plan

### Phase 1: High Priority Fixes

#### 1.1 SVG Support

**Test First** (`tests/structure_test.rs`):
```rust
#[test]
fn test_kfx_svg_content_type() {
    // Create EPUB with inline SVG
    // Verify $274 content type is generated
    // Verify SVG data is preserved
}

#[test]
fn test_kfx_svg_dimensions() {
    // Verify width/height from SVG viewBox are used
}
```

**Implementation Steps**:
1. Add `CONTENT_SVG = 274` to `symbols.rs`
2. Remove "svg" from skip list in `extraction.rs`
3. Add `ContentItem::Svg` variant with serialized XML
4. Handle SVG in `builder/content.rs`

---

#### 1.2 Ruby Content Structure

**Test First**:
```rust
#[test]
fn test_kfx_ruby_content_structure() {
    // Create EPUB with <ruby>漢字<rt>かんじ</rt></ruby>
    // Verify $764 (RUBY) container is generated
    // Verify $765 (RUBY_TEXT) for <rt> content
}

#[test]
fn test_kfx_ruby_with_rp() {
    // Test <ruby>漢字<rp>(</rp><rt>かんじ</rt><rp>)</rp></ruby>
    // Verify $766 (RUBY_CONTAINER) for <rp>
}
```

**Implementation Steps**:
1. Add ruby content type symbols to `symbols.rs`
2. Add `ContentItem::Ruby` variant with base and annotation
3. Handle `<ruby>`, `<rt>`, `<rp>` in `extraction.rs`
4. Build ruby structure in `builder/content.rs`

---

#### 1.3 CSS Position Coordinates

**Test First**:
```rust
#[test]
fn test_kfx_absolute_position() {
    // Create EPUB with position: absolute; top: 10px; left: 20px
    // Verify $183 (position) = $324 (absolute)
    // Verify $58 (top) = 10px value struct
    // Verify $59 (left) = 20px value struct
}

#[test]
fn test_kfx_relative_position() {
    // Test position: relative with offsets
}
```

**Implementation Steps**:
1. Add position coordinate fields to `ParsedStyle`
2. Parse top/left/right/bottom in CSS parsing
3. Add `add_position_coordinates()` to style conversion

---

#### 1.4 Background Image Properties

**Test First**:
```rust
#[test]
fn test_kfx_background_image() {
    // Create EPUB with background-image: url(image.png)
    // Verify $479 references the resource
}

#[test]
fn test_kfx_background_position_size() {
    // Test background-position: center; background-size: cover
    // Verify $480/$481 (position) and $482/$483 (size)
}

#[test]
fn test_kfx_background_repeat() {
    // Test background-repeat: no-repeat
    // Verify $484 = $487 (no-repeat)
}
```

**Implementation Steps**:
1. Add background property symbols to `symbols.rs`
2. Add background fields to `ParsedStyle`
3. Parse background-* CSS properties
4. Add `add_background_properties()` to style conversion
5. Track background image resources in builder

---

### Phase 2: Medium Priority Fixes

#### 2.1 Direction Property

**Test First**:
```rust
#[test]
fn test_kfx_direction_rtl() {
    // Create EPUB with direction: rtl
    // Verify $192 = $375 (rtl)
}
```

**Implementation Steps**:
1. Add `direction: Option<Direction>` to `ParsedStyle`
2. Parse CSS direction property
3. Add to style conversion

---

#### 2.2 Text Decoration Color

**Test First**:
```rust
#[test]
fn test_kfx_underline_color() {
    // Create EPUB with text-decoration: underline; text-decoration-color: red
    // Verify $23 (underline) is present
    // Verify $24 (underline-color) = red ARGB value
}
```

**Implementation Steps**:
1. Add decoration color fields to `ParsedStyle`
2. Parse text-decoration-color CSS
3. Output $24/$28/$555 in style conversion

---

#### 2.3 Page List Navigation

**Test First**:
```rust
#[test]
fn test_kfx_page_list_navigation() {
    // Create EPUB with page-list nav
    // Verify $237 nav container is generated
    // Verify page entries have correct targets
}
```

**Implementation Steps**:
1. Add `page_list: Vec<PageEntry>` to `Book`
2. Extract page-list from EPUB nav document
3. Generate $237 nav container in `navigation.rs`

---

#### 2.4 Structured Shadow Format

**Test First**:
```rust
#[test]
fn test_kfx_box_shadow_structured() {
    // Create EPUB with box-shadow: 2px 2px 4px rgba(0,0,0,0.5)
    // Verify $496 is a List, not String
    // Verify shadow components are correct
}
```

**Implementation Steps**:
1. Create `Shadow` struct in `css/types.rs`
2. Parse box-shadow/text-shadow into structured format
3. Output as Ion List with proper fields

---

#### 2.5 Issue Date Metadata

**Test First**:
```rust
#[test]
fn test_kfx_issue_date_metadata() {
    // Create EPUB with dc:date
    // Verify kindle_title_metadata includes issue_date
}
```

**Implementation Steps**:
1. Add `publication_date: Option<String>` to `BookMetadata`
2. Extract dc:date from EPUB OPF
3. Output in metadata fragment

---

### Phase 3: Low Priority Fixes

#### 3.1 CJK List Style Types

**Test First**:
```rust
#[test]
fn test_kfx_list_style_hiragana() {
    // Create EPUB with list-style-type: hiragana
    // Verify $100 = $739
}
```

**Implementation Steps**:
1. Fix symbol conflicts ($736-$740 text-emphasis vs list-style)
2. Add CJK list types to `ListStyleType` enum
3. Map to correct symbols

---

#### 3.2 Additional Image Formats

**Test First**:
```rust
#[test]
fn test_kfx_bmp_image_format() {
    // Create EPUB with BMP image
    // Verify $161 = $599 (BMP)
}
```

**Implementation Steps**:
1. Add format detection for BMP, TIFF, etc.
2. Add format symbols
3. Update `create_resource_fragments()`

---

#### 3.3 Generator Info JSON

**Test First**:
```rust
#[test]
fn test_kfx_generator_info_present() {
    // Build KFX and parse
    // Verify generator info JSON block exists after container info
}
```

**Implementation Steps**:
1. Create generator info JSON structure
2. Write after container info in serialization

---

## Implementation Order

| Order | Item | Priority | Effort | Dependencies |
|-------|------|----------|--------|--------------|
| 1 | CSS Position Coordinates | HIGH | Small | None |
| 2 | Direction Property | MEDIUM | Small | None |
| 3 | Text Decoration Color | MEDIUM | Small | None |
| 4 | Background Image Properties | HIGH | Medium | None |
| 5 | SVG Support | HIGH | Medium | None |
| 6 | Ruby Content Structure | HIGH | Large | None |
| 7 | Page List Navigation | MEDIUM | Medium | Book struct changes |
| 8 | Structured Shadow Format | MEDIUM | Medium | CSS parsing changes |
| 9 | Issue Date Metadata | MEDIUM | Small | Book struct changes |
| 10 | Outline Properties | MEDIUM | Small | None |
| 11 | Text Emphasis Position | MEDIUM | Small | None |
| 12 | Multi-Column Layout | MEDIUM | Small | None |
| 13 | Headings Navigation | MEDIUM | Medium | None |
| 14 | CJK List Style Types | LOW | Small | Symbol fixes |
| 15 | Additional Image Formats | LOW | Small | None |
| 16 | Heading Level Values | LOW | Small | None |
| 17 | Generator Info JSON | LOW | Small | None |
| 18 | Orphans/Widows | LOW | Small | None |
| 19 | Orientation Support | LOW | Small | None |
| 20 | Section Navigation | LOW | Medium | None |

---

## Success Criteria

### Phase 1 Complete When:
- [ ] All HIGH priority tests pass
- [ ] SVG images render on Kindle
- [ ] Ruby text displays correctly for Japanese content
- [ ] Absolutely positioned elements position correctly
- [ ] Background images display

### Phase 2 Complete When:
- [ ] All MEDIUM priority tests pass
- [ ] RTL text renders correctly
- [ ] Colored text decorations work
- [ ] Page list navigation functional
- [ ] Shadows render correctly

### Phase 3 Complete When:
- [ ] All LOW priority tests pass
- [ ] CJK list styles work
- [ ] All image formats supported
- [ ] Full metadata preserved

---

## Test File Organization

```
tests/
├── structure_test.rs          # Existing - add new tests here
├── fixtures/
│   ├── svg-test.epub          # New - SVG content
│   ├── ruby-test.epub         # New - Ruby/furigana content
│   ├── position-test.epub     # New - Positioned elements
│   ├── background-test.epub   # New - Background images
│   └── pagelist-test.epub     # New - Page list nav
```

---

## References

- `docs/kfx-format-specification.md` - Full format specification
- `src/kfx/writer/symbols.rs` - Symbol definitions
- `src/kfx/writer/style/conversion.rs` - Style conversion logic
- `src/kfx/writer/content/extraction.rs` - Content extraction
- `src/kfx/writer/navigation.rs` - Navigation building
