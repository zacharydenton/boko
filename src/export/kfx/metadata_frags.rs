use super::*;

/// Build style fragments from the registry.
///
/// KFX requires every storyline element to have a style reference.
/// This generates all collected styles from the registry, including the default.
pub(super) fn build_style_fragments(ctx: &mut ExportContext) -> Vec<KfxFragment> {
    // Drain all styles from the registry to generate Ion fragments
    let style_pairs = ctx.style_registry.drain_to_ion();

    style_pairs
        .into_iter()
        .filter(|(name, _)| {
            // The default style is registered unconditionally; only ship it
            // when something actually referenced it.
            name != "s0" || ctx.default_style_used
        })
        .map(|(name, ion)| KfxFragment::new(KfxSymbol::Style, &name, ion))
        .collect()
}

/// Build the metadata fragment ($258) - contains reading_orders.
pub(super) fn build_metadata_fragment(ctx: &ExportContext) -> KfxFragment {
    // Build section list from context's registered sections
    let sections: Vec<IonValue> = ctx
        .section_ids
        .iter()
        .map(|&id| IonValue::Symbol(id))
        .collect();

    // reading_order_name should be a STRING (not a symbol) per KFX spec
    let reading_order = IonValue::Struct(vec![
        (
            KfxSymbol::ReadingOrderName as u64,
            IonValue::Symbol(KfxSymbol::Default as u64),
        ),
        (KfxSymbol::Sections as u64, IonValue::List(sections)),
    ]);

    let reading_orders = IonValue::List(vec![reading_order]);

    // $258 (metadata) contains reading_orders directly
    let metadata = IonValue::Struct(vec![(KfxSymbol::ReadingOrders as u64, reading_orders)]);

    KfxFragment::singleton(KfxSymbol::Metadata, metadata)
}

/// Build the book metadata fragment ($490) - contains categorised_metadata.
///
/// Uses the metadata schema to map IR metadata to KFX categories.
/// To add new metadata fields, update the schema in `kfx/metadata.rs`.
pub(super) fn build_book_metadata_fragment(
    book: &Book,
    container_id: &str,
    ctx: &ExportContext,
) -> KfxFragment {
    let meta = book.metadata();

    // Build metadata context with transformed values
    // Cover path in metadata may not match the registered resource path exactly.
    // Try common path variations (with/without epub/ prefix, etc.)
    let cover_resource_name = meta.cover_image.as_ref().and_then(|path| {
        // Try exact path first
        if let Some(name) = ctx.resource_registry.get_name(path) {
            return Some(name);
        }
        // Try with epub/ prefix
        let with_prefix = format!("epub/{}", path);
        if let Some(name) = ctx.resource_registry.get_name(&with_prefix) {
            return Some(name);
        }
        // Try stripping leading path components to match filename
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())?;
        // Search for a resource ending with this filename
        for (href, _) in ctx.resource_registry.iter() {
            if href.ends_with(filename) {
                return ctx.resource_registry.get_name(href);
            }
        }
        None
    });

    // Generate book_id from identifier (deterministic per publication)
    let book_id = if !meta.identifier.is_empty() {
        Some(generate_book_id(&meta.identifier))
    } else {
        None
    };
    let content_id = if !meta.identifier.is_empty() {
        Some(generate_content_id(&meta.identifier))
    } else {
        None
    };

    let meta_ctx = MetadataContext {
        version: Some(env!("CARGO_PKG_VERSION")),
        cover_resource_name,
        asset_id: Some(container_id),
        book_id,
        content_id,
    };

    // Build each category using the schema
    let categories = [
        MetadataCategory::KindleEbook,
        MetadataCategory::KindleTitle,
        MetadataCategory::KindleAudit,
    ];

    let categorised: Vec<IonValue> = categories
        .iter()
        .map(|&cat| {
            let entries = build_category_entries(cat, meta, &meta_ctx);
            let mut ion_entries: Vec<IonValue> = entries
                .into_iter()
                .map(|(k, v)| metadata_kv(k, &v))
                .collect();
            // Boolean flags the reference always carries; they don't fit the
            // string-valued schema, so they're emitted directly.
            if cat == MetadataCategory::KindleTitle {
                ion_entries.push(metadata_kv_bool("is_sample", false));
                ion_entries.push(metadata_kv_bool("override_kindle_font", false));
            }

            IonValue::Struct(vec![
                (
                    KfxSymbol::Category as u64,
                    IonValue::String(cat.as_str().to_string()),
                ),
                (KfxSymbol::Metadata as u64, IonValue::List(ion_entries)),
            ])
        })
        .collect();

    let book_metadata = IonValue::Struct(vec![(
        KfxSymbol::CategorisedMetadata as u64,
        IonValue::List(categorised),
    )]);

    KfxFragment::singleton(KfxSymbol::BookMetadata, book_metadata)
}

/// Helper to create a metadata key-value struct.
pub(super) fn metadata_kv(key: &str, value: &str) -> IonValue {
    IonValue::Struct(vec![
        (KfxSymbol::Key as u64, IonValue::String(key.to_string())),
        (KfxSymbol::Value as u64, IonValue::String(value.to_string())),
    ])
}

/// Helper to create a metadata key-value struct with a boolean value.
fn metadata_kv_bool(key: &str, value: bool) -> IonValue {
    IonValue::Struct(vec![
        (KfxSymbol::Key as u64, IonValue::String(key.to_string())),
        (KfxSymbol::Value as u64, IonValue::Bool(value)),
    ])
}

/// Build the content features fragment ($585).
///
/// This describes the content capabilities/features of the book.
pub(super) fn build_content_features_fragment(ctx: &ExportContext) -> KfxFragment {
    // Build feature entries matching reference KFX. Baseline features are
    // unconditional; media-derived features (yj_hdv, yj_jpg_rst_marker_present)
    // depend on facts gathered during the resource pass, so the fragment is
    // rebuilt after resources are appended (see build_kfx_container).
    let mut features = vec![
        feature_entry("com.amazon.yjconversion", "reflow-style", 6),
        feature_entry("SDK.Marker", "CanonicalFormat", 1),
    ];

    // HDV ("high-definition variant") declares images above the classic
    // 1920px bound; the reference only emits it when such an image exists.
    if ctx.has_hdv_image {
        features.push(feature_entry("com.amazon.yjconversion", "yj_hdv", 1));
    }

    // Declared when any JPEG payload contains restart markers (FF D0-D7);
    // renderers use it to enable segmented decoding.
    if ctx.jpg_rst_marker_present {
        features.push(feature_entry(
            "com.amazon.yjconversion",
            "yj_jpg_rst_marker_present",
            1,
        ));
    }

    // Tables enable the device's table renderer and the double-tap table
    // viewer; Kindle Previewer declares both for books with tables.
    if ctx.has_tables {
        features.push(feature_entry("com.amazon.yjconversion", "yj_table", 7));
        features.push(feature_entry(
            "com.amazon.yjconversion",
            "yj_table_viewer",
            1,
        ));
    }

    // Sections above 65536 positions overflow the renderer's default position
    // handling (crashes when paging deep into a long chapter, and on
    // "go to page"). Declaring reflow-section-size enables the device's
    // large-section support; the value follows Amazon's formula.
    let max_section_pids = max_section_position_count(ctx);
    if max_section_pids > 65536 {
        let size = (((max_section_pids - 65536) / 16384) + 2).min(256);
        features.push(feature_entry(
            "com.amazon.yjconversion",
            "reflow-section-size",
            size,
        ));
    }

    let content_features =
        IonValue::Struct(vec![(KfxSymbol::Features as u64, IonValue::List(features))]);

    KfxFragment::singleton(KfxSymbol::ContentFeatures, content_features)
}

/// Build one $585 feature entry: `{namespace, key, version_info: {version}}`.
fn feature_entry(namespace: &str, key: &str, major: i64) -> IonValue {
    IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String(namespace.to_string()),
        ),
        (KfxSymbol::Key as u64, IonValue::String(key.to_string())),
        (
            KfxSymbol::VersionInfo as u64,
            IonValue::Struct(vec![(
                KfxSymbol::Version as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::MajorVersion as u64, IonValue::Int(major)),
                    (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
                ]),
            )]),
        ),
    ])
}

/// Build the document data fragment ($538).
///
/// Contains document-level settings like direction, font size, line height, max_id.
pub(super) fn build_document_data_fragment(ctx: &ExportContext) -> KfxFragment {
    // Build section list from context's registered sections
    let sections: Vec<IonValue> = ctx
        .section_ids
        .iter()
        .map(|&id| IonValue::Symbol(id))
        .collect();

    let reading_order = IonValue::Struct(vec![
        (
            KfxSymbol::ReadingOrderName as u64,
            IonValue::Symbol(KfxSymbol::Default as u64),
        ),
        (KfxSymbol::Sections as u64, IonValue::List(sections)),
    ]);

    // Calculate max_id from context (highest EID used)
    let max_id = ctx.max_eid();

    let document_data = IonValue::Struct(vec![
        (
            KfxSymbol::Direction as u64,
            IonValue::Symbol(KfxSymbol::Ltr as u64),
        ),
        (
            KfxSymbol::ColumnCount as u64,
            IonValue::Symbol(KfxSymbol::Auto as u64),
        ),
        (
            KfxSymbol::FontSize as u64,
            IonValue::Struct(vec![
                (KfxSymbol::Value as u64, IonValue::Decimal("1".to_string())),
                (
                    KfxSymbol::Unit as u64,
                    IonValue::Symbol(KfxSymbol::Em as u64),
                ),
            ]),
        ),
        (
            KfxSymbol::WritingMode as u64,
            IonValue::Symbol(KfxSymbol::HorizontalTb as u64),
        ),
        (
            KfxSymbol::Selection as u64,
            IonValue::Symbol(KfxSymbol::Enabled as u64),
        ),
        (KfxSymbol::MaxId as u64, IonValue::Int(max_id as i64)),
        (
            KfxSymbol::LineHeight as u64,
            IonValue::Struct(vec![
                (
                    KfxSymbol::Value as u64,
                    IonValue::Decimal("1.2".to_string()),
                ),
                (
                    KfxSymbol::Unit as u64,
                    IonValue::Symbol(KfxSymbol::Em as u64),
                ),
            ]),
        ),
        (
            KfxSymbol::SpacingPercentBase as u64,
            IonValue::Symbol(KfxSymbol::Width as u64),
        ),
        (
            KfxSymbol::ReadingOrders as u64,
            IonValue::List(vec![reading_order]),
        ),
    ]);

    KfxFragment::singleton(KfxSymbol::DocumentData, document_data)
}
