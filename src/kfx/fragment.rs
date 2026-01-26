//! KFX fragment representation.
//!
//! A fragment is the fundamental unit of KFX content. Fragments can contain
//! either Ion data (for structured content) or raw bytes (for media like images).

use super::ion::IonValue;
use super::symbols::KfxSymbol;

/// Fragment payload - Ion for structured data, Raw for media.
#[derive(Debug, Clone)]
pub enum FragmentData {
    /// Ion-encoded structured data (metadata, storylines, styles, etc.)
    Ion(IonValue),
    /// Raw binary data (JPEG, PNG, TTF, etc.)
    Raw(Vec<u8>),
}

/// A KFX fragment - the fundamental unit of KFX content.
#[derive(Debug, Clone)]
pub struct KfxFragment {
    /// Fragment type (symbol ID like $260, $145, etc.)
    pub ftype: u64,
    /// Fragment ID (unique identifier, or same as ftype for singletons)
    pub fid: String,
    /// The payload (Ion or raw bytes)
    pub data: FragmentData,
}

impl KfxFragment {
    /// Create a new fragment with Ion data.
    pub fn new(ftype: impl Into<u64>, fid: impl Into<String>, value: IonValue) -> Self {
        Self {
            ftype: ftype.into(),
            fid: fid.into(),
            data: FragmentData::Ion(value),
        }
    }

    /// Create a new fragment with a numeric fragment ID.
    ///
    /// This is used when the fragment ID was pre-assigned during Pass 1.
    pub fn new_with_id(
        ftype: impl Into<u64>,
        fragment_id: u64,
        name: impl Into<String>,
        value: IonValue,
    ) -> Self {
        // Store the name as fid for debugging, but the fragment_id is what matters
        // for serialization
        let _ = fragment_id; // ID is embedded in the entity table during serialization
        Self {
            ftype: ftype.into(),
            fid: name.into(),
            data: FragmentData::Ion(value),
        }
    }

    /// Create a new fragment with raw binary data.
    pub fn raw(ftype: impl Into<u64>, fid: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            ftype: ftype.into(),
            fid: fid.into(),
            data: FragmentData::Raw(bytes),
        }
    }

    /// Create a singleton fragment (fid equals ftype name).
    /// Used for fragments where only one instance exists (e.g., metadata).
    pub fn singleton(ftype: impl Into<u64>, value: IonValue) -> Self {
        let ftype_val = ftype.into();
        Self {
            ftype: ftype_val,
            fid: format!("${ftype_val}"),
            data: FragmentData::Ion(value),
        }
    }

    /// Check if this is a singleton fragment.
    pub fn is_singleton(&self) -> bool {
        self.fid == format!("${}", self.ftype)
    }

    /// Check if this fragment contains raw data.
    pub fn is_raw(&self) -> bool {
        matches!(self.data, FragmentData::Raw(_))
    }

    /// Get the Ion value if this is an Ion fragment.
    pub fn as_ion(&self) -> Option<&IonValue> {
        match &self.data {
            FragmentData::Ion(v) => Some(v),
            FragmentData::Raw(_) => None,
        }
    }

    /// Get the raw bytes if this is a raw fragment.
    pub fn as_raw(&self) -> Option<&[u8]> {
        match &self.data {
            FragmentData::Ion(_) => None,
            FragmentData::Raw(bytes) => Some(bytes),
        }
    }
}

// Convenience conversions from KfxSymbol
impl From<KfxSymbol> for u64 {
    fn from(sym: KfxSymbol) -> u64 {
        sym as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragment_new() {
        let frag = KfxFragment::new(260u64, "section-1", IonValue::Null);
        assert_eq!(frag.ftype, 260);
        assert_eq!(frag.fid, "section-1");
        assert!(!frag.is_singleton());
        assert!(!frag.is_raw());
    }

    #[test]
    fn test_fragment_singleton() {
        let frag = KfxFragment::singleton(KfxSymbol::Metadata, IonValue::Null);
        assert!(frag.is_singleton());
        assert_eq!(frag.fid, "$258");
    }

    #[test]
    fn test_fragment_raw() {
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
        let frag = KfxFragment::raw(KfxSymbol::Bcrawmedia, "image-1", data.clone());
        assert!(frag.is_raw());
        assert_eq!(frag.as_raw(), Some(data.as_slice()));
        assert!(frag.as_ion().is_none());
    }

    #[test]
    fn test_kfx_symbol_conversion() {
        let frag = KfxFragment::new(KfxSymbol::Section, "sec-1", IonValue::Null);
        assert_eq!(frag.ftype, 260);
    }
}
