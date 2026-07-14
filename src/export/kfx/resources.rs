use super::*;

/// Build an external_resource fragment ($164) - metadata about a resource.
pub(super) fn build_external_resource_fragment(
    href: &str,
    data: &[u8],
    ctx: &mut ExportContext,
) -> KfxFragment {
    // Generate a short resource name (e.g., "e0", "e1", etc.)
    let resource_name = generate_resource_name(href, ctx);
    let resource_name_symbol = ctx.symbols.get_or_intern(&resource_name);

    let mut fields = Vec::new();

    // resource_name - the symbolic name for this resource
    fields.push((
        KfxSymbol::ResourceName as u64,
        IonValue::Symbol(resource_name_symbol),
    ));

    // location - path to the bcRawMedia entity
    let location = format!("resource/{}", resource_name);
    fields.push((KfxSymbol::Location as u64, IonValue::String(location)));

    // format - file type symbol
    let format_symbol = detect_format_symbol(href, data);
    fields.push((KfxSymbol::Format as u64, IonValue::Symbol(format_symbol)));

    // For images, try to extract dimensions
    if let Some((width, height)) = crate::util::extract_image_dimensions(data) {
        fields.push((KfxSymbol::ResourceWidth as u64, IonValue::Int(width as i64)));
        fields.push((
            KfxSymbol::ResourceHeight as u64,
            IonValue::Int(height as i64),
        ));
    }

    // mime type for images
    if let Some(mime) = crate::util::detect_mime_type(href, data) {
        fields.push((KfxSymbol::Mime as u64, IonValue::String(mime.to_string())));
    }

    let ion = IonValue::Struct(fields);
    KfxFragment::new(KfxSymbol::ExternalResource, &resource_name, ion)
}

/// Build a resource fragment (bcRawMedia $417) - the actual bytes.
pub(super) fn build_resource_fragment(
    href: &str,
    data: &[u8],
    ctx: &mut ExportContext,
) -> KfxFragment {
    // Use resource/ prefix to distinguish from external_resource fragment
    // This ensures bcRawMedia gets a different entity ID
    let resource_name = generate_resource_name(href, ctx);
    let raw_name = format!("resource/{}", resource_name);

    // Register the prefixed name as a symbol
    ctx.symbols.get_or_intern(&raw_name);

    // Create raw fragment for binary resources
    KfxFragment::raw(KfxSymbol::Bcrawmedia as u64, &raw_name, data.to_vec())
}

/// Build font entity fragments ($262) from @font-face rules.
///
/// Font entities link font_family names (e.g., "cover-Ubuntu") to resource locations.
/// This enables Kindle to properly render custom fonts.
pub(super) fn build_font_fragments(book: &mut Book, ctx: &mut ExportContext) -> Vec<KfxFragment> {
    use crate::style::{FontStyle, FontWeight};

    let mut fragments = Vec::new();
    let font_faces = book.font_faces();

    for font_face in font_faces {
        // Check if the font file exists as a resource
        let resource_name = match ctx.resource_registry.get_name(&font_face.src) {
            Some(name) => name.to_string(),
            None => {
                // Try without leading path components
                let filename = std::path::Path::new(&font_face.src)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&font_face.src);

                // Search for matching resource
                let mut found = None;
                for (href, _) in ctx.resource_registry.iter() {
                    if href.ends_with(filename) {
                        found = ctx.resource_registry.get_name(href).map(|s| s.to_string());
                        break;
                    }
                }
                match found {
                    Some(name) => name,
                    None => continue, // Skip if font file not found
                }
            }
        };

        // Build location path
        let location = format!("resource/{}", resource_name);

        // Use original font family name (no "cover-" prefix)
        // This matches how styles reference fonts and is source-faithful
        let font_family = font_face.font_family.clone();

        // Convert font_weight to KFX symbol
        let weight_symbol = match font_face.font_weight {
            FontWeight(w) if w >= 700 => KfxSymbol::Bold,
            _ => KfxSymbol::Normal,
        };

        // Convert font_style to KFX symbol
        let style_symbol = match font_face.font_style {
            FontStyle::Italic | FontStyle::Oblique => KfxSymbol::Italic,
            FontStyle::Normal => KfxSymbol::Normal,
        };

        // Build font entity ION structure
        let ion = IonValue::Struct(vec![
            (
                KfxSymbol::FontFamily as u64,
                IonValue::String(font_family.clone()),
            ),
            (
                KfxSymbol::FontStyle as u64,
                IonValue::Symbol(style_symbol as u64),
            ),
            (KfxSymbol::Location as u64, IonValue::String(location)),
            (
                KfxSymbol::FontWeight as u64,
                IonValue::Symbol(weight_symbol as u64),
            ),
            (
                KfxSymbol::FontStretch as u64,
                IonValue::Symbol(KfxSymbol::Normal as u64),
            ),
        ]);

        // Generate unique fragment name for this font face
        let frag_name = format!(
            "font-{}-{}-{}",
            font_face.font_family,
            if font_face.font_weight.0 >= 700 {
                "bold"
            } else {
                "normal"
            },
            match font_face.font_style {
                FontStyle::Italic | FontStyle::Oblique => "italic",
                FontStyle::Normal => "normal",
            }
        );

        fragments.push(KfxFragment::new(KfxSymbol::Font, &frag_name, ion));
    }

    fragments
}

/// Build anchor fragments ($266) for all recorded anchors.
///
/// Returns (fragments, anchor_ids_by_fragment) where anchor_ids_by_fragment
/// maps fragment_id → list of anchor symbol IDs for use in position_map.
pub(super) fn build_anchor_fragments(
    ctx: &mut ExportContext,
) -> (Vec<KfxFragment>, HashMap<u64, Vec<u64>>) {
    let mut fragments = Vec::new();
    let mut anchor_ids_by_fragment: HashMap<u64, Vec<u64>> = HashMap::new();

    // Get resolved internal anchors from the AnchorRegistry
    let resolved_anchors = ctx.anchor_registry.drain_anchors();

    for anchor in resolved_anchors {
        // Intern the anchor symbol to get its ID
        let anchor_symbol_id = ctx.symbols.get_or_intern(&anchor.symbol);

        // Track which anchors belong to which SECTION for position_map
        // Key by section_id (page_template ID), not fragment_id (content ID)
        anchor_ids_by_fragment
            .entry(anchor.section_id)
            .or_default()
            .push(anchor_symbol_id);

        // Build position struct - uses content fragment_id for navigation target
        let mut pos_fields = Vec::new();
        pos_fields.push((
            KfxSymbol::Id as u64,
            IonValue::Int(anchor.fragment_id as i64),
        ));
        // Only include offset when non-zero - reference KFX omits offset for fragment-only positions
        if anchor.offset > 0 {
            pos_fields.push((
                KfxSymbol::Offset as u64,
                IonValue::Int(anchor.offset as i64),
            ));
        }

        let ion = IonValue::Struct(vec![
            (
                KfxSymbol::AnchorName as u64,
                IonValue::Symbol(anchor_symbol_id),
            ),
            (KfxSymbol::Position as u64, IonValue::Struct(pos_fields)),
        ]);

        fragments.push(KfxFragment::new(KfxSymbol::Anchor, &anchor.symbol, ion));
    }

    // Get external anchors (http/https links) from the AnchorRegistry
    let external_anchors = ctx.anchor_registry.drain_external_anchors();

    for anchor in external_anchors {
        // Intern the anchor symbol to get its ID
        let anchor_symbol_id = ctx.symbols.get_or_intern(&anchor.symbol);

        // External anchors use uri instead of position
        let ion = IonValue::Struct(vec![
            (KfxSymbol::Uri as u64, IonValue::String(anchor.uri.clone())),
            (
                KfxSymbol::AnchorName as u64,
                IonValue::Symbol(anchor_symbol_id),
            ),
        ]);

        fragments.push(KfxFragment::new(KfxSymbol::Anchor, &anchor.symbol, ion));
    }

    (fragments, anchor_ids_by_fragment)
}

/// Generate a short resource name for a given href.
pub(super) fn generate_resource_name(href: &str, ctx: &mut ExportContext) -> String {
    ctx.resource_registry.get_or_create_name(href)
}

// ============================================================================
// Navigation Maps ($264, $265, $550)
// ============================================================================

/// Build resource_path fragment ($395).
///
/// This entity lists additional resource paths. For simple conversions,
/// the entries array is empty.
pub(super) fn build_resource_path_fragment() -> KfxFragment {
    let ion = IonValue::Struct(vec![(KfxSymbol::Entries as u64, IonValue::List(vec![]))]);
    KfxFragment::singleton(KfxSymbol::ResourcePath, ion)
}

/// Detect format symbol from file extension/magic bytes.
///
/// Delegates to the pure `detect_media_format()` utility and maps to KFX symbol.
pub(super) fn detect_format_symbol(href: &str, data: &[u8]) -> u64 {
    let format = detect_media_format(href, data);
    format_to_kfx_symbol(format)
}

/// Check if a path is a media asset (image, font, etc.)
pub(super) fn is_media_asset(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "ttf" | "otf" | "woff" | "woff2"
    )
}

#[cfg(test)]
mod resource_export_tests {
    use super::*;
    use crate::model::Book;

    #[test]
    fn test_kfx_export_includes_images() {
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let data = build_kfx_container(&mut book).unwrap();

        // KFX should be > 400KB (images alone are ~401KB)
        assert!(
            data.len() > 400000,
            "KFX should include image data, got {} bytes",
            data.len()
        );
    }

    #[test]
    fn test_kfx_asset_roundtrip() {
        // Export EPUB to KFX
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let kfx_data = build_kfx_container(&mut book).unwrap();

        // Write to temp file and re-open
        let temp_path = std::env::temp_dir().join("test_roundtrip.kfx");
        std::fs::write(&temp_path, &kfx_data).unwrap();

        let mut reimported = Book::open(&temp_path).unwrap();
        let assets: Vec<_> = reimported.list_assets().to_vec();

        // Load all assets and verify total size
        let total_size: usize = assets
            .iter()
            .filter_map(|a| reimported.load_asset(a).ok())
            .map(|d| d.len())
            .sum();

        std::fs::remove_file(&temp_path).ok();

        // Should have ~401KB of image data
        assert!(
            total_size > 100000,
            "Expected > 100KB of assets from KFX, got {} bytes",
            total_size
        );
    }
}

#[cfg(test)]
mod anchor_resolution_tests {
    use super::*;
    use crate::model::Book;

    #[test]
    fn test_cross_file_anchor_resolution_flow() {
        // Test the full anchor resolution flow with epictetus.epub
        // This EPUB has endnotes in endnotes.xhtml with links from the main text
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();

        // Step 1: Resolve all links using centralized resolver
        let resolved = book.resolve_links().unwrap();

        // Should have resolved links (enchiridion has links to endnotes)
        assert!(!resolved.is_empty(), "Should have resolved some links");

        // Check for some broken links (external links won't resolve)
        // but internal endnote links should resolve
        let broken_count = resolved.broken_links().len();
        eprintln!("Resolved {} links, {} broken", resolved.len(), broken_count);
    }

    #[test]
    fn test_anchor_symbol_reuse() {
        // Test that anchor symbols are consistent between link_to and anchor creation
        // This tests the core invariant of the anchor registry
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();

        let mut ctx = ExportContext::new();

        // Collect spine info
        let spine_info: Vec<_> = book
            .spine()
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let section_name = format!("c{}", idx);
                (entry.id, section_name)
            })
            .collect();

        // Step 1: Resolve links
        let resolved = book.resolve_links().unwrap();

        // Step 2: Register link targets from ResolvedLinks
        register_link_targets(&mut book, &spine_info, &resolved, &mut ctx).unwrap();

        // Step 3: Verify that href lookups return the same symbol as GlobalNodeId lookups
        // Find an internal link that has both
        for (source, target) in resolved.iter() {
            if let AnchorTarget::Internal(gid) = target {
                // Get the href for this link
                if let Ok(chapter) = book.load_chapter(source.chapter)
                    && let Some(href) = chapter.semantics.href(source.node)
                {
                    // Both lookups should return the same symbol
                    let href_symbol = ctx.anchor_registry.get_href_symbol(href);
                    let node_symbol = ctx.anchor_registry.get_symbol(*gid);

                    assert_eq!(
                        href_symbol, node_symbol,
                        "href '{}' and GlobalNodeId {:?} should have same symbol",
                        href, gid
                    );

                    // Only need to verify one link
                    return;
                }
            }
        }

        // If we get here, no internal links were found (shouldn't happen with epictetus.epub)
        panic!("Should have found at least one internal link to verify");
    }

    #[test]
    fn test_anchor_entities_created_in_full_export() {
        // Test that anchor entities are actually created during full export
        let mut book = Book::open("tests/fixtures/epictetus.epub").unwrap();
        let kfx_data = build_kfx_container(&mut book).unwrap();

        // Parse the KFX container to find anchor entities
        use crate::kfx::container::{
            parse_container_header, parse_container_info, parse_index_table,
        };

        // 1. Parse header to get container_info location
        let header = parse_container_header(&kfx_data).expect("Failed to parse header");

        // 2. Parse container_info to get index table location
        let ci_start = header.container_info_offset;
        let ci_end = ci_start + header.container_info_length;
        let container_info = parse_container_info(&kfx_data[ci_start..ci_end])
            .expect("Failed to parse container info");

        // 3. Parse the index table
        let (idx_offset, idx_len) = container_info.index.expect("No index table");
        let index = parse_index_table(
            &kfx_data[idx_offset..idx_offset + idx_len],
            header.header_len,
        );

        // Find anchor entities (type 266 = $266 = Anchor)
        let anchor_count = index.iter().filter(|e| e.type_id == 266).count();

        // Should have anchors for internal links (endnotes, uncopyright, etc.)
        // The EPUB has 42 endnotes from Enchiridion + some from other sections
        // Plus backlinks and other internal links
        assert!(
            anchor_count >= 40,
            "Expected at least 40 anchor entities for endnotes, got {}",
            anchor_count
        );
    }
}
