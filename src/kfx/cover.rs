//! Cover section handling for KFX export.
//!
//! This module provides pure functions for detecting and building
//! standalone cover sections when an EPUB's cover image differs from
//! the first spine chapter's image.

use std::path::{Path, PathBuf};

use crate::ir::{IRChapter, Role};
use crate::kfx::context::ExportContext;
use crate::kfx::fragment::KfxFragment;
use crate::kfx::ion::IonValue;
use crate::kfx::symbols::KfxSymbol;

/// Section name for the standalone cover (always index 0).
pub const COVER_SECTION_NAME: &str = "c0";

/// Check if a chapter contains only an image and no text content.
///
/// Returns true if the chapter has:
/// - Exactly one Image node
/// - No text content (or whitespace-only text)
///
/// This is used to detect cover pages that need special KFX formatting.
pub fn is_image_only_chapter(chapter: &IRChapter) -> bool {
    let mut image_count = 0;
    let mut has_text = false;

    for node_id in chapter.iter_dfs() {
        let node = match chapter.node(node_id) {
            Some(n) => n,
            None => continue,
        };

        match node.role {
            Role::Image => {
                image_count += 1;
            }
            Role::Text => {
                // Check if there's actual text content (not just whitespace)
                if !node.text.is_empty() {
                    let text = chapter.text(node.text);
                    if !text.trim().is_empty() {
                        has_text = true;
                    }
                }
            }
            _ => {}
        }
    }

    image_count == 1 && !has_text
}

/// Get the image path from a chapter if it contains exactly one image.
///
/// Returns the src attribute of the single image node, or None if
/// the chapter doesn't contain exactly one image.
pub fn get_chapter_image_path(chapter: &IRChapter) -> Option<String> {
    let mut image_path = None;
    let mut image_count = 0;

    for node_id in chapter.iter_dfs() {
        if let Some(node) = chapter.node(node_id) {
            if node.role == Role::Image {
                image_count += 1;
                if let Some(src) = chapter.semantics.src(node_id) {
                    image_path = Some(src.to_string());
                }
            }
        }
    }

    if image_count == 1 { image_path } else { None }
}

/// Check if a standalone cover section is needed.
///
/// Returns true if the EPUB has a cover image in metadata that differs
/// from the image displayed in the first chapter. This happens when:
/// - The EPUB has a cover image defined in metadata
/// - The first chapter displays a different image (e.g., titlepage.png vs cover.jpg)
///
/// # Arguments
/// * `cover_image_path` - The cover image path from EPUB metadata
/// * `first_chapter` - The first chapter in the spine
pub fn needs_standalone_cover(cover_image_path: &str, first_chapter: &IRChapter) -> bool {
    // Get the image from the first chapter (if it's image-only)
    let Some(first_image_path) = get_chapter_image_path(first_chapter) else {
        // First chapter doesn't have a single image, so we need standalone cover
        return true;
    };

    // Compare filenames (ignoring directory prefixes)
    let cover_filename = Path::new(cover_image_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cover_image_path);

    let first_filename = Path::new(&first_image_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&first_image_path);

    // Need standalone cover if filenames differ
    cover_filename != first_filename
}

/// Build a dedicated cover section and storyline.
///
/// Creates a c0 section with container type and fixed dimensions,
/// plus a storyline containing just the cover image.
///
/// # Arguments
/// * `cover_path` - Path to the cover image resource
/// * `section_id` - Fragment ID for the section
/// * `ctx` - Export context for symbol interning and resource lookup
///
/// # Returns
/// A tuple of (section_fragment, storyline_fragment)
pub fn build_cover_section(
    cover_path: &str,
    section_id: u64,
    ctx: &mut ExportContext,
) -> (KfxFragment, KfxFragment) {
    let section_name = COVER_SECTION_NAME;
    let story_name = format!("story_{}", section_name);

    // Intern story name
    let story_name_symbol = ctx.symbols.get_or_intern(&story_name);

    // Get the resource name for the cover image
    let resource_name = ctx.resource_registry.get_or_create_name(cover_path);
    let resource_symbol = ctx.symbols.get_or_intern(&resource_name);

    // Use default style for the cover image
    let style_symbol = ctx.default_style_symbol;

    // Assign a fragment ID for the cover image content
    let cover_content_id = ctx.next_fragment_id();

    // Build storyline content: [{ id, type: image, resource_name, style }]
    let content_list = IonValue::List(vec![IonValue::Struct(vec![
        (KfxSymbol::Id as u64, IonValue::Int(cover_content_id as i64)),
        (
            KfxSymbol::Type as u64,
            IonValue::Symbol(KfxSymbol::Image as u64),
        ),
        (
            KfxSymbol::ResourceName as u64,
            IonValue::Symbol(resource_symbol),
        ),
        (KfxSymbol::Style as u64, IonValue::Symbol(style_symbol)),
    ])]);

    let storyline_ion = IonValue::Struct(vec![
        (
            KfxSymbol::StoryName as u64,
            IonValue::Symbol(story_name_symbol),
        ),
        (KfxSymbol::ContentList as u64, content_list),
    ]);

    let storyline_fragment = KfxFragment::new(KfxSymbol::Storyline, &story_name, storyline_ion);

    // Build section (page_template): container type with fixed dimensions
    let page_template = IonValue::Struct(vec![
        (KfxSymbol::Id as u64, IonValue::Int(section_id as i64)),
        (
            KfxSymbol::StoryName as u64,
            IonValue::Symbol(story_name_symbol),
        ),
        (
            KfxSymbol::Type as u64,
            IonValue::Symbol(KfxSymbol::Container as u64),
        ),
        (KfxSymbol::FixedWidth as u64, IonValue::Int(1400)),
        (KfxSymbol::FixedHeight as u64, IonValue::Int(2100)),
        (
            KfxSymbol::Layout as u64,
            IonValue::Symbol(KfxSymbol::ScaleFit as u64),
        ),
        (
            KfxSymbol::Float as u64,
            IonValue::Symbol(KfxSymbol::Center as u64),
        ),
    ]);

    let section_ion = IonValue::Struct(vec![
        (
            KfxSymbol::SectionName as u64,
            IonValue::Symbol(ctx.symbols.get_or_intern(section_name)),
        ),
        (
            KfxSymbol::PageTemplates as u64,
            IonValue::List(vec![page_template]),
        ),
    ]);

    let section_fragment = KfxFragment::new(KfxSymbol::Section, section_name, section_ion);

    (section_fragment, storyline_fragment)
}

/// Normalize cover path to match asset paths.
///
/// EPUB metadata may use a shorter path (e.g., "images/cover.jpg") while
/// the asset list uses a full path (e.g., "epub/images/cover.jpg").
/// This matches by filename to find the correct asset path.
pub fn normalize_cover_path(cover_path: &str, asset_paths: &[PathBuf]) -> String {
    let cover_filename = Path::new(cover_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cover_path);

    for asset in asset_paths {
        if let Some(asset_filename) = asset.file_name().and_then(|s| s.to_str()) {
            if asset_filename == cover_filename {
                return asset.to_string_lossy().to_string();
            }
        }
    }

    // Fall back to original path if no match found
    cover_path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Book;

    #[test]
    fn test_is_image_only_chapter_with_css_hidden_text() {
        // epictetus.epub titlepage has text hidden via CSS (display:none)
        // so the IR only contains the image
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let spine = book.spine();

        if let Some(first) = spine.first() {
            let chapter = book.load_chapter(first.id).unwrap();
            assert!(
                is_image_only_chapter(&chapter),
                "titlepage should appear image-only (CSS hides text)"
            );
        }
    }

    #[test]
    fn test_needs_standalone_cover() {
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let cover_path = book
            .metadata()
            .cover_image
            .clone()
            .expect("should have cover");

        let spine = book.spine();
        let first = spine.first().expect("should have spine");
        let chapter = book.load_chapter(first.id).unwrap();

        // epictetus.epub has cover.jpg but titlepage shows titlepage.png
        assert!(
            needs_standalone_cover(&cover_path, &chapter),
            "should need standalone cover when images differ"
        );
    }

    #[test]
    fn test_get_chapter_image_path() {
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let spine = book.spine();

        if let Some(first) = spine.first() {
            let chapter = book.load_chapter(first.id).unwrap();
            let path = get_chapter_image_path(&chapter);
            assert!(path.is_some(), "should find image path");
            assert!(
                path.unwrap().contains("titlepage"),
                "should be titlepage image"
            );
        }
    }
}
