//! Pure slug generation for Markdown heading anchors.
//!
//! Generates GitHub-style slugs from heading text for use in internal links.

use std::collections::{HashMap, HashSet};

use crate::import::ChapterId;
use crate::model::{AnchorTarget, Chapter, GlobalNodeId, NodeId, ResolvedLinks, Role};

/// Generate a GitHub-style slug from text.
///
/// Converts text to lowercase, replaces spaces and special characters with hyphens,
/// and removes consecutive/leading/trailing hyphens.
///
/// # Examples
///
/// ```
/// use boko::markdown::slugify;
///
/// assert_eq!(slugify("Chapter One"), "chapter-one");
/// assert_eq!(slugify("Hello, World!"), "hello-world");
/// assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
/// ```
pub fn slugify(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c.is_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                // Skip other characters
                '\0'
            }
        })
        .filter(|&c| c != '\0')
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Collect text content from a heading node (for slug generation).
///
/// Recursively collects all text from a node and its children,
/// normalizing whitespace.
pub fn collect_heading_text(chapter: &Chapter, id: NodeId) -> String {
    let mut result = String::new();
    collect_text_recursive(chapter, id, &mut result);
    result
}

fn collect_text_recursive(chapter: &Chapter, id: NodeId, result: &mut String) {
    let Some(node) = chapter.node(id) else {
        return;
    };

    if node.role == Role::Text && !node.text.is_empty() {
        let text = chapter.text(node.text);
        let has_leading = text.starts_with(char::is_whitespace);
        let has_trailing = text.ends_with(char::is_whitespace);
        let words: Vec<&str> = text.split_whitespace().collect();

        if !words.is_empty() {
            if has_leading && !result.is_empty() && !result.ends_with(' ') {
                result.push(' ');
            }
            result.push_str(&words.join(" "));
            if has_trailing {
                result.push(' ');
            }
        } else if !text.is_empty() && !result.is_empty() && !result.ends_with(' ') {
            result.push(' ');
        }
    }

    for child_id in chapter.children(id) {
        collect_text_recursive(chapter, child_id, result);
    }
}

/// Build a map of heading targets to their GitHub-style slugs.
///
/// For internal links pointing to headings, we use the heading text as a slug
/// (e.g., "Chapter One" → "#chapter-one") instead of explicit anchor IDs.
///
/// This function takes pre-loaded chapters (no I/O) and builds the slug map.
/// It accepts any chapter container that derefs to `Chapter` (e.g., `&Chapter`,
/// `Arc<Chapter>`, `Box<Chapter>`).
///
/// # Arguments
///
/// * `chapters` - Slice of (ChapterId, C) pairs where C derefs to Chapter
/// * `resolved` - The resolved links to check for internal targets
///
/// # Returns
///
/// HashMap mapping GlobalNodeId (heading nodes) to their slugs
pub fn build_heading_slugs<C: std::ops::Deref<Target = Chapter>>(
    chapters: &[(ChapterId, C)],
    resolved: &ResolvedLinks,
) -> HashMap<GlobalNodeId, String> {
    // Collect all internal link targets
    let mut targets: HashSet<GlobalNodeId> = HashSet::new();
    for (_, target) in resolved.iter() {
        if let AnchorTarget::Internal(gid) = target {
            targets.insert(*gid);
        }
    }

    let mut heading_slugs = HashMap::new();

    // Check each target - if it's a heading, compute and store its slug
    for (chapter_id, chapter) in chapters {
        let chapter: &Chapter = chapter;
        for &target in &targets {
            if target.chapter != *chapter_id {
                continue;
            }

            if let Some(node) = chapter.node(target.node)
                && matches!(node.role, Role::Heading(_))
            {
                let text = collect_heading_text(chapter, target.node);
                let slug = slugify(&text);
                if !slug.is_empty() {
                    heading_slugs.insert(target, slug);
                }
            }
        }
    }

    heading_slugs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_simple() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn test_slugify_with_punctuation() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
    }

    #[test]
    fn test_slugify_multiple_spaces() {
        assert_eq!(slugify("Hello   World"), "hello-world");
    }

    #[test]
    fn test_slugify_leading_trailing_spaces() {
        assert_eq!(slugify("  Hello World  "), "hello-world");
    }

    #[test]
    fn test_slugify_underscores() {
        assert_eq!(slugify("hello_world"), "hello-world");
    }

    #[test]
    fn test_slugify_mixed_case() {
        assert_eq!(slugify("Chapter ONE"), "chapter-one");
    }

    #[test]
    fn test_slugify_numbers() {
        assert_eq!(slugify("Chapter 1"), "chapter-1");
    }

    #[test]
    fn test_slugify_empty() {
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("   "), "");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn test_slugify_hyphens() {
        assert_eq!(slugify("hello--world"), "hello-world");
        assert_eq!(slugify("-hello-"), "hello");
    }
}
