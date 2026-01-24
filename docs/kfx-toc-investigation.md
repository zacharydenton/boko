# KFX Table of Contents Investigation

## Problem

The TOC popup does not work on Kindle Paperwhite when reading boko-generated KFX files. The TOC menu opens but shows no entries.

## What We've Verified Works

1. **Navigation structure exists** - The book navigation fragment ($389) is present with correct structure
2. **TOC container present** - NAV_TYPE=$212 (TOC) container exists with correct entries
3. **All titles match** - 243 TOC entries with titles matching the reference KFX exactly
4. **Valid EIDs** - All navigation targets point to valid EIDs in the position map (1029 valid EIDs)
5. **Hierarchical structure** - Nested TOC entries are correctly structured

## Changes Made (Not Yet Working)

### 1. Field Ordering (OrderedStruct)

Changed all navigation structures from `HashMap` to `OrderedStruct` to ensure consistent field ordering:

- `NAV_TITLE` ($241) now serializes before `NAV_TARGET` ($246) in all nav entries
- Verified via byte-level test that serialization preserves order correctly
- The Ion parser reads back into HashMap (losing order in display), but binary output is correct

**Files changed:** `src/kfx/writer/navigation.rs`

### 2. Container Order

Reordered navigation containers to match reference KFX:

**Before:**
1. HEADINGS ($798)
2. PAGE_LIST ($237)
3. TOC ($212)
4. LANDMARKS ($236)

**After:**
1. HEADINGS ($798)
2. TOC ($212)
3. LANDMARKS ($236)
4. PAGE_LIST ($237)

**Files changed:** `src/kfx/writer/navigation.rs`

## Remaining Differences from Reference

### 1. HEADINGS Container Structure

**Reference:** Hierarchical structure with frontmatter/bodymatter/backmatter grouping:
```
$391:: {
  $235: $798  // HEADINGS
  $247: [
    $393:: {
      $238: $800  // READING_ORDER_FRONTMATTER
      $241: { $244: "heading-nav-unit" }
      $247: [ ...nested heading entries... ]
    }
    $393:: {
      $238: $801  // READING_ORDER_BODYMATTER
      $247: [ ...nested heading entries... ]
    }
    $393:: {
      $238: $802  // READING_ORDER_BACKMATTER
      $247: [ ...nested heading entries... ]
    }
  ]
}
```

**Boko:** Flat list of heading-nav-unit entries without reading order grouping.

### 2. Extra PAGE_LIST Container

Boko has 4 containers vs reference's 3. The PAGE_LIST ($237) container is not present in the reference KFX.

### 3. Storyline Content Structure

The storyline ($259) content structure differs significantly:
- Reference has complex nested `$146` (CONTENT_ARRAY) structures
- Reference has `$790` (CONTENT_ROLE) markers on some content items
- Boko has a flatter content structure

## Hypotheses to Investigate

1. **HEADINGS structure required** - The Kindle may require the hierarchical frontmatter/bodymatter/backmatter structure in the HEADINGS container for TOC to work

2. **Storyline content linkage** - The TOC entries may need specific linkage to storyline content that we're missing

3. **Hidden dependency** - There may be a field or structure in sections/storylines that the TOC popup depends on

4. **Container count sensitivity** - Having 4 containers instead of 3 may confuse the Kindle parser

## Test Files

- Reference KFX: `tests/fixtures/epictetus.kfx` (working TOC)
- Generated KFX: `/tmp/boko_test.kfx` (TOC not working)

## Relevant Code Locations

- Navigation building: `src/kfx/writer/navigation.rs`
- Section building: `src/kfx/writer/builder/fragments.rs`
- Storyline building: `src/kfx/writer/content/mod.rs`
- Position maps: `src/kfx/writer/position.rs`
- Symbol definitions: `src/kfx/writer/symbols.rs`

## Comparison Tools

```bash
# Smart diff between reference and generated
python3 scripts/kfx_smart_diff.py tests/fixtures/epictetus.kfx /tmp/boko_test.kfx --full

# Section-specific comparisons
python3 scripts/kfx_smart_diff.py ... --section storylines
python3 scripts/kfx_smart_diff.py ... --section sections

# Run navigation structure tests
cargo test test_kfx_navigation -- --nocapture
```
