//! MOBI/AZW3 format support.

mod headers;
pub mod huffcdic;
pub(crate) mod index;
pub mod palmdoc;
pub mod parser;

// Internal modules for AZW3 export
pub(crate) mod skeleton;
pub(crate) mod writer_transform;

pub use parser::{
    Compression, Encoding, ExthHeader, HuffCdicReader, MobiFormat, MobiHeader, NULL_INDEX,
    NcxEntry, PdbInfo, TocNode, build_toc_from_ncx, detect_font_type, detect_format,
    detect_image_type, is_metadata_record, parse_exth, parse_fdst, parse_ncx_index, read_index,
    strip_trailing_data,
};

/// Parse Kindle base32 encoding (0-9A-V) to number.
/// Used for kindle: URI references like kindle:embed:XXXX.
#[inline]
pub fn parse_base32(s: &[u8]) -> usize {
    let mut result = 0usize;
    for &b in s {
        result = result.wrapping_mul(32);
        let val = match b {
            b'0'..=b'9' => (b - b'0') as usize,
            b'A'..=b'V' => (b - b'A') as usize + 10,
            b'a'..=b'v' => (b - b'a') as usize + 10,
            _ => continue,
        };
        result = result.wrapping_add(val);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_base32() {
        // Single digits
        assert_eq!(parse_base32(b"0"), 0);
        assert_eq!(parse_base32(b"1"), 1);
        assert_eq!(parse_base32(b"9"), 9);
        assert_eq!(parse_base32(b"A"), 10);
        assert_eq!(parse_base32(b"V"), 31);

        // Lowercase
        assert_eq!(parse_base32(b"a"), 10);
        assert_eq!(parse_base32(b"v"), 31);

        // Multi-digit
        assert_eq!(parse_base32(b"10"), 32); // 1*32 + 0
        assert_eq!(parse_base32(b"100"), 1024); // 1*32*32 + 0*32 + 0

        // Real kindle embed reference
        assert_eq!(parse_base32(b"0001"), 1);
        assert_eq!(parse_base32(b"000V"), 31);
    }
}
