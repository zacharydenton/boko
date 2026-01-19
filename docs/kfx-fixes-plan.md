# KFX Discrepancy Fixes Plan

TDD approach to fixing discrepancies between generated and reference KFX.

## Priority Order

1. **Container Over-Nesting** - Most impactful structural issue
2. **Style Verbosity** - File size optimization
3. **Text Fragment Consolidation** - Low priority, complex
4. **Page Template Count** - Low priority, minor impact

---

## Fix 1: Container Over-Nesting

### Problem
Generated KFX has ~2x more container nesting than reference. The reference has a flatter structure where paragraphs are direct children of the content array, while generated wraps each paragraph in additional containers.

### Analysis
Current behavior in `extract_content_from_xhtml`:
- Every block element (`<section>`, `<header>`, `<p>`, `<div>`) becomes a `ContentItem::Container`
- This creates unnecessary nesting when the container's only purpose is grouping

Reference behavior:
- Containers are only created when they have meaningful styling or structure
- Paragraphs with same parent style are siblings, not wrapped

### TDD Tests

```rust
#[test]
fn test_imprint_container_nesting_depth() {
    // The imprint section should have at most 3 levels of $146 nesting
    // (content array -> section -> items)
    let book = read_epub("tests/fixtures/epictetus.epub").unwrap();
    let kfx = KfxBookBuilder::from_book(&book);

    fn max_nesting_depth(value: &IonValue) -> usize {
        match value {
            IonValue::Struct(s) => {
                if let Some(IonValue::List(items)) = s.get(&sym::CONTENT_ARRAY) {
                    1 + items.iter().map(|i| max_nesting_depth(i)).max().unwrap_or(0)
                } else {
                    s.values().map(|v| max_nesting_depth(v)).max().unwrap_or(0)
                }
            }
            IonValue::List(items) => {
                items.iter().map(|i| max_nesting_depth(i)).max().unwrap_or(0)
            }
            _ => 0,
        }
    }

    let imprint_block = kfx.fragments.iter()
        .filter(|f| f.ftype == sym::CONTENT_BLOCK)
        .find(|f| /* contains imprint text */);

    let depth = max_nesting_depth(&imprint_block.value);
    assert!(depth <= 3, "Container nesting too deep: {} (max 3)", depth);
}

#[test]
fn test_paragraph_siblings_not_wrapped() {
    // Consecutive paragraphs should be siblings, not each wrapped in a container
    let book = read_epub("tests/fixtures/epictetus.epub").unwrap();
    let kfx = KfxBookBuilder::from_book(&book);

    // Find imprint content block
    // Check that paragraph items are direct children of section container
    // Not: section -> wrapper -> paragraph
}
```

### Implementation Plan

1. Modify `extract_content_from_xhtml` to flatten unnecessary containers:
   - If a Container has only one child and no special styling, unwrap it
   - If consecutive children are all paragraphs with compatible styles, keep them as siblings

2. Add a post-processing pass to flatten the content tree:
   ```rust
   fn flatten_content(items: Vec<ContentItem>) -> Vec<ContentItem> {
       items.into_iter().flat_map(|item| {
           match item {
               ContentItem::Container { children, style, .. }
                   if can_flatten(&style) => flatten_content(children),
               other => vec![other],
           }
       }).collect()
   }
   ```

3. Define `can_flatten()` - a container can be flattened if:
   - It has no meaningful style properties (only defaults)
   - Its children don't depend on the container for styling inheritance

---

## Fix 2: Style Verbosity

### Problem
Generated styles include many properties; reference styles are minimal.

### Analysis
Generated emits all computed CSS properties. Reference only emits non-default properties that differ from the base style.

### TDD Tests

```rust
#[test]
fn test_style_minimal_properties() {
    // A basic paragraph style should only have essential properties
    let book = read_epub("tests/fixtures/epictetus.epub").unwrap();
    let kfx = KfxBookBuilder::from_book(&book);

    // Find a paragraph style
    let para_style = kfx.fragments.iter()
        .filter(|f| f.ftype == sym::STYLE)
        .find(|f| /* is paragraph style */);

    if let IonValue::Struct(s) = &para_style.value {
        // Should have <= 5 properties (name + display + maybe 2-3 others)
        assert!(s.len() <= 5, "Style too verbose: {} properties", s.len());
    }
}

#[test]
fn test_style_omits_defaults() {
    // Default values should not be explicitly set
    // e.g., text-align: left is default, shouldn't be in style
}
```

### Implementation Plan

1. Define default values for each style property
2. In `build_style_fragment`, only emit properties that differ from defaults
3. Consider style inheritance - child styles only need properties that differ from parent

---

## Fix 3: Text Fragment Consolidation (Low Priority)

### Problem
Reference combines text from multiple sections into fewer $145 fragments.

### Analysis
This is likely an optimization where multiple sections share a text content fragment, with different `$403` indices pointing into it. The complexity of implementing this may not be worth the minor space savings.

### Decision
**Defer** - Current approach (one $145 per section) is correct and simpler. Only revisit if file size becomes a significant issue.

---

## Fix 4: Page Template Count (Low Priority)

### Problem
262 vs 207 page templates.

### Analysis
The page template generation uses a character-based pagination algorithm. The difference might be due to:
- Different character counting (with/without whitespace)
- Different page size assumptions
- Including templates for content that reference doesn't

### TDD Tests

```rust
#[test]
fn test_page_template_count_reasonable() {
    let book = read_epub("tests/fixtures/epictetus.epub").unwrap();
    let kfx = KfxBookBuilder::from_book(&book);

    let template_count = kfx.fragments.iter()
        .filter(|f| f.ftype == sym::PAGE_TEMPLATE)
        .filter(|f| f.fid.starts_with("template-"))
        .count();

    // Should be within 20% of reference (207)
    assert!(template_count >= 166 && template_count <= 248,
        "Page template count {} outside expected range", template_count);
}
```

### Implementation Plan
1. Review `add_page_templates` algorithm
2. Compare character counting with reference
3. Adjust CHARS_PER_PAGE constant if needed

---

## Execution Order

### Phase 1: Container Flattening
1. Write `test_imprint_container_nesting_depth` - expect FAIL
2. Write `test_paragraph_siblings_not_wrapped` - expect FAIL
3. Implement container flattening in `extract_content_from_xhtml`
4. Run tests - expect PASS
5. Verify with comparison script

### Phase 2: Style Optimization
1. Write `test_style_minimal_properties` - expect FAIL
2. Write `test_style_omits_defaults` - expect FAIL
3. Implement default value filtering in style generation
4. Run tests - expect PASS
5. Verify file size reduction

### Phase 3: Validation
1. Run full test suite
2. Generate KFX and compare with reference
3. Test on actual Kindle device (if available)
