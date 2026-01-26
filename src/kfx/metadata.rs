//! KFX metadata schema - declarative mapping from IR to KFX metadata.
//!
//! This module defines the rules for converting book metadata into KFX's
//! categorised_metadata format. Adding new metadata fields requires only
//! updating the schema, not changing export logic.

use crate::book::Metadata;

/// Category for KFX metadata entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataCategory {
    /// Book title, author, language, etc.
    KindleTitle,
    /// eBook capabilities (selection, nested_span, etc.)
    KindleEbook,
    /// Creator/audit information
    KindleAudit,
}

impl MetadataCategory {
    /// Get the KFX category string.
    pub fn as_str(self) -> &'static str {
        match self {
            MetadataCategory::KindleTitle => "kindle_title_metadata",
            MetadataCategory::KindleEbook => "kindle_ebook_metadata",
            MetadataCategory::KindleAudit => "kindle_audit_metadata",
        }
    }
}

/// A rule for mapping a metadata field to KFX format.
#[derive(Debug, Clone)]
pub struct MetadataRule {
    /// The KFX key name (e.g., "title", "author").
    pub key: &'static str,
    /// Which category this belongs to.
    pub category: MetadataCategory,
    /// How to extract the value from Metadata.
    pub source: MetadataSource,
}

/// Source of metadata value.
#[derive(Debug, Clone)]
pub enum MetadataSource {
    /// Static string value.
    Static(&'static str),
    /// Dynamic value from Metadata struct.
    Dynamic(MetadataField),
}

/// Fields that can be extracted from Metadata.
#[derive(Debug, Clone, Copy)]
pub enum MetadataField {
    Title,
    Language,
    FirstAuthor,
    Description,
    Publisher,
    Identifier,
    Date,
}

impl MetadataField {
    /// Extract the value from a Metadata struct.
    /// Returns None if the field is empty or not set.
    pub fn extract(self, meta: &Metadata) -> Option<&str> {
        match self {
            MetadataField::Title => {
                if meta.title.is_empty() {
                    None
                } else {
                    Some(&meta.title)
                }
            }
            MetadataField::Language => {
                if meta.language.is_empty() {
                    None
                } else {
                    Some(&meta.language)
                }
            }
            MetadataField::FirstAuthor => meta.authors.first().map(|s| s.as_str()),
            MetadataField::Description => meta.description.as_deref(),
            MetadataField::Publisher => meta.publisher.as_deref(),
            MetadataField::Identifier => {
                if meta.identifier.is_empty() {
                    None
                } else {
                    Some(&meta.identifier)
                }
            }
            MetadataField::Date => meta.date.as_deref(),
        }
    }
}

/// Get the standard KFX metadata schema.
///
/// This returns all the rules for converting book metadata to KFX format.
/// To add a new metadata field, add a rule here - no export code changes needed.
pub fn metadata_schema() -> Vec<MetadataRule> {
    vec![
        // kindle_title_metadata category
        MetadataRule {
            key: "title",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::Title),
        },
        MetadataRule {
            key: "language",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::Language),
        },
        MetadataRule {
            key: "author",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::FirstAuthor),
        },
        MetadataRule {
            key: "description",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::Description),
        },
        MetadataRule {
            key: "publisher",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::Publisher),
        },
        // kindle_ebook_metadata category
        MetadataRule {
            key: "selection",
            category: MetadataCategory::KindleEbook,
            source: MetadataSource::Static("enabled"),
        },
        MetadataRule {
            key: "nested_span",
            category: MetadataCategory::KindleEbook,
            source: MetadataSource::Static("enabled"),
        },
        // kindle_audit_metadata category
        MetadataRule {
            key: "file_creator",
            category: MetadataCategory::KindleAudit,
            source: MetadataSource::Static("boko"),
        },
    ]
}

/// Build metadata entries for a category from the schema.
///
/// This is a pure function that applies the schema rules to extract
/// metadata values from the book's Metadata struct.
///
/// # Arguments
///
/// * `category` - The category to build entries for
/// * `meta` - The book's metadata
/// * `version` - Optional version string for audit metadata
///
/// # Returns
///
/// A vector of (key, value) pairs for the category.
pub fn build_category_entries(
    category: MetadataCategory,
    meta: &Metadata,
    version: Option<&str>,
) -> Vec<(&'static str, String)> {
    let schema = metadata_schema();
    let mut entries = Vec::new();

    for rule in schema.iter().filter(|r| r.category == category) {
        let value = match &rule.source {
            MetadataSource::Static(s) => Some(s.to_string()),
            MetadataSource::Dynamic(field) => field.extract(meta).map(|s| s.to_string()),
        };

        if let Some(v) = value {
            entries.push((rule.key, v));
        }
    }

    // Special case: add version to audit metadata
    if category == MetadataCategory::KindleAudit {
        if let Some(v) = version {
            entries.push(("creator_version", v.to_string()));
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_field_extraction() {
        let meta = Metadata {
            title: "Test Book".to_string(),
            authors: vec!["Author One".to_string()],
            language: "en".to_string(),
            description: Some("A description".to_string()),
            publisher: None,
            ..Default::default()
        };

        assert_eq!(MetadataField::Title.extract(&meta), Some("Test Book"));
        assert_eq!(MetadataField::FirstAuthor.extract(&meta), Some("Author One"));
        assert_eq!(MetadataField::Language.extract(&meta), Some("en"));
        assert_eq!(
            MetadataField::Description.extract(&meta),
            Some("A description")
        );
        assert_eq!(MetadataField::Publisher.extract(&meta), None);
    }

    #[test]
    fn test_build_category_entries() {
        let meta = Metadata {
            title: "Test Book".to_string(),
            authors: vec!["Author".to_string()],
            language: "en".to_string(),
            ..Default::default()
        };

        let entries = build_category_entries(MetadataCategory::KindleTitle, &meta, None);

        // Should have title, language, author (but not description/publisher since they're None)
        assert!(entries.iter().any(|(k, v)| *k == "title" && v == "Test Book"));
        assert!(entries.iter().any(|(k, v)| *k == "language" && v == "en"));
        assert!(entries.iter().any(|(k, v)| *k == "author" && v == "Author"));
        assert!(!entries.iter().any(|(k, _)| *k == "description"));
    }

    #[test]
    fn test_build_ebook_entries() {
        let meta = Metadata::default();
        let entries = build_category_entries(MetadataCategory::KindleEbook, &meta, None);

        assert!(entries
            .iter()
            .any(|(k, v)| *k == "selection" && v == "enabled"));
        assert!(entries
            .iter()
            .any(|(k, v)| *k == "nested_span" && v == "enabled"));
    }

    #[test]
    fn test_build_audit_entries_with_version() {
        let meta = Metadata::default();
        let entries = build_category_entries(MetadataCategory::KindleAudit, &meta, Some("1.0.0"));

        assert!(entries
            .iter()
            .any(|(k, v)| *k == "file_creator" && v == "boko"));
        assert!(entries
            .iter()
            .any(|(k, v)| *k == "creator_version" && v == "1.0.0"));
    }

    #[test]
    fn test_category_strings() {
        assert_eq!(
            MetadataCategory::KindleTitle.as_str(),
            "kindle_title_metadata"
        );
        assert_eq!(
            MetadataCategory::KindleEbook.as_str(),
            "kindle_ebook_metadata"
        );
        assert_eq!(
            MetadataCategory::KindleAudit.as_str(),
            "kindle_audit_metadata"
        );
    }
}
