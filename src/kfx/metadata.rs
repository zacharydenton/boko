//! KFX metadata schema - declarative mapping from IR to KFX metadata.
//!
//! This module defines the rules for converting book metadata into KFX's
//! categorised_metadata format. Adding new metadata fields requires only
//! updating the schema, not changing export logic.

use crate::model::Metadata;

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
    CoverImage,
    /// Asset ID - from context (container ID), not Metadata.
    AssetId,
    /// Book ID - from context (derived from identifier), not Metadata.
    BookId,
    /// dcterms:modified timestamp
    ModifiedDate,
    /// First contributor with role="trl" (translator)
    Translator,
    /// file-as for title (sort key)
    TitleSort,
    /// file-as for first author (sort key)
    AuthorSort,
    /// Series/collection name
    SeriesName,
    /// Series position (group-position)
    SeriesPosition,
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
            MetadataField::CoverImage => meta.cover_image.as_deref(),
            MetadataField::ModifiedDate => meta.modified_date.as_deref(),
            MetadataField::Translator => {
                // Find first contributor with role "trl"
                meta.contributors
                    .iter()
                    .find(|c| c.role.as_deref() == Some("trl"))
                    .map(|c| c.name.as_str())
            }
            MetadataField::TitleSort => meta.title_sort.as_deref(),
            MetadataField::AuthorSort => meta.author_sort.as_deref(),
            MetadataField::SeriesName => meta.collection.as_ref().map(|c| c.name.as_str()),
            // These are context-driven or need special handling
            MetadataField::AssetId | MetadataField::BookId | MetadataField::SeriesPosition => None,
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
        MetadataRule {
            key: "issue_date",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::Date),
        },
        MetadataRule {
            key: "cover_image",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::CoverImage),
        },
        MetadataRule {
            key: "asset_id",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::AssetId),
        },
        MetadataRule {
            key: "book_id",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::BookId),
        },
        MetadataRule {
            key: "cde_content_type",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Static("EBOK"),
        },
        // Extended metadata for better round-trip fidelity
        MetadataRule {
            key: "modified_date",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::ModifiedDate),
        },
        MetadataRule {
            key: "translator",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::Translator),
        },
        MetadataRule {
            key: "title_pronunciation",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::TitleSort),
        },
        MetadataRule {
            key: "author_pronunciation",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::AuthorSort),
        },
        MetadataRule {
            key: "series_name",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::SeriesName),
        },
        MetadataRule {
            key: "series_position",
            category: MetadataCategory::KindleTitle,
            source: MetadataSource::Dynamic(MetadataField::SeriesPosition),
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

use crate::util::truncate_to_date;

/// Context for metadata entry building.
///
/// This provides values that need transformation during export,
/// such as resource names that are generated during the export process.
#[derive(Debug, Default)]
pub struct MetadataContext<'a> {
    /// Version string for audit metadata.
    pub version: Option<&'a str>,
    /// Cover image resource name (e.g., "e6"), not the path.
    pub cover_resource_name: Option<&'a str>,
    /// Asset ID (same as container ID, changes per export).
    /// Format: "CR!" + 28 uppercase alphanumeric characters.
    pub asset_id: Option<&'a str>,
    /// Book ID (stable per publication, derived from identifier).
    /// Format: 23-character URL-safe Base64.
    pub book_id: Option<String>,
}

/// Generate a book ID from a publication identifier.
///
/// The book ID is a stable identifier for the publication that persists
/// across different exports of the same book. It's derived deterministically
/// from the book's identifier (e.g., ISBN, UUID).
///
/// Format: 23-character URL-safe Base64 (version byte + 16 derived bytes)
pub fn generate_book_id(identifier: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Version prefix (0x05 based on reference KFX files)
    let mut bytes = vec![0x05u8];

    // Hash the identifier to get deterministic bytes
    let mut hasher = DefaultHasher::new();
    identifier.hash(&mut hasher);
    let hash1 = hasher.finish();
    // Hash again with salt for more bytes
    "boko-book-id".hash(&mut hasher);
    let hash2 = hasher.finish();

    bytes.extend_from_slice(&hash1.to_le_bytes());
    bytes.extend_from_slice(&hash2.to_le_bytes());

    // URL-safe Base64 encode (no padding), 17 bytes → 23 chars
    base64_url_encode(&bytes[..17])
}

/// URL-safe Base64 encoding without padding.
fn base64_url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut result = String::new();
    let mut bits: u32 = 0;
    let mut bit_count = 0;

    for &byte in bytes {
        bits = (bits << 8) | byte as u32;
        bit_count += 8;

        while bit_count >= 6 {
            bit_count -= 6;
            let idx = ((bits >> bit_count) & 0x3F) as usize;
            result.push(ALPHABET[idx] as char);
        }
    }

    // Handle remaining bits (no padding)
    if bit_count > 0 {
        let idx = ((bits << (6 - bit_count)) & 0x3F) as usize;
        result.push(ALPHABET[idx] as char);
    }

    result
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
/// * `ctx` - Export context with transformed values (version, cover resource name)
///
/// # Returns
///
/// A vector of (key, value) pairs for the category.
pub fn build_category_entries(
    category: MetadataCategory,
    meta: &Metadata,
    ctx: &MetadataContext,
) -> Vec<(&'static str, String)> {
    let schema = metadata_schema();
    let mut entries = Vec::new();

    for rule in schema.iter().filter(|r| r.category == category) {
        let value = match &rule.source {
            MetadataSource::Static(s) => Some(s.to_string()),
            MetadataSource::Dynamic(field) => {
                // Special handling for fields that need transformation
                match field {
                    MetadataField::CoverImage => {
                        // Use the resource name from context, not the path from metadata
                        ctx.cover_resource_name.map(|s| s.to_string())
                    }
                    MetadataField::Date => {
                        // KFX expects YYYY-MM-DD format, not full ISO timestamp
                        field.extract(meta).map(truncate_to_date)
                    }
                    MetadataField::AssetId => {
                        // Asset ID from context (same as container ID)
                        ctx.asset_id.map(|s| s.to_string())
                    }
                    MetadataField::BookId => {
                        // Book ID from context (derived from identifier)
                        ctx.book_id.clone()
                    }
                    MetadataField::SeriesPosition => {
                        // Series position from collection
                        meta.collection.as_ref().and_then(|c| c.position).map(|p| {
                            // Format as integer if whole number, otherwise with decimal
                            if p.fract() == 0.0 {
                                format!("{}", p as i64)
                            } else {
                                format!("{}", p)
                            }
                        })
                    }
                    _ => field.extract(meta).map(|s| s.to_string()),
                }
            }
        };

        if let Some(v) = value {
            entries.push((rule.key, v));
        }
    }

    // Special case: add version to audit metadata
    if category == MetadataCategory::KindleAudit
        && let Some(v) = ctx.version
    {
        entries.push(("creator_version", v.to_string()));
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
        assert_eq!(
            MetadataField::FirstAuthor.extract(&meta),
            Some("Author One")
        );
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

        let ctx = MetadataContext::default();
        let entries = build_category_entries(MetadataCategory::KindleTitle, &meta, &ctx);

        // Should have title, language, author (but not description/publisher since they're None)
        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "title" && v == "Test Book")
        );
        assert!(entries.iter().any(|(k, v)| *k == "language" && v == "en"));
        assert!(entries.iter().any(|(k, v)| *k == "author" && v == "Author"));
        assert!(!entries.iter().any(|(k, _)| *k == "description"));
    }

    #[test]
    fn test_build_ebook_entries() {
        let meta = Metadata::default();
        let ctx = MetadataContext::default();
        let entries = build_category_entries(MetadataCategory::KindleEbook, &meta, &ctx);

        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "selection" && v == "enabled")
        );
        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "nested_span" && v == "enabled")
        );
    }

    #[test]
    fn test_build_audit_entries_with_version() {
        let meta = Metadata::default();
        let ctx = MetadataContext {
            version: Some("1.0.0"),
            ..Default::default()
        };
        let entries = build_category_entries(MetadataCategory::KindleAudit, &meta, &ctx);

        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "file_creator" && v == "boko")
        );
        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "creator_version" && v == "1.0.0")
        );
    }

    #[test]
    fn test_build_entries_with_cover_image() {
        let meta = Metadata {
            title: "Test".to_string(),
            language: "en".to_string(),
            cover_image: Some("images/cover.jpg".to_string()),
            ..Default::default()
        };

        // Without resource name in context, cover_image should not appear
        let ctx = MetadataContext::default();
        let entries = build_category_entries(MetadataCategory::KindleTitle, &meta, &ctx);
        assert!(!entries.iter().any(|(k, _)| *k == "cover_image"));

        // With resource name in context, cover_image should use the resource name
        let ctx = MetadataContext {
            cover_resource_name: Some("e6"),
            ..Default::default()
        };
        let entries = build_category_entries(MetadataCategory::KindleTitle, &meta, &ctx);
        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "cover_image" && v == "e6")
        );
    }

    #[test]
    fn test_build_entries_with_issue_date() {
        let meta = Metadata {
            title: "Test".to_string(),
            language: "en".to_string(),
            date: Some("2022-05-26".to_string()),
            ..Default::default()
        };

        let ctx = MetadataContext::default();
        let entries = build_category_entries(MetadataCategory::KindleTitle, &meta, &ctx);
        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "issue_date" && v == "2022-05-26")
        );
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

    #[test]
    fn test_generate_book_id_format() {
        let id = super::generate_book_id("urn:uuid:12345678-1234-1234-1234-123456789abc");

        // Should be 23 characters (URL-safe Base64, no padding)
        assert_eq!(id.len(), 23, "book_id should be 23 characters");

        // Should only contain URL-safe Base64 characters
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "book_id should only contain URL-safe Base64 characters"
        );
    }

    #[test]
    fn test_generate_book_id_deterministic() {
        let id1 = super::generate_book_id("urn:uuid:12345678-1234-1234-1234-123456789abc");
        let id2 = super::generate_book_id("urn:uuid:12345678-1234-1234-1234-123456789abc");

        // Same identifier should produce same book_id
        assert_eq!(id1, id2, "book_id should be deterministic");
    }

    #[test]
    fn test_generate_book_id_different_inputs() {
        let id1 = super::generate_book_id("urn:uuid:aaaaaaaa-1234-1234-1234-123456789abc");
        let id2 = super::generate_book_id("urn:uuid:bbbbbbbb-1234-1234-1234-123456789abc");

        // Different identifiers should produce different book_ids
        assert_ne!(
            id1, id2,
            "different identifiers should produce different book_ids"
        );
    }

    #[test]
    fn test_cde_content_type_is_ebok() {
        let meta = Metadata {
            title: "Test".to_string(),
            language: "en".to_string(),
            ..Default::default()
        };

        let ctx = MetadataContext::default();
        let entries = build_category_entries(MetadataCategory::KindleTitle, &meta, &ctx);

        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "cde_content_type" && v == "EBOK")
        );
    }

    #[test]
    fn test_build_entries_with_asset_id_and_book_id() {
        let meta = Metadata {
            title: "Test".to_string(),
            language: "en".to_string(),
            identifier: "urn:uuid:test-id".to_string(),
            ..Default::default()
        };

        let ctx = MetadataContext {
            asset_id: Some("CR!ABCDEFGHIJKLMNOPQRSTUVWXYZ12"),
            book_id: Some("BtestBookId12345678901".to_string()),
            ..Default::default()
        };
        let entries = build_category_entries(MetadataCategory::KindleTitle, &meta, &ctx);

        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "asset_id" && v == "CR!ABCDEFGHIJKLMNOPQRSTUVWXYZ12")
        );
        assert!(
            entries
                .iter()
                .any(|(k, v)| *k == "book_id" && v == "BtestBookId12345678901")
        );
    }
}
