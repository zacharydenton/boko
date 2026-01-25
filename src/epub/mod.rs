//! EPUB format support - pure parsing functions.

mod parser;

pub use parser::{parse_container_xml, parse_ncx, parse_opf, strip_bom, OpfData};
