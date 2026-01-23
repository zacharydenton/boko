//! Resource handling for KFX (images and fonts).

use std::collections::HashMap;

use crate::book::{Book, Resource};
use crate::kfx::ion::IonValue;

use super::fragment::KfxFragment;
use super::symbols::{SymbolTable, sym};

/// Check if a media type is an image
pub fn is_image_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/jpeg" | "image/jpg" | "image/png" | "image/gif" | "image/webp"
    )
}

/// Check if a media type is a font
pub fn is_font_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "font/ttf"
            | "font/otf"
            | "font/woff"
            | "font/woff2"
            | "application/font-sfnt"
            | "application/x-font-ttf"
            | "application/x-font-otf"
            | "application/font-woff"
            | "application/font-woff2"
            | "application/vnd.ms-opentype"
    )
}

/// Get image dimensions from raw bytes (basic parsing for JPEG/PNG)
pub fn get_image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 {
        return None;
    }

    // PNG: starts with 89 50 4E 47 0D 0A 1A 0A
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) && data.len() >= 24 {
        let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((width, height));
    }

    // JPEG: starts with FF D8 FF
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let mut pos = 2;
        while pos + 4 < data.len() {
            if data[pos] != 0xFF {
                pos += 1;
                continue;
            }
            let marker = data[pos + 1];
            if marker == 0xD9 {
                break;
            }
            if (marker == 0xC0 || marker == 0xC2) && pos + 9 < data.len() {
                let height = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;
                let width = u16::from_be_bytes([data[pos + 7], data[pos + 8]]) as u32;
                return Some((width, height));
            }
            if pos + 3 < data.len() {
                let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
                pos += 2 + len;
            } else {
                break;
            }
        }
    }

    None
}

/// Check if image data is PNG format
pub fn is_png_data(data: &[u8]) -> bool {
    data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
}

/// Check if image data is GIF format
pub fn is_gif_data(data: &[u8]) -> bool {
    data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")
}

/// Build resource symbol mapping for image references
pub fn build_resource_symbols(book: &Book, symtab: &mut SymbolTable) -> HashMap<String, u64> {
    let mut resource_symbols = HashMap::new();
    let mut resource_index = 0;

    for (href, resource) in &book.resources {
        if !is_image_media_type(&resource.media_type) {
            continue;
        }

        let resource_id = format!("rsrc{resource_index}");
        let resource_sym = symtab.get_or_intern(&resource_id);
        resource_symbols.insert(href.clone(), resource_sym);
        resource_index += 1;
    }

    resource_symbols
}

/// Add media resources (images and fonts) from the book.
///
/// Only includes resources that are:
/// - Referenced in content (in `referenced_hrefs`)
/// - The cover image
/// - Fonts (always included)
///
/// Returns (fragments, resource_to_media mappings)
pub fn create_resource_fragments(
    book: &Book,
    symtab: &mut SymbolTable,
    resource_symbols: &HashMap<String, u64>,
    referenced_hrefs: &std::collections::HashSet<String>,
) -> (Vec<KfxFragment>, Vec<(u64, u64)>) {
    let mut fragments = Vec::new();
    let mut resource_to_media = Vec::new();
    let mut resource_index = 0;
    let cover_href = book.metadata.cover_image.as_deref();

    for (href, resource) in book.resources.iter() {
        let is_image = is_image_media_type(&resource.media_type);
        let is_font = is_font_media_type(&resource.media_type);

        if !is_image && !is_font {
            continue;
        }

        let is_cover = cover_href == Some(href.as_str());

        // Skip images that are not referenced in content (e.g., mobi fallback images)
        // Always include: cover, fonts, and images referenced in content
        if is_image && !is_cover && !referenced_hrefs.contains(href) {
            continue;
        }
        let resource_id = format!("rsrc{resource_index}");
        let resource_sym = resource_symbols
            .get(href)
            .copied()
            .unwrap_or_else(|| symtab.get_or_intern(&resource_id));

        let (image_data, media_type) = (resource.data.clone(), resource.media_type.clone());

        // Create P164 resource fragment
        let mut res_meta = HashMap::new();
        res_meta.insert(sym::RESOURCE_NAME, IonValue::Symbol(resource_sym));
        if !is_cover {
            res_meta.insert(sym::MIME_TYPE, IonValue::String(media_type));
        }
        res_meta.insert(
            sym::LOCATION,
            IonValue::String(format!("resource/{resource_id}")),
        );

        if is_image {
            let format_sym = if is_png_data(&image_data) {
                sym::PNG_FORMAT
            } else if is_gif_data(&image_data) {
                sym::GIF_FORMAT
            } else {
                sym::JPG_FORMAT
            };
            res_meta.insert(sym::FORMAT, IonValue::Symbol(format_sym));
            let (width, height) = get_image_dimensions(&image_data).unwrap_or((800, 600));
            res_meta.insert(sym::WIDTH, IonValue::Int(width as i64));
            res_meta.insert(sym::HEIGHT, IonValue::Int(height as i64));
        } else if is_font {
            res_meta.insert(sym::FORMAT, IonValue::Symbol(sym::FONT_FORMAT));
        }

        fragments.push(KfxFragment::new(
            sym::RESOURCE,
            &resource_id,
            IonValue::Struct(res_meta),
        ));

        // Create P417 raw media fragment
        let media_id = format!("resource/{resource_id}");
        let media_sym = symtab.get_or_intern(&media_id);
        fragments.push(KfxFragment::new(
            sym::RAW_MEDIA,
            &media_id,
            IonValue::Blob(image_data),
        ));

        resource_to_media.push((resource_sym, media_sym));
        resource_index += 1;
    }

    (fragments, resource_to_media)
}

/// Populate image dimensions in content items
pub fn populate_image_dimensions(
    item: &mut crate::kfx::writer::content::ContentItem,
    resources: &std::collections::HashMap<String, Resource>,
) {
    use crate::kfx::writer::content::ContentItem;

    match item {
        ContentItem::Image {
            resource_href,
            style,
            ..
        } => {
            if let Some(resource) = resources.get(resource_href)
                && let Some((width, height)) = get_image_dimensions(&resource.data)
            {
                style.image_width_px = Some(width);
                style.image_height_px = Some(height);
            }
        }
        ContentItem::Container { children, .. } => {
            for child in children {
                populate_image_dimensions(child, resources);
            }
        }
        ContentItem::Text { .. } => {}
        ContentItem::Svg { .. } => {
            // SVG dimensions are extracted during content extraction, not from resources
        }
    }
}
