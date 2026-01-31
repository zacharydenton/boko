//! Style pool for interning and deduplication.

use std::collections::HashMap;

use super::types::{ComputedStyle, StyleId};

/// SoA style pool for efficient storage and deduplication.
///
/// Styles are interned: identical styles share the same StyleId.
/// This is memory-efficient when many elements share the same style.
#[derive(Clone)]
pub struct StylePool {
    /// All unique styles.
    styles: Vec<ComputedStyle>,
    /// Hash-based deduplication map.
    intern_map: HashMap<ComputedStyle, StyleId>,
}

impl Default for StylePool {
    fn default() -> Self {
        Self::new()
    }
}

impl StylePool {
    /// Create a new style pool with the default style at index 0.
    pub fn new() -> Self {
        let default_style = ComputedStyle::default();
        let mut intern_map = HashMap::new();
        intern_map.insert(default_style.clone(), StyleId::DEFAULT);

        Self {
            styles: vec![default_style],
            intern_map,
        }
    }

    /// Intern a style, returning its StyleId.
    ///
    /// If an identical style already exists, returns the existing ID.
    /// Otherwise, allocates a new style and returns its ID.
    pub fn intern(&mut self, style: ComputedStyle) -> StyleId {
        if let Some(&id) = self.intern_map.get(&style) {
            return id;
        }

        let id = StyleId(self.styles.len() as u32);
        self.intern_map.insert(style.clone(), id);
        self.styles.push(style);
        id
    }

    /// Get a style by ID.
    pub fn get(&self, id: StyleId) -> Option<&ComputedStyle> {
        self.styles.get(id.0 as usize)
    }

    /// Get the number of unique styles.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Check if the pool is empty (should never be, as default style is always present).
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Iterate over all (StyleId, ComputedStyle) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (StyleId, &ComputedStyle)> {
        self.styles
            .iter()
            .enumerate()
            .map(|(i, s)| (StyleId(i as u32), s))
    }
}

impl std::fmt::Debug for StylePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StylePool")
            .field("count", &self.styles.len())
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::style::FontWeight;

    #[test]
    fn test_style_pool_interning() {
        let mut pool = StylePool::new();

        let mut style1 = ComputedStyle::default();
        style1.font_weight = FontWeight::BOLD;

        let id1 = pool.intern(style1.clone());
        let id2 = pool.intern(style1);

        // Same style should get same ID
        assert_eq!(id1, id2);
        assert_eq!(pool.len(), 2); // default + bold
    }

    #[test]
    fn test_style_pool_iter() {
        let mut pool = StylePool::new();

        let mut style = ComputedStyle::default();
        style.font_weight = FontWeight::BOLD;
        pool.intern(style);

        let ids: Vec<StyleId> = pool.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![StyleId(0), StyleId(1)]);
    }
}
