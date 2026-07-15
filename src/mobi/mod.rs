//! MOBI/AZW3 format support.

mod headers;
pub mod huffcdic;
pub(crate) mod index;
pub mod palmdoc;
pub mod parser;

// Internal modules for AZW3 export
pub(crate) mod skeleton;
pub(crate) mod split;
pub(crate) mod tbs;
pub(crate) mod writer_transform;

// Transform for reading MOBI/KF8 files
pub mod transform;

// Filepos handling for link resolution
pub mod filepos;

pub use parser::{
    Compression, Encoding, ExthHeader, HuffCdicReader, MobiFormat, MobiHeader, NULL_INDEX, PdbInfo,
    TocNode, build_toc_from_ncx, decode_font_record, detect_font_type, detect_image_type,
    is_metadata_record, parse_exth, parse_fdst, parse_ncx_index, read_index, strip_trailing_data,
};
