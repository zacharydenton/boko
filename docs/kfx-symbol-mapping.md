# KFX Symbol Mapping Reference

This document maps CSS properties to KFX (Amazon Kindle Format 10) symbols, derived from testing with Kindle Previewer.

## Unit Types ($306 values)

| Symbol | Unit Type |
|--------|-----------|
| $308   | em        |
| $310   | multiplier (line-height, etc.) |
| $314   | percent (%) |
| $318   | px/points |
| $505   | em (for font-size) |

## Text Alignment ($34)

| CSS Value | KFX Symbol |
|-----------|------------|
| left      | $59        |
| right     | $61        |
| center    | $320       |
| justify   | $321       |

## Font Properties

### Font Size ($16)

Font size uses a structure: `{$307: value, $306: $505}` where value is relative to 1.0 (1em).

| CSS Value | KFX Value |
|-----------|-----------|
| medium, 1em, 100%, 16px | (omitted - baseline) |
| smaller | 0.833333 |
| small | 0.8125 |
| x-small, 10px | 0.625 |
| xx-small | 0.5625 |
| larger, 1.2em | 1.2 |
| large, 18px | 1.125 |
| x-large, 1.5em, 150%, 24px | 1.5 |
| xx-large, 2em, 200% | 2.0 |
| 0.5em, 50% | 0.5 |
| 0.75em, 75%, 12px | 0.75 |
| 0.8em | 0.8 |
| 14px | 0.875 |
| 125%, 20px | 1.25 |

### Font Weight ($13)

| CSS Value | KFX Symbol |
|-----------|------------|
| 100 | $355 |
| 200 | $356 |
| 300 | $357 |
| 400, normal | $350 |
| 500 | $359 |
| 600 | $360 |
| 700, bold | $361 |
| 800 | $362 |
| 900 | $363 |

### Font Style ($12)

| CSS Value | KFX Symbol |
|-----------|------------|
| normal | $350 |
| oblique | $381 |
| italic | $382 |

### Font Variant ($583)

| CSS Value | KFX Symbol |
|-----------|------------|
| normal | $349 |
| small-caps | $369 |

### Font Family ($11)

| CSS Value | KFX Value |
|-----------|-----------|
| serif | serif |
| sans-serif | sans-serif |
| monospace | monospace |
| cursive | cursive |
| fantasy | fantasy |

## Text Properties

### Text Indent ($36)

Uses structure: `{$307: value, $306: $308}` (em units)

| CSS Value | KFX Value |
|-----------|-----------|
| 0 | 0 |
| 1em | 1 |
| 2em | 2 |
| 10px | 0.375 |
| 20px | 0.75 |

### Text Decoration

| CSS Value | KFX Property |
|-----------|--------------|
| underline | $23=$328 |
| overline | $554=$328 |
| line-through | $27=$328 |

### Text Transform ($41)

| CSS Value | KFX Symbol |
|-----------|------------|
| none | $349 |
| uppercase | $372 |
| lowercase | $373 |
| capitalize | $374 |

### Letter Spacing ($32)

Uses structure: `{$307: value, $306: unit}` where unit is $308 (em) or $318 (px).

| CSS Value | KFX Value | Unit |
|-----------|-----------|------|
| normal | 0 | $308 |
| 0.05em | 0.05 | $308 |
| 0.1em | 0.1 | $308 |
| 0.2em | 0.2 | $308 |
| 1px | 0.45 | $318 |
| 2px | 0.9 | $318 |

### Word Spacing ($33)

Uses structure: `{$307: value, $306: unit}` where unit is $308 (em) or $318 (px).

| CSS Value | KFX Value | Unit |
|-----------|-----------|------|
| normal | 0 | $308 |
| 0.25em | 0.25 | $308 |
| 0.5em | 0.5 | $308 |
| 2px | 0.9 | $318 |
| 5px | 2.25 | $318 |

### White Space ($45)

| CSS Value | KFX Value |
|-----------|-----------|
| normal | False |
| nowrap | True |

## Line Height ($42)

Uses structure: `{$307: value, $306: $310}` (multiplier).

Note: Line height 0 is converted to 0.6 (minimum).

| CSS Value | KFX Value |
|-----------|-----------|
| 0 | 0.6 |
| 1, 100%, 1em, 16px | 0.833333 |
| 1.2, 120% | 0.99 |
| 1.5, 150%, 1.5em, 24px | 1.25 |
| 2, 200%, 2em | 1.66667 |
| 20px | 1.04167 |

## Margins

### Margin Top ($47)

Uses structure: `{$307: value, $306: $310}` (multiplier).

Note: This shares the same property as line-height spacing adjustment.

| CSS Value | Approximate KFX Value |
|-----------|-----------------------|
| 0 | (removed) |
| 0.5em | 0.416667 |
| 1em | 0.833333 |
| 1.5em | 1.25 |
| 2em | 1.66667 |
| 3em | 2.5 |

### Margin Bottom ($47)

Similar to margin-top, uses the same $47 property.

### Margin Left ($48)

Uses structure: `{$307: value, $306: $314}` (percent).

| CSS Value | KFX Value (%) |
|-----------|---------------|
| 1em | 3.125 |
| 2em | 6.25 |
| 2.5em | 7.813 |
| 3em | 9.375 |
| 10px | 1.172 |
| 20px | 2.344 |

### Margin Right ($50)

Uses structure: `{$307: value, $306: $314}` (percent).

Values same as margin-left.

## Padding

### Padding Top ($52)

Uses structure: `{$307: value, $306: $310}` (multiplier).

| CSS Value | KFX Value |
|-----------|-----------|
| 1em | 0.833333 |
| 0.5em | 0.416667 |

### Padding Bottom ($54)

Same as padding-top.

### Padding Left ($48)

Same as margin-left property.

### Padding Right ($50)

Same as margin-right property.

## Width ($56)

Uses structure: `{$307: value, $306: unit}` where unit is $314 (%) or $308 (em).

When width is set, $546=$377 (IMAGE_FIT=CONTAIN) is also required.

For em widths, $65 with 100% is also set.

| CSS Value | KFX Value | Unit |
|-----------|-----------|------|
| 25% | 25 | $314 |
| 50% | 50 | $314 |
| 75% | 75 | $314 |
| 100% | 100 | $314 |
| 50px | 5.859 | $314 |
| 100px | 11.719 | $314 |
| 200px | 23.438 | $314 |
| 300px | 35.156 | $314 |
| 10em | 10 | $308 |
| 20em | 20 | $308 |

## Color ($19)

Color is stored as a 32-bit integer in ARGB format (0xAARRGGBB).

| CSS Value | KFX Value (decimal) |
|-----------|---------------------|
| black | 4278190080 (0xFF000000) |
| white | 4294967295 (0xFFFFFFFF) |
| red, #ff0000 | 4293787648 (0xFFFF0000) |
| green | 4278222848 (0xFF008000) |
| #00ff00 | 4278225408 (0xFF00FF00) |
| blue, #0000ff | 4278190335 (0xFF0000FF) |
| gray | 4285953654 (0xFF808080) |
| #333333 | 4281545523 |
| #666666 | 4284900966 |
| #999999 | 4285887861 |

## Image/Block Layout

### IMAGE_FIT ($546)

| Symbol | Meaning |
|--------|---------|
| $377   | CONTAIN |

### IMAGE_LAYOUT ($580)

| Symbol | Meaning |
|--------|---------|
| $320   | CENTER |
| $321   | JUSTIFY |

## Symbol Reference Summary

| Symbol | Property |
|--------|----------|
| $11 | font-family |
| $12 | font-style |
| $13 | font-weight |
| $16 | font-size |
| $19 | color |
| $23 | text-decoration: underline |
| $27 | text-decoration: line-through |
| $32 | letter-spacing |
| $33 | word-spacing |
| $34 | text-align |
| $36 | text-indent |
| $41 | text-transform |
| $42 | line-height |
| $45 | white-space (nowrap) |
| $47 | margin-top / spacing |
| $48 | margin-left / padding-left |
| $50 | margin-right / padding-right |
| $52 | padding-top |
| $54 | padding-bottom |
| $56 | width |
| $59 | ALIGN_LEFT |
| $61 | ALIGN_RIGHT |
| $65 | max-width |
| $173 | style ID reference |
| $306 | unit type |
| $307 | numeric value |
| $320 | ALIGN_CENTER |
| $321 | ALIGN_JUSTIFY |
| $328 | decoration present |
| $349 | NORMAL |
| $350 | NORMAL (weight/style) |
| $355-$363 | font weights 100-900 |
| $369 | SMALL_CAPS |
| $372 | UPPERCASE |
| $373 | LOWERCASE |
| $374 | CAPITALIZE |
| $377 | CONTAIN (image fit) |
| $381 | OBLIQUE |
| $382 | ITALIC |
| $505 | em unit (font-size) |
| $546 | IMAGE_FIT |
| $554 | text-decoration: overline |
| $580 | IMAGE_LAYOUT |
| $583 | font-variant |
