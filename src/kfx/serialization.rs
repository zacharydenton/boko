//! KFX container serialization.
//!
//! Handles the binary format for KFX containers and entities.
//! This module provides functions to serialize KFX fragments into
//! the container format that Kindle devices can read.

use super::ion::{IonValue, IonWriter};
use super::symbols::KfxSymbol;

/// Serialized entity ready for container output.
pub struct SerializedEntity<'a> {
    /// Entity ID (fragment ID symbol)
    pub id: u32,
    /// Entity type (fragment type symbol)
    pub entity_type: u32,
    /// ENTY header (+ Ion content for structured entities).
    pub data: Vec<u8>,
    /// Raw media body written after `data`, borrowed from the fragment so
    /// multi-MB image/font payloads are never duplicated per entity.
    pub raw: Option<&'a [u8]>,
}

impl SerializedEntity<'_> {
    /// Total serialized length (header/content + raw body).
    pub fn len(&self) -> usize {
        self.data.len() + self.raw.map_or(0, <[u8]>::len)
    }

    /// Whether the entity serializes to zero bytes (never, in practice).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Container magic bytes.
const CONTAINER_MAGIC: &[u8; 4] = b"CONT";

/// Entity magic bytes.
const ENTITY_MAGIC: &[u8; 4] = b"ENTY";

/// Serialize a complete KFX container.
///
/// Container layout:
/// - Header: CONT magic + version + header_len + ci_offset + ci_len
/// - Entity table (indexed by $413/$414)
/// - Doc symbols ION (indexed by $415/$416)
/// - Format capabilities ION (indexed by $594/$595)
/// - Container info ION
/// - kfxgen_info JSON
/// - Entity payloads (after header_len)
#[allow(clippy::vec_init_then_push)]
pub fn serialize_container(
    container_id: &str,
    entities: &[SerializedEntity],
    symtab_ion: &[u8],
    format_caps_ion: &[u8],
) -> Vec<u8> {
    // Build entity table and calculate payload offsets
    let mut entity_table = Vec::new();
    let mut current_offset = 0u64;

    for entity in entities {
        entity_table.extend_from_slice(&entity.id.to_le_bytes());
        entity_table.extend_from_slice(&entity.entity_type.to_le_bytes());
        entity_table.extend_from_slice(&current_offset.to_le_bytes());
        entity_table.extend_from_slice(&(entity.len() as u64).to_le_bytes());
        current_offset += entity.len() as u64;
    }

    // SHA1 of entity payloads for kfxgen_info, streamed so the payloads are
    // never concatenated into a temporary whole-container buffer.
    let mut sha = sha1_smol::Sha1::new();
    for entity in entities {
        sha.update(&entity.data);
        if let Some(raw) = entity.raw {
            sha.update(raw);
        }
    }
    let payload_sha1 = sha.digest().to_string();

    // Header is 18 bytes: magic(4) + version(2) + header_len(4) + ci_offset(4) + ci_len(4)
    const HEADER_SIZE: usize = 18;

    // Calculate offsets within the header section (after the 18-byte fixed header)
    let entity_table_offset = HEADER_SIZE;
    let symtab_offset = entity_table_offset + entity_table.len();
    let format_caps_offset = symtab_offset + symtab_ion.len();

    // Build container info with all the offset pointers
    let mut container_info_fields = Vec::new();
    container_info_fields.push((
        KfxSymbol::Bccontid as u64,
        IonValue::String(container_id.to_string()),
    ));
    container_info_fields.push((KfxSymbol::Bccomprtype as u64, IonValue::Int(0)));
    container_info_fields.push((KfxSymbol::Bcdrmscheme as u64, IonValue::Int(0)));
    container_info_fields.push((KfxSymbol::Bcchunksize as u64, IonValue::Int(4096)));
    container_info_fields.push((
        KfxSymbol::Bcindextaboffset as u64,
        IonValue::Int(entity_table_offset as i64),
    ));
    container_info_fields.push((
        KfxSymbol::Bcindextablength as u64,
        IonValue::Int(entity_table.len() as i64),
    ));
    container_info_fields.push((
        KfxSymbol::Bcdocsymboloffset as u64,
        IonValue::Int(symtab_offset as i64),
    ));
    container_info_fields.push((
        KfxSymbol::Bcdocsymbollength as u64,
        IonValue::Int(symtab_ion.len() as i64),
    ));

    // Only include format capabilities offset if we have them
    if !format_caps_ion.is_empty() {
        container_info_fields.push((
            KfxSymbol::Bcfcapabilitiesoffset as u64,
            IonValue::Int(format_caps_offset as i64),
        ));
        container_info_fields.push((
            KfxSymbol::Bcfcapabilitieslength as u64,
            IonValue::Int(format_caps_ion.len() as i64),
        ));
    }

    let mut ion_writer = IonWriter::new();
    ion_writer.write_bvm();
    ion_writer.write_value(&IonValue::Struct(container_info_fields));
    let container_info_data = ion_writer.into_bytes();

    let container_info_offset = format_caps_offset + format_caps_ion.len();

    // kfxgen info JSON (matches Amazon's format)
    let kfxgen_info = format!(
        r#"[{{key:kfxgen_package_version,value:boko-{}}},{{key:kfxgen_application_version,value:boko}},{{key:kfxgen_payload_sha1,value:{}}},{{key:kfxgen_acr,value:{}}}]"#,
        env!("CARGO_PKG_VERSION"),
        payload_sha1,
        container_id
    );

    let header_len = container_info_offset + container_info_data.len() + kfxgen_info.len();

    // Build output
    let mut output = Vec::with_capacity(header_len + current_offset as usize);

    // Fixed header (18 bytes)
    output.extend_from_slice(CONTAINER_MAGIC);
    output.extend_from_slice(&2u16.to_le_bytes()); // version
    output.extend_from_slice(&(header_len as u32).to_le_bytes());
    output.extend_from_slice(&(container_info_offset as u32).to_le_bytes());
    output.extend_from_slice(&(container_info_data.len() as u32).to_le_bytes());

    // Entity table
    output.extend_from_slice(&entity_table);

    // Doc symbols (symbol table)
    output.extend_from_slice(symtab_ion);

    // Format capabilities
    output.extend_from_slice(format_caps_ion);

    // Container info
    output.extend_from_slice(&container_info_data);

    // kfxgen info JSON
    output.extend_from_slice(kfxgen_info.as_bytes());

    // Entity payloads (after header)
    for entity in entities {
        output.extend_from_slice(&entity.data);
        if let Some(raw) = entity.raw {
            output.extend_from_slice(raw);
        }
    }

    output
}

/// Create entity data with ENTY header for Ion content.
pub fn create_entity_data(value: &IonValue) -> Vec<u8> {
    // Entity header ION: {$410:0, $411:0}
    let header_fields = vec![
        (KfxSymbol::Bccomprtype as u64, IonValue::Int(0)),
        (KfxSymbol::Bcdrmscheme as u64, IonValue::Int(0)),
    ];

    let mut header_writer = IonWriter::new();
    header_writer.write_bvm();
    header_writer.write_value(&IonValue::Struct(header_fields));
    let header_ion = header_writer.into_bytes();

    // Content ION
    let mut content_writer = IonWriter::new();
    content_writer.write_bvm();
    content_writer.write_value(value);
    let content_ion = content_writer.into_bytes();

    // ENTY header: magic(4) + version(2) + header_len(4) = 10
    let header_len = 10 + header_ion.len();

    let mut data = Vec::with_capacity(header_len + content_ion.len());
    data.extend_from_slice(ENTITY_MAGIC);
    data.extend_from_slice(&1u16.to_le_bytes()); // version
    data.extend_from_slice(&(header_len as u32).to_le_bytes());
    data.extend_from_slice(&header_ion);
    data.extend_from_slice(&content_ion);

    data
}

/// Create the ENTY header for a raw media entity (images, fonts).
///
/// Raw media stores bytes directly without Ion encoding; the body is not
/// copied here — [`SerializedEntity::raw`] borrows it from the fragment and
/// the container writer emits it after this header.
pub fn create_raw_media_header() -> Vec<u8> {
    // Entity header ION: {$410:0, $411:0}
    let header_fields = vec![
        (KfxSymbol::Bccomprtype as u64, IonValue::Int(0)),
        (KfxSymbol::Bcdrmscheme as u64, IonValue::Int(0)),
    ];

    let mut header_writer = IonWriter::new();
    header_writer.write_bvm();
    header_writer.write_value(&IonValue::Struct(header_fields));
    let header_ion = header_writer.into_bytes();

    // ENTY header: magic(4) + version(2) + header_len(4) = 10
    let header_len = 10 + header_ion.len();

    let mut data = Vec::with_capacity(header_len);
    data.extend_from_slice(ENTITY_MAGIC);
    data.extend_from_slice(&1u16.to_le_bytes()); // version
    data.extend_from_slice(&(header_len as u32).to_le_bytes());
    data.extend_from_slice(&header_ion);

    data
}

/// Serialize an annotated Ion value (for $ion_symbol_table and $593).
pub fn serialize_annotated_ion(annotation_id: u64, value: &IonValue) -> Vec<u8> {
    let annotated = IonValue::Annotated(vec![annotation_id], Box::new(value.clone()));

    let mut writer = IonWriter::new();
    writer.write_bvm();
    writer.write_value(&annotated);
    writer.into_bytes()
}

/// Generate a container ID deterministically from a seed string.
///
/// The same seed always yields the same ID, so converting the same book
/// twice produces byte-identical output (reproducible builds). Distinct
/// books get distinct IDs via the SHA-1 of their identifier — this was
/// previously clock-seeded, which both broke reproducibility and could
/// collide for two exports in the same clock tick.
pub fn generate_container_id(seed: &str) -> String {
    let digest = sha1_smol::Sha1::from(seed.as_bytes()).digest().bytes();
    let mut state = u128::from_be_bytes(digest[..16].try_into().expect("sha1 digest >= 16 bytes"));

    const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut id = String::from("CR!");

    for _ in 0..28 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = ((state >> 56) as usize) % CHARS.len();
        id.push(CHARS[idx] as char);
    }

    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_id_format() {
        let id = generate_container_id("test-seed");
        assert!(id.starts_with("CR!"));
        assert_eq!(id.len(), 31); // CR! + 28 chars

        // Verify all characters after "CR!" are valid (alphanumeric uppercase)
        let suffix = &id[3..];
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
            "Container ID should only contain uppercase alphanumeric: {}",
            id
        );
    }

    #[test]
    fn test_create_entity_data() {
        let value = IonValue::Struct(vec![(
            KfxSymbol::Title as u64,
            IonValue::String("Test".into()),
        )]);
        let data = create_entity_data(&value);

        // Should start with ENTY magic
        assert_eq!(&data[..4], b"ENTY");
        // Version should be 1
        assert_eq!(u16::from_le_bytes([data[4], data[5]]), 1);
    }

    #[test]
    fn test_create_raw_media_header() {
        let header = create_raw_media_header();

        // Should start with ENTY magic
        assert_eq!(&header[..4], b"ENTY");
        // Declared header length covers exactly the returned bytes; the raw
        // body is appended separately by the container writer.
        let header_len = u32::from_le_bytes([header[6], header[7], header[8], header[9]]);
        assert_eq!(header_len as usize, header.len());
    }

    #[test]
    fn test_serialize_annotated_ion() {
        let value = IonValue::List(vec![IonValue::String("symbol1".into())]);
        let data = serialize_annotated_ion(3, &value); // $ion_symbol_table

        // Should start with Ion BVM
        assert_eq!(&data[..4], &[0xe0, 0x01, 0x00, 0xea]);
    }
}
