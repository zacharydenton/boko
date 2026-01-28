//! Auxiliary data generation for KFX export.
//!
//! Each section gets an auxiliary_data entity marking it as a navigation target.
//! This enables the Kindle reader to recognize which sections can be navigated to.

use super::context::ExportContext;
use super::fragment::KfxFragment;
use super::ion::IonValue;
use super::symbols::KfxSymbol;

/// Build an auxiliary_data fragment for a section.
///
/// Each section needs an auxiliary_data entity with metadata marking it
/// as a target section for navigation.
///
/// # Arguments
/// * `section_name` - The section name (e.g., "c0", "c1")
/// * `ctx` - Export context for symbol interning
///
/// # Returns
/// A KfxFragment with type auxiliary_data ($597)
///
/// # Structure
/// ```ion
/// {
///   kfx_id: 'c0-ad',
///   metadata: [
///     { key: "IS_TARGET_SECTION", value: true }
///   ]
/// }
/// ```
pub fn build_auxiliary_data_fragment(section_name: &str, ctx: &mut ExportContext) -> KfxFragment {
    let kfx_id = format!("{}-ad", section_name);
    // kfx_id must be a symbol, not a string (per reference KFX)
    let kfx_id_symbol = ctx.symbols.get_or_intern(&kfx_id);

    let metadata_entry = IonValue::Struct(vec![
        (
            KfxSymbol::Key as u64,
            IonValue::String("IS_TARGET_SECTION".to_string()),
        ),
        (KfxSymbol::Value as u64, IonValue::Bool(true)),
    ]);

    let ion = IonValue::Struct(vec![
        (KfxSymbol::KfxId as u64, IonValue::Symbol(kfx_id_symbol)),
        (
            KfxSymbol::Metadata as u64,
            IonValue::List(vec![metadata_entry]),
        ),
    ]);

    KfxFragment::new(KfxSymbol::AuxiliaryData, &kfx_id, ion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kfx::fragment::FragmentData;

    #[test]
    fn test_build_auxiliary_data_fragment() {
        let mut ctx = ExportContext::new();
        let frag = build_auxiliary_data_fragment("c0", &mut ctx);

        assert_eq!(frag.ftype, KfxSymbol::AuxiliaryData as u64);
        assert_eq!(frag.fid, "c0-ad");

        if let FragmentData::Ion(IonValue::Struct(fields)) = &frag.data {
            // Check kfx_id - should be a symbol now
            let kfx_id = fields.iter().find(|(id, _)| *id == KfxSymbol::KfxId as u64);
            assert!(kfx_id.is_some(), "should have kfx_id");
            assert!(
                matches!(kfx_id, Some((_, IonValue::Symbol(_)))),
                "kfx_id should be a symbol"
            );

            // Check metadata
            let metadata = fields
                .iter()
                .find(|(id, _)| *id == KfxSymbol::Metadata as u64);
            assert!(metadata.is_some(), "should have metadata");
            if let Some((_, IonValue::List(entries))) = metadata {
                assert_eq!(entries.len(), 1);
                if let IonValue::Struct(entry_fields) = &entries[0] {
                    let key = entry_fields
                        .iter()
                        .find(|(id, _)| *id == KfxSymbol::Key as u64);
                    let value = entry_fields
                        .iter()
                        .find(|(id, _)| *id == KfxSymbol::Value as u64);

                    if let Some((_, IonValue::String(k))) = key {
                        assert_eq!(k, "IS_TARGET_SECTION");
                    }
                    if let Some((_, IonValue::Bool(v))) = value {
                        assert!(*v);
                    }
                }
            }
        } else {
            panic!("expected Ion struct data");
        }
    }
}
