//! Build a minimal KFX fixture for the annotated-entity test.
//!
//! Produces a tiny KFX container with a single `book_metadata` entity wrapped
//! in a `$490::{ ... }` annotation (the real-world pattern observed in KFX-ZIP
//! metadata sidecars). All identifying values are synthetic.
//!
//! Run with: `cargo run --release --example build_annotated_kfx_fixture`
//!
//! Output: `tests/fixtures/annotated_metadata.kfx`

use std::io::Write;

use boko::kfx::ion::IonValue;
use boko::kfx::serialization::{SerializedEntity, create_entity_data, serialize_container};
use boko::kfx::symbols::KfxSymbol;

fn metadata_entry(key: &str, value: &str) -> IonValue {
    IonValue::Struct(vec![
        (KfxSymbol::Key as u64, IonValue::String(key.to_string())),
        (KfxSymbol::Value as u64, IonValue::String(value.to_string())),
    ])
}

fn main() -> std::io::Result<()> {
    // Build categorised_metadata list: a single "kindle_title_metadata" category
    // with synthetic title/author/ASIN/etc.
    let kindle_title_metadata = IonValue::Struct(vec![
        (
            KfxSymbol::Category as u64,
            IonValue::String("kindle_title_metadata".to_string()),
        ),
        (
            KfxSymbol::Metadata as u64,
            IonValue::List(vec![
                metadata_entry("title", "Annotated Entity Test Book"),
                metadata_entry("author", "Boko Test Author"),
                metadata_entry("publisher", "Boko Test Press"),
                metadata_entry("language", "en"),
                metadata_entry("ASIN", "B000TESTASIN"),
                metadata_entry("book_id", "synthetic-book-id-0001"),
                metadata_entry("cde_content_type", "EBOK"),
                metadata_entry("issue_date", "2026-01-01"),
            ]),
        ),
    ]);

    // book_metadata struct: { categorised_metadata: [ ... ] }
    let book_metadata_inner = IonValue::Struct(vec![(
        KfxSymbol::CategorisedMetadata as u64,
        IonValue::List(vec![kindle_title_metadata]),
    )]);

    // Wrap in the $490 (book_metadata) annotation — the pattern this fixture
    // exists to test.
    let annotated = IonValue::Annotated(
        vec![KfxSymbol::BookMetadata as u64],
        Box::new(book_metadata_inner),
    );

    let entity_data = create_entity_data(&annotated);

    let entities = vec![SerializedEntity {
        id: 0,
        entity_type: KfxSymbol::BookMetadata as u32,
        data: entity_data,
        raw: None,
    }];

    let container_id = "CR!BOKOTESTANNOTATEDMETADATAFIX";
    let symtab_ion: Vec<u8> = Vec::new();
    let format_caps_ion: Vec<u8> = Vec::new();

    let bytes = serialize_container(container_id, &entities, &symtab_ion, &format_caps_ion);

    let out_path = "tests/fixtures/annotated_metadata.kfx";
    let mut f = std::fs::File::create(out_path)?;
    f.write_all(&bytes)?;
    println!("Wrote {} ({} bytes)", out_path, bytes.len());

    Ok(())
}
