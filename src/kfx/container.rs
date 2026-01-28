//! KFX container format parsing.
//!
//! This module contains pure functions for parsing KFX container structures.
//! All functions operate on byte slices and do not perform I/O.

use super::ion::{IonParser, IonValue};
use super::symbols::KFX_SYMBOL_TABLE;

/// KFX container header (18 bytes).
#[derive(Debug, Clone, Copy)]
pub struct ContainerHeader {
    /// Header length (offset to entity data).
    pub header_len: usize,
    /// Container info offset.
    pub container_info_offset: usize,
    /// Container info length.
    pub container_info_length: usize,
}

/// Location of an entity within the container.
#[derive(Debug, Clone, Copy)]
pub struct EntityLoc {
    /// Entity ID.
    pub id: u32,
    /// Entity type ID (symbol ID).
    pub type_id: u32,
    /// Byte offset within container (after header).
    pub offset: usize,
    /// Length in bytes.
    pub length: usize,
}

/// Parsed container info fields.
#[derive(Debug, Clone, Default)]
pub struct ContainerInfo {
    /// Index table offset and length.
    pub index: Option<(usize, usize)>,
    /// Document symbols offset and length.
    pub doc_symbols: Option<(usize, usize)>,
}

/// Error type for container parsing.
#[derive(Debug)]
pub enum ContainerError {
    /// Invalid magic bytes.
    InvalidMagic,
    /// Data too short.
    TooShort,
    /// Invalid Ion data.
    InvalidIon(String),
    /// Missing required field.
    MissingField(&'static str),
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::InvalidMagic => write!(f, "Not a valid KFX container"),
            ContainerError::TooShort => write!(f, "Data too short"),
            ContainerError::InvalidIon(msg) => write!(f, "Invalid Ion data: {}", msg),
            ContainerError::MissingField(field) => write!(f, "Missing field: {}", field),
        }
    }
}

impl std::error::Error for ContainerError {}

impl From<std::io::Error> for ContainerError {
    fn from(e: std::io::Error) -> Self {
        ContainerError::InvalidIon(e.to_string())
    }
}

// --- Byte reading helpers ---

/// Read a little-endian u32 from a byte slice at the given offset.
#[inline]
pub fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Read a little-endian u64 from a byte slice at the given offset.
#[inline]
pub fn read_u64_le(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

// --- Container header parsing ---

/// Parse the KFX container header (first 18 bytes).
///
/// Returns the header structure containing offsets and lengths.
pub fn parse_container_header(data: &[u8]) -> Result<ContainerHeader, ContainerError> {
    if data.len() < 18 {
        return Err(ContainerError::TooShort);
    }

    if &data[0..4] != b"CONT" {
        return Err(ContainerError::InvalidMagic);
    }

    Ok(ContainerHeader {
        header_len: read_u32_le(data, 6) as usize,
        container_info_offset: read_u32_le(data, 10) as usize,
        container_info_length: read_u32_le(data, 14) as usize,
    })
}

// --- Container info parsing ---

/// Parse container info to extract index table and doc symbols locations.
pub fn parse_container_info(data: &[u8]) -> Result<ContainerInfo, ContainerError> {
    let mut parser = IonParser::new(data);
    let elem = parser.parse()?;

    let fields = elem.as_struct().ok_or(ContainerError::InvalidIon(
        "Container info is not a struct".to_string(),
    ))?;

    let mut info = ContainerInfo::default();

    // Index table
    if let (Some(offset), Some(length)) = (
        get_field_int(fields, "bcIndexTabOffset"),
        get_field_int(fields, "bcIndexTabLength"),
    ) {
        info.index = Some((offset as usize, length as usize));
    }

    // Document symbols
    if let (Some(offset), Some(length)) = (
        get_field_int(fields, "bcDocSymbolOffset"),
        get_field_int(fields, "bcDocSymbolLength"),
    ) {
        info.doc_symbols = Some((offset as usize, length as usize));
    }

    Ok(info)
}

/// Get an integer field from a struct by field name.
fn get_field_int(fields: &[(u64, IonValue)], name: &str) -> Option<i64> {
    let sym_id = symbol_id_for_name(name)?;
    fields
        .iter()
        .find(|(k, _)| *k == sym_id)
        .and_then(|(_, v)| v.as_int())
}

/// Look up a symbol ID by name from the static symbol table.
pub fn symbol_id_for_name(name: &str) -> Option<u64> {
    KFX_SYMBOL_TABLE
        .iter()
        .position(|&s| s == name)
        .map(|i| i as u64)
}

// --- Index table parsing ---

/// Parse the entity index table.
///
/// Each entry is 24 bytes: id(4) + type_id(4) + offset(8) + length(8).
/// The `header_len` is added to offsets to get absolute positions.
pub fn parse_index_table(data: &[u8], header_len: usize) -> Vec<EntityLoc> {
    const ENTRY_SIZE: usize = 24;
    let num_entries = data.len() / ENTRY_SIZE;
    let mut entities = Vec::with_capacity(num_entries);

    for i in 0..num_entries {
        let entry_offset = i * ENTRY_SIZE;
        if entry_offset + ENTRY_SIZE > data.len() {
            break;
        }

        entities.push(EntityLoc {
            id: read_u32_le(data, entry_offset),
            type_id: read_u32_le(data, entry_offset + 4),
            offset: header_len + read_u64_le(data, entry_offset + 8) as usize,
            length: read_u64_le(data, entry_offset + 16) as usize,
        });
    }

    entities
}

// --- Entity header parsing ---

/// Skip the ENTY header if present and return the payload data.
///
/// Returns the slice after the ENTY header, or the original slice if no header.
pub fn skip_enty_header(data: &[u8]) -> &[u8] {
    if data.len() >= 10 && &data[0..4] == b"ENTY" {
        let header_len = read_u32_le(data, 6) as usize;
        if header_len < data.len() {
            return &data[header_len..];
        }
    }
    data
}

// --- Document symbols parsing ---

/// Extract document-specific symbols from the doc symbols section.
///
/// These are local symbols that extend the base KFX symbol table.
pub fn extract_doc_symbols(data: &[u8]) -> Vec<String> {
    // Parse the $ion_symbol_table entity using the Ion parser.
    // The data is: BVM + $3::{ $6: [{ $4: "YJ_symbols", $5: 10, $8: 851 }], $7: [...] }
    // We need the strings from the $7 (symbols) field.
    let mut parser = IonParser::new(data);
    let value = match parser.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    // Unwrap annotation ($3 = $ion_symbol_table)
    let inner = value.unwrap_annotated();

    // Get the struct fields
    let fields = match inner.as_struct() {
        Some(f) => f,
        None => return Vec::new(),
    };

    // Field $7 = "symbols" in Ion system symbols
    let symbols_list = match get_field(fields, 7) {
        Some(list) => list,
        None => return Vec::new(),
    };

    let items = match symbols_list.as_list() {
        Some(l) => l,
        None => return Vec::new(),
    };

    items
        .iter()
        .filter_map(|v| {
            if let IonValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect()
}

// --- Symbol resolution ---

/// Resolve a symbol ID to its text representation.
///
/// Checks the base KFX symbol table first, then document-local symbols.
pub fn resolve_symbol(id: u64, doc_symbols: &[String]) -> Option<&str> {
    let id = id as usize;
    if id < KFX_SYMBOL_TABLE.len() {
        Some(KFX_SYMBOL_TABLE[id])
    } else {
        let doc_idx = id.saturating_sub(KFX_SYMBOL_TABLE.len());
        doc_symbols.get(doc_idx).map(|s| s.as_str())
    }
}

/// Get a symbol's text from an IonValue (handles both Symbol and String).
pub fn get_symbol_text<'a>(value: &'a IonValue, doc_symbols: &'a [String]) -> Option<&'a str> {
    match value {
        IonValue::Symbol(id) => resolve_symbol(*id, doc_symbols),
        IonValue::String(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Get a field from a struct by symbol ID.
#[inline]
pub fn get_field(fields: &[(u64, IonValue)], symbol_id: u64) -> Option<&IonValue> {
    fields
        .iter()
        .find(|(k, _)| *k == symbol_id)
        .map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u32_le() {
        let data = [0x01, 0x02, 0x03, 0x04];
        assert_eq!(read_u32_le(&data, 0), 0x04030201);
    }

    #[test]
    fn test_read_u64_le() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        assert_eq!(read_u64_le(&data, 0), 0x0807060504030201);
    }

    #[test]
    fn test_parse_container_header() {
        let mut data = vec![0u8; 18];
        data[0..4].copy_from_slice(b"CONT");
        // Skip 2 bytes (unknown)
        // header_len at offset 6
        data[6..10].copy_from_slice(&100u32.to_le_bytes());
        // container_info_offset at offset 10
        data[10..14].copy_from_slice(&200u32.to_le_bytes());
        // container_info_length at offset 14
        data[14..18].copy_from_slice(&50u32.to_le_bytes());

        let header = parse_container_header(&data).unwrap();
        assert_eq!(header.header_len, 100);
        assert_eq!(header.container_info_offset, 200);
        assert_eq!(header.container_info_length, 50);
    }

    #[test]
    fn test_parse_container_header_invalid_magic() {
        let data = [0u8; 18];
        let result = parse_container_header(&data);
        assert!(matches!(result, Err(ContainerError::InvalidMagic)));
    }

    #[test]
    fn test_parse_container_header_too_short() {
        let data = [0u8; 10];
        let result = parse_container_header(&data);
        assert!(matches!(result, Err(ContainerError::TooShort)));
    }

    #[test]
    fn test_parse_index_table() {
        // Create a simple index table with 2 entries
        let mut data = vec![0u8; 48];

        // Entry 1: id=1, type_id=100, offset=1000, length=500
        data[0..4].copy_from_slice(&1u32.to_le_bytes());
        data[4..8].copy_from_slice(&100u32.to_le_bytes());
        data[8..16].copy_from_slice(&1000u64.to_le_bytes());
        data[16..24].copy_from_slice(&500u64.to_le_bytes());

        // Entry 2: id=2, type_id=200, offset=2000, length=300
        data[24..28].copy_from_slice(&2u32.to_le_bytes());
        data[28..32].copy_from_slice(&200u32.to_le_bytes());
        data[32..40].copy_from_slice(&2000u64.to_le_bytes());
        data[40..48].copy_from_slice(&300u64.to_le_bytes());

        let entities = parse_index_table(&data, 50);

        assert_eq!(entities.len(), 2);

        assert_eq!(entities[0].id, 1);
        assert_eq!(entities[0].type_id, 100);
        assert_eq!(entities[0].offset, 50 + 1000);
        assert_eq!(entities[0].length, 500);

        assert_eq!(entities[1].id, 2);
        assert_eq!(entities[1].type_id, 200);
        assert_eq!(entities[1].offset, 50 + 2000);
        assert_eq!(entities[1].length, 300);
    }

    #[test]
    fn test_skip_enty_header() {
        // Data with ENTY header
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(b"ENTY");
        // header_len at offset 6
        data[6..10].copy_from_slice(&10u32.to_le_bytes());
        // Payload after header
        data[10..20].copy_from_slice(b"0123456789");

        let payload = skip_enty_header(&data);
        assert_eq!(payload, b"0123456789");
    }

    #[test]
    fn test_skip_enty_header_no_header() {
        let data = b"no enty header here";
        let payload = skip_enty_header(data);
        assert_eq!(payload, data.as_slice());
    }

    #[test]
    fn test_resolve_symbol_base_table() {
        let doc_symbols: Vec<String> = vec![];
        // Symbol 10 is "language" in the base table
        assert_eq!(resolve_symbol(10, &doc_symbols), Some("language"));
    }

    #[test]
    fn test_resolve_symbol_doc_local() {
        let doc_symbols = vec!["custom_symbol".to_string()];
        // Document-local symbols start after the base table
        let doc_symbol_id = KFX_SYMBOL_TABLE.len() as u64;
        assert_eq!(resolve_symbol(doc_symbol_id, &doc_symbols), Some("custom_symbol"));
    }

    #[test]
    fn test_symbol_id_for_name() {
        assert_eq!(symbol_id_for_name("language"), Some(10));
        assert_eq!(symbol_id_for_name("nonexistent"), None);
    }

    #[test]
    fn test_extract_doc_symbols() {
        use crate::kfx::ion::IonWriter;

        // Build a valid $ion_symbol_table: $3::{ $7: ["hello", "world"] }
        let mut writer = IonWriter::new();
        writer.write_bvm();
        let symtab = IonValue::Struct(vec![
            (7, IonValue::List(vec![
                IonValue::String("hello".into()),
                IonValue::String("world".into()),
            ])),
        ]);
        writer.write_annotated(&[3], &symtab);
        let data = writer.into_bytes();

        let symbols = extract_doc_symbols(&data);
        assert_eq!(symbols, vec!["hello", "world"]);
    }

    #[test]
    fn test_extract_doc_symbols_with_imports() {
        use crate::kfx::ion::IonWriter;

        // Build $ion_symbol_table with imports and symbols
        let mut writer = IonWriter::new();
        writer.write_bvm();
        let import_entry = IonValue::Struct(vec![
            (4, IonValue::String("YJ_symbols".into())),
            (5, IonValue::Int(10)),
            (8, IonValue::Int(851)),
        ]);
        let symtab = IonValue::Struct(vec![
            (6, IonValue::List(vec![import_entry])),
            (7, IonValue::List(vec![
                IonValue::String("custom_sym".into()),
            ])),
        ]);
        writer.write_annotated(&[3], &symtab);
        let data = writer.into_bytes();

        let symbols = extract_doc_symbols(&data);
        assert_eq!(symbols, vec!["custom_sym"]);
    }
}
