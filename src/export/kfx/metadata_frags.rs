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
            let ion_entries: Vec<IonValue> = entries
                .into_iter()
                .map(|(k, v)| metadata_kv(k, &v))
                .collect();

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

/// Build the content features fragment ($585).
///
/// This describes the content capabilities/features of the book.
pub(super) fn build_content_features_fragment() -> KfxFragment {
    // Build feature entries matching reference KFX
    let reflow_style = IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String("com.amazon.yjconversion".to_string()),
        ),
        (
            KfxSymbol::Key as u64,
            IonValue::String("reflow-style".to_string()),
        ),
        (
            KfxSymbol::VersionInfo as u64,
            IonValue::Struct(vec![(
                KfxSymbol::Version as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::MajorVersion as u64, IonValue::Int(6)),
                    (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
                ]),
            )]),
        ),
    ]);

    let canonical_format = IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String("SDK.Marker".to_string()),
        ),
        (
            KfxSymbol::Key as u64,
            IonValue::String("CanonicalFormat".to_string()),
        ),
        (
            KfxSymbol::VersionInfo as u64,
            IonValue::Struct(vec![(
                KfxSymbol::Version as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::MajorVersion as u64, IonValue::Int(1)),
                    (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
                ]),
            )]),
        ),
    ]);

    let yj_hdv = IonValue::Struct(vec![
        (
            KfxSymbol::Namespace as u64,
            IonValue::String("com.amazon.yjconversion".to_string()),
        ),
        (
            KfxSymbol::Key as u64,
            IonValue::String("yj_hdv".to_string()),
        ),
        (
            KfxSymbol::VersionInfo as u64,
            IonValue::Struct(vec![(
                KfxSymbol::Version as u64,
                IonValue::Struct(vec![
                    (KfxSymbol::MajorVersion as u64, IonValue::Int(1)),
                    (KfxSymbol::MinorVersion as u64, IonValue::Int(0)),
                ]),
            )]),
        ),
    ]);

    let content_features = IonValue::Struct(vec![(
        KfxSymbol::Features as u64,
        IonValue::List(vec![reflow_style, canonical_format, yj_hdv]),
    )]);

    KfxFragment::singleton(KfxSymbol::ContentFeatures, content_features)
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
