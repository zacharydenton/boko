//! MOBI/AZW3 format support.

mod headers;
pub mod huffcdic;
mod index;
pub mod palmdoc;
pub mod parser;

// Old reader/writer disabled - use import::MobiImporter instead
// mod skeleton;
// mod transform;
// mod reader;
// mod writer;
// mod writer_transform;

pub use parser::{
    Compression, Encoding, ExthHeader, HuffCdicReader, MobiFormat, MobiHeader, NcxEntry, PdbInfo,
    TocNode, build_toc_from_ncx, detect_font_type, detect_format, detect_image_type,
    is_metadata_record, parse_exth, parse_fdst, parse_ncx_index, read_index, strip_trailing_data,
    NULL_INDEX,
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
