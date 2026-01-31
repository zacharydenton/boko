//! Maps HTML elements to semantic roles.
//!
//! This module defines the mapping from HTML element names to IR `Role` values.

use html5ever::LocalName;

use crate::model::Role;

/// Map an HTML element name to its semantic role.
pub fn element_to_role(local_name: &LocalName) -> Role {
    match local_name.as_ref() {
        // Block containers
        "div" | "section" | "article" | "nav" | "header" | "footer" | "main" | "address"
        | "details" | "summary" | "hgroup" => Role::Container,

        // Line break (leaf node, not a container)
        "br" => Role::Break,

        // Horizontal rule (thematic break)
        "hr" => Role::Rule,

        // Aside/sidebar
        "aside" => Role::Sidebar,

        // Figure and caption
        "figure" => Role::Figure,
        "figcaption" | "caption" => Role::Caption,

        // Paragraphs - block-level text containers
        "p" => Role::Paragraph,

        // Preformatted code blocks
        "pre" => Role::CodeBlock,

        // Inline elements with styling (rendered via ComputedStyle)
        "span" | "em" | "i" | "cite" | "var" | "dfn" | "strong" | "b" | "code" | "kbd" | "samp"
        | "tt" | "sup" | "sub" | "u" | "ins" | "s" | "strike" | "del" | "small" | "mark"
        | "abbr" | "time" | "q" => Role::Inline,

        // Headings with level
        "h1" => Role::Heading(1),
        "h2" => Role::Heading(2),
        "h3" => Role::Heading(3),
        "h4" => Role::Heading(4),
        "h5" => Role::Heading(5),
        "h6" => Role::Heading(6),

        // Links
        "a" => Role::Link,

        // Images
        "img" => Role::Image,

        // Lists
        "ul" => Role::UnorderedList,
        "ol" => Role::OrderedList,
        "li" => Role::ListItem,

        // Block quote
        "blockquote" => Role::BlockQuote,

        // Definition lists
        "dl" => Role::DefinitionList,
        "dt" => Role::DefinitionTerm,
        "dd" => Role::DefinitionDescription,

        // Tables
        "table" => Role::Table,
        "thead" => Role::TableHead,
        "tbody" => Role::TableBody,
        "tr" => Role::TableRow,
        "td" | "th" => Role::TableCell,

        // Other inline containers
        "label" | "legend" | "output" | "data" | "ruby" | "rt" | "rp" | "bdi" | "bdo" | "wbr" => {
            Role::Inline
        }

        // Default to container for unknown block elements
        _ => Role::Container,
    }
}
