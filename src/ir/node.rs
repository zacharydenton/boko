//! IR node types and roles.

/// Unique identifier for a node within an IRChapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// The root node ID (always 0).
    pub const ROOT: NodeId = NodeId(0);
}

/// Semantic role of a node (independent of source element).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Role {
    /// Generic block container (div, section, article).
    #[default]
    Block,
    /// Paragraph.
    Paragraph,
    /// Heading with level 1-6.
    Heading(u8),
    /// Inline span.
    Span,
    /// Hyperlink.
    Link,
    /// Image.
    Image,
    /// Emphasized text (em, i).
    Emphasis,
    /// Strong text (strong, b).
    Strong,
    /// Code/monospace text.
    Code,
    /// Block quote.
    BlockQuote,
    /// Ordered or unordered list.
    List { ordered: bool },
    /// List item.
    ListItem,
    /// Table.
    Table,
    /// Table row.
    TableRow,
    /// Table cell.
    TableCell { header: bool },
    /// Preformatted text.
    Preformatted,
    /// Line break.
    LineBreak,
    /// Horizontal rule.
    HorizontalRule,
    /// Text content (leaf node).
    Text,
    /// Root document node.
    Root,
}

/// Range into the global text buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextRange {
    /// Byte offset into IRChapter.text.
    pub start: u32,
    /// Length in bytes.
    pub len: u32,
}

impl TextRange {
    /// Create a new text range.
    pub fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    /// Check if the range is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the end offset.
    pub fn end(&self) -> u32 {
        self.start + self.len
    }
}

use super::style::StyleId;

/// A node in the IR tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// Semantic role.
    pub role: Role,
    /// Parent node (None for root).
    pub parent: Option<NodeId>,
    /// First child node.
    pub first_child: Option<NodeId>,
    /// Next sibling node.
    pub next_sibling: Option<NodeId>,
    /// Style identifier.
    pub style: StyleId,
    /// Text content range (only for Text nodes).
    pub text: TextRange,
}

impl Node {
    /// Create a new node with default values.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            parent: None,
            first_child: None,
            next_sibling: None,
            style: StyleId::DEFAULT,
            text: TextRange::default(),
        }
    }

    /// Create a text node with the given range.
    pub fn text(range: TextRange) -> Self {
        Self {
            role: Role::Text,
            parent: None,
            first_child: None,
            next_sibling: None,
            style: StyleId::DEFAULT,
            text: range,
        }
    }
}
