//! Universal link representation for ebook formats.
//!
//! Ebooks use fundamentally different addressing modes:
//! - **EPUB**: Semantic IDs (`#footnote-1`, `chapter2.xhtml#section-5`)
//! - **AZW3/KFX**: Physical offsets (`kindle:pos:fid:000B:off:00000002SO`)
//!
//! This module provides a format-agnostic representation that captures both.

use super::NodeId;

/// Location within a chapter/spine item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InternalLocation {
    /// EPUB style: Go to the element with this ID.
    /// Example: `footnote-1` from `#footnote-1`
    ElementId(String),

    /// AZW3/KFX style: Go to this byte offset in the text stream.
    /// Kindle uses FID (file/fragment ID) + offset addressing.
    TextOffset(u32),
}

/// Target of an internal link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkTarget {
    /// Index into the spine (chapter order).
    /// None means same chapter (fragment-only link like `#footnote-1`).
    pub spine_index: Option<usize>,

    /// Location within the target chapter.
    pub location: InternalLocation,
}

/// A parsed link, either internal or external.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// External URL (http://, https://, mailto:, etc.)
    External(String),

    /// Internal link to another location in the book.
    Internal(LinkTarget),

    /// Unresolved/unknown link format.
    /// Stored for debugging but not actionable.
    Unknown(String),
}

impl Link {
    /// Check if this is an external link.
    pub fn is_external(&self) -> bool {
        matches!(self, Link::External(_))
    }

    /// Check if this is an internal link.
    pub fn is_internal(&self) -> bool {
        matches!(self, Link::Internal(_))
    }

    /// Get the URL if this is an external link.
    pub fn as_external(&self) -> Option<&str> {
        match self {
            Link::External(url) => Some(url),
            _ => None,
        }
    }

    /// Get the target if this is an internal link.
    pub fn as_internal(&self) -> Option<&LinkTarget> {
        match self {
            Link::Internal(target) => Some(target),
            _ => None,
        }
    }

    /// Parse a raw href string into a Link.
    ///
    /// This handles:
    /// - External URLs (http://, https://, mailto:)
    /// - EPUB fragment IDs (#footnote-1)
    /// - Kindle position URLs (kindle:pos:fid:...:off:...)
    /// - Relative paths (chapter2.xhtml#section-5)
    pub fn parse(href: &str) -> Link {
        let href = href.trim();

        // External URLs
        if href.starts_with("http://")
            || href.starts_with("https://")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
        {
            return Link::External(href.to_string());
        }

        // Kindle position links
        if href.starts_with("kindle:") {
            return Self::parse_kindle_link(href);
        }

        // Fragment-only link (#id)
        if let Some(fragment) = href.strip_prefix('#') {
            return Link::Internal(LinkTarget {
                spine_index: None,
                location: InternalLocation::ElementId(fragment.to_string()),
            });
        }

        // Relative path with fragment (file.xhtml#id)
        // For now, store as Unknown since we need spine resolution
        // The importer should handle this with full context
        if href.contains('#') || href.ends_with(".xhtml") || href.ends_with(".html") {
            return Link::Unknown(href.to_string());
        }

        // Anything else is unknown
        Link::Unknown(href.to_string())
    }

    /// Parse a Kindle position link.
    ///
    /// Format: `kindle:pos:fid:XXXX:off:YYYYYYYYYYYY`
    /// - fid: Fragment ID (hex, maps to spine position)
    /// - off: Byte offset within fragment (Kindle's custom base32)
    fn parse_kindle_link(href: &str) -> Link {
        // Try to extract fid and off values
        let parts: Vec<&str> = href.split(':').collect();

        // Expected: ["kindle", "pos", "fid", "XXXX", "off", "YYYY"]
        if parts.len() >= 6 && parts[1] == "pos" && parts[2] == "fid" && parts[4] == "off" {
            let fid_str = parts[3];
            let off_str = parts[5];

            // Parse FID as hex
            if let Ok(fid) = u32::from_str_radix(fid_str, 16) {
                // Parse offset using Kindle's base32
                if let Some(offset) = kindle_base32_decode(off_str) {
                    return Link::Internal(LinkTarget {
                        // FID maps to spine index, but we'd need the book's
                        // fragment map to do this properly. For now, store raw.
                        spine_index: Some(fid as usize),
                        location: InternalLocation::TextOffset(offset),
                    });
                }
            }
        }

        // Couldn't parse, store as unknown
        Link::Unknown(href.to_string())
    }
}

/// Decode Kindle's custom base32 offset encoding.
///
/// Kindle uses a non-standard base32 with digits: 0-9, A-V (case insensitive).
/// The offset is big-endian.
fn kindle_base32_decode(s: &str) -> Option<u32> {
    let mut result: u64 = 0;

    for c in s.chars() {
        let digit = match c {
            '0'..='9' => c as u64 - '0' as u64,
            'A'..='V' => c as u64 - 'A' as u64 + 10,
            'a'..='v' => c as u64 - 'a' as u64 + 10,
            _ => return None,
        };

        result = result * 32 + digit;

        // Overflow check
        if result > u32::MAX as u64 {
            return None;
        }
    }

    Some(result as u32)
}

/// Map from NodeId to parsed Link.
///
/// This replaces storing raw href strings with structured link data.
#[derive(Debug, Default, Clone)]
pub struct LinkMap {
    links: std::collections::HashMap<NodeId, Link>,
}

impl LinkMap {
    /// Create a new empty link map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a link for a node, parsing the raw href.
    pub fn set(&mut self, node: NodeId, href: &str) {
        if !href.is_empty() {
            self.links.insert(node, Link::parse(href));
        }
    }

    /// Set an already-parsed link for a node.
    pub fn set_parsed(&mut self, node: NodeId, link: Link) {
        self.links.insert(node, link);
    }

    /// Get the link for a node.
    pub fn get(&self, node: NodeId) -> Option<&Link> {
        self.links.get(&node)
    }

    /// Check if a node has a link.
    pub fn contains(&self, node: NodeId) -> bool {
        self.links.contains_key(&node)
    }

    /// Get the number of links.
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_external_links() {
        assert!(matches!(
            Link::parse("https://example.com"),
            Link::External(_)
        ));
        assert!(matches!(
            Link::parse("http://example.com"),
            Link::External(_)
        ));
        assert!(matches!(
            Link::parse("mailto:user@example.com"),
            Link::External(_)
        ));
    }

    #[test]
    fn test_parse_fragment_link() {
        let link = Link::parse("#footnote-1");
        match link {
            Link::Internal(target) => {
                assert_eq!(target.spine_index, None);
                assert_eq!(
                    target.location,
                    InternalLocation::ElementId("footnote-1".to_string())
                );
            }
            _ => panic!("Expected internal link"),
        }
    }

    #[test]
    fn test_parse_kindle_link() {
        let link = Link::parse("kindle:pos:fid:000B:off:00000002SO");
        match link {
            Link::Internal(target) => {
                assert_eq!(target.spine_index, Some(11)); // 0x000B = 11
                match target.location {
                    InternalLocation::TextOffset(offset) => {
                        // "2SO" in Kindle base32
                        assert!(offset > 0);
                    }
                    _ => panic!("Expected TextOffset"),
                }
            }
            _ => panic!("Expected internal link, got {:?}", link),
        }
    }

    #[test]
    fn test_kindle_base32_decode() {
        // Simple cases
        assert_eq!(kindle_base32_decode("0"), Some(0));
        assert_eq!(kindle_base32_decode("1"), Some(1));
        assert_eq!(kindle_base32_decode("A"), Some(10));
        assert_eq!(kindle_base32_decode("V"), Some(31));

        // Multi-digit
        assert_eq!(kindle_base32_decode("10"), Some(32)); // 1*32 + 0
        assert_eq!(kindle_base32_decode("11"), Some(33)); // 1*32 + 1
    }
}
