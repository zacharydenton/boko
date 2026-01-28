//! EPUB format support - pure parsing functions.

mod parser;

pub use parser::{
    OpfData, parse_container_xml, parse_nav_landmarks, parse_ncx, parse_opf, strip_bom,
};
