//! IR node types and roles.

/// Unique identifier for a node within an IRChapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// The root node ID (always 0).
    pub const ROOT: NodeId = NodeId(0);
}

/// Semantic role of a node (independent of source element).
///
/// This is a simplified role system focused on structural semantics.
/// Visual styling (bold, italic, font-size) is handled by `ComputedStyle`.
/// Semantic attributes (href, src, alt, epub:type) are in `SemanticMap`.
///
/// Design principle: Roles map to markdown concepts:
/// - Text (leaf text content)
/// - Heading(level) (h1-h6)
/// - Link, Image
/// - List(kind), ListItem
/// - BlockQuote
/// - Table, TableRow, TableCell
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Role {
    /// Leaf text content node containing actual string data.
    /// References a range in the chapter's text buffer.
    #[default]
    Text,
    /// Headings with level 1-6.
    Heading(u8),
    /// Generic layout/box container (div, section, article, etc.)
    Container,
    /// Raster images. src/alt in SemanticMap.
    Image,
    /// Hyperlinks. href in SemanticMap.
    Link,
    /// Ordered list (`<ol>`).
    OrderedList,
    /// Unordered list (`<ul>`).
    UnorderedList,
    /// Individual list items.
    ListItem,
    /// Table structure.
    Table,
    /// Table rows.
    TableRow,
    /// Table cells (header vs data is in style).
    TableCell,
    /// Sidebar/aside content.
    Sidebar,
    /// Footnote containers.
    Footnote,
    /// Figure/illustration wrappers.
    Figure,
    /// Generic inline container (e.g., `<span>`).
    /// Distinct from Text which contains actual string data.
    Inline,
    /// Block quotes.
    BlockQuote,
    /// Root document node.
    Root,
    /// Semantic line break (`<br>`).
    /// A leaf node that signifies a layout break, not a container.
    Break,
    /// Horizontal rule (`<hr>`).
    /// A leaf node representing a thematic break.
    Rule,
    /// Definition list container (`<dl>`).
    DefinitionList,
    /// Definition term (`<dt>`).
    DefinitionTerm,
    /// Definition description (`<dd>`).
    DefinitionDescription,
    /// Code block (`<pre><code>`).
    /// Language is stored in SemanticMap.language.
    CodeBlock,
    /// Caption for figures or tables (`<figcaption>`, `<caption>`).
    Caption,
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
