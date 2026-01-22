//! KFX symbol definitions and symbol table management.
//!
//! Contains the YJ_symbols shared table constants and local symbol management.

use std::collections::HashMap;

use crate::kfx::ion::IonValue;

// =============================================================================
// YJ_SYMBOLS - Shared symbol table (subset of the full 800+ symbols)
// =============================================================================

/// Symbol IDs from YJ_symbols shared table (version 10)
/// These are the well-known symbols used in KFX format.
/// VERIFIED via comprehensive CSS-to-KFX mapping test with Kindle Previewer (2024-01)
#[allow(dead_code)]
pub mod sym {
    // Core property symbols
    pub const ID: u64 = 4; // $4 - generic id field
    pub const LANGUAGE: u64 = 10; // $10 - language

    // ==========================================================================
    // STYLE PROPERTY SYMBOLS (verified via CSS mapping test)
    // ==========================================================================

    // Font properties
    pub const FONT_FAMILY: u64 = 11; // $11 - font-family (string value: serif, sans-serif, etc.)
    pub const FONT_STYLE: u64 = 12; // $12 - font-style (italic, oblique, normal)
    pub const FONT_WEIGHT: u64 = 13; // $13 - font-weight (100-900, bold, normal)
    pub const FONT_STRETCH: u64 = 15; // $15 - font-stretch (condensed, expanded, etc.)
    pub const FONT_SIZE: u64 = 16; // $16 - font-size (relative to 1.0 = 1em)
    pub const COLOR: u64 = 19; // $19 - text color (ARGB integer)

    // Text decoration
    pub const TEXT_DECORATION_UNDERLINE: u64 = 23; // $23 - text-decoration: underline
    pub const TEXT_DECORATION_LINE_THROUGH: u64 = 27; // $27 - text-decoration: line-through

    // Spacing properties
    pub const LETTER_SPACING: u64 = 32; // $32 - letter-spacing
    pub const WORD_SPACING: u64 = 33; // $33 - word-spacing
    pub const TEXT_ALIGN: u64 = 34; // $34 - text alignment
    pub const TEXT_ALIGN_LAST: u64 = 35; // $35 - text-align-last
    pub const TEXT_INDENT: u64 = 36; // $36 - text indent
    pub const TEXT_TRANSFORM: u64 = 41; // $41 - text-transform (uppercase, lowercase, etc.)
    pub const LINE_HEIGHT: u64 = 42; // $42 - line-height
    pub const WHITE_SPACE_NOWRAP: u64 = 45; // $45 - white-space: nowrap (boolean)

    // Margin/padding (note: $47 is shared between margin-top/bottom and spacing)
    pub const MARGIN: u64 = 46; // $46 - margin (shorthand)
    pub const SPACE_BEFORE: u64 = 47; // $47 - margin-top/space-before (multiplier)
    pub const MARGIN_LEFT: u64 = 48; // $48 - margin-left (percent)
    pub const SPACE_AFTER: u64 = 49; // $49 - margin-bottom/space-after (multiplier)
    pub const MARGIN_RIGHT: u64 = 50; // $50 - margin-right (percent)
    pub const PADDING: u64 = 51; // $51 - padding (shorthand)
    pub const PADDING_TOP: u64 = 52; // $52 - padding-top (multiplier)
    pub const PADDING_LEFT: u64 = 53; // $53 - padding-left
    pub const PADDING_BOTTOM: u64 = 54; // $54 - padding-bottom (multiplier)
    pub const PADDING_RIGHT: u64 = 55; // $55 - padding-right

    // Dimensions
    pub const STYLE_WIDTH: u64 = 56; // $56 - width in style
    pub const STYLE_HEIGHT: u64 = 57; // $57 - height in style
    pub const MAX_HEIGHT: u64 = 64; // $64 - max-height
    pub const MAX_WIDTH: u64 = 65; // $65 - max-width (for em widths)
    pub const OPACITY: u64 = 72; // $72 - opacity (0.0-1.0 decimal)

    // Legacy aliases for compatibility
    pub const MARGIN_TOP: u64 = 47; // alias for SPACE_BEFORE
    pub const MARGIN_BOTTOM: u64 = 49; // alias for SPACE_AFTER

    // Background
    pub const BACKGROUND_COLOR: u64 = 21; // $21 - background color

    // ==========================================================================
    // UNIT TYPES ($306 values)
    // ==========================================================================
    pub const UNIT: u64 = 306; // $306 - unit field in value struct
    pub const VALUE: u64 = 307; // $307 - value field in value struct
    pub const UNIT_EM: u64 = 308; // $308 - em unit (for text-indent, letter-spacing, etc.)
    pub const UNIT_EX: u64 = 309; // $309 - ex unit (x-height)
    pub const UNIT_MULTIPLIER: u64 = 310; // $310 - multiplier unit (for line-height, margins)
    pub const UNIT_VW: u64 = 311; // $311 - vw unit (viewport width)
    pub const UNIT_VH: u64 = 312; // $312 - vh unit (viewport height)
    pub const UNIT_VMIN: u64 = 313; // $313 - vmin unit (viewport minimum)
    pub const UNIT_PERCENT: u64 = 314; // $314 - percent unit (for margin-left/right, width)
    pub const UNIT_CM: u64 = 315; // $315 - cm unit (centimeters)
    pub const UNIT_MM: u64 = 316; // $316 - mm unit (millimeters)
    pub const UNIT_IN: u64 = 317; // $317 - in unit (inches)
    pub const UNIT_PT: u64 = 318; // $318 - pt unit (points)
    pub const UNIT_PX: u64 = 319; // $319 - px unit (pixels)
    pub const UNIT_EM_FONTSIZE: u64 = 505; // $505 - rem unit (root em / font-size context)
    pub const UNIT_CH: u64 = 506; // $506 - ch unit (character width)
    pub const UNIT_VMAX: u64 = 507; // $507 - vmax unit (viewport maximum)

    // ==========================================================================
    // TEXT ALIGNMENT VALUES ($34)
    // ==========================================================================
    pub const ALIGN_LEFT: u64 = 59; // $59 - text-align: left
    pub const ALIGN_RIGHT: u64 = 61; // $61 - text-align: right
    pub const ALIGN_CENTER: u64 = 320; // $320 - text-align: center
    pub const ALIGN_JUSTIFY: u64 = 321; // $321 - text-align: justify

    // ==========================================================================
    // TEXT ALIGN LAST VALUES ($35)
    // ==========================================================================
    pub const ALIGN_LAST_START: u64 = 680; // $680 - text-align-last: start
    pub const ALIGN_LAST_END: u64 = 681; // $681 - text-align-last: end
    // Also uses ALIGN_LEFT, ALIGN_RIGHT, ALIGN_CENTER, ALIGN_JUSTIFY, BREAK_AUTO

    // ==========================================================================
    // TABLE CELL ALIGNMENT ($633)
    // ==========================================================================
    pub const CELL_ALIGN: u64 = 633; // $633 - -kfx-table-vertical-align
    // Values: $350 (baseline), $60 (bottom), $320 (middle), $58 (top)

    // ==========================================================================
    // FONT WEIGHT VALUES ($13)
    // ==========================================================================
    pub const FONT_WEIGHT_NORMAL: u64 = 350; // $350 - font-weight: normal/400
    pub const FONT_WEIGHT_100: u64 = 355; // $355 - font-weight: 100
    pub const FONT_WEIGHT_200: u64 = 356; // $356 - font-weight: 200
    pub const FONT_WEIGHT_300: u64 = 357; // $357 - font-weight: 300
    pub const FONT_WEIGHT_500: u64 = 359; // $359 - font-weight: 500
    pub const FONT_WEIGHT_600: u64 = 360; // $360 - font-weight: 600
    pub const FONT_WEIGHT_BOLD: u64 = 361; // $361 - font-weight: bold/700
    pub const FONT_WEIGHT_800: u64 = 362; // $362 - font-weight: 800
    pub const FONT_WEIGHT_900: u64 = 363; // $363 - font-weight: 900

    // ==========================================================================
    // FONT STYLE VALUES ($12)
    // ==========================================================================
    pub const FONT_STYLE_NORMAL: u64 = 350; // $350 - font-style: normal
    pub const FONT_STYLE_OBLIQUE: u64 = 381; // $381 - font-style: oblique
    pub const FONT_STYLE_ITALIC: u64 = 382; // $382 - font-style: italic

    // ==========================================================================
    // FONT STRETCH VALUES ($15)
    // ==========================================================================
    pub const FONT_STRETCH_NORMAL: u64 = 350; // $350 - font-stretch: normal
    pub const FONT_STRETCH_CONDENSED: u64 = 365; // $365 - font-stretch: condensed
    pub const FONT_STRETCH_SEMI_CONDENSED: u64 = 366; // $366 - font-stretch: semi-condensed
    pub const FONT_STRETCH_SEMI_EXPANDED: u64 = 367; // $367 - font-stretch: semi-expanded
    pub const FONT_STRETCH_EXPANDED: u64 = 368; // $368 - font-stretch: expanded

    // ==========================================================================
    // DIRECTION VALUES ($192, $682)
    // ==========================================================================
    pub const DIRECTION: u64 = 192; // $192 - direction property (also $682)
    pub const DIRECTION_LTR: u64 = 376; // $376 - direction: ltr
    pub const DIRECTION_RTL: u64 = 375; // $375 - direction: rtl

    // ==========================================================================
    // TEXT TRANSFORM VALUES ($41)
    // ==========================================================================
    pub const TEXT_TRANSFORM_NONE: u64 = 349; // $349 - text-transform: none
    pub const TEXT_TRANSFORM_UPPERCASE: u64 = 372; // $372 - text-transform: uppercase
    pub const TEXT_TRANSFORM_LOWERCASE: u64 = 373; // $373 - text-transform: lowercase
    pub const TEXT_TRANSFORM_CAPITALIZE: u64 = 374; // $374 - text-transform: capitalize

    // ==========================================================================
    // FONT VARIANT VALUES ($583)
    // ==========================================================================
    pub const FONT_VARIANT: u64 = 583; // $583 - font-variant property
    pub const FONT_VARIANT_NORMAL: u64 = 349; // $349 - font-variant: normal
    pub const FONT_VARIANT_SMALL_CAPS: u64 = 369; // $369 - font-variant: small-caps

    // ==========================================================================
    // TEXT DECORATION VALUES
    // ==========================================================================
    pub const DECORATION_PRESENT: u64 = 328; // $328 - decoration is present (solid)
    pub const TEXT_DECORATION_OVERLINE: u64 = 554; // $554 - text-decoration: overline
    pub const DECORATION_BOX_CLONE: u64 = 99; // $99 - box-decoration-break: clone

    // Text decoration color (separate symbols for underline/line-through/overline)
    pub const TEXT_DECORATION_UNDERLINE_COLOR: u64 = 24; // $24 - underline text-decoration-color
    pub const TEXT_DECORATION_LINE_THROUGH_COLOR: u64 = 28; // $28 - line-through text-decoration-color
    pub const TEXT_DECORATION_OVERLINE_COLOR: u64 = 555; // $555 - overline text-decoration-color

    // Text decoration line styles (from yj_to_epub_properties.py)
    // Note: $707 is -kfx-character-width in kfxlib, not decoration style
    // Note: $708 is also -kfx-character-width in kfxlib
    // Values $329, $330, $331 are the actual line styles (shared with border)
    pub const TEXT_DECORATION_STYLE_DOUBLE: u64 = 329; // $329 - double line style
    pub const TEXT_DECORATION_STYLE_DASHED: u64 = 330; // $330 - dashed line style
    pub const TEXT_DECORATION_STYLE_DOTTED: u64 = 331; // $331 - dotted line style

    // ==========================================================================
    // VERTICAL ALIGN VALUES ($44)
    // ==========================================================================
    pub const VERTICAL_ALIGN: u64 = 44; // $44 - vertical-align property
    pub const VERTICAL_TOP: u64 = 58; // $58 - vertical-align: top
    pub const VERTICAL_BOTTOM: u64 = 60; // $60 - vertical-align: bottom
    pub const VERTICAL_SUPER: u64 = 370; // $370 - vertical-align: super
    pub const VERTICAL_SUB: u64 = 371; // $371 - vertical-align: sub
    pub const VERTICAL_TEXT_TOP: u64 = 447; // $447 - vertical-align: text-top
    pub const VERTICAL_TEXT_BOTTOM: u64 = 449; // $449 - vertical-align: text-bottom
    // Note: $350 (FONT_WEIGHT_NORMAL) = baseline, $320 (ALIGN_CENTER) = middle

    // ==========================================================================
    // LAYOUT PROPERTIES
    // ==========================================================================
    pub const MIN_HEIGHT: u64 = 62; // $62 - min-height (also used for height in some contexts)
    pub const MIN_WIDTH: u64 = 63; // $63 - min-width
    pub const VISIBILITY: u64 = 68; // $68 - visibility (boolean: true = visible)
    pub const OVERFLOW_CLIP: u64 = 476; // $476 - overflow: hidden/clip (boolean: true = clip)

    // ==========================================================================
    // POSITION PROPERTY ($183)
    // ==========================================================================
    pub const CSS_POSITION: u64 = 183; // $183 - position property
    pub const POSITION_OEB_PAGE_HEAD: u64 = 151; // $151 - position: oeb-page-head
    pub const POSITION_ABSOLUTE: u64 = 324; // $324 - position: absolute
    pub const POSITION_OEB_PAGE_FOOT: u64 = 455; // $455 - position: oeb-page-foot
    pub const POSITION_RELATIVE: u64 = 488; // $488 - position: relative
    pub const POSITION_FIXED: u64 = 489; // $489 - position: fixed
    // Note: Position coordinates use $58 (top), $59 (left), $60 (bottom), $61 (right)

    // ==========================================================================
    // OUTLINE PROPERTIES
    // ==========================================================================
    pub const OUTLINE_COLOR: u64 = 105; // $105 - outline-color
    pub const OUTLINE_OFFSET: u64 = 106; // $106 - outline-offset
    pub const OUTLINE_STYLE: u64 = 107; // $107 - outline-style (uses BORDER_STYLES values)
    pub const OUTLINE_WIDTH: u64 = 108; // $108 - outline-width

    // ==========================================================================
    // CLEAR PROPERTY ($628)
    // ==========================================================================
    pub const CLEAR: u64 = 628; // $628 - clear property
    pub const CLEAR_BOTH: u64 = 421; // $421 - clear: both
    // Note: $349 (TEXT_TRANSFORM_NONE) = none, $59 (ALIGN_LEFT) = left, $61 (ALIGN_RIGHT) = right

    // ==========================================================================
    // WORD BREAK ($569)
    // ==========================================================================
    pub const WORD_BREAK: u64 = 569; // $569 - word-break property
    pub const WORD_BREAK_ALL: u64 = 570; // $570 - word-break: break-all
    // Note: $350 (FONT_WEIGHT_NORMAL) = normal

    // ==========================================================================
    // PAGE BREAK CONTROL
    // ==========================================================================
    // Legacy page-break-* properties (CSS 2.1)
    pub const PAGE_BREAK_AFTER: u64 = 133; // $133 - page-break-after (legacy)
    pub const PAGE_BREAK_BEFORE: u64 = 134; // $134 - page-break-before (legacy)
    pub const BREAK_INSIDE: u64 = 135; // $135 - break-inside / page-break-inside property

    // Modern break-* properties (CSS3)
    pub const BREAK_AFTER: u64 = 788; // $788 - break-after property
    pub const BREAK_BEFORE: u64 = 789; // $789 - break-before property

    // Break values
    pub const BREAK_ALWAYS: u64 = 352; // $352 - always value
    pub const BREAK_AVOID: u64 = 353; // $353 - avoid value
    pub const BREAK_AUTO: u64 = 383; // $383 - auto value

    // ==========================================================================
    // SHADOW PROPERTIES ($496, $497) - P4 improvement
    // ==========================================================================
    pub const BOX_SHADOW: u64 = 496; // $496 - box-shadow property
    pub const TEXT_SHADOW: u64 = 497; // $497 - text-shadow property

    // ==========================================================================
    // WRITING MODE ($560) - P2 improvement
    // ==========================================================================
    pub const WRITING_MODE: u64 = 560; // $560 - writing-mode property
    pub const WRITING_MODE_HORIZONTAL_TB: u64 = 557; // $557 - horizontal-tb
    pub const WRITING_MODE_VERTICAL_LR: u64 = 558; // $558 - vertical-lr
    pub const WRITING_MODE_VERTICAL_RL: u64 = 559; // $559 - vertical-rl

    // ==========================================================================
    // TEXT COMBINE UPRIGHT ($561)
    // ==========================================================================
    pub const TEXT_COMBINE_UPRIGHT: u64 = 561; // $561 - text-combine-upright property

    // ==========================================================================
    // UNICODE-BIDI ($674) - Bidirectional text control
    // ==========================================================================
    pub const UNICODE_BIDI: u64 = 674; // $674 - unicode-bidi property
    pub const BIDI_EMBED: u64 = 675; // $675 - unicode-bidi: embed
    pub const BIDI_ISOLATE: u64 = 676; // $676 - unicode-bidi: isolate
    pub const BIDI_OVERRIDE: u64 = 677; // $677 - unicode-bidi: bidi-override
    pub const BIDI_ISOLATE_OVERRIDE: u64 = 678; // $678 - unicode-bidi: isolate-override
    pub const BIDI_PLAINTEXT: u64 = 679; // $679 - unicode-bidi: plaintext

    // ==========================================================================
    // TEXT-ORIENTATION ($706) - Vertical text orientation
    // ==========================================================================
    pub const TEXT_ORIENTATION: u64 = 706; // $706 - text-orientation property
    pub const TEXT_ORIENTATION_MIXED: u64 = 383; // $383 - text-orientation: mixed (uses BREAK_AUTO)
    pub const TEXT_ORIENTATION_UPRIGHT: u64 = 779; // $779 - text-orientation: upright
    pub const TEXT_ORIENTATION_SIDEWAYS: u64 = 778; // $778 - text-orientation: sideways

    // ==========================================================================
    // LINE-BREAK ($780) - Line break rules for CJK text
    // ==========================================================================
    pub const LINE_BREAK: u64 = 780; // $780 - line-break property
    pub const LINE_BREAK_LOOSE: u64 = 781; // $781 - line-break: loose
    pub const LINE_BREAK_STRICT: u64 = 782; // $782 - line-break: strict
    pub const LINE_BREAK_ANYWHERE: u64 = 783; // $783 - line-break: anywhere
    // LINE_BREAK_AUTO uses $383 (BREAK_AUTO), LINE_BREAK_NORMAL uses $350 (FONT_WEIGHT_NORMAL)

    // ==========================================================================
    // RUBY ANNOTATION PROPERTIES (P1 Phase 2)
    // ==========================================================================
    pub const RUBY_POSITION: u64 = 762; // $762 - ruby-position property
    pub const RUBY_POSITION_OVER: u64 = 58; // $58 - ruby-position: over (reuses VERTICAL_TOP)
    pub const RUBY_POSITION_UNDER: u64 = 60; // $60 - ruby-position: under (reuses VERTICAL_BOTTOM)
    pub const RUBY_ALIGN: u64 = 766; // $766 - ruby-align property
    pub const RUBY_ALIGN_CENTER: u64 = 767; // $767 - ruby-align: center
    pub const RUBY_ALIGN_START: u64 = 768; // $768 - ruby-align: start
    pub const RUBY_ALIGN_SPACE_AROUND: u64 = 769; // $769 - ruby-align: space-around
    pub const RUBY_ALIGN_SPACE_BETWEEN: u64 = 770; // $770 - ruby-align: space-between
    pub const RUBY_MERGE: u64 = 764; // $764 - ruby-merge property
    pub const RUBY_MERGE_SEPARATE: u64 = 765; // $765 - ruby-merge: separate
    pub const RUBY_MERGE_COLLAPSE: u64 = 763; // $763 - ruby-merge: collapse

    // ==========================================================================
    // TEXT EMPHASIS PROPERTIES (P1 Phase 2)
    // ==========================================================================
    pub const TEXT_EMPHASIS_STYLE: u64 = 717; // $717 - text-emphasis-style property
    pub const TEXT_EMPHASIS_COLOR: u64 = 718; // $718 - text-emphasis-color property
    pub const TEXT_EMPHASIS_FILLED: u64 = 724; // $724 - filled
    pub const TEXT_EMPHASIS_OPEN: u64 = 725; // $725 - open
    pub const TEXT_EMPHASIS_DOT: u64 = 726; // $726 - dot
    pub const TEXT_EMPHASIS_CIRCLE: u64 = 727; // $727 - circle
    pub const TEXT_EMPHASIS_FILLED_CIRCLE: u64 = 728; // $728 - filled circle
    pub const TEXT_EMPHASIS_OPEN_CIRCLE: u64 = 729; // $729 - open circle
    pub const TEXT_EMPHASIS_FILLED_DOT: u64 = 730; // $730 - filled dot
    pub const TEXT_EMPHASIS_OPEN_DOT: u64 = 731; // $731 - open dot
    pub const TEXT_EMPHASIS_DOUBLE_CIRCLE: u64 = 732; // $732 - double-circle
    pub const TEXT_EMPHASIS_FILLED_DOUBLE_CIRCLE: u64 = 733; // $733 - filled double-circle
    pub const TEXT_EMPHASIS_OPEN_DOUBLE_CIRCLE: u64 = 734; // $734 - open double-circle
    pub const TEXT_EMPHASIS_TRIANGLE: u64 = 735; // $735 - triangle
    pub const TEXT_EMPHASIS_FILLED_TRIANGLE: u64 = 736; // $736 - filled triangle
    pub const TEXT_EMPHASIS_OPEN_TRIANGLE: u64 = 737; // $737 - open triangle
    pub const TEXT_EMPHASIS_SESAME: u64 = 738; // $738 - sesame
    pub const TEXT_EMPHASIS_FILLED_SESAME: u64 = 739; // $739 - filled sesame
    pub const TEXT_EMPHASIS_OPEN_SESAME: u64 = 740; // $740 - open sesame

    // ==========================================================================
    // BORDER COLLAPSE ($150)
    // Note: Uses boolean values (false=separate, true=collapse), not symbols
    // ==========================================================================
    pub const BORDER_COLLAPSE: u64 = 150; // $150 - border-collapse property

    // ==========================================================================
    // DROP CAP PROPERTIES (P1 Phase 2)
    // ==========================================================================
    pub const DROP_CAP_LINES: u64 = 125; // $125 - number of lines drop cap spans
    pub const DROP_CAP_CHARS: u64 = 126; // $126 - number of characters in drop cap

    // ==========================================================================
    // TRANSFORM PROPERTIES (P2 Phase 2)
    // ==========================================================================
    pub const TRANSFORM: u64 = 98; // $98 - transform property (6-element matrix array)
    pub const TRANSFORM_ORIGIN: u64 = 549; // $549 - transform-origin property
    // Note: Transform-origin uses $59 (left/x) and $58 (top/y) as sub-properties

    // ==========================================================================
    // BASELINE-SHIFT (P2 Phase 2)
    // ==========================================================================
    pub const BASELINE_SHIFT: u64 = 31; // $31 - baseline-shift (numeric value for vertical tuning)

    // ==========================================================================
    // COLUMN PROPERTIES (P2 Phase 2)
    // ==========================================================================
    pub const COLUMN_COUNT: u64 = 112; // $112 - column-count property
    pub const COLUMN_COUNT_AUTO: u64 = 383; // $383 - column-count: auto (same as BLOCK_TYPE_BLOCK)

    // ==========================================================================
    // FLOAT PROPERTIES (P2 Phase 2)
    // ==========================================================================
    pub const FLOAT: u64 = 140; // $140 - float property
    pub const FLOAT_LEFT: u64 = 59; // $59 - float: left (same as ALIGN_LEFT)
    pub const FLOAT_RIGHT: u64 = 61; // $61 - float: right (same as ALIGN_RIGHT)
    pub const FLOAT_SNAP_BLOCK: u64 = 786; // $786 - float: snap-block (KFX-specific)

    // ==========================================================================
    // LAYOUT HINTS (P2 Phase 2)
    // ==========================================================================
    pub const LAYOUT_HINTS: u64 = 761; // $761 - layout hints list
    pub const LAYOUT_HINT_HEADING: u64 = 760; // $760 - heading hint
    pub const LAYOUT_HINT_CAPTION: u64 = 453; // $453 - caption hint
    pub const LAYOUT_HINT_FIGURE: u64 = 282; // $282 - figure hint

    // ==========================================================================
    // BORDER RADIUS ($459-$462)
    // ==========================================================================
    pub const BORDER_RADIUS_TL: u64 = 459; // $459 - border-top-left-radius
    pub const BORDER_RADIUS_TR: u64 = 460; // $460 - border-top-right-radius
    pub const BORDER_RADIUS_BR: u64 = 461; // $461 - border-bottom-right-radius
    pub const BORDER_RADIUS_BL: u64 = 462; // $462 - border-bottom-left-radius

    // ==========================================================================
    // BORDER PROPERTIES (from yj_to_epub_properties.py)
    // ==========================================================================
    // Border color
    pub const BORDER_COLOR: u64 = 83; // $83 - border-color (shorthand)
    pub const BORDER_TOP_COLOR: u64 = 84; // $84 - border-top-color
    pub const BORDER_LEFT_COLOR: u64 = 85; // $85 - border-left-color
    pub const BORDER_BOTTOM_COLOR: u64 = 86; // $86 - border-bottom-color
    pub const BORDER_RIGHT_COLOR: u64 = 87; // $87 - border-right-color

    // Border style
    pub const BORDER_STYLE: u64 = 88; // $88 - border-style (shorthand)
    pub const BORDER_TOP_STYLE: u64 = 89; // $89 - border-top-style
    pub const BORDER_LEFT_STYLE: u64 = 90; // $90 - border-left-style
    pub const BORDER_BOTTOM_STYLE: u64 = 91; // $91 - border-bottom-style
    pub const BORDER_RIGHT_STYLE: u64 = 92; // $92 - border-right-style

    // Border width
    pub const BORDER_WIDTH: u64 = 93; // $93 - border-width (shorthand)
    pub const BORDER_TOP_WIDTH: u64 = 94; // $94 - border-top-width
    pub const BORDER_LEFT_WIDTH: u64 = 95; // $95 - border-left-width
    pub const BORDER_BOTTOM_WIDTH: u64 = 96; // $96 - border-bottom-width
    pub const BORDER_RIGHT_WIDTH: u64 = 97; // $97 - border-right-width

    // Border style values (from BORDER_STYLES)
    pub const BORDER_STYLE_NONE: u64 = 349; // $349 - none
    pub const BORDER_STYLE_SOLID: u64 = 328; // $328 - solid
    pub const BORDER_STYLE_DOTTED: u64 = 331; // $331 - dotted
    pub const BORDER_STYLE_DASHED: u64 = 330; // $330 - dashed
    pub const BORDER_STYLE_DOUBLE: u64 = 329; // $329 - double
    pub const BORDER_STYLE_RIDGE: u64 = 335; // $335 - ridge
    pub const BORDER_STYLE_GROOVE: u64 = 334; // $334 - groove
    pub const BORDER_STYLE_INSET: u64 = 336; // $336 - inset
    pub const BORDER_STYLE_OUTSET: u64 = 337; // $337 - outset

    // ==========================================================================
    // TABLE PROPERTIES
    // ==========================================================================
    pub const ATTRIB_COLSPAN: u64 = 148; // $148 - -kfx-attrib-colspan
    pub const ATTRIB_ROWSPAN: u64 = 149; // $149 - -kfx-attrib-rowspan
    // BORDER_COLLAPSE is at $150 (defined above)
    pub const CAPTION_SIDE: u64 = 453; // $453 - caption-side property
    pub const BORDER_SPACING_VERTICAL: u64 = 456; // $456 - -webkit-border-vertical-spacing
    pub const BORDER_SPACING_HORIZONTAL: u64 = 457; // $457 - -webkit-border-horizontal-spacing
    pub const EMPTY_CELLS: u64 = 458; // $458 - empty-cells (boolean: true=hide)

    // ==========================================================================
    // BOX SIZING / IMAGE FIT ($546)
    // Note: In CSS this is "box-sizing", but KFX uses same symbol for image fitting
    // ==========================================================================
    pub const BOX_SIZING: u64 = 546; // $546 - box-sizing / image fit mode
    pub const BOX_SIZING_CONTENT_BOX: u64 = 377; // $377 - content-box (images: contain-like)
    pub const BOX_SIZING_BORDER_BOX: u64 = 378; // $378 - border-box (images: none/baseline)
    pub const BOX_SIZING_PADDING_BOX: u64 = 379; // $379 - padding-box

    // Legacy aliases for image fit (same symbols)
    pub const IMAGE_FIT: u64 = 546; // alias for BOX_SIZING
    pub const IMAGE_FIT_CONTAIN: u64 = 377; // alias for BOX_SIZING_CONTENT_BOX
    pub const IMAGE_FIT_NONE: u64 = 378; // alias for BOX_SIZING_BORDER_BOX

    // ==========================================================================
    // BOX ALIGN / IMAGE LAYOUT ($580)
    // ==========================================================================
    pub const BOX_ALIGN: u64 = 580; // $580 - -kfx-box-align / image layout
    pub const IMAGE_LAYOUT: u64 = 580; // alias for BOX_ALIGN

    // Hyphens property ($127)
    // Note: Previously incorrectly named STYLE_BLOCK_TYPE - $127 is actually CSS "hyphens"
    pub const HYPHENS: u64 = 127; // $127 - CSS hyphens property
    pub const HYPHENS_AUTO: u64 = 383; // $383 - hyphens: auto
    pub const HYPHENS_MANUAL: u64 = 384; // $384 - hyphens: manual
    pub const HYPHENS_NONE: u64 = 349; // $349 - hyphens: none

    // ==========================================================================
    // LIST STYLE TYPE VALUES ($100) - P1 improvement
    // ==========================================================================
    pub const LIST_TYPE: u64 = 100; // $100 - list-style-type property
    pub const LIST_TYPE_DISC: u64 = 340; // $340 - list-style-type: disc
    pub const LIST_TYPE_SQUARE: u64 = 341; // $341 - list-style-type: square
    pub const LIST_TYPE_CIRCLE: u64 = 342; // $342 - list-style-type: circle
    pub const LIST_TYPE_DECIMAL: u64 = 343; // $343 - list-style-type: decimal
    pub const LIST_TYPE_LOWER_ROMAN: u64 = 344; // $344 - list-style-type: lower-roman
    pub const LIST_TYPE_UPPER_ROMAN: u64 = 345; // $345 - list-style-type: upper-roman
    pub const LIST_TYPE_LOWER_ALPHA: u64 = 346; // $346 - list-style-type: lower-alpha
    pub const LIST_TYPE_UPPER_ALPHA: u64 = 347; // $347 - list-style-type: upper-alpha
    pub const LIST_TYPE_NONE: u64 = 349; // $349 - list-style-type: none

    // List style position ($551)
    pub const LIST_POSITION: u64 = 551; // $551 - list-style-position property
    pub const LIST_POSITION_INSIDE: u64 = 552; // $552 - list-style-position: inside
    pub const LIST_POSITION_OUTSIDE: u64 = 553; // $553 - list-style-position: outside

    // List content types
    pub const CONTENT_LIST: u64 = 276; // $276 - content type for list container (ol/ul)
    pub const CONTENT_LIST_ITEM: u64 = 277; // $277 - content type for list item (li)

    // Additional content types
    pub const HIDDEN_CONTAINER: u64 = 439; // $439 - hidden container (display: none)
    pub const HORIZONTAL_RULE: u64 = 596; // $596 - horizontal rule (<hr>)

    // Content symbols
    pub const SECTION_CONTENT: u64 = 141; // $141 - section content list
    pub const INLINE_STYLE_RUNS: u64 = 142; // $142 - inline style runs array
    pub const TEXT_CONTENT: u64 = 145; // $145 - text content fragment type

    /// Maximum size for a text content chunk (in characters)
    /// Larger chapters are split into multiple chunks
    pub const MAX_CHUNK_SIZE: usize = 15000;
    pub const CONTENT_ARRAY: u64 = 146; // $146 - array of content items
    pub const DESCRIPTION: u64 = 154; // $154 - description
    pub const POSITION: u64 = 155; // $155 - position / EID
    pub const STYLE: u64 = 157; // $157 - style fragment type
    pub const CONTENT_TYPE: u64 = 159; // $159 - content type symbol
    pub const FORMAT: u64 = 161; // $161 - format
    pub const LOCATION: u64 = 165; // $165 - resource location
    pub const READING_ORDERS: u64 = 169; // $169 - reading orders list
    pub const SECTIONS_LIST: u64 = 170; // $170 - list of sections
    pub const STYLE_NAME: u64 = 173; // $173 - style name/id
    pub const SECTION_NAME: u64 = 174; // $174 - section name/id
    pub const RESOURCE_NAME: u64 = 175; // $175 - external resource name
    pub const CONTENT_NAME: u64 = 176; // $176 - content block name/id
    pub const READING_ORDER_NAME: u64 = 178; // $178 - reading order name
    pub const ENTITY_LIST: u64 = 181; // $181 - list of entities
    pub const LOCATION_ENTRIES: u64 = 182; // $182 - location entries list

    // Navigation symbols
    pub const OFFSET: u64 = 143; // $143 - offset within section/content
    pub const COUNT: u64 = 144; // $144 - count/length
    pub const NAV_TYPE: u64 = 235; // $235 - navigation type
    pub const TOC: u64 = 212; // $212 - table of contents nav type
    pub const LANDMARKS_NAV_TYPE: u64 = 236; // $236 - landmarks navigation type value
    pub const LANDMARKS: u64 = 237; // $237 - landmarks
    pub const LANDMARK_TYPE: u64 = 238; // $238 - landmark type field
    pub const NAV_ID: u64 = 239; // $239 - nav container id reference
    pub const NAV_UNIT_REF: u64 = 240; // $240 - nav unit reference
    pub const NAV_TITLE: u64 = 241; // $241 - navigation title struct
    pub const TEXT: u64 = 244; // $244 - text content field
    pub const NAV_TARGET: u64 = 246; // $246 - navigation target struct
    pub const NAV_ENTRIES: u64 = 247; // $247 - navigation entries list
    pub const NAV_CONTAINER: u64 = 249; // $249 - nav container
    pub const CONTAINER_CONTENTS: u64 = 252; // $252 - container contents
    pub const ENTITY_DEPS: u64 = 253; // $253 - entity dependencies
    pub const MANDATORY_DEPS: u64 = 254; // $254 - mandatory dependencies

    // Metadata symbols
    pub const METADATA: u64 = 258; // $258 - metadata fragment type
    pub const CONTENT_BLOCK: u64 = 259; // $259 - content block fragment type
    pub const SECTION: u64 = 260; // $260 - section fragment type
    pub const POSITION_MAP: u64 = 264; // $264 - position map
    pub const POSITION_ID_MAP: u64 = 265; // $265 - position id map
    pub const PAGE_TEMPLATE: u64 = 266; // $266 - page template
    pub const CONTENT_PARAGRAPH: u64 = 269; // $269 - paragraph content type

    // Table content types
    pub const CONTENT_TABLE: u64 = 278; // $278 - table content type
    pub const CONTENT_TABLE_ROW: u64 = 279; // $279 - table row (tr) content type
    pub const CONTENT_THEAD: u64 = 151; // $151 - thead content type
    pub const CONTENT_TBODY: u64 = 454; // $454 - tbody content type
    pub const CONTENT_TFOOT: u64 = 455; // $455 - tfoot content type
    pub const COL_SPAN: u64 = 118; // $118 - column span in colgroup

    // Content item role/position indicator ($790)
    // This field appears on paragraph content items and indicates their role:
    // - 2: First paragraph in content block (index 0)
    // - 3: Normal paragraph
    // - 4: Special paragraph (endnotes, back matter)
    pub const CONTENT_ROLE: u64 = 790; // $790 - content item role indicator

    // ==========================================================================
    // FOOTNOTE/ENDNOTE SYMBOLS (for popup footnotes)
    // ==========================================================================
    pub const CLASSIFICATION: u64 = 615; // $615 - content classification (footnote/endnote)
    pub const NOTEREF_TYPE: u64 = 616; // $616 - noteref type field in inline style runs
    pub const NOTEREF: u64 = 617; // $617 - noteref value (link is a note reference)
    pub const FOOTNOTE: u64 = 618; // $618 - footnote classification value
    pub const ENDNOTE: u64 = 619; // $619 - endnote classification value

    // Page template / anchor symbols
    pub const ANCHOR_REF: u64 = 179; // $179 - reference to anchor fragment in inline style runs
    pub const TEMPLATE_NAME: u64 = 180; // $180 - template name/id (also anchor ID)
    pub const POSITION_INFO: u64 = 183; // $183 - position info struct (contains P155, optional P143)
    pub const EXTERNAL_URL: u64 = 186; // $186 - external URL for anchor fragments
    pub const CONTAINER_INFO: u64 = 270; // $270 - container info fragment type

    // Section dimension symbols
    pub const SECTION_WIDTH: u64 = 66; // $66 - section width in pixels
    pub const SECTION_HEIGHT: u64 = 67; // $67 - section height in pixels
    pub const DEFAULT_TEXT_ALIGN: u64 = 140; // $140 - default text alignment for section
    pub const PAGE_LAYOUT: u64 = 156; // $156 - page layout type
    pub const LAYOUT_FULL_PAGE: u64 = 326; // $326 - full page layout value

    // Value/metadata symbols
    pub const LANDMARK_COVER: u64 = 233; // $233 - cover landmark type
    pub const DEFAULT_READING_ORDER: u64 = 351; // $351 - default reading order name
    pub const LANDMARK_BODYMATTER: u64 = 396; // $396 - bodymatter landmark type

    // Navigation fragment symbols
    pub const BOOK_NAVIGATION: u64 = 389; // $389 - book navigation fragment type
    pub const NAV_CONTAINER_TYPE: u64 = 391; // $391 - nav container fragment type
    pub const NAV_CONTAINER_REF: u64 = 392; // $392 - nav container reference
    pub const NAV_DEFINITION: u64 = 393; // $393 - nav unit fragment type (navigation entry)
    pub const NAV_UNIT_NAME: u64 = 394; // $394 - nav unit name field
    pub const NAV_UNIT_LIST: u64 = 395; // $395 - nav unit list (used for empty nav structure)

    // Resource symbols
    pub const TEXT_OFFSET: u64 = 403; // $403 - text offset
    pub const CONTAINER_ID: u64 = 409; // $409 - container ID string
    pub const COMPRESSION_TYPE: u64 = 410; // $410 - compression type
    pub const DRM_SCHEME: u64 = 411; // $411 - DRM scheme
    pub const CHUNK_SIZE: u64 = 412; // $412 - chunk size
    pub const INDEX_TABLE_OFFSET: u64 = 413; // $413 - index table offset
    pub const INDEX_TABLE_LENGTH: u64 = 414; // $414 - index table length
    pub const SYMBOL_TABLE_OFFSET: u64 = 415; // $415 - symbol table offset
    pub const SYMBOL_TABLE_LENGTH: u64 = 416; // $416 - symbol table length
    pub const RAW_MEDIA: u64 = 417; // $417 - raw media fragment type
    pub const CONTAINER_ENTITY_MAP: u64 = 419; // $419 - container entity map
    pub const WIDTH: u64 = 422; // $422 - image width in pixels
    pub const HEIGHT: u64 = 423; // $423 - image height in pixels

    // Resource symbols
    pub const RESOURCE: u64 = 164; // $164 - resource fragment type
    pub const PNG_FORMAT: u64 = 284; // $284 - PNG image format
    pub const JPG_FORMAT: u64 = 285; // $285 - JPEG image format
    pub const GIF_FORMAT: u64 = 286; // $286 - GIF image format (also used for fonts)
    pub const IMAGE_CONTENT: u64 = 271; // $271 - image content type
    pub const IMAGE_ALT_TEXT: u64 = 584; // $584 - alt text for image accessibility
    pub const MIME_TYPE: u64 = 162; // $162 - MIME type string
    pub const FONT_FORMAT: u64 = 286; // $286 - font format type

    // Metadata entry symbols
    pub const KINDLE_METADATA: u64 = 490; // $490 - kindle metadata fragment type
    pub const METADATA_ENTRIES: u64 = 491; // $491 - metadata entries list
    pub const METADATA_KEY: u64 = 492; // $492 - metadata key
    pub const METADATA_GROUP: u64 = 495; // $495 - metadata group name

    // Document structure symbols
    pub const DOCUMENT_DATA: u64 = 538; // $538 - document data fragment type
    pub const LOCATION_MAP: u64 = 550; // $550 - location map fragment type

    // Position map symbols
    pub const ENTITY_ID_LIST: u64 = 181; // $181 - list of entity IDs
    pub const EID_INDEX: u64 = 184; // $184 - EID index (character offset)
    pub const EID_VALUE: u64 = 185; // $185 - EID value (position ID)

    // Format capabilities symbols
    pub const FORMAT_CAPABILITIES_OLD: u64 = 585; // $585 - old format capabilities
    pub const CAPABILITY_NAME: u64 = 586; // $586 - capability provider name
    pub const MIN_VERSION: u64 = 587; // $587 - kfxgen app version / min ver
    pub const VERSION: u64 = 588; // $588 - kfxgen package version / version
    pub const CAPABILITY_VERSION: u64 = 589; // $589 - capability version struct
    pub const CAPABILITIES_LIST: u64 = 590; // $590 - capabilities list
    pub const FORMAT_CAPABILITIES: u64 = 593; // $593 - format capabilities fragment type
    pub const FC_OFFSET: u64 = 594; // $594 - format capabilities offset
    pub const FC_LENGTH: u64 = 595; // $595 - format capabilities length

    // Auxiliary data symbols
    pub const AUXILIARY_DATA: u64 = 597; // $597 - auxiliary/section metadata
    pub const AUX_DATA_REF: u64 = 598; // $598 - auxiliary data reference

    // Special singleton ID
    pub const SINGLETON_ID: u64 = 348; // $348 - used for singleton entity IDs
}

// =============================================================================
// Symbol Table
// =============================================================================

/// Simple symbol table for tracking local symbols
#[derive(Debug, Default)]
pub struct SymbolTable {
    /// Local symbols (book-specific IDs)
    local_symbols: Vec<String>,
    /// Map from symbol name to ID
    symbol_map: HashMap<String, u64>,
    /// Next local symbol ID (starts after YJ_symbols max_id)
    next_id: u64,
}

impl SymbolTable {
    /// Local symbol IDs start here (after YJ_symbols shared table)
    pub const LOCAL_MIN_ID: u64 = 860;

    pub fn new() -> Self {
        Self {
            local_symbols: Vec::new(),
            symbol_map: HashMap::new(),
            next_id: Self::LOCAL_MIN_ID,
        }
    }

    /// Get or create a symbol ID for a name
    pub fn get_or_intern(&mut self, name: &str) -> u64 {
        // Check if it's a shared symbol reference (starts with $)
        if let Some(id_str) = name.strip_prefix('$')
            && let Ok(id) = id_str.parse::<u64>()
        {
            return id;
        }

        // Check if already interned
        if let Some(&id) = self.symbol_map.get(name) {
            return id;
        }

        // Create new local symbol
        let id = self.next_id;
        self.next_id += 1;
        self.local_symbols.push(name.to_string());
        self.symbol_map.insert(name.to_string(), id);
        id
    }

    /// Get symbol ID without interning (returns None if not found)
    pub fn get(&self, name: &str) -> Option<u64> {
        if let Some(id_str) = name.strip_prefix('$')
            && let Ok(id) = id_str.parse::<u64>()
        {
            return Some(id);
        }
        self.symbol_map.get(name).copied()
    }

    /// Get local symbols for $ion_symbol_table fragment
    #[allow(dead_code)]
    pub fn local_symbols(&self) -> &[String] {
        &self.local_symbols
    }

    /// Create the $ion_symbol_table import structure
    pub fn create_import(&self) -> IonValue {
        let mut import = HashMap::new();
        import.insert(4, IonValue::String("YJ_symbols".to_string())); // name
        import.insert(5, IonValue::Int(10)); // version
        import.insert(8, IonValue::Int(Self::LOCAL_MIN_ID as i64 - 1)); // max_id

        let mut symtab = HashMap::new();
        symtab.insert(6, IonValue::List(vec![IonValue::Struct(import)])); // imports

        if !self.local_symbols.is_empty() {
            let symbols: Vec<IonValue> = self
                .local_symbols
                .iter()
                .map(|s| IonValue::String(s.clone()))
                .collect();
            symtab.insert(7, IonValue::List(symbols)); // symbols
        }

        IonValue::Struct(symtab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table() {
        let mut symtab = SymbolTable::new();

        // Shared symbols should return their ID
        assert_eq!(symtab.get_or_intern("$260"), 260);
        assert_eq!(symtab.get_or_intern("$145"), 145);

        // Local symbols should get new IDs
        let id1 = symtab.get_or_intern("section-1");
        let id2 = symtab.get_or_intern("section-2");
        assert!(id1 >= SymbolTable::LOCAL_MIN_ID);
        assert_eq!(id2, id1 + 1);

        // Same symbol should return same ID
        assert_eq!(symtab.get_or_intern("section-1"), id1);
    }

    #[test]
    fn test_unicode_bidi_symbols_exist() {
        // Verify unicode-bidi symbols are defined correctly (TDD Phase 1.1)
        assert_eq!(sym::UNICODE_BIDI, 674);
        assert_eq!(sym::BIDI_EMBED, 675);
        assert_eq!(sym::BIDI_ISOLATE, 676);
        assert_eq!(sym::BIDI_OVERRIDE, 677);
        assert_eq!(sym::BIDI_ISOLATE_OVERRIDE, 678);
        assert_eq!(sym::BIDI_PLAINTEXT, 679);
    }

    #[test]
    fn test_line_break_symbols_exist() {
        // Verify line-break symbols are defined correctly (TDD Phase 1.2)
        assert_eq!(sym::LINE_BREAK, 780);
        assert_eq!(sym::LINE_BREAK_LOOSE, 781);
        assert_eq!(sym::LINE_BREAK_STRICT, 782);
        assert_eq!(sym::LINE_BREAK_ANYWHERE, 783);
    }

    #[test]
    fn test_text_orientation_symbols_exist() {
        // Verify text-orientation symbols are defined correctly (TDD Phase 1.3)
        assert_eq!(sym::TEXT_ORIENTATION, 706);
        assert_eq!(sym::TEXT_ORIENTATION_UPRIGHT, 779);
        assert_eq!(sym::TEXT_ORIENTATION_SIDEWAYS, 778);
        assert_eq!(sym::TEXT_ORIENTATION_MIXED, 383); // Reuses BREAK_AUTO
    }

    #[test]
    fn test_content_type_symbols_exist() {
        // Verify content type symbols for hr and hidden container (TDD Phase 3)
        assert_eq!(sym::HORIZONTAL_RULE, 596);
        assert_eq!(sym::HIDDEN_CONTAINER, 439);
    }
}
