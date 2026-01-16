//! Cached regex patterns for MOBI processing.
//!
//! Uses LazyLock to compile patterns once on first use, avoiding
//! repeated regex compilation which was a major performance bottleneck.

use regex_lite::Regex;
use std::sync::LazyLock;

// === Writer patterns ===

/// Matches <link ... href="..." ...> tags
pub static LINK_HREF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<link\s+[^>]*href\s*=\s*["']([^"']+)["'][^>]*>"#).unwrap()
});

/// Matches <img ... src="..." ...> tags
pub static IMG_SRC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<img\s+[^>]*src\s*=\s*["']([^"']+)["']"#).unwrap()
});

/// Matches <a ... href="..." ...> tags
pub static ANCHOR_HREF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a\s+([^>]*)href\s*=\s*["']([^"']+)["']([^>]*)>"#).unwrap()
});

/// Matches url(...) in CSS
pub static CSS_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"url\s*\(\s*["']?([^"')]+)["']?\s*\)"#).unwrap()
});

// === Reader patterns ===

/// Matches aid="..." attributes for removal
pub static AID_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s+aid\s*=\s*["'][^"']*["']"#).unwrap()
});

/// Matches data-AmznRemoved attributes
pub static AMZN_REMOVED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s+data-AmznRemoved[^=]*\s*=\s*["'][^"']*["']"#).unwrap()
});

/// Matches data-AmznPageBreak attributes
pub static AMZN_PAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s+data-AmznPageBreak\s*=\s*["'][^"']*["']"#).unwrap()
});

/// Matches <img ...> tags for cleanup
pub static IMG_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<img\s+([^>]*?)(\s*/?>)"#).unwrap()
});

/// Matches <meta charset="..."> tags
pub static META_CHARSET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<meta\s*charset\s*=\s*["']([^"']+)["']\s*/?\s*>"#).unwrap()
});

/// Matches @font-face rules with placeholder URLs
pub static FONTFACE_PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@font-face\s*\{[^}]*url\s*\(\s*X{10,}\s*\)[^}]*\}"#).unwrap()
});

/// Matches kindle:embed:XXXX references
pub static KINDLE_EMBED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"kindle:embed:([0-9A-V]+)(\?[^"')]*)?["']?"#).unwrap()
});

/// Matches id="..." attributes in HTML
pub static ID_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<[^>]+\s(?:id|ID)\s*=\s*['"]([^'"]+)['"]"#).unwrap()
});

// === Skeleton patterns ===

/// Matches aid="..." attributes for offset mapping
pub static AID_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\said=['"]([\dA-V]+)['"]"#).unwrap()
});

/// Matches id="..." attributes in tags
pub static TAG_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bid=['"]([\w\-:\.]+)['"]"#).unwrap()
});

/// Creates a regex for matching aidable HTML tags.
/// This one needs to be dynamic based on tag list, so we cache the compiled version.
pub static AIDABLE_TAGS_RE: LazyLock<Regex> = LazyLock::new(|| {
    use super::skeleton::AID_ABLE_TAGS;
    let tag_pattern = format!(r"<({})(\s[^>]*)?>", AID_ABLE_TAGS.join("|"));
    Regex::new(&tag_pattern).unwrap()
});
