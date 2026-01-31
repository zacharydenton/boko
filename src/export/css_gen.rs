//! CSS generation from StylePool.
//!
//! This module converts the computed styles stored in a StylePool back into
//! CSS text for inclusion in exported ebooks. Rather than using inline styles
//! (which bloat the file), we generate a deduplicated stylesheet with class names.
//!
//! # Example
//!
//! ```
//! use boko::model::Chapter;
//! use boko::style::StyleId;
//! use boko::export::generate_css;
//!
//! let chapter = Chapter::new();
//! let used_styles = vec![StyleId::DEFAULT];
//! let artifact = generate_css(&chapter.styles, &used_styles);
//!
//! // artifact.stylesheet contains the CSS text
//! // artifact.class_map maps StyleId -> class name (e.g., "c0")
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::style::{StyleId, StylePool, ToCss};

/// Generated CSS artifact containing the stylesheet and class mapping.
#[derive(Debug, Clone)]
pub struct CssArtifact {
    /// The generated CSS stylesheet text.
    pub stylesheet: String,
    /// Map from StyleId to CSS class name (e.g., StyleId(5) -> "c5").
    pub class_map: HashMap<StyleId, String>,
}

impl CssArtifact {
    /// Get the CSS class name for a style ID, if one exists.
    pub fn class_name(&self, id: StyleId) -> Option<&str> {
        self.class_map.get(&id).map(|s| s.as_str())
    }

    /// Check if the stylesheet is empty (no non-default styles).
    pub fn is_empty(&self) -> bool {
        self.stylesheet.is_empty()
    }
}

/// Generate CSS from a StylePool for the given used styles.
///
/// This function:
/// 1. Deduplicates the provided style IDs
/// 2. Generates a unique CSS class for each unique style (e.g., `.c1`, `.c2`)
/// 3. Only outputs properties that differ from defaults
///
/// # Arguments
///
/// * `pool` - The StylePool containing all interned styles
/// * `used_styles` - Slice of StyleIds actually used in the content
///
/// # Returns
///
/// A `CssArtifact` containing the stylesheet text and class name mapping.
pub fn generate_css(pool: &StylePool, used_styles: &[StyleId]) -> CssArtifact {
    let mut stylesheet = String::new();
    let mut class_map = HashMap::new();

    // Deduplicate and sort for deterministic output
    let unique_styles: HashSet<StyleId> = used_styles.iter().copied().collect();
    let mut sorted_styles: Vec<StyleId> = unique_styles.into_iter().collect();
    sorted_styles.sort_by_key(|s| s.0);

    for id in sorted_styles {
        let Some(style) = pool.get(id) else {
            continue;
        };

        // Skip the default style (no CSS needed)
        if style.is_default() {
            continue;
        }

        let class_name = format!("c{}", id.0);

        // Generate CSS rule
        write!(stylesheet, ".{} {{ ", class_name).unwrap();
        style.to_css(&mut stylesheet);
        stylesheet.push_str("}\n");

        class_map.insert(id, class_name);
    }

    CssArtifact {
        stylesheet,
        class_map,
    }
}

/// Generate CSS from a StylePool, including all styles in the pool.
///
/// This is a convenience function when you don't know which styles are used.
/// Generally prefer `generate_css()` with the actual used styles.
pub fn generate_css_all(pool: &StylePool) -> CssArtifact {
    let all_ids: Vec<StyleId> = pool.iter().map(|(id, _)| id).collect();
    generate_css(pool, &all_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, ComputedStyle, FontWeight, TextAlign};

    #[test]
    fn test_generate_css_empty() {
        let pool = StylePool::new();
        let artifact = generate_css(&pool, &[StyleId::DEFAULT]);

        // Default style should produce no CSS
        assert!(artifact.stylesheet.is_empty());
        assert!(artifact.class_map.is_empty());
    }

    #[test]
    fn test_generate_css_with_styles() {
        let mut pool = StylePool::new();

        // Create a bold style
        let bold_style = ComputedStyle {
            font_weight: FontWeight::BOLD,
            ..Default::default()
        };
        let bold_id = pool.intern(bold_style);

        // Create a centered style
        let center_style = ComputedStyle {
            text_align: TextAlign::Center,
            ..Default::default()
        };
        let center_id = pool.intern(center_style);

        let artifact = generate_css(&pool, &[bold_id, center_id]);

        // Should have two CSS rules
        assert!(artifact.stylesheet.contains(".c1"));
        assert!(artifact.stylesheet.contains(".c2"));
        assert!(artifact.stylesheet.contains("font-weight: bold"));
        assert!(artifact.stylesheet.contains("text-align: center"));

        // Class map should have entries
        assert_eq!(artifact.class_name(bold_id), Some("c1"));
        assert_eq!(artifact.class_name(center_id), Some("c2"));
    }

    #[test]
    fn test_generate_css_color() {
        let mut pool = StylePool::new();

        let style = ComputedStyle {
            color: Some(Color::rgb(255, 0, 0)),
            ..Default::default()
        };
        let id = pool.intern(style);

        let artifact = generate_css(&pool, &[id]);

        assert!(artifact.stylesheet.contains("color: #ff0000"));
    }

    #[test]
    fn test_generate_css_deduplicates() {
        let mut pool = StylePool::new();

        let style = ComputedStyle {
            font_weight: FontWeight::BOLD,
            ..Default::default()
        };
        let id = pool.intern(style);

        // Use the same style multiple times
        let artifact = generate_css(&pool, &[id, id, id]);

        // Should only generate one rule
        let rule_count = artifact.stylesheet.matches(".c").count();
        assert_eq!(rule_count, 1);
    }

    #[test]
    fn test_generate_css_all() {
        let mut pool = StylePool::new();

        let style = ComputedStyle {
            font_weight: FontWeight::BOLD,
            ..Default::default()
        };
        pool.intern(style);

        let artifact = generate_css_all(&pool);

        // Should include the bold style
        assert!(artifact.stylesheet.contains("font-weight: bold"));
    }
}
