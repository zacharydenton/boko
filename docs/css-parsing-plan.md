# CSS Parsing Plan for KFX Typography

## Current State
- KFX writer generates element-based default styles (h1-h6, p, blockquote, li)
- Styles use predefined values (bold headings, justified paragraphs, etc.)
- EPUB CSS files are ignored

## Goal
Parse actual CSS from EPUB stylesheets and apply those styles to KFX output.

## Implementation Plan

### Phase 1: Add cssparser dependency
```toml
[dependencies]
cssparser = "0.34"
```

### Phase 2: CSS Data Structures

Create a simplified style representation:

```rust
/// Parsed CSS style properties relevant to KFX
struct ParsedStyle {
    font_family: Option<String>,
    font_size: Option<CssValue>,      // em, px, %
    font_weight: Option<FontWeight>,  // normal, bold, 100-900
    font_style: Option<FontStyle>,    // normal, italic
    text_align: Option<TextAlign>,    // left, center, right, justify
    line_height: Option<CssValue>,
    margin_top: Option<CssValue>,
    margin_bottom: Option<CssValue>,
    margin_left: Option<CssValue>,
    margin_right: Option<CssValue>,
    text_indent: Option<CssValue>,
}

enum CssValue {
    Px(f32),
    Em(f32),
    Percent(f32),
    Inherit,
}
```

### Phase 3: CSS Parser Module

Create `src/css/mod.rs`:

1. **parse_stylesheet(css: &str) -> Vec<CssRule>**
   - Use cssparser to tokenize and parse
   - Extract selectors and declarations
   - Handle @import, @font-face (for font mapping)

2. **Selector matching**
   - Simple selectors: `p`, `h1`, `.class`, `#id`
   - Descendant: `blockquote p`
   - Pseudo-classes (limited): `:first-child`

3. **Property parsing**
   - font-family, font-size, font-weight, font-style
   - text-align, text-indent, line-height
   - margin, margin-top/bottom/left/right
   - Skip unsupported properties (color, background, etc.)

### Phase 4: Style Cascade

```rust
fn compute_style(element: &Element, stylesheets: &[Stylesheet]) -> ComputedStyle {
    // 1. Start with user-agent defaults
    // 2. Apply matching rules in source order
    // 3. Apply inline styles
    // 4. Resolve inheritance
}
```

### Phase 5: Integration with KFX Writer

1. **During XHTML extraction:**
   - Parse CSS files from EPUB resources
   - Track element classes/ids alongside element type

2. **Style generation:**
   - Deduplicate computed styles
   - Create P157 fragments for unique style combinations
   - Map elements to style symbols

3. **Fallback:**
   - If CSS parsing fails, fall back to element-based defaults

### Example Flow

```
EPUB:
  styles.css: "h1 { font-size: 2em; text-align: center; }"
  chapter.xhtml: "<h1 class="title">Hello</h1><p>World</p>"

KFX Output:
  P157 style-1: { P13: 2em, P44: center }   // from CSS
  P157 style-2: { P44: justify }             // default paragraph
  P259 content: [ {P157: style-1, text: "Hello"}, {P157: style-2, text: "World"} ]
```

### Properties to Support

| CSS Property | KFX Symbol | Notes |
|--------------|------------|-------|
| font-family | P12 | Map to P350 (default) or P382 (serif) |
| font-size | P13 | Convert to em/px/% struct |
| font-weight | P45 | Boolean for bold |
| font-style | P45 | Combine with bold? |
| text-align | P44 | P370=justify, P371=center, P372=left |
| line-height | P16 | Percent struct |
| margin-top | P42 | Em struct |
| margin-bottom | P47 | Em struct |
| margin-left | P49 | Em struct |
| text-indent | P48? | Need to verify symbol |

### Out of Scope (for now)
- Complex selectors (`:not()`, `::before`)
- CSS variables
- @media queries
- Flexbox/grid layout
- Colors, backgrounds
- Borders, shadows
- Transforms, animations

### Testing Strategy
1. Unit tests for CSS parsing
2. Integration test: parse epictetus.epub CSS, verify styles applied
3. Comparison test: generated KFX styles vs reference
