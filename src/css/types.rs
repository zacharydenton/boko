//! CSS value types and enums.
//!
//! Contains all CSS property value types used throughout the library.

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

/// Ruby position (where ruby text appears relative to base text)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RubyPosition {
    #[default]
    Over,
    Under,
}

/// Ruby alignment (how ruby text is aligned with base text)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RubyAlign {
    #[default]
    Center,
    Start,
    SpaceAround,
    SpaceBetween,
}

/// Ruby merge (how adjacent ruby annotations are combined)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RubyMerge {
    #[default]
    Separate,
    Collapse,
}

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

/// Border collapse for tables
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderCollapse {
    #[default]
    Separate,
    Collapse,
}

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

/// Drop cap configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DropCap {
    /// Number of lines the drop cap spans
    pub lines: u8,
    /// Number of characters in the drop cap
    pub chars: u8,
}

/// Text decoration line style (how the decoration line is drawn)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextDecorationLineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
    Double,
}

/// 2D Transform matrix [a, b, c, d, tx, ty]
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    /// Matrix values [a, b, c, d, tx, ty]
    pub matrix: [f32; 6],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
}

impl Eq for Transform {}

impl std::hash::Hash for Transform {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
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

/// Column count for multi-column layout
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColumnCount {
    #[default]
    Auto,
    Count(u32),
}

/// Unicode bidirectional text algorithm control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UnicodeBidi {
    #[default]
    Normal,
    Embed,
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

/// Line breaking rules (primarily for CJK text)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineBreak {
    #[default]
    Auto,
    Normal,
    Loose,
    Strict,
    Anywhere,
}

/// Text orientation in vertical writing mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextOrientation {
    #[default]
    Mixed,
    Upright,
    Sideways,
}
