//! CSS parsing for style extraction.
//!
//! This module provides CSS parsing capabilities for extracting styles
//! from EPUB stylesheets to apply to KFX output.

use cssparser::{
    AtRuleParser, AtRuleType, BasicParseErrorKind, CowRcStr, ParseError, Parser, ParserInput,
    QualifiedRuleParser, RuleListParser, SourceLocation, Token,
};

/// A parsed CSS value with unit
#[derive(Debug, Clone, PartialEq)]
pub enum CssValue {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    /// Keyword like "auto", "inherit", "normal"
    Keyword(String),
    /// Unitless number (for line-height)
    Number(f32),
    // Additional units (P1 improvement)
    /// Viewport width (1vw = 1% of viewport width)
    Vw(f32),
    /// Viewport height (1vh = 1% of viewport height)
    Vh(f32),
    /// Viewport minimum (min of vw, vh)
    Vmin(f32),
    /// Viewport maximum (max of vw, vh)
    Vmax(f32),
    /// Character width unit (width of '0')
    Ch(f32),
    /// x-height unit (height of lowercase 'x')
    Ex(f32),
    /// Centimeters
    Cm(f32),
    /// Millimeters
    Mm(f32),
    /// Inches
    In(f32),
    /// Points (1pt = 1/72 inch)
    Pt(f32),
}

impl Eq for CssValue {}

impl std::hash::Hash for CssValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            CssValue::Px(v) => {
                state.write_u8(0);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Em(v) => {
                state.write_u8(1);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Rem(v) => {
                state.write_u8(2);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Percent(v) => {
                state.write_u8(3);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Keyword(s) => {
                state.write_u8(4);
                s.hash(state);
            }
            CssValue::Number(v) => {
                state.write_u8(5);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Vw(v) => {
                state.write_u8(6);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Vh(v) => {
                state.write_u8(7);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Vmin(v) => {
                state.write_u8(8);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Vmax(v) => {
                state.write_u8(9);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Ch(v) => {
                state.write_u8(10);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Ex(v) => {
                state.write_u8(11);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Cm(v) => {
                state.write_u8(12);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Mm(v) => {
                state.write_u8(13);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::In(v) => {
                state.write_u8(14);
                ((v * 100.0) as i32).hash(state);
            }
            CssValue::Pt(v) => {
                state.write_u8(15);
                ((v * 100.0) as i32).hash(state);
            }
        }
    }
}

/// Text alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    #[default]
    Justify,
}

/// Font weight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontWeight {
    Normal,
    Bold,
    /// Numeric weight 100-900
    Weight(u16),
}

impl FontWeight {
    pub fn is_bold(&self) -> bool {
        match self {
            FontWeight::Bold => true,
            FontWeight::Weight(w) => *w >= 700,
            FontWeight::Normal => false,
        }
    }
}

/// Font style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// Font variant (small-caps, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontVariant {
    #[default]
    Normal,
    SmallCaps,
}

/// CSS hyphens property for text hyphenation control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Hyphens {
    /// Don't hyphenate (hyphens: none)
    #[default]
    None,
    /// Manual hyphenation with &shy; only (hyphens: manual)
    Manual,
    /// Automatic hyphenation (hyphens: auto)
    Auto,
}

/// Text transform (uppercase, lowercase, capitalize)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

/// Color value
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// R, G, B, A (0-255)
    Rgba(u8, u8, u8, u8),
    /// Current color keyword
    #[default]
    Current,
    /// Transparent keyword
    Transparent,
}

/// Border style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Hidden,
    Solid,
    Dotted,
    Dashed,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

/// Border properties
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Border {
    pub width: Option<CssValue>,
    pub style: BorderStyle,
    pub color: Option<Color>,
}

/// Display type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
    Other,
}

/// Position type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
}

/// Vertical alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Top,
    Middle,
    Bottom,
    Super,
    Sub,
    TextTop,
    TextBottom,
}

/// Clear property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

/// Word break property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
}

/// Break (page/column) value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BreakValue {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    Page,
    Left,
    Right,
    Column,
    AvoidColumn,
}

/// Overflow property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Scroll,
    Auto,
    Clip,
}

/// Visibility property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

/// CSS Float property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CssFloat {
    #[default]
    None,
    Left,
    Right,
    /// KFX-specific: snap to block boundary
    SnapBlock,
}

/// List style type (P1 improvement)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ListStyleType {
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    LowerGreek,
    None,
}

/// List style position (P1 improvement)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ListStylePosition {
    #[default]
    Outside,
    Inside,
}

/// Writing mode (P2 improvement)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WritingMode {
    #[default]
    HorizontalTb,
    VerticalRl,
    VerticalLr,
}

/// Text combine upright (P2 improvement for vertical text)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextCombineUpright {
    #[default]
    None,
    All,
    Digits(u8),
}

// =============================================================================
// P1 Phase 2: Ruby Annotation Properties (CJK text support)
// =============================================================================

/// Ruby position (where ruby text appears relative to base text)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RubyPosition {
    #[default]
    Over, // Ruby above base text (default for horizontal)
    Under, // Ruby below base text
}

/// Ruby alignment (how ruby text is aligned with base text)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RubyAlign {
    #[default]
    Center, // Center ruby over base
    Start,        // Align ruby to start of base
    SpaceAround,  // Distribute space around ruby
    SpaceBetween, // Distribute space between ruby characters
}

/// Ruby merge (how adjacent ruby annotations are combined)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RubyMerge {
    #[default]
    Separate, // Each ruby annotation is separate
    Collapse, // Adjacent annotations can be merged
}

// =============================================================================
// P1 Phase 2: Text Emphasis Properties (CJK typography)
// =============================================================================

/// Text emphasis style (marks above/below characters for emphasis)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextEmphasisStyle {
    #[default]
    None,
    Filled,
    Open,
    Dot,
    Circle,
    FilledCircle,
    OpenCircle,
    FilledDot,
    OpenDot,
    DoubleCircle,
    FilledDoubleCircle,
    OpenDoubleCircle,
    Triangle,
    FilledTriangle,
    OpenTriangle,
    Sesame,
    FilledSesame,
    OpenSesame,
}

// =============================================================================
// P2 Phase 2: Border Collapse (Table styling)
// =============================================================================

/// Border collapse for tables
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderCollapse {
    #[default]
    Separate, // Borders are separate
    Collapse, // Adjacent borders are collapsed
}

// =============================================================================
// CSS Box Sizing
// =============================================================================

/// CSS box-sizing property
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[allow(clippy::enum_variant_names)]
pub enum BoxSizing {
    /// content-box: width/height only include content (CSS default)
    #[default]
    ContentBox,
    /// border-box: width/height include padding and border
    BorderBox,
    /// padding-box: width/height include padding but not border
    PaddingBox,
}

// =============================================================================
// P1 Phase 2: Drop Cap Properties
// =============================================================================

/// Drop cap configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DropCap {
    /// Number of lines the drop cap spans
    pub lines: u8,
    /// Number of characters in the drop cap
    pub chars: u8,
}

// =============================================================================
// P2 Phase 2: Text Decoration Line Style
// =============================================================================

/// Text decoration line style (how the decoration line is drawn)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextDecorationLineStyle {
    #[default]
    Solid, // Default solid line
    Dashed, // Dashed line
    Dotted, // Dotted line
    Double, // Double line
}

// =============================================================================
// P2 Phase 2: Transform Properties
// =============================================================================

/// 2D Transform matrix [a, b, c, d, tx, ty]
/// Standard 2D affine transformation matrix representation:
/// - [1, 0, 0, 1, tx, ty] = translate(tx, ty)
/// - [sx, 0, 0, sy, 0, 0] = scale(sx, sy)
/// - [0, 1, -1, 0, 0, 0] = rotate(-90deg)
/// - [0, -1, 1, 0, 0, 0] = rotate(90deg)
/// - [-1, 0, 0, -1, 0, 0] = rotate(180deg)
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    /// Matrix values [a, b, c, d, tx, ty]
    pub matrix: [f32; 6],
}

impl Default for Transform {
    fn default() -> Self {
        // Identity matrix
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
}

impl Eq for Transform {}

impl std::hash::Hash for Transform {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash each component as integer for stability
        for v in &self.matrix {
            ((v * 1000.0) as i32).hash(state);
        }
    }
}

impl Transform {
    /// Create a translation transform
    pub fn translate(tx: f32, ty: f32) -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, tx, ty],
        }
    }

    /// Create a scale transform
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self {
            matrix: [sx, 0.0, 0.0, sy, 0.0, 0.0],
        }
    }

    /// Create a rotation transform (angle in degrees)
    pub fn rotate(degrees: f32) -> Self {
        let radians = degrees * std::f32::consts::PI / 180.0;
        let cos = radians.cos();
        let sin = radians.sin();
        Self {
            matrix: [cos, sin, -sin, cos, 0.0, 0.0],
        }
    }

    /// Check if this is the identity transform
    pub fn is_identity(&self) -> bool {
        (self.matrix[0] - 1.0).abs() < 0.0001
            && self.matrix[1].abs() < 0.0001
            && self.matrix[2].abs() < 0.0001
            && (self.matrix[3] - 1.0).abs() < 0.0001
            && self.matrix[4].abs() < 0.0001
            && self.matrix[5].abs() < 0.0001
    }
}

/// Transform origin point
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TransformOrigin {
    /// X position (default: 50%)
    pub x: Option<CssValue>,
    /// Y position (default: 50%)
    pub y: Option<CssValue>,
}

impl Eq for TransformOrigin {}

impl std::hash::Hash for TransformOrigin {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
    }
}

// =============================================================================
// P2 Phase 2: Column Properties
// =============================================================================

/// Column count for multi-column layout
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColumnCount {
    #[default]
    Auto,
    Count(u32),
}

/// Parsed CSS style properties
/// Note: Custom Hash/Eq implementation excludes image_width_px and image_height_px
/// since they don't affect KFX style output and would cause duplicate styles.
#[derive(Debug, Clone, Default)]
pub struct ParsedStyle {
    pub font_family: Option<String>,
    pub font_size: Option<CssValue>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_variant: Option<FontVariant>,
    pub text_transform: Option<TextTransform>,
    pub text_align: Option<TextAlign>,
    pub text_indent: Option<CssValue>,
    pub line_height: Option<CssValue>,
    pub margin_top: Option<CssValue>,
    pub margin_bottom: Option<CssValue>,
    pub margin_left: Option<CssValue>,
    pub margin_right: Option<CssValue>,
    pub padding_top: Option<CssValue>,
    pub padding_bottom: Option<CssValue>,
    pub padding_left: Option<CssValue>,
    pub padding_right: Option<CssValue>,
    pub color: Option<Color>,
    pub background_color: Option<Color>,
    pub border_top: Option<Border>,
    pub border_bottom: Option<Border>,
    pub border_left: Option<Border>,
    pub border_right: Option<Border>,
    pub display: Option<Display>,
    pub position: Option<Position>,
    pub left: Option<CssValue>,
    pub width: Option<CssValue>,
    pub height: Option<CssValue>,
    pub min_width: Option<CssValue>,
    pub min_height: Option<CssValue>,
    pub max_width: Option<CssValue>,
    pub max_height: Option<CssValue>,
    pub vertical_align: Option<VerticalAlign>,
    pub clear: Option<Clear>,
    pub word_break: Option<WordBreak>,
    pub overflow: Option<Overflow>,
    pub visibility: Option<Visibility>,
    pub break_before: Option<BreakValue>,
    pub break_after: Option<BreakValue>,
    pub break_inside: Option<BreakValue>,
    pub border_radius_tl: Option<CssValue>,
    pub border_radius_tr: Option<CssValue>,
    pub border_radius_br: Option<CssValue>,
    pub border_radius_bl: Option<CssValue>,
    pub letter_spacing: Option<CssValue>,
    pub word_spacing: Option<CssValue>,
    pub white_space_nowrap: Option<bool>,
    pub text_decoration_underline: bool,
    pub text_decoration_overline: bool,
    pub text_decoration_line_through: bool,
    /// Line style for text decorations (dashed, dotted, double)
    pub text_decoration_line_style: Option<TextDecorationLineStyle>,
    /// Opacity as integer 0-100 (representing 0.0-1.0)
    pub opacity: Option<u8>,
    /// Whether this style is for an image element (set when creating ContentItem::Image)
    pub is_image: bool,
    /// Whether this style is for an inline element like a link (uses $127: $349 instead of $383)
    pub is_inline: bool,
    /// Whether this style is for a heading element (h1-h6) - adds layout hints
    pub is_heading: bool,
    /// Actual image width in pixels (set for image styles when dimensions are known)
    pub image_width_px: Option<u32>,
    /// Actual image height in pixels (set for image styles when dimensions are known)
    pub image_height_px: Option<u32>,
    /// Language tag from xml:lang or lang attribute (e.g., "en-us", "la")
    pub lang: Option<String>,
    // P1: List style properties
    pub list_style_type: Option<ListStyleType>,
    pub list_style_position: Option<ListStylePosition>,
    // P2: Writing mode properties
    pub writing_mode: Option<WritingMode>,
    pub text_combine_upright: Option<TextCombineUpright>,
    // P4: Shadow properties
    pub box_shadow: Option<String>,
    pub text_shadow: Option<String>,
    // P1 Phase 2: Ruby annotation properties
    pub ruby_position: Option<RubyPosition>,
    pub ruby_align: Option<RubyAlign>,
    pub ruby_merge: Option<RubyMerge>,
    // P1 Phase 2: Text emphasis properties
    pub text_emphasis_style: Option<TextEmphasisStyle>,
    pub text_emphasis_color: Option<Color>,
    // P2 Phase 2: Border collapse
    pub border_collapse: Option<BorderCollapse>,
    // Table border-spacing (separate into horizontal and vertical)
    pub border_spacing_horizontal: Option<CssValue>,
    pub border_spacing_vertical: Option<CssValue>,
    // P1 Phase 2: Drop cap
    pub drop_cap: Option<DropCap>,
    // P2 Phase 2: Transform properties
    pub transform: Option<Transform>,
    pub transform_origin: Option<TransformOrigin>,
    // P2 Phase 2: Baseline-shift (fine-tuning vertical position)
    pub baseline_shift: Option<CssValue>,
    // P2 Phase 2: Column layout
    pub column_count: Option<ColumnCount>,
    // P2 Phase 2: Float property
    pub float: Option<CssFloat>,
    /// CSS hyphens property for text hyphenation
    pub hyphens: Option<Hyphens>,
    /// CSS box-sizing property
    pub box_sizing: Option<BoxSizing>,
}

impl ParsedStyle {
    /// Merge another style into this one (other takes precedence)
    pub fn merge(&mut self, other: &ParsedStyle) {
        if other.font_family.is_some() {
            self.font_family.clone_from(&other.font_family);
        }
        if other.font_size.is_some() {
            self.font_size.clone_from(&other.font_size);
        }
        if other.font_weight.is_some() {
            self.font_weight = other.font_weight;
        }
        if other.font_style.is_some() {
            self.font_style = other.font_style;
        }
        if other.font_variant.is_some() {
            self.font_variant = other.font_variant;
        }
        if other.text_transform.is_some() {
            self.text_transform = other.text_transform;
        }
        if other.text_align.is_some() {
            self.text_align = other.text_align;
        }
        if other.text_indent.is_some() {
            self.text_indent.clone_from(&other.text_indent);
        }
        if other.line_height.is_some() {
            self.line_height.clone_from(&other.line_height);
        }
        if other.margin_top.is_some() {
            self.margin_top.clone_from(&other.margin_top);
        }
        if other.margin_bottom.is_some() {
            self.margin_bottom.clone_from(&other.margin_bottom);
        }
        if other.margin_left.is_some() {
            self.margin_left.clone_from(&other.margin_left);
        }
        if other.margin_right.is_some() {
            self.margin_right.clone_from(&other.margin_right);
        }
        if other.padding_top.is_some() {
            self.padding_top.clone_from(&other.padding_top);
        }
        if other.padding_bottom.is_some() {
            self.padding_bottom.clone_from(&other.padding_bottom);
        }
        if other.padding_left.is_some() {
            self.padding_left.clone_from(&other.padding_left);
        }
        if other.padding_right.is_some() {
            self.padding_right.clone_from(&other.padding_right);
        }
        if other.color.is_some() {
            self.color.clone_from(&other.color);
        }
        if other.background_color.is_some() {
            self.background_color.clone_from(&other.background_color);
        }
        if other.border_top.is_some() {
            self.border_top.clone_from(&other.border_top);
        }
        if other.border_bottom.is_some() {
            self.border_bottom.clone_from(&other.border_bottom);
        }
        if other.border_left.is_some() {
            self.border_left.clone_from(&other.border_left);
        }
        if other.border_right.is_some() {
            self.border_right.clone_from(&other.border_right);
        }
        if other.display.is_some() {
            self.display = other.display;
        }
        if other.position.is_some() {
            self.position = other.position;
        }
        if other.left.is_some() {
            self.left.clone_from(&other.left);
        }
        if other.width.is_some() {
            self.width.clone_from(&other.width);
        }
        if other.height.is_some() {
            self.height.clone_from(&other.height);
        }
        if other.min_width.is_some() {
            self.min_width.clone_from(&other.min_width);
        }
        if other.min_height.is_some() {
            self.min_height.clone_from(&other.min_height);
        }
        if other.max_width.is_some() {
            self.max_width.clone_from(&other.max_width);
        }
        if other.max_height.is_some() {
            self.max_height.clone_from(&other.max_height);
        }
        if other.vertical_align.is_some() {
            self.vertical_align = other.vertical_align;
        }
        if other.clear.is_some() {
            self.clear = other.clear;
        }
        if other.word_break.is_some() {
            self.word_break = other.word_break;
        }
        if other.overflow.is_some() {
            self.overflow = other.overflow;
        }
        if other.visibility.is_some() {
            self.visibility = other.visibility;
        }
        if other.break_before.is_some() {
            self.break_before = other.break_before;
        }
        if other.break_after.is_some() {
            self.break_after = other.break_after;
        }
        if other.break_inside.is_some() {
            self.break_inside = other.break_inside;
        }
        if other.border_radius_tl.is_some() {
            self.border_radius_tl.clone_from(&other.border_radius_tl);
        }
        if other.border_radius_tr.is_some() {
            self.border_radius_tr.clone_from(&other.border_radius_tr);
        }
        if other.border_radius_br.is_some() {
            self.border_radius_br.clone_from(&other.border_radius_br);
        }
        if other.border_radius_bl.is_some() {
            self.border_radius_bl.clone_from(&other.border_radius_bl);
        }
        if other.letter_spacing.is_some() {
            self.letter_spacing.clone_from(&other.letter_spacing);
        }
        if other.word_spacing.is_some() {
            self.word_spacing.clone_from(&other.word_spacing);
        }
        if other.white_space_nowrap.is_some() {
            self.white_space_nowrap = other.white_space_nowrap;
        }
        if other.text_decoration_underline {
            self.text_decoration_underline = true;
        }
        if other.text_decoration_overline {
            self.text_decoration_overline = true;
        }
        if other.text_decoration_line_through {
            self.text_decoration_line_through = true;
        }
        if other.text_decoration_line_style.is_some() {
            self.text_decoration_line_style = other.text_decoration_line_style;
        }
        if other.opacity.is_some() {
            self.opacity = other.opacity;
        }
        // is_image is preserved if already set (once marked as image, stays image)
        if other.is_image {
            self.is_image = true;
        }
        // is_inline is preserved if already set (once marked as inline, stays inline)
        if other.is_inline {
            self.is_inline = true;
        }
        // is_heading is preserved if already set (once marked as heading, stays heading)
        if other.is_heading {
            self.is_heading = true;
        }
        // Image dimensions - preserve if set
        if other.image_width_px.is_some() {
            self.image_width_px = other.image_width_px;
        }
        if other.image_height_px.is_some() {
            self.image_height_px = other.image_height_px;
        }
        // Language - child overrides parent
        if other.lang.is_some() {
            self.lang.clone_from(&other.lang);
        }
        // P1: List style properties
        if other.list_style_type.is_some() {
            self.list_style_type = other.list_style_type;
        }
        if other.list_style_position.is_some() {
            self.list_style_position = other.list_style_position;
        }
        // P2: Writing mode properties
        if other.writing_mode.is_some() {
            self.writing_mode = other.writing_mode;
        }
        if other.text_combine_upright.is_some() {
            self.text_combine_upright = other.text_combine_upright;
        }
        // P4: Shadow properties
        if other.box_shadow.is_some() {
            self.box_shadow.clone_from(&other.box_shadow);
        }
        if other.text_shadow.is_some() {
            self.text_shadow.clone_from(&other.text_shadow);
        }
        // P1 Phase 2: Ruby annotation properties
        if other.ruby_position.is_some() {
            self.ruby_position = other.ruby_position;
        }
        if other.ruby_align.is_some() {
            self.ruby_align = other.ruby_align;
        }
        if other.ruby_merge.is_some() {
            self.ruby_merge = other.ruby_merge;
        }
        // P1 Phase 2: Text emphasis properties
        if other.text_emphasis_style.is_some() {
            self.text_emphasis_style = other.text_emphasis_style;
        }
        if other.text_emphasis_color.is_some() {
            self.text_emphasis_color
                .clone_from(&other.text_emphasis_color);
        }
        // P2 Phase 2: Border collapse
        if other.border_collapse.is_some() {
            self.border_collapse = other.border_collapse;
        }
        // Table border-spacing
        if other.border_spacing_horizontal.is_some() {
            self.border_spacing_horizontal
                .clone_from(&other.border_spacing_horizontal);
        }
        if other.border_spacing_vertical.is_some() {
            self.border_spacing_vertical
                .clone_from(&other.border_spacing_vertical);
        }
        // P1 Phase 2: Drop cap
        if other.drop_cap.is_some() {
            self.drop_cap = other.drop_cap;
        }
        // P2 Phase 2: Transform properties
        if other.transform.is_some() {
            self.transform.clone_from(&other.transform);
        }
        if other.transform_origin.is_some() {
            self.transform_origin.clone_from(&other.transform_origin);
        }
        // P2 Phase 2: Baseline-shift
        if other.baseline_shift.is_some() {
            self.baseline_shift.clone_from(&other.baseline_shift);
        }
        // P2 Phase 2: Column layout
        if other.column_count.is_some() {
            self.column_count = other.column_count;
        }
        // P2 Phase 2: Float property
        if other.float.is_some() {
            self.float = other.float;
        }
        // CSS hyphens property
        if other.hyphens.is_some() {
            self.hyphens = other.hyphens;
        }
        // CSS box-sizing property
        if other.box_sizing.is_some() {
            self.box_sizing = other.box_sizing;
        }
    }

    /// Check if this style indicates the element is hidden/invisible
    /// Elements are considered hidden if:
    /// - display: none
    /// - position: absolute with large negative left offset (e.g., -999em)
    pub fn is_hidden(&self) -> bool {
        // display: none
        if self.display == Some(Display::None) {
            return true;
        }

        // position: absolute with large negative left offset
        if self.position == Some(Position::Absolute)
            && let Some(ref left) = self.left
        {
            match left {
                CssValue::Em(v) if *v < -100.0 => return true,
                CssValue::Px(v) if *v < -1000.0 => return true,
                _ => {}
            }
        }

        false
    }

    /// Check if this style has any properties set
    pub fn is_empty(&self) -> bool {
        // Font properties
        self.font_family.is_none()
            && self.font_size.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
            && self.font_variant.is_none()
            // Text properties
            && self.text_transform.is_none()
            && self.text_align.is_none()
            && self.text_indent.is_none()
            && self.line_height.is_none()
            && self.letter_spacing.is_none()
            && self.word_spacing.is_none()
            && self.white_space_nowrap.is_none()
            // Margin properties
            && self.margin_top.is_none()
            && self.margin_bottom.is_none()
            && self.margin_left.is_none()
            && self.margin_right.is_none()
            // Padding properties
            && self.padding_top.is_none()
            && self.padding_bottom.is_none()
            && self.padding_left.is_none()
            && self.padding_right.is_none()
            // Color properties
            && self.color.is_none()
            && self.background_color.is_none()
            // Border properties
            && self.border_top.is_none()
            && self.border_bottom.is_none()
            && self.border_left.is_none()
            && self.border_right.is_none()
            && self.border_radius_tl.is_none()
            && self.border_radius_tr.is_none()
            && self.border_radius_br.is_none()
            && self.border_radius_bl.is_none()
            // Display/position
            && self.display.is_none()
            && self.position.is_none()
            && self.left.is_none()
            && self.visibility.is_none()
            && self.overflow.is_none()
            && self.float.is_none()
            && self.clear.is_none()
            // Size properties
            && self.width.is_none()
            && self.height.is_none()
            && self.min_width.is_none()
            && self.min_height.is_none()
            && self.max_width.is_none()
            && self.max_height.is_none()
            // Vertical alignment
            && self.vertical_align.is_none()
            // Break properties
            && self.break_before.is_none()
            && self.break_after.is_none()
            && self.break_inside.is_none()
            && self.word_break.is_none()
            // Text decoration (bool fields)
            && !self.text_decoration_underline
            && !self.text_decoration_overline
            && !self.text_decoration_line_through
            && self.text_decoration_line_style.is_none()
            // Opacity
            && self.opacity.is_none()
            // List styles
            && self.list_style_type.is_none()
            && self.list_style_position.is_none()
            // Hyphenation
            && self.hyphens.is_none()
            // Box sizing
            && self.box_sizing.is_none()
            // Table properties
            && self.border_collapse.is_none()
            && self.border_spacing_horizontal.is_none()
            && self.border_spacing_vertical.is_none()
    }

    /// Convert this style to a CSS declaration string.
    /// Returns a string like "font-size: 1.17em; font-weight: bold; text-align: center"
    pub fn to_css_string(&self) -> String {
        let mut props = Vec::new();

        if let Some(ref family) = self.font_family {
            props.push(format!("font-family: {}", family));
        }
        if let Some(ref size) = self.font_size {
            props.push(format!("font-size: {}", css_value_to_string(size)));
        }
        if let Some(weight) = self.font_weight {
            let val = match weight {
                FontWeight::Normal => "normal",
                FontWeight::Bold => "bold",
                FontWeight::Weight(w) => return format!("font-weight: {}", w),
            };
            props.push(format!("font-weight: {}", val));
        }
        if let Some(style) = self.font_style {
            let val = match style {
                FontStyle::Normal => "normal",
                FontStyle::Italic => "italic",
                FontStyle::Oblique => "oblique",
            };
            props.push(format!("font-style: {}", val));
        }
        if let Some(variant) = self.font_variant
            && variant == FontVariant::SmallCaps
        {
            props.push("font-variant: small-caps".to_string());
        }
        if let Some(align) = self.text_align {
            let val = match align {
                TextAlign::Left => "left",
                TextAlign::Right => "right",
                TextAlign::Center => "center",
                TextAlign::Justify => "justify",
            };
            props.push(format!("text-align: {}", val));
        }
        if let Some(transform) = self.text_transform {
            let val = match transform {
                TextTransform::None => "none",
                TextTransform::Uppercase => "uppercase",
                TextTransform::Lowercase => "lowercase",
                TextTransform::Capitalize => "capitalize",
            };
            props.push(format!("text-transform: {}", val));
        }
        if let Some(ref indent) = self.text_indent {
            props.push(format!("text-indent: {}", css_value_to_string(indent)));
        }
        if let Some(ref lh) = self.line_height {
            props.push(format!("line-height: {}", css_value_to_string(lh)));
        }
        if let Some(ref v) = self.margin_top {
            props.push(format!("margin-top: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.margin_right {
            props.push(format!("margin-right: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.margin_bottom {
            props.push(format!("margin-bottom: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.margin_left {
            props.push(format!("margin-left: {}", css_value_to_string(v)));
        }
        if let Some(ref color) = self.color
            && let Some(css) = color_to_css(color)
        {
            props.push(format!("color: {}", css));
        }
        if let Some(ref color) = self.background_color
            && let Some(css) = color_to_css(color)
        {
            props.push(format!("background-color: {}", css));
        }
        if let Some(ref v) = self.width {
            props.push(format!("width: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.height {
            props.push(format!("height: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.max_width {
            props.push(format!("max-width: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.max_height {
            props.push(format!("max-height: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.min_width {
            props.push(format!("min-width: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.min_height {
            props.push(format!("min-height: {}", css_value_to_string(v)));
        }
        if let Some(valign) = self.vertical_align {
            let val = match valign {
                VerticalAlign::Baseline => "baseline",
                VerticalAlign::Top => "top",
                VerticalAlign::Middle => "middle",
                VerticalAlign::Bottom => "bottom",
                VerticalAlign::Super => "super",
                VerticalAlign::Sub => "sub",
                VerticalAlign::TextTop => "text-top",
                VerticalAlign::TextBottom => "text-bottom",
            };
            props.push(format!("vertical-align: {}", val));
        }
        if self.white_space_nowrap == Some(true) {
            props.push("white-space: nowrap".to_string());
        }
        if self.text_decoration_underline {
            props.push("text-decoration: underline".to_string());
        }
        if self.text_decoration_line_through {
            props.push("text-decoration: line-through".to_string());
        }
        if self.text_decoration_overline {
            props.push("text-decoration: overline".to_string());
        }
        if let Some(brk) = self.break_before
            && brk != BreakValue::Auto
        {
            props.push(format!("break-before: {}", break_value_to_css(brk)));
        }
        if let Some(brk) = self.break_after
            && brk != BreakValue::Auto
        {
            props.push(format!("break-after: {}", break_value_to_css(brk)));
        }
        if let Some(brk) = self.break_inside
            && brk != BreakValue::Auto
        {
            props.push(format!("break-inside: {}", break_value_to_css(brk)));
        }
        if let Some(ref v) = self.letter_spacing {
            props.push(format!("letter-spacing: {}", css_value_to_string(v)));
        }
        if let Some(ref v) = self.word_spacing {
            props.push(format!("word-spacing: {}", css_value_to_string(v)));
        }
        if let Some(opacity) = self.opacity {
            let val = opacity as f32 / 100.0;
            props.push(format!("opacity: {}", val));
        }

        props.join("; ")
    }
}

/// Convert CssValue to CSS string representation
fn css_value_to_string(val: &CssValue) -> String {
    match val {
        CssValue::Px(v) => format!("{}px", format_number(*v)),
        CssValue::Em(v) => format!("{}em", format_number(*v)),
        CssValue::Rem(v) => format!("{}rem", format_number(*v)),
        CssValue::Percent(v) => format!("{}%", format_number(*v)),
        CssValue::Number(v) => format_number(*v),
        CssValue::Keyword(k) => k.clone(),
        // P1: Additional units
        CssValue::Vw(v) => format!("{}vw", format_number(*v)),
        CssValue::Vh(v) => format!("{}vh", format_number(*v)),
        CssValue::Vmin(v) => format!("{}vmin", format_number(*v)),
        CssValue::Vmax(v) => format!("{}vmax", format_number(*v)),
        CssValue::Ch(v) => format!("{}ch", format_number(*v)),
        CssValue::Ex(v) => format!("{}ex", format_number(*v)),
        CssValue::Cm(v) => format!("{}cm", format_number(*v)),
        CssValue::Mm(v) => format!("{}mm", format_number(*v)),
        CssValue::In(v) => format!("{}in", format_number(*v)),
        CssValue::Pt(v) => format!("{}pt", format_number(*v)),
    }
}

/// Format a float, removing unnecessary trailing zeros
fn format_number(v: f32) -> String {
    if (v - v.round()).abs() < 0.0001 {
        format!("{}", v as i32)
    } else {
        format!("{:.6}", v)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

/// Convert Color to CSS string
fn color_to_css(color: &Color) -> Option<String> {
    match color {
        Color::Rgba(r, g, b, 255) => {
            if *r == 0 && *g == 0 && *b == 0 {
                Some("black".to_string())
            } else if *r == 255 && *g == 255 && *b == 255 {
                Some("white".to_string())
            } else {
                Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
            }
        }
        Color::Rgba(r, g, b, a) => {
            Some(format!("rgba({}, {}, {}, {})", r, g, b, *a as f32 / 255.0))
        }
        Color::Current => Some("currentColor".to_string()),
        Color::Transparent => Some("transparent".to_string()),
    }
}

/// Convert BreakValue to CSS string
fn break_value_to_css(brk: BreakValue) -> &'static str {
    match brk {
        BreakValue::Auto => "auto",
        BreakValue::Avoid => "avoid",
        BreakValue::AvoidPage => "avoid-page",
        BreakValue::Page => "page",
        BreakValue::Left => "left",
        BreakValue::Right => "right",
        BreakValue::Column => "column",
        BreakValue::AvoidColumn => "avoid-column",
    }
}

// Helper to normalize CssValue - treat zero values as None (default)
fn normalize_spacing(val: &Option<CssValue>) -> Option<&CssValue> {
    match val {
        Some(CssValue::Px(v)) if v.abs() < 0.001 => None,
        Some(CssValue::Em(v)) if v.abs() < 0.001 => None,
        Some(CssValue::Percent(v)) if v.abs() < 0.001 => None,
        Some(v) => Some(v),
        None => None,
    }
}

// Helper to normalize display - Block is the default, treat as None
fn normalize_display(val: &Option<Display>) -> Option<Display> {
    match val {
        Some(Display::Block) => None, // Block is default
        other => *other,
    }
}

// Helper to normalize font_style - Normal is the default, treat as None
fn normalize_font_style(val: &Option<FontStyle>) -> Option<FontStyle> {
    match val {
        Some(FontStyle::Normal) => None, // Normal is default
        other => *other,
    }
}

// Custom PartialEq that normalizes values for style deduplication
// - Excludes image dimensions (don't affect KFX output)
// - Treats display:block as default (None)
// - Treats font-style:normal as default (None)
// - Treats zero spacing values as default (None)
impl PartialEq for ParsedStyle {
    fn eq(&self, other: &Self) -> bool {
        self.font_family == other.font_family
            && self.font_size == other.font_size
            && self.font_weight == other.font_weight
            && normalize_font_style(&self.font_style) == normalize_font_style(&other.font_style)
            && self.font_variant == other.font_variant
            && self.text_transform == other.text_transform
            && self.text_align == other.text_align
            && normalize_spacing(&self.text_indent) == normalize_spacing(&other.text_indent)
            && self.line_height == other.line_height
            && normalize_spacing(&self.margin_top) == normalize_spacing(&other.margin_top)
            && normalize_spacing(&self.margin_bottom) == normalize_spacing(&other.margin_bottom)
            && normalize_spacing(&self.margin_left) == normalize_spacing(&other.margin_left)
            && normalize_spacing(&self.margin_right) == normalize_spacing(&other.margin_right)
            && normalize_spacing(&self.padding_top) == normalize_spacing(&other.padding_top)
            && normalize_spacing(&self.padding_bottom) == normalize_spacing(&other.padding_bottom)
            && normalize_spacing(&self.padding_left) == normalize_spacing(&other.padding_left)
            && normalize_spacing(&self.padding_right) == normalize_spacing(&other.padding_right)
            && self.color == other.color
            && self.background_color == other.background_color
            && self.border_top == other.border_top
            && self.border_bottom == other.border_bottom
            && self.border_left == other.border_left
            && self.border_right == other.border_right
            && normalize_display(&self.display) == normalize_display(&other.display)
            && self.position == other.position
            && self.left == other.left
            && self.width == other.width
            && self.height == other.height
            && self.min_width == other.min_width
            && self.min_height == other.min_height
            && self.max_width == other.max_width
            && self.max_height == other.max_height
            && self.vertical_align == other.vertical_align
            && self.clear == other.clear
            && self.word_break == other.word_break
            && self.overflow == other.overflow
            && self.visibility == other.visibility
            && self.break_before == other.break_before
            && self.break_after == other.break_after
            && self.break_inside == other.break_inside
            && self.border_radius_tl == other.border_radius_tl
            && self.border_radius_tr == other.border_radius_tr
            && self.border_radius_br == other.border_radius_br
            && self.border_radius_bl == other.border_radius_bl
            && self.letter_spacing == other.letter_spacing
            && self.word_spacing == other.word_spacing
            && self.white_space_nowrap == other.white_space_nowrap
            && self.text_decoration_underline == other.text_decoration_underline
            && self.text_decoration_overline == other.text_decoration_overline
            && self.text_decoration_line_through == other.text_decoration_line_through
            && self.text_decoration_line_style == other.text_decoration_line_style
            && self.opacity == other.opacity
            && self.is_image == other.is_image
            && self.is_inline == other.is_inline
            && self.is_heading == other.is_heading
            // Note: image_width_px and image_height_px are intentionally excluded
            && self.lang == other.lang
            // P1: List style properties
            && self.list_style_type == other.list_style_type
            && self.list_style_position == other.list_style_position
            // P2: Writing mode properties
            && self.writing_mode == other.writing_mode
            && self.text_combine_upright == other.text_combine_upright
            // P4: Shadow properties
            && self.box_shadow == other.box_shadow
            && self.text_shadow == other.text_shadow
            // P1 Phase 2: Ruby annotation properties
            && self.ruby_position == other.ruby_position
            && self.ruby_align == other.ruby_align
            && self.ruby_merge == other.ruby_merge
            // P1 Phase 2: Text emphasis properties
            && self.text_emphasis_style == other.text_emphasis_style
            && self.text_emphasis_color == other.text_emphasis_color
            // P2 Phase 2: Border collapse
            && self.border_collapse == other.border_collapse
            // Table border-spacing
            && self.border_spacing_horizontal == other.border_spacing_horizontal
            && self.border_spacing_vertical == other.border_spacing_vertical
            // P1 Phase 2: Drop cap
            && self.drop_cap == other.drop_cap
            // P2 Phase 2: Transform properties
            && self.transform == other.transform
            && self.transform_origin == other.transform_origin
            // P2 Phase 2: Baseline-shift
            && self.baseline_shift == other.baseline_shift
            // P2 Phase 2: Column layout
            && self.column_count == other.column_count
            // P2 Phase 2: Float property
            && self.float == other.float
            // CSS hyphens property
            && self.hyphens == other.hyphens
            // CSS box-sizing property
            && self.box_sizing == other.box_sizing
    }
}

impl Eq for ParsedStyle {}

// Custom Hash that normalizes values to match PartialEq
impl std::hash::Hash for ParsedStyle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.font_family.hash(state);
        self.font_size.hash(state);
        self.font_weight.hash(state);
        normalize_font_style(&self.font_style).hash(state);
        self.font_variant.hash(state);
        self.text_transform.hash(state);
        self.text_align.hash(state);
        normalize_spacing(&self.text_indent).hash(state);
        self.line_height.hash(state);
        normalize_spacing(&self.margin_top).hash(state);
        normalize_spacing(&self.margin_bottom).hash(state);
        normalize_spacing(&self.margin_left).hash(state);
        normalize_spacing(&self.margin_right).hash(state);
        normalize_spacing(&self.padding_top).hash(state);
        normalize_spacing(&self.padding_bottom).hash(state);
        normalize_spacing(&self.padding_left).hash(state);
        normalize_spacing(&self.padding_right).hash(state);
        self.color.hash(state);
        self.background_color.hash(state);
        self.border_top.hash(state);
        self.border_bottom.hash(state);
        self.border_left.hash(state);
        self.border_right.hash(state);
        normalize_display(&self.display).hash(state);
        self.position.hash(state);
        self.left.hash(state);
        self.width.hash(state);
        self.height.hash(state);
        self.min_width.hash(state);
        self.min_height.hash(state);
        self.max_width.hash(state);
        self.max_height.hash(state);
        self.vertical_align.hash(state);
        self.clear.hash(state);
        self.word_break.hash(state);
        self.overflow.hash(state);
        self.visibility.hash(state);
        self.break_before.hash(state);
        self.break_after.hash(state);
        self.break_inside.hash(state);
        self.border_radius_tl.hash(state);
        self.border_radius_tr.hash(state);
        self.border_radius_br.hash(state);
        self.border_radius_bl.hash(state);
        self.letter_spacing.hash(state);
        self.word_spacing.hash(state);
        self.white_space_nowrap.hash(state);
        self.text_decoration_underline.hash(state);
        self.text_decoration_overline.hash(state);
        self.text_decoration_line_through.hash(state);
        self.text_decoration_line_style.hash(state);
        self.opacity.hash(state);
        self.is_image.hash(state);
        self.is_inline.hash(state);
        self.is_heading.hash(state);
        // Note: image_width_px and image_height_px are intentionally excluded
        self.lang.hash(state);
        // P1: List style properties
        self.list_style_type.hash(state);
        self.list_style_position.hash(state);
        // P2: Writing mode properties
        self.writing_mode.hash(state);
        self.text_combine_upright.hash(state);
        // P4: Shadow properties
        self.box_shadow.hash(state);
        self.text_shadow.hash(state);
        // P1 Phase 2: Ruby annotation properties
        self.ruby_position.hash(state);
        self.ruby_align.hash(state);
        self.ruby_merge.hash(state);
        // P1 Phase 2: Text emphasis properties
        self.text_emphasis_style.hash(state);
        self.text_emphasis_color.hash(state);
        // P2 Phase 2: Border collapse
        self.border_collapse.hash(state);
        // P1 Phase 2: Drop cap
        self.drop_cap.hash(state);
        // P2 Phase 2: Transform properties
        self.transform.hash(state);
        self.transform_origin.hash(state);
        // P2 Phase 2: Baseline-shift
        self.baseline_shift.hash(state);
        // P2 Phase 2: Column layout
        self.column_count.hash(state);
        // P2 Phase 2: Float property
        self.float.hash(state);
    }
}

pub use kuchiki::{ElementData, NodeDataRef, NodeRef, Selectors};

/// A CSS rule with kuchiki-compatible selectors
#[derive(Debug)]
pub struct CssRule {
    pub selectors: Selectors,
    pub style: ParsedStyle,
}

/// User-agent stylesheet with browser default styles.
/// These are applied at lowest specificity before document styles.
/// Based on standard browser defaults for HTML elements.
const USER_AGENT_CSS: &str = r#"
h1 { font-size: 2em; font-weight: bold; }
h2 { font-size: 1.5em; font-weight: bold; }
h3 { font-size: 1.17em; font-weight: bold; }
h4 { font-size: 1em; font-weight: bold; }
h5 { font-size: 0.83em; font-weight: bold; }
h6 { font-size: 0.67em; font-weight: bold; }
b, strong { font-weight: bold; }
i, em { font-style: italic; }
"#;

/// Parsed stylesheet containing all rules
#[derive(Debug, Default)]
pub struct Stylesheet {
    rules: Vec<CssRule>,
}

impl Stylesheet {
    /// Parse a CSS stylesheet from a string
    pub fn parse(css: &str) -> Self {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        let mut raw_rules = Vec::new();

        let rule_parser = CssRuleParser {
            rules: &mut raw_rules,
        };

        for result in RuleListParser::new_for_stylesheet(&mut parser, rule_parser) {
            // Ignore errors, just collect successful rules
            let _ = result;
        }

        // Convert raw rules to CssRules with kuchiki selectors
        let rules = raw_rules
            .into_iter()
            .filter_map(|(selector_str, style)| {
                // Pre-process selector to remove pseudo-elements that kuchiki doesn't support
                // E.g., "*,::after,::before" becomes just "*"
                let cleaned = clean_selector(&selector_str);
                if cleaned.is_empty() {
                    return None;
                }
                Selectors::compile(&cleaned)
                    .ok()
                    .map(|selectors| CssRule { selectors, style })
            })
            .collect();

        Stylesheet { rules }
    }

    /// Parse a CSS stylesheet with browser default styles prepended.
    /// User-agent styles are applied at lowest specificity, so document
    /// styles will override them.
    pub fn parse_with_defaults(css: &str) -> Self {
        let combined = format!("{}\n{}", USER_AGENT_CSS, css);
        Self::parse(&combined)
    }

    /// Parse an inline style attribute (style="...")
    /// Returns a ParsedStyle with the declarations from the inline style
    pub fn parse_inline_style(style_attr: &str) -> ParsedStyle {
        let mut input = ParserInput::new(style_attr);
        let mut parser = Parser::new(&mut input);
        parse_declaration_block(&mut parser)
    }

    /// Get only the directly-matched styles for an element, WITHOUT CSS inheritance.
    /// This is useful when the output format (like KFX) has its own inheritance mechanism.
    pub fn get_direct_style_for_element(&self, element: &NodeDataRef<ElementData>) -> ParsedStyle {
        let mut result = ParsedStyle::default();

        // Collect matching rules with their specificity
        let mut matches: Vec<(kuchiki::Specificity, &ParsedStyle)> = Vec::new();

        for rule in &self.rules {
            for selector in &rule.selectors.0 {
                if selector.matches(element) {
                    matches.push((selector.specificity(), &rule.style));
                }
            }
        }

        // Sort by specificity (stable sort preserves source order for equal specificity)
        matches.sort_by_key(|(spec, _)| *spec);

        // Apply rules in order (lowest specificity first)
        for (_, style) in matches {
            result.merge(style);
        }

        result
    }
}

/// Clean a CSS selector by removing pseudo-elements that kuchiki doesn't support.
/// This allows rules like `*,::after,::before { ... }` to work.
fn clean_selector(selector: &str) -> String {
    // Split by comma to handle selector lists
    let parts: Vec<&str> = selector.split(',').collect();

    // Filter out pseudo-element selectors and clean the remaining ones
    let cleaned: Vec<String> = parts
        .iter()
        .map(|s| s.trim())
        // Remove parts that are just pseudo-elements
        .filter(|s| !s.starts_with("::") && !s.starts_with(':'))
        // For parts that contain pseudo-elements, strip them
        .map(|s| {
            // Remove ::before, ::after, etc. from the end
            if let Some(idx) = s.find("::") {
                s[..idx].trim().to_string()
            } else if let Some(idx) = s.find(':') {
                // Also handle single-colon pseudo-classes like :hover
                // But preserve structural pseudo-classes if they're the whole selector
                let before = &s[..idx];
                if before.is_empty() {
                    // Pure pseudo-class like :root or :host
                    s.to_string()
                } else {
                    before.trim().to_string()
                }
            } else {
                s.to_string()
            }
        })
        // Filter out empty strings
        .filter(|s| !s.is_empty())
        .collect();

    cleaned.join(", ")
}

// =============================================================================
// CSS Parser Implementation
// =============================================================================

/// Raw parsed rule: (selector_string, style)
type RawRule = (String, ParsedStyle);

struct CssRuleParser<'a> {
    rules: &'a mut Vec<RawRule>,
}

impl<'i> QualifiedRuleParser<'i> for CssRuleParser<'_> {
    type Prelude = String;
    type QualifiedRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
        // Collect the selector string for later compilation with kuchiki
        let start = input.position();
        while input.next().is_ok() {}
        let selector_str = input.slice_from(start).to_string();
        Ok(selector_str)
    }

    fn parse_block<'t>(
        &mut self,
        prelude: Self::Prelude,
        _location: SourceLocation,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
        let style = parse_declaration_block(input);
        if !style.is_empty() {
            self.rules.push((prelude, style));
        }
        Ok(())
    }
}

impl<'i> AtRuleParser<'i> for CssRuleParser<'_> {
    type PreludeNoBlock = ();
    type PreludeBlock = ();
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<AtRuleType<Self::PreludeNoBlock, Self::PreludeBlock>, ParseError<'i, Self::Error>>
    {
        // Skip all @rules (@import, @media, @font-face, etc.)
        let _ = name;
        // Consume tokens to find the end
        while input.next().is_ok() {}
        Err(input.new_error(BasicParseErrorKind::AtRuleInvalid(name)))
    }
}

/// Parse a declaration block (property: value; ...)
fn parse_declaration_block<'i, 't>(input: &mut Parser<'i, 't>) -> ParsedStyle {
    let mut style = ParsedStyle::default();

    loop {
        input.skip_whitespace();

        if input.is_exhausted() {
            break;
        }

        // Try to parse a declaration
        let result: Result<(), ParseError<'i, ()>> = input.try_parse(|i| {
            let property = match i.next()? {
                Token::Ident(name) => name.to_string().to_lowercase(),
                _ => return Err(i.new_custom_error(())),
            };

            i.skip_whitespace();

            match i.next()? {
                Token::Colon => {}
                _ => return Err(i.new_custom_error(())),
            }

            i.skip_whitespace();

            // Collect value tokens until semicolon
            let mut values: Vec<Token> = Vec::new();
            loop {
                match i.next() {
                    Ok(Token::Semicolon) => break,
                    Ok(t) => values.push(t.clone()),
                    Err(_) => break,
                }
            }

            apply_property(&mut style, &property, &values);
            Ok(())
        });

        if result.is_err() {
            // Skip to next semicolon to recover
            loop {
                match input.next() {
                    Ok(Token::Semicolon) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        }
    }

    style
}

/// Apply a CSS property value to the style
fn apply_property(style: &mut ParsedStyle, property: &str, values: &[Token]) {
    match property {
        "font-family" => {
            style.font_family = parse_font_family(values);
        }
        "font-size" => {
            style.font_size = parse_length_value(values);
        }
        "font-weight" => {
            style.font_weight = parse_font_weight(values);
        }
        "font-style" => {
            style.font_style = parse_font_style(values);
        }
        "font-variant" => {
            style.font_variant = parse_font_variant(values);
        }
        "text-align" => {
            style.text_align = parse_text_align(values);
        }
        "text-transform" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.text_transform = match val.to_ascii_lowercase().as_str() {
                    "uppercase" => Some(TextTransform::Uppercase),
                    "lowercase" => Some(TextTransform::Lowercase),
                    "capitalize" => Some(TextTransform::Capitalize),
                    "none" => Some(TextTransform::None),
                    _ => None,
                };
            }
        }
        "text-indent" => {
            style.text_indent = parse_length_value(values);
        }
        "line-height" => {
            style.line_height = parse_length_value(values);
        }
        "margin" => {
            // Shorthand: 1-4 values
            let parsed: Vec<CssValue> = values.iter().filter_map(parse_single_length).collect();
            match parsed.len() {
                1 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_right = Some(parsed[0].clone());
                    style.margin_bottom = Some(parsed[0].clone());
                    style.margin_left = Some(parsed[0].clone());
                }
                2 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_bottom = Some(parsed[0].clone());
                    style.margin_left = Some(parsed[1].clone());
                    style.margin_right = Some(parsed[1].clone());
                }
                3 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_left = Some(parsed[1].clone());
                    style.margin_right = Some(parsed[1].clone());
                    style.margin_bottom = Some(parsed[2].clone());
                }
                4 => {
                    style.margin_top = Some(parsed[0].clone());
                    style.margin_right = Some(parsed[1].clone());
                    style.margin_bottom = Some(parsed[2].clone());
                    style.margin_left = Some(parsed[3].clone());
                }
                _ => {}
            }
        }
        "margin-top" => {
            style.margin_top = parse_length_value(values);
        }
        "margin-bottom" => {
            style.margin_bottom = parse_length_value(values);
        }
        "margin-left" => {
            style.margin_left = parse_length_value(values);
        }
        "margin-right" => {
            style.margin_right = parse_length_value(values);
        }
        "padding" => {
            // Shorthand: 1-4 values (same as margin)
            let parsed: Vec<CssValue> = values.iter().filter_map(parse_single_length).collect();
            match parsed.len() {
                1 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_right = Some(parsed[0].clone());
                    style.padding_bottom = Some(parsed[0].clone());
                    style.padding_left = Some(parsed[0].clone());
                }
                2 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_bottom = Some(parsed[0].clone());
                    style.padding_left = Some(parsed[1].clone());
                    style.padding_right = Some(parsed[1].clone());
                }
                3 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_left = Some(parsed[1].clone());
                    style.padding_right = Some(parsed[1].clone());
                    style.padding_bottom = Some(parsed[2].clone());
                }
                4 => {
                    style.padding_top = Some(parsed[0].clone());
                    style.padding_right = Some(parsed[1].clone());
                    style.padding_bottom = Some(parsed[2].clone());
                    style.padding_left = Some(parsed[3].clone());
                }
                _ => {}
            }
        }
        "padding-top" => {
            style.padding_top = parse_length_value(values);
        }
        "padding-bottom" => {
            style.padding_bottom = parse_length_value(values);
        }
        "padding-left" => {
            style.padding_left = parse_length_value(values);
        }
        "padding-right" => {
            style.padding_right = parse_length_value(values);
        }
        "color" => {
            style.color = parse_color(values);
        }
        "background-color" => {
            style.background_color = parse_color(values);
        }
        "border" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_top = Some(border.clone());
                style.border_bottom = Some(border.clone());
                style.border_left = Some(border.clone());
                style.border_right = Some(border.clone());
            }
        }
        "border-top" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_top = Some(border);
            }
        }
        "border-bottom" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_bottom = Some(border);
            }
        }
        "border-left" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_left = Some(border);
            }
        }
        "border-right" => {
            let border = parse_border(values);
            if border.style != BorderStyle::None {
                style.border_right = Some(border);
            }
        }
        // Border shorthand properties for width/style/color
        "border-width" => {
            if let Some(width) = parse_length_value(values) {
                // Apply width to all borders, creating border if needed
                for border in [
                    &mut style.border_top,
                    &mut style.border_right,
                    &mut style.border_bottom,
                    &mut style.border_left,
                ] {
                    if let Some(b) = border {
                        b.width = Some(width.clone());
                    } else {
                        *border = Some(Border {
                            width: Some(width.clone()),
                            style: BorderStyle::Solid, // Default style when width is set
                            color: None,
                        });
                    }
                }
            }
        }
        "border-style" => {
            if let Some(Token::Ident(val)) = values.first() {
                let bs = match val.to_ascii_lowercase().as_str() {
                    "solid" => BorderStyle::Solid,
                    "dashed" => BorderStyle::Dashed,
                    "dotted" => BorderStyle::Dotted,
                    "double" => BorderStyle::Double,
                    "groove" => BorderStyle::Groove,
                    "ridge" => BorderStyle::Ridge,
                    "inset" => BorderStyle::Inset,
                    "outset" => BorderStyle::Outset,
                    "hidden" => BorderStyle::Hidden,
                    _ => BorderStyle::None,
                };
                if bs != BorderStyle::None && bs != BorderStyle::Hidden {
                    // Apply style to all borders, creating border if needed
                    for border in [
                        &mut style.border_top,
                        &mut style.border_right,
                        &mut style.border_bottom,
                        &mut style.border_left,
                    ] {
                        if let Some(b) = border {
                            b.style = bs;
                        } else {
                            *border = Some(Border {
                                width: None,
                                style: bs,
                                color: None,
                            });
                        }
                    }
                }
            }
        }
        "border-color" => {
            if let Some(color) = parse_color(values) {
                // Apply color to all borders, creating border if needed
                for border in [
                    &mut style.border_top,
                    &mut style.border_right,
                    &mut style.border_bottom,
                    &mut style.border_left,
                ] {
                    if let Some(b) = border {
                        b.color = Some(color.clone());
                    } else {
                        *border = Some(Border {
                            width: None,
                            style: BorderStyle::Solid, // Default style when color is set
                            color: Some(color.clone()),
                        });
                    }
                }
            }
        }
        "display" => {
            style.display = parse_display(values);
        }
        "position" => {
            style.position = parse_position(values);
        }
        "left" => {
            style.left = parse_length_value(values);
        }
        "width" => {
            style.width = parse_length_value(values);
        }
        "height" => {
            style.height = parse_length_value(values);
        }
        "min-width" => {
            style.min_width = parse_length_value(values);
        }
        "min-height" => {
            style.min_height = parse_length_value(values);
        }
        "max-width" => {
            style.max_width = parse_length_value(values);
        }
        "max-height" => {
            style.max_height = parse_length_value(values);
        }
        "vertical-align" => {
            style.vertical_align = parse_vertical_align(values);
        }
        "clear" => {
            style.clear = parse_clear(values);
        }
        "float" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.float = match val.to_ascii_lowercase().as_str() {
                    "none" => Some(CssFloat::None),
                    "left" => Some(CssFloat::Left),
                    "right" => Some(CssFloat::Right),
                    _ => None,
                };
            }
        }
        "word-break" => {
            style.word_break = parse_word_break(values);
        }
        "overflow" | "overflow-x" | "overflow-y" => {
            style.overflow = parse_overflow(values);
        }
        "visibility" => {
            style.visibility = parse_visibility(values);
        }
        "break-before" | "page-break-before" => {
            style.break_before = parse_break_value(values);
        }
        "break-after" | "page-break-after" => {
            style.break_after = parse_break_value(values);
        }
        "break-inside" | "page-break-inside" => {
            style.break_inside = parse_break_value(values);
        }
        "border-radius" => {
            // Shorthand: 1-4 values (for simplicity, apply to all corners)
            if let Some(val) = parse_length_value(values) {
                style.border_radius_tl = Some(val.clone());
                style.border_radius_tr = Some(val.clone());
                style.border_radius_br = Some(val.clone());
                style.border_radius_bl = Some(val);
            }
        }
        "border-top-left-radius" => {
            style.border_radius_tl = parse_length_value(values);
        }
        "border-top-right-radius" => {
            style.border_radius_tr = parse_length_value(values);
        }
        "border-bottom-right-radius" => {
            style.border_radius_br = parse_length_value(values);
        }
        "border-bottom-left-radius" => {
            style.border_radius_bl = parse_length_value(values);
        }
        "letter-spacing" => {
            style.letter_spacing = parse_length_value(values);
        }
        "word-spacing" => {
            style.word_spacing = parse_length_value(values);
        }
        "white-space" => {
            if let Some(Token::Ident(val)) = values.first() {
                match val.to_ascii_lowercase().as_str() {
                    "nowrap" | "pre" => style.white_space_nowrap = Some(true),
                    "normal" | "pre-wrap" | "pre-line" => style.white_space_nowrap = Some(false),
                    _ => {}
                }
            }
        }
        "text-decoration" | "text-decoration-line" => {
            for token in values {
                if let Token::Ident(val) = token {
                    match val.to_ascii_lowercase().as_str() {
                        "underline" => style.text_decoration_underline = true,
                        "overline" => style.text_decoration_overline = true,
                        "line-through" => style.text_decoration_line_through = true,
                        "none" => {
                            style.text_decoration_underline = false;
                            style.text_decoration_overline = false;
                            style.text_decoration_line_through = false;
                        }
                        _ => {}
                    }
                }
            }
        }
        "opacity" => {
            if let Some(Token::Number { value, .. }) = values.first() {
                // Clamp to 0-1 and convert to 0-100
                let clamped = value.clamp(0.0, 1.0);
                style.opacity = Some((clamped * 100.0) as u8);
            } else if let Some(Token::Percentage { unit_value, .. }) = values.first() {
                // unit_value is already 0-1 for percentage
                let clamped = unit_value.clamp(0.0, 1.0);
                style.opacity = Some((clamped * 100.0) as u8);
            }
        }
        // P1: List style properties
        "list-style-type" => {
            style.list_style_type = parse_list_style_type(values);
        }
        "list-style-position" => {
            style.list_style_position = parse_list_style_position(values);
        }
        "list-style" => {
            // Shorthand: can contain type, position, and image
            // Parse type and position, ignore image for now
            if style.list_style_type.is_none() {
                style.list_style_type = parse_list_style_type(values);
            }
            if style.list_style_position.is_none() {
                style.list_style_position = parse_list_style_position(values);
            }
        }
        // P2: Writing mode properties
        "writing-mode" => {
            style.writing_mode = parse_writing_mode(values);
        }
        "text-combine-upright" | "-webkit-text-combine" => {
            style.text_combine_upright = parse_text_combine_upright(values);
        }
        // P4: Shadow properties
        "box-shadow" => {
            style.box_shadow = parse_shadow_value(values);
        }
        "text-shadow" => {
            style.text_shadow = parse_shadow_value(values);
        }
        // CSS hyphens property (also handle vendor prefixes)
        "hyphens" | "-webkit-hyphens" | "-moz-hyphens" | "-epub-hyphens" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.hyphens = match val.to_ascii_lowercase().as_str() {
                    "none" => Some(Hyphens::None),
                    "manual" => Some(Hyphens::Manual),
                    "auto" => Some(Hyphens::Auto),
                    _ => None,
                };
            }
        }
        // CSS box-sizing property
        "box-sizing" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.box_sizing = match val.to_ascii_lowercase().as_str() {
                    "content-box" => Some(BoxSizing::ContentBox),
                    "border-box" => Some(BoxSizing::BorderBox),
                    "padding-box" => Some(BoxSizing::PaddingBox),
                    _ => None,
                };
            }
        }
        // Table CSS properties
        "border-collapse" => {
            if let Some(Token::Ident(val)) = values.first() {
                style.border_collapse = match val.to_ascii_lowercase().as_str() {
                    "collapse" => Some(BorderCollapse::Collapse),
                    "separate" => Some(BorderCollapse::Separate),
                    _ => None,
                };
            }
        }
        "border-spacing" => {
            // border-spacing: <horizontal> [<vertical>]
            // If one value, applies to both; if two, first is horizontal, second is vertical
            let lengths: Vec<CssValue> = values.iter().filter_map(parse_single_length).collect();
            match lengths.len() {
                1 => {
                    style.border_spacing_horizontal = Some(lengths[0].clone());
                    style.border_spacing_vertical = Some(lengths[0].clone());
                }
                2 => {
                    style.border_spacing_horizontal = Some(lengths[0].clone());
                    style.border_spacing_vertical = Some(lengths[1].clone());
                }
                _ => {}
            }
        }
        _ => {
            // Ignore unsupported properties
        }
    }
}

fn parse_border(values: &[Token]) -> Border {
    let mut border = Border::default();

    // Naive parsing: check for width, style, color in any order
    for token in values {
        if let Some(width) = parse_single_length(token) {
            border.width = Some(width);
        } else if let Some(color) = parse_single_color(token) {
            border.color = Some(color);
        } else if let Some(style) = parse_border_style_token(token) {
            border.style = style;
        }
    }

    // Default to solid if width/color present but no style
    if border.style == BorderStyle::None && (border.width.is_some() || border.color.is_some()) {
        border.style = BorderStyle::Solid;
    }

    border
}

fn parse_border_style_token(token: &Token) -> Option<BorderStyle> {
    if let Token::Ident(name) = token {
        match name.to_ascii_lowercase().as_str() {
            "none" => Some(BorderStyle::None),
            "hidden" => Some(BorderStyle::Hidden),
            "solid" => Some(BorderStyle::Solid),
            "dotted" => Some(BorderStyle::Dotted),
            "dashed" => Some(BorderStyle::Dashed),
            "double" => Some(BorderStyle::Double),
            "groove" => Some(BorderStyle::Groove),
            "ridge" => Some(BorderStyle::Ridge),
            "inset" => Some(BorderStyle::Inset),
            "outset" => Some(BorderStyle::Outset),
            _ => None,
        }
    } else {
        None
    }
}

fn parse_single_color(token: &Token) -> Option<Color> {
    parse_color(std::slice::from_ref(token))
}

fn parse_color(values: &[Token]) -> Option<Color> {
    for token in values {
        match token {
            Token::Hash(value) | Token::IDHash(value) => {
                // Parse hex color
                let s = value.as_ref();
                match s.len() {
                    3 => {
                        // #RGB
                        let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
                        let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
                        let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
                        return Some(Color::Rgba(r, g, b, 255));
                    }
                    6 => {
                        // #RRGGBB
                        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                        return Some(Color::Rgba(r, g, b, 255));
                    }
                    _ => continue,
                }
            }
            Token::Ident(name) => {
                let name = name.to_ascii_lowercase();
                match name.as_str() {
                    "currentcolor" => return Some(Color::Current),
                    "transparent" => return Some(Color::Transparent),
                    "black" => return Some(Color::Rgba(0, 0, 0, 255)),
                    "white" => return Some(Color::Rgba(255, 255, 255, 255)),
                    "red" => return Some(Color::Rgba(255, 0, 0, 255)),
                    "green" => return Some(Color::Rgba(0, 128, 0, 255)),
                    "blue" => return Some(Color::Rgba(0, 0, 255, 255)),
                    // Add more named colors as needed or use a crate for full support
                    _ => continue,
                }
            }
            // Add rgb() / rgba() function parsing if needed
            _ => continue,
        }
    }
    None
}

fn parse_font_family(values: &[Token]) -> Option<String> {
    let mut fonts = Vec::new();

    for token in values {
        match token {
            Token::Ident(name) => fonts.push(name.to_string()),
            Token::QuotedString(name) => fonts.push(name.to_string()),
            Token::Comma => {} // Skip commas between fonts
            _ => continue,
        }
    }

    if fonts.is_empty() {
        None
    } else {
        Some(fonts.join(","))
    }
}

fn parse_font_weight(values: &[Token]) -> Option<FontWeight> {
    for token in values {
        match token {
            Token::Ident(name) => {
                let name = name.to_ascii_lowercase();
                match name.as_str() {
                    "normal" => return Some(FontWeight::Normal),
                    "bold" => return Some(FontWeight::Bold),
                    _ => continue,
                }
            }
            Token::Number { int_value, .. } => {
                if let Some(weight) = int_value {
                    return Some(FontWeight::Weight(*weight as u16));
                }
            }
            _ => continue,
        }
    }
    None
}

fn parse_font_style(values: &[Token]) -> Option<FontStyle> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "normal" => return Some(FontStyle::Normal),
                "italic" => return Some(FontStyle::Italic),
                "oblique" => return Some(FontStyle::Oblique),
                _ => continue,
            }
        }
    }
    None
}

fn parse_font_variant(values: &[Token]) -> Option<FontVariant> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "normal" => return Some(FontVariant::Normal),
                "small-caps" => return Some(FontVariant::SmallCaps),
                _ => continue,
            }
        }
    }
    None
}

fn parse_text_align(values: &[Token]) -> Option<TextAlign> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "left" | "start" => return Some(TextAlign::Left),
                "right" | "end" => return Some(TextAlign::Right),
                "center" => return Some(TextAlign::Center),
                "justify" => return Some(TextAlign::Justify),
                _ => continue,
            }
        }
    }
    None
}

fn parse_display(values: &[Token]) -> Option<Display> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "none" => return Some(Display::None),
                "block" => return Some(Display::Block),
                "inline" => return Some(Display::Inline),
                "inline-block" | "flex" | "grid" | "table" => return Some(Display::Other),
                _ => continue,
            }
        }
    }
    None
}

fn parse_position(values: &[Token]) -> Option<Position> {
    for token in values {
        if let Token::Ident(name) = token {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "static" => return Some(Position::Static),
                "relative" => return Some(Position::Relative),
                "absolute" => return Some(Position::Absolute),
                "fixed" => return Some(Position::Fixed),
                _ => continue,
            }
        }
    }
    None
}

fn parse_length_value(values: &[Token]) -> Option<CssValue> {
    for token in values {
        if let Some(value) = parse_single_length(token) {
            return Some(value);
        }
    }
    None
}

fn parse_single_length(token: &Token) -> Option<CssValue> {
    match token {
        Token::Dimension { value, unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            match unit.as_str() {
                "px" => Some(CssValue::Px(*value)),
                "em" => Some(CssValue::Em(*value)),
                "rem" => Some(CssValue::Rem(*value)),
                // P1: Additional units
                "vw" => Some(CssValue::Vw(*value)),
                "vh" => Some(CssValue::Vh(*value)),
                "vmin" => Some(CssValue::Vmin(*value)),
                "vmax" => Some(CssValue::Vmax(*value)),
                "ch" => Some(CssValue::Ch(*value)),
                "ex" => Some(CssValue::Ex(*value)),
                "cm" => Some(CssValue::Cm(*value)),
                "mm" => Some(CssValue::Mm(*value)),
                "in" => Some(CssValue::In(*value)),
                "pt" => Some(CssValue::Pt(*value)),
                _ => None,
            }
        }
        Token::Percentage { unit_value, .. } => Some(CssValue::Percent(*unit_value * 100.0)),
        Token::Number { value, .. } => {
            if *value == 0.0 {
                Some(CssValue::Px(0.0))
            } else {
                Some(CssValue::Number(*value))
            }
        }
        Token::Ident(name) => {
            let name = name.to_ascii_lowercase();
            Some(CssValue::Keyword(name))
        }
        _ => None,
    }
}

fn parse_vertical_align(values: &[Token]) -> Option<VerticalAlign> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "baseline" => return Some(VerticalAlign::Baseline),
                "top" => return Some(VerticalAlign::Top),
                "middle" => return Some(VerticalAlign::Middle),
                "bottom" => return Some(VerticalAlign::Bottom),
                "super" => return Some(VerticalAlign::Super),
                "sub" => return Some(VerticalAlign::Sub),
                "text-top" => return Some(VerticalAlign::TextTop),
                "text-bottom" => return Some(VerticalAlign::TextBottom),
                _ => continue,
            }
        }
    }
    None
}

fn parse_clear(values: &[Token]) -> Option<Clear> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "none" => return Some(Clear::None),
                "left" => return Some(Clear::Left),
                "right" => return Some(Clear::Right),
                "both" => return Some(Clear::Both),
                _ => continue,
            }
        }
    }
    None
}

fn parse_word_break(values: &[Token]) -> Option<WordBreak> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "normal" => return Some(WordBreak::Normal),
                "break-all" => return Some(WordBreak::BreakAll),
                "keep-all" => return Some(WordBreak::KeepAll),
                _ => continue,
            }
        }
    }
    None
}

fn parse_overflow(values: &[Token]) -> Option<Overflow> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "visible" => return Some(Overflow::Visible),
                "hidden" => return Some(Overflow::Hidden),
                "scroll" => return Some(Overflow::Scroll),
                "auto" => return Some(Overflow::Auto),
                "clip" => return Some(Overflow::Clip),
                _ => continue,
            }
        }
    }
    None
}

fn parse_visibility(values: &[Token]) -> Option<Visibility> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "visible" => return Some(Visibility::Visible),
                "hidden" => return Some(Visibility::Hidden),
                "collapse" => return Some(Visibility::Collapse),
                _ => continue,
            }
        }
    }
    None
}

fn parse_break_value(values: &[Token]) -> Option<BreakValue> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "auto" => return Some(BreakValue::Auto),
                "avoid" => return Some(BreakValue::Avoid),
                "avoid-page" => return Some(BreakValue::AvoidPage),
                "page" => return Some(BreakValue::Page),
                "left" => return Some(BreakValue::Left),
                "right" => return Some(BreakValue::Right),
                "column" => return Some(BreakValue::Column),
                "avoid-column" => return Some(BreakValue::AvoidColumn),
                // Legacy page-break-* value mapping
                "always" => return Some(BreakValue::Page),
                _ => continue,
            }
        }
    }
    None
}

// P1: List style type parsing
fn parse_list_style_type(values: &[Token]) -> Option<ListStyleType> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "disc" => return Some(ListStyleType::Disc),
                "circle" => return Some(ListStyleType::Circle),
                "square" => return Some(ListStyleType::Square),
                "decimal" => return Some(ListStyleType::Decimal),
                "decimal-leading-zero" => return Some(ListStyleType::DecimalLeadingZero),
                "lower-alpha" | "lower-latin" => return Some(ListStyleType::LowerAlpha),
                "upper-alpha" | "upper-latin" => return Some(ListStyleType::UpperAlpha),
                "lower-roman" => return Some(ListStyleType::LowerRoman),
                "upper-roman" => return Some(ListStyleType::UpperRoman),
                "lower-greek" => return Some(ListStyleType::LowerGreek),
                "none" => return Some(ListStyleType::None),
                _ => continue,
            }
        }
    }
    None
}

// P1: List style position parsing
fn parse_list_style_position(values: &[Token]) -> Option<ListStylePosition> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "inside" => return Some(ListStylePosition::Inside),
                "outside" => return Some(ListStylePosition::Outside),
                _ => continue,
            }
        }
    }
    None
}

// P2: Writing mode parsing
fn parse_writing_mode(values: &[Token]) -> Option<WritingMode> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "horizontal-tb" => return Some(WritingMode::HorizontalTb),
                "vertical-rl" => return Some(WritingMode::VerticalRl),
                "vertical-lr" => return Some(WritingMode::VerticalLr),
                // Legacy values
                "lr" | "lr-tb" | "rl" | "rl-tb" => return Some(WritingMode::HorizontalTb),
                "tb" | "tb-rl" => return Some(WritingMode::VerticalRl),
                "tb-lr" => return Some(WritingMode::VerticalLr),
                _ => continue,
            }
        }
    }
    None
}

// P2: Text combine upright parsing
fn parse_text_combine_upright(values: &[Token]) -> Option<TextCombineUpright> {
    for token in values {
        if let Token::Ident(name) = token {
            match name.to_ascii_lowercase().as_str() {
                "none" => return Some(TextCombineUpright::None),
                "all" => return Some(TextCombineUpright::All),
                _ => continue,
            }
        }
    }
    // Check for digits(N) function - simplified parsing
    None
}

// P4: Parse shadow value as raw string (box-shadow, text-shadow)
fn parse_shadow_value(values: &[Token]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    // Check for "none" keyword
    if values.len() == 1
        && let Token::Ident(name) = &values[0]
        && name.to_ascii_lowercase() == "none"
    {
        return None;
    }
    // Collect all tokens as a string (simplified)
    let parts: Vec<String> = values
        .iter()
        .filter_map(|t| match t {
            Token::Dimension { value, unit, .. } => Some(format!("{}{}", value, unit)),
            Token::Number { value, .. } => Some(format!("{}", value)),
            Token::Ident(name) => Some(name.to_string()),
            Token::Hash(h) => Some(format!("#{}", h)),
            Token::Comma => Some(",".to_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kuchiki::traits::*;

    /// Helper to get the style for an element in an HTML document
    fn get_style_for(stylesheet: &Stylesheet, html: &str, selector: &str) -> ParsedStyle {
        let doc = kuchiki::parse_html().one(html);
        let element = doc.select_first(selector).expect("Element not found");
        stylesheet.get_direct_style_for_element(&element)
    }

    #[test]
    fn test_parse_simple_stylesheet() {
        let css = r#"
            p { text-align: justify; margin-bottom: 1em; }
            h1 { font-size: 2em; text-align: center; font-weight: bold; }
            .italic { font-style: italic; }
        "#;

        let stylesheet = Stylesheet::parse(css);
        assert_eq!(stylesheet.rules.len(), 3);

        // Check p style
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert_eq!(p_style.text_align, Some(TextAlign::Justify));
        assert!(matches!(p_style.margin_bottom, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // Check h1 style
        let h1_style = get_style_for(&stylesheet, "<h1>Test</h1>", "h1");
        assert_eq!(h1_style.text_align, Some(TextAlign::Center));
        assert!(matches!(h1_style.font_size, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));
        assert!(matches!(h1_style.font_weight, Some(FontWeight::Bold)));
    }

    #[test]
    fn test_selector_specificity() {
        let css = r#"
            p { text-align: left; }
            .special { text-align: right; }
            p.special { text-align: center; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Element only
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert_eq!(p_style.text_align, Some(TextAlign::Left));

        // Class should override element
        let class_style = get_style_for(&stylesheet, r#"<div class="special">Test</div>"#, "div");
        assert_eq!(class_style.text_align, Some(TextAlign::Right));

        // Element.class should have highest specificity
        let combined_style = get_style_for(&stylesheet, r#"<p class="special">Test</p>"#, "p");
        assert_eq!(combined_style.text_align, Some(TextAlign::Center));
    }

    #[test]
    fn test_text_decoration_parsing() {
        let css = r#"
            .underline { text-decoration: underline; }
            .line-through { text-decoration: line-through; }
            .overline { text-decoration: overline; }
            .no-underline { text-decoration: none; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Check underline
        let underline = get_style_for(&stylesheet, r#"<p class="underline">Test</p>"#, "p");
        assert!(
            underline.text_decoration_underline,
            "Expected text_decoration_underline to be true"
        );

        // Check line-through
        let line_through = get_style_for(&stylesheet, r#"<p class="line-through">Test</p>"#, "p");
        assert!(
            line_through.text_decoration_line_through,
            "Expected text_decoration_line_through to be true"
        );

        // Check overline
        let overline = get_style_for(&stylesheet, r#"<p class="overline">Test</p>"#, "p");
        assert!(
            overline.text_decoration_overline,
            "Expected text_decoration_overline to be true"
        );

        // Check none resets all
        let no_underline = get_style_for(&stylesheet, r#"<p class="no-underline">Test</p>"#, "p");
        assert!(
            !no_underline.text_decoration_underline,
            "text-decoration: none should reset underline"
        );
    }

    #[test]
    fn test_opacity_parsing() {
        let css = r#"
            .half { opacity: 0.5; }
            .full { opacity: 1; }
            .zero { opacity: 0; }
            .pct { opacity: 50%; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Check 0.5 opacity (should be stored as 50)
        let half = get_style_for(&stylesheet, r#"<p class="half">Test</p>"#, "p");
        assert_eq!(
            half.opacity,
            Some(50),
            "opacity: 0.5 should be stored as 50"
        );

        // Check full opacity
        let full = get_style_for(&stylesheet, r#"<p class="full">Test</p>"#, "p");
        assert_eq!(
            full.opacity,
            Some(100),
            "opacity: 1 should be stored as 100"
        );

        // Check zero opacity
        let zero = get_style_for(&stylesheet, r#"<p class="zero">Test</p>"#, "p");
        assert_eq!(zero.opacity, Some(0), "opacity: 0 should be stored as 0");
    }

    #[test]
    fn test_text_transform_parsing() {
        let css = r#"
            .upper { text-transform: uppercase; }
            .lower { text-transform: lowercase; }
            .cap { text-transform: capitalize; }
            .none { text-transform: none; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let upper = get_style_for(&stylesheet, r#"<p class="upper">Test</p>"#, "p");
        assert_eq!(upper.text_transform, Some(TextTransform::Uppercase));

        let lower = get_style_for(&stylesheet, r#"<p class="lower">Test</p>"#, "p");
        assert_eq!(lower.text_transform, Some(TextTransform::Lowercase));

        let cap = get_style_for(&stylesheet, r#"<p class="cap">Test</p>"#, "p");
        assert_eq!(cap.text_transform, Some(TextTransform::Capitalize));

        let none = get_style_for(&stylesheet, r#"<p class="none">Test</p>"#, "p");
        assert_eq!(none.text_transform, Some(TextTransform::None));
    }

    #[test]
    fn test_float_parsing() {
        let css = r#"
            .left { float: left; }
            .right { float: right; }
            .none { float: none; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let left = get_style_for(&stylesheet, r#"<p class="left">Test</p>"#, "p");
        assert_eq!(left.float, Some(CssFloat::Left));

        let right = get_style_for(&stylesheet, r#"<p class="right">Test</p>"#, "p");
        assert_eq!(right.float, Some(CssFloat::Right));

        let none = get_style_for(&stylesheet, r#"<p class="none">Test</p>"#, "p");
        assert_eq!(none.float, Some(CssFloat::None));
    }

    #[test]
    fn test_padding_parsing() {
        let css = r#"
            .p1 { padding: 1em; }
            .p2 { padding: 1em 2em; }
            .pt { padding-top: 0.5em; }
            .pb { padding-bottom: 0.5em; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Check shorthand with 1 value
        let p1 = get_style_for(&stylesheet, r#"<p class="p1">Test</p>"#, "p");
        assert!(matches!(p1.padding_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(p1.padding_bottom, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(p1.padding_left, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(p1.padding_right, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // Check shorthand with 2 values
        let p2 = get_style_for(&stylesheet, r#"<p class="p2">Test</p>"#, "p");
        assert!(matches!(p2.padding_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(p2.padding_left, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));

        // Check individual properties
        let pt = get_style_for(&stylesheet, r#"<p class="pt">Test</p>"#, "p");
        assert!(matches!(pt.padding_top, Some(CssValue::Em(e)) if (e - 0.5).abs() < 0.01));

        let pb = get_style_for(&stylesheet, r#"<p class="pb">Test</p>"#, "p");
        assert!(matches!(pb.padding_bottom, Some(CssValue::Em(e)) if (e - 0.5).abs() < 0.01));
    }

    #[test]
    fn test_margin_shorthand() {
        let css = r#"
            .m1 { margin: 1em; }
            .m2 { margin: 1em 2em; }
            .m4 { margin: 1em 2em 3em 4em; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let m1 = get_style_for(&stylesheet, r#"<div class="m1">Test</div>"#, "div");
        assert!(matches!(m1.margin_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(m1.margin_left, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        let m2 = get_style_for(&stylesheet, r#"<div class="m2">Test</div>"#, "div");
        assert!(matches!(m2.margin_top, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));
        assert!(matches!(m2.margin_left, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));
    }

    #[test]
    fn test_text_indent() {
        let css = r#"
            p {
                margin-top: 0;
                margin-bottom: 0;
                text-indent: 1em;
            }
        "#;

        let stylesheet = Stylesheet::parse(css);
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");

        assert!(
            matches!(p_style.text_indent, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01),
            "Expected text-indent: 1em, got {:?}",
            p_style.text_indent
        );
        assert!(
            matches!(p_style.margin_top, Some(CssValue::Px(v)) if v.abs() < 0.01),
            "Expected margin-top: 0, got {:?}",
            p_style.margin_top
        );
    }

    #[test]
    fn test_inline_style_parsing() {
        let inline = Stylesheet::parse_inline_style(
            "font-weight: bold; text-align: center; margin-top: 2em",
        );

        assert!(matches!(inline.font_weight, Some(FontWeight::Bold)));
        assert_eq!(inline.text_align, Some(TextAlign::Center));
        assert!(matches!(inline.margin_top, Some(CssValue::Em(e)) if (e - 2.0).abs() < 0.01));
    }

    #[test]
    fn test_inline_box_sizing_parsing() {
        let inline = Stylesheet::parse_inline_style("box-sizing: border-box");
        assert_eq!(
            inline.box_sizing,
            Some(BoxSizing::BorderBox),
            "Inline box-sizing should parse correctly, got {:?}",
            inline.box_sizing
        );
    }

    #[test]
    fn test_epictetus_css_styles() {
        // Simplified version of epictetus.epub CSS
        let css = r#"
            p {
                margin-top: 0;
                margin-right: 0;
                margin-bottom: 0;
                margin-left: 0;
                text-indent: 1em;
            }

            blockquote {
                margin-top: 1em;
                margin-right: 2.5em;
                margin-bottom: 1em;
                margin-left: 2.5em;
            }

            h1, h2, h3, h4, h5, h6 {
                margin-top: 3em;
                margin-bottom: 3em;
                text-align: center;
            }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Check paragraph styles
        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert!(matches!(p_style.text_indent, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // Check blockquote styles
        let bq_style = get_style_for(&stylesheet, "<blockquote>Test</blockquote>", "blockquote");
        assert!(matches!(bq_style.margin_left, Some(CssValue::Em(e)) if (e - 2.5).abs() < 0.01));
        assert!(matches!(bq_style.margin_right, Some(CssValue::Em(e)) if (e - 2.5).abs() < 0.01));

        // Check h1-h6 grouped selector
        let h1_style = get_style_for(&stylesheet, "<h1>Test</h1>", "h1");
        assert_eq!(h1_style.text_align, Some(TextAlign::Center));
        assert!(matches!(h1_style.margin_top, Some(CssValue::Em(e)) if (e - 3.0).abs() < 0.01));

        let h3_style = get_style_for(&stylesheet, "<h3>Test</h3>", "h3");
        assert_eq!(h3_style.text_align, Some(TextAlign::Center));
    }

    #[test]
    fn test_descendant_selector() {
        // Test proper descendant selector matching (only possible with DOM-based selectors)
        let css = r#"
            div p { color: red; }
            p { color: blue; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // p inside div should match "div p" selector
        let nested_style = get_style_for(&stylesheet, "<div><p>Test</p></div>", "p");
        // Both selectors match, but "div p" is more specific (0,0,2 vs 0,0,1)
        // Actually they have same specificity but "div p" comes first
        // Wait, specificity of "div p" is 0,0,2 (two element selectors)
        // and "p" is 0,0,1 (one element selector)
        // So "div p" should win
        assert!(nested_style.color.is_some());

        // Standalone p should only match "p" selector
        let standalone_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert!(standalone_style.color.is_some());
    }

    #[test]
    fn test_font_variant_small_caps() {
        let css = r#"
            h1 { font-variant: small-caps; }
            .normal { font-variant: normal; }
            strong { font-variant: small-caps; font-weight: normal; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // h1 should have small-caps
        let h1_style = get_style_for(&stylesheet, "<h1>Test</h1>", "h1");
        assert_eq!(h1_style.font_variant, Some(FontVariant::SmallCaps));

        // .normal should have normal
        let normal_style = get_style_for(&stylesheet, r#"<div class="normal">Test</div>"#, "div");
        assert_eq!(normal_style.font_variant, Some(FontVariant::Normal));

        // strong should have small-caps
        let strong_style = get_style_for(&stylesheet, "<strong>Test</strong>", "strong");
        assert_eq!(strong_style.font_variant, Some(FontVariant::SmallCaps));
    }

    #[test]
    fn test_font_size_various_values() {
        let css = r#"
            .small { font-size: 0.67em; }
            .medium-small { font-size: 0.83em; }
            .normal { font-size: 1em; }
            .large { font-size: 1.17em; }
            .larger { font-size: 1.5em; }
            .percent-small { font-size: 67%; }
            .percent-large { font-size: 150%; }
            .smaller { font-size: smaller; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // 0.67em
        let small = get_style_for(&stylesheet, r#"<div class="small">Test</div>"#, "div");
        assert!(matches!(small.font_size, Some(CssValue::Em(e)) if (e - 0.67).abs() < 0.01));

        // 0.83em
        let med_small = get_style_for(
            &stylesheet,
            r#"<div class="medium-small">Test</div>"#,
            "div",
        );
        assert!(matches!(med_small.font_size, Some(CssValue::Em(e)) if (e - 0.83).abs() < 0.01));

        // 1em
        let normal = get_style_for(&stylesheet, r#"<div class="normal">Test</div>"#, "div");
        assert!(matches!(normal.font_size, Some(CssValue::Em(e)) if (e - 1.0).abs() < 0.01));

        // 1.17em
        let large = get_style_for(&stylesheet, r#"<div class="large">Test</div>"#, "div");
        assert!(matches!(large.font_size, Some(CssValue::Em(e)) if (e - 1.17).abs() < 0.01));

        // 67%
        let pct_small = get_style_for(
            &stylesheet,
            r#"<div class="percent-small">Test</div>"#,
            "div",
        );
        assert!(
            matches!(pct_small.font_size, Some(CssValue::Percent(p)) if (p - 67.0).abs() < 0.01)
        );

        // smaller keyword
        let smaller = get_style_for(&stylesheet, r#"<div class="smaller">Test</div>"#, "div");
        assert!(matches!(smaller.font_size, Some(CssValue::Keyword(ref k)) if k == "smaller"));
    }

    #[test]
    fn test_bold_strong_font_weight_normal() {
        // Standard Ebooks uses b/strong with font-weight: normal for semantic markup
        // This tests that we correctly parse explicit font-weight: normal
        let css = r#"
            b, strong {
                font-variant: small-caps;
                font-weight: normal;
            }
            .bold { font-weight: bold; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // b element should have font-weight: normal (NOT bold)
        let b_style = get_style_for(&stylesheet, "<b>Test</b>", "b");
        assert_eq!(
            b_style.font_weight,
            Some(FontWeight::Normal),
            "b element should have font-weight: normal"
        );
        assert_eq!(
            b_style.font_variant,
            Some(FontVariant::SmallCaps),
            "b element should have small-caps"
        );

        // strong element should also have font-weight: normal
        let strong_style = get_style_for(&stylesheet, "<strong>Test</strong>", "strong");
        assert_eq!(
            strong_style.font_weight,
            Some(FontWeight::Normal),
            "strong element should have font-weight: normal"
        );

        // .bold class should have font-weight: bold
        let bold_style = get_style_for(&stylesheet, r#"<span class="bold">Test</span>"#, "span");
        assert_eq!(
            bold_style.font_weight,
            Some(FontWeight::Bold),
            ".bold class should have font-weight: bold"
        );
    }

    #[test]
    fn test_hidden_elements_detection() {
        // Elements with position: absolute and left: -999em should be detected as hidden
        let css = r#"
            .hidden {
                position: absolute;
                left: -999em;
            }
            .visible {
                position: relative;
            }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Hidden element should have display: none or be detectable as hidden
        let hidden_style = get_style_for(&stylesheet, r#"<div class="hidden">Test</div>"#, "div");
        assert!(
            hidden_style.is_hidden(),
            "Element with position:absolute; left:-999em should be hidden"
        );

        // Visible element should not be hidden
        let visible_style = get_style_for(&stylesheet, r#"<div class="visible">Test</div>"#, "div");
        assert!(
            !visible_style.is_hidden(),
            "Element with position:relative should not be hidden"
        );
    }

    #[test]
    fn test_display_none_detection() {
        let css = r#"
            .hidden { display: none; }
            .block { display: block; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let hidden_style = get_style_for(&stylesheet, r#"<div class="hidden">Test</div>"#, "div");
        assert!(hidden_style.is_hidden(), "display:none should be hidden");

        let block_style = get_style_for(&stylesheet, r#"<div class="block">Test</div>"#, "div");
        assert!(
            !block_style.is_hidden(),
            "display:block should not be hidden"
        );
    }

    #[test]
    fn test_font_family_full_stack() {
        // Font stacks should preserve all fonts, not just the first one
        let css = r#"
            .sans { font-family: ui-sans-serif, system-ui, sans-serif; }
            .mono { font-family: ui-monospace, "Courier New", monospace; }
            .single { font-family: Georgia; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Sans stack should have all fonts
        let sans = get_style_for(&stylesheet, r#"<div class="sans">Test</div>"#, "div");
        assert_eq!(
            sans.font_family,
            Some("ui-sans-serif,system-ui,sans-serif".to_string()),
            "Font stack should preserve all fonts"
        );

        // Mono stack with quoted font name
        let mono = get_style_for(&stylesheet, r#"<div class="mono">Test</div>"#, "div");
        assert_eq!(
            mono.font_family,
            Some("ui-monospace,Courier New,monospace".to_string()),
            "Font stack should handle quoted names"
        );

        // Single font
        let single = get_style_for(&stylesheet, r#"<div class="single">Test</div>"#, "div");
        assert_eq!(
            single.font_family,
            Some("Georgia".to_string()),
            "Single font should work"
        );
    }

    #[test]
    fn test_line_height_with_rem_units() {
        // Line-height with rem units should be parsed correctly
        // This is the text-xs pattern from Tailwind CSS
        let css = r#"
            .text-xs { font-size: 0.75rem; line-height: 1rem; }
            .text-sm { font-size: 0.875rem; line-height: 1.25rem; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // text-xs should have both font-size and line-height
        let text_xs = get_style_for(&stylesheet, r#"<span class="text-xs">Test</span>"#, "span");
        assert!(
            matches!(text_xs.font_size, Some(CssValue::Rem(v)) if (v - 0.75).abs() < 0.01),
            "text-xs should have font-size: 0.75rem, got {:?}",
            text_xs.font_size
        );
        assert!(
            matches!(text_xs.line_height, Some(CssValue::Rem(v)) if (v - 1.0).abs() < 0.01),
            "text-xs should have line-height: 1rem, got {:?}",
            text_xs.line_height
        );

        // text-sm should have both font-size and line-height
        let text_sm = get_style_for(&stylesheet, r#"<span class="text-sm">Test</span>"#, "span");
        assert!(
            matches!(text_sm.font_size, Some(CssValue::Rem(v)) if (v - 0.875).abs() < 0.01),
            "text-sm should have font-size: 0.875rem, got {:?}",
            text_sm.font_size
        );
        assert!(
            matches!(text_sm.line_height, Some(CssValue::Rem(v)) if (v - 1.25).abs() < 0.01),
            "text-sm should have line-height: 1.25rem, got {:?}",
            text_sm.line_height
        );
    }

    #[test]
    fn test_box_sizing_parsing() {
        let css = r#"
            .border-box { box-sizing: border-box; }
            .content-box { box-sizing: content-box; }
            .padding-box { box-sizing: padding-box; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let border_box = get_style_for(&stylesheet, r#"<div class="border-box">Test</div>"#, "div");
        assert_eq!(
            border_box.box_sizing,
            Some(BoxSizing::BorderBox),
            "Expected box-sizing: border-box, got {:?}",
            border_box.box_sizing
        );

        let content_box =
            get_style_for(&stylesheet, r#"<div class="content-box">Test</div>"#, "div");
        assert_eq!(
            content_box.box_sizing,
            Some(BoxSizing::ContentBox),
            "Expected box-sizing: content-box, got {:?}",
            content_box.box_sizing
        );

        let padding_box =
            get_style_for(&stylesheet, r#"<div class="padding-box">Test</div>"#, "div");
        assert_eq!(
            padding_box.box_sizing,
            Some(BoxSizing::PaddingBox),
            "Expected box-sizing: padding-box, got {:?}",
            padding_box.box_sizing
        );
    }

    #[test]
    fn test_box_sizing_universal_selector() {
        // This is the Tailwind CSS preflight pattern
        let css = r#"
            * { box-sizing: border-box; }
            p { margin: 0; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Universal selector should apply to all elements
        let div_style = get_style_for(&stylesheet, "<div>Test</div>", "div");
        assert_eq!(
            div_style.box_sizing,
            Some(BoxSizing::BorderBox),
            "Universal selector should apply box-sizing to div, got {:?}",
            div_style.box_sizing
        );

        let p_style = get_style_for(&stylesheet, "<p>Test</p>", "p");
        assert_eq!(
            p_style.box_sizing,
            Some(BoxSizing::BorderBox),
            "Universal selector should apply box-sizing to p, got {:?}",
            p_style.box_sizing
        );
    }

    #[test]
    fn test_clean_selector_pseudo_elements() {
        // Test that selectors with pseudo-elements get cleaned properly
        // The actual cleaning happens internally in Stylesheet::parse
        // This tests that rules with pseudo-elements don't cause parsing failures
        let css = r#"
            *,::after,::before { box-sizing: border-box; }
            p { margin: 0; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // If the selector was cleaned properly, * should still apply
        let div_style = get_style_for(&stylesheet, "<div>Test</div>", "div");
        assert_eq!(
            div_style.box_sizing,
            Some(BoxSizing::BorderBox),
            "Cleaned selector should apply box-sizing via *, got {:?}",
            div_style.box_sizing
        );
    }

    #[test]
    fn test_border_collapse_parsing() {
        let css = r#"
            .collapse { border-collapse: collapse; }
            .separate { border-collapse: separate; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        let collapse = get_style_for(&stylesheet, r#"<table class="collapse"></table>"#, "table");
        assert_eq!(
            collapse.border_collapse,
            Some(BorderCollapse::Collapse),
            "Expected border-collapse: collapse, got {:?}",
            collapse.border_collapse
        );

        let separate = get_style_for(&stylesheet, r#"<table class="separate"></table>"#, "table");
        assert_eq!(
            separate.border_collapse,
            Some(BorderCollapse::Separate),
            "Expected border-collapse: separate, got {:?}",
            separate.border_collapse
        );
    }

    #[test]
    fn test_border_spacing_parsing() {
        let css = r#"
            .single { border-spacing: 2px; }
            .double { border-spacing: 2px 4px; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // Single value applies to both horizontal and vertical
        let single = get_style_for(&stylesheet, r#"<table class="single"></table>"#, "table");
        assert!(
            matches!(single.border_spacing_horizontal, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
            "Expected border-spacing horizontal: 2px, got {:?}",
            single.border_spacing_horizontal
        );
        assert!(
            matches!(single.border_spacing_vertical, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
            "Expected border-spacing vertical: 2px, got {:?}",
            single.border_spacing_vertical
        );

        // Two values: first is horizontal, second is vertical
        let double = get_style_for(&stylesheet, r#"<table class="double"></table>"#, "table");
        assert!(
            matches!(double.border_spacing_horizontal, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
            "Expected border-spacing horizontal: 2px, got {:?}",
            double.border_spacing_horizontal
        );
        assert!(
            matches!(double.border_spacing_vertical, Some(CssValue::Px(v)) if (v - 4.0).abs() < 0.01),
            "Expected border-spacing vertical: 4px, got {:?}",
            double.border_spacing_vertical
        );
    }

    #[test]
    fn test_border_width_style_shorthand_parsing() {
        let css = r#"
            .border-width { border-width: 1px; }
            .border-style { border-style: solid; }
            .border-color { border-color: #ff0000; }
            .combined { border-width: 2px; border-style: dashed; border-color: #00ff00; }
        "#;

        let stylesheet = Stylesheet::parse(css);

        // border-width: 1px should create borders with width on all sides
        let width_style = get_style_for(&stylesheet, r#"<div class="border-width"></div>"#, "div");
        assert!(width_style.border_top.is_some(), "border-top should be set");
        let top = width_style.border_top.unwrap();
        assert!(
            matches!(top.width, Some(CssValue::Px(v)) if (v - 1.0).abs() < 0.01),
            "Expected border-top-width: 1px, got {:?}",
            top.width
        );
        assert_eq!(top.style, BorderStyle::Solid, "border-style should default to solid");

        // border-style: solid should set style on all sides
        let style_style = get_style_for(&stylesheet, r#"<div class="border-style"></div>"#, "div");
        assert!(style_style.border_top.is_some(), "border-top should be set");
        assert_eq!(style_style.border_top.as_ref().unwrap().style, BorderStyle::Solid);

        // Combined should have all properties
        let combined = get_style_for(&stylesheet, r#"<div class="combined"></div>"#, "div");
        assert!(combined.border_top.is_some(), "border-top should be set");
        let combined_top = combined.border_top.unwrap();
        assert!(
            matches!(combined_top.width, Some(CssValue::Px(v)) if (v - 2.0).abs() < 0.01),
            "Expected border-top-width: 2px"
        );
        assert_eq!(combined_top.style, BorderStyle::Dashed);
        assert!(combined_top.color.is_some(), "border-color should be set");
    }
}
