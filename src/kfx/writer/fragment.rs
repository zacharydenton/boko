//! KFX fragment representation.
//!
//! A fragment is the fundamental unit of KFX content.

use crate::kfx::ion::IonValue;

use super::symbols::{SymbolTable, sym};

/// A KFX fragment - the fundamental unit of KFX content
#[derive(Debug, Clone)]
pub struct KfxFragment {
    /// Fragment type (symbol ID like $260, $145, etc.)
    pub ftype: u64,
    /// Fragment ID (unique identifier, or same as ftype for singletons)
    pub fid: String,
    /// The ION value payload
    pub value: IonValue,
}

impl KfxFragment {
    /// Create a new fragment
    pub fn new(ftype: u64, fid: impl Into<String>, value: IonValue) -> Self {
        Self {
            ftype,
            fid: fid.into(),
            value,
        }
    }

    /// Create a singleton fragment (fid equals ftype name)
    pub fn singleton(ftype: u64, value: IonValue) -> Self {
        Self {
            ftype,
            fid: format!("${ftype}"),
            value,
        }
    }

    /// Check if this is a singleton fragment
    pub fn is_singleton(&self) -> bool {
        self.fid == format!("${}", self.ftype)
    }

    /// Get the entity ID number for serialization
    #[allow(dead_code)]
    pub fn entity_id(&self, symtab: &SymbolTable) -> u32 {
        if self.is_singleton() {
            sym::SINGLETON_ID as u32
        } else {
            symtab.get(&self.fid).unwrap_or(sym::SINGLETON_ID) as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragment_singleton() {
        let frag = KfxFragment::singleton(sym::DOCUMENT_DATA, IonValue::Null);
        assert!(frag.is_singleton());
        assert_eq!(frag.fid, "$538");
    }
}
