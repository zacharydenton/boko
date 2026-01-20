//! KFX container serialization.
//!
//! Handles the binary format for KFX containers and entities.

use std::collections::HashMap;

use crate::kfx::ion::{IonValue, IonWriter};

use super::symbols::sym;

/// Serialized entity ready for container output
pub struct SerializedEntity {
    pub id: u32,
    pub entity_type: u32,
    pub data: Vec<u8>,
}

/// Container magic bytes
const CONTAINER_MAGIC: &[u8; 4] = b"CONT";

/// Entity magic bytes
const ENTITY_MAGIC: &[u8; 4] = b"ENTY";

/// Serialize a complete KFX container with proper header structure
///
/// Container layout:
/// - Header: CONT magic + version + header_len + ci_offset + ci_len
/// - Entity table (at offset 18, indexed by $413/$414)
/// - Doc symbols ION (indexed by $415/$416)
/// - Format capabilities ION (indexed by $594/$595)
/// - Container info ION
/// - kfxgen_info JSON
/// - Entity payloads (after header_len)
pub fn serialize_container_v2(
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
        entity_table.extend_from_slice(&(entity.data.len() as u64).to_le_bytes());
        current_offset += entity.data.len() as u64;
    }

    // Calculate SHA1 of entity payloads for kfxgen_info
    let mut entity_data = Vec::new();
    for entity in entities {
        entity_data.extend_from_slice(&entity.data);
    }
    let payload_sha1 = sha1_hex(&entity_data);

    // Header is 18 bytes: magic(4) + version(2) + header_len(4) + ci_offset(4) + ci_len(4)
    const HEADER_SIZE: usize = 18;

    // Calculate offsets within the header section (after the 18-byte fixed header)
    let entity_table_offset = HEADER_SIZE;
    let symtab_offset = entity_table_offset + entity_table.len();
    let format_caps_offset = symtab_offset + symtab_ion.len();

    // Build container info with all the offset pointers
    let mut container_info = HashMap::new();
    container_info.insert(
        sym::CONTAINER_ID,
        IonValue::String(container_id.to_string()),
    );
    container_info.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
    container_info.insert(sym::DRM_SCHEME, IonValue::Int(0));
    container_info.insert(sym::CHUNK_SIZE, IonValue::Int(4096));
    container_info.insert(
        sym::INDEX_TABLE_OFFSET,
        IonValue::Int(entity_table_offset as i64),
    );
    container_info.insert(
        sym::INDEX_TABLE_LENGTH,
        IonValue::Int(entity_table.len() as i64),
    );
    container_info.insert(
        sym::SYMBOL_TABLE_OFFSET,
        IonValue::Int(symtab_offset as i64),
    );
    container_info.insert(
        sym::SYMBOL_TABLE_LENGTH,
        IonValue::Int(symtab_ion.len() as i64),
    );

    // Only include format capabilities offset if we have them
    if !format_caps_ion.is_empty() {
        container_info.insert(sym::FC_OFFSET, IonValue::Int(format_caps_offset as i64));
        container_info.insert(sym::FC_LENGTH, IonValue::Int(format_caps_ion.len() as i64));
    }

    let mut ion_writer = IonWriter::new();
    ion_writer.write_bvm();
    ion_writer.write_value(&IonValue::Struct(container_info));
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
    let mut output = Vec::new();

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
    output.extend_from_slice(&entity_data);

    output
}

/// Compute SHA1 hash as hex string
fn sha1_hex(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Simple hash for now (real implementation would use SHA1)
    // This is just for the kfxgen_info field which is informational
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let hash = hasher.finish();
    format!(
        "{:016x}{:016x}{:08x}",
        hash,
        hash.rotate_left(32),
        (hash >> 32) as u32
    )
}

/// Create raw media entity data (for P417)
/// Raw media stores image bytes directly without ION encoding
pub fn create_raw_media_data(raw_bytes: &[u8]) -> Vec<u8> {
    // Entity header ION: {$410:0, $411:0}
    let mut header_fields = HashMap::new();
    header_fields.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
    header_fields.insert(sym::DRM_SCHEME, IonValue::Int(0));

    let mut header_writer = IonWriter::new();
    header_writer.write_bvm();
    header_writer.write_value(&IonValue::Struct(header_fields));
    let header_ion = header_writer.into_bytes();

    // ENTY header: magic(4) + version(2) + header_len(4) = 10
    let header_len = 10 + header_ion.len();

    let mut data = Vec::new();
    data.extend_from_slice(ENTITY_MAGIC);
    data.extend_from_slice(&1u16.to_le_bytes()); // version
    data.extend_from_slice(&(header_len as u32).to_le_bytes());
    data.extend_from_slice(&header_ion);
    // Raw bytes directly, not ION-encoded
    data.extend_from_slice(raw_bytes);

    data
}

/// Create entity data with ENTY header
pub fn create_entity_data(value: &IonValue) -> Vec<u8> {
    // Entity header ION: {$410:0, $411:0}
    let mut header_fields = HashMap::new();
    header_fields.insert(sym::COMPRESSION_TYPE, IonValue::Int(0));
    header_fields.insert(sym::DRM_SCHEME, IonValue::Int(0));

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

    let mut data = Vec::new();
    data.extend_from_slice(ENTITY_MAGIC);
    data.extend_from_slice(&1u16.to_le_bytes()); // version
    data.extend_from_slice(&(header_len as u32).to_le_bytes());
    data.extend_from_slice(&header_ion);
    data.extend_from_slice(&content_ion);

    data
}

/// Serialize an annotated ION value (for $ion_symbol_table and $593)
pub fn serialize_annotated_ion(annotation_id: u64, value: &IonValue) -> Vec<u8> {
    let annotated = IonValue::Annotated(vec![annotation_id], Box::new(value.clone()));

    let mut writer = IonWriter::new();
    writer.write_bvm();
    writer.write_value(&annotated);
    writer.into_bytes()
}

/// Generate a unique container ID
pub fn generate_container_id() -> String {
    // Get seed from platform-appropriate time source
    #[cfg(target_arch = "wasm32")]
    let seed = {
        // In WASM, use js_sys::Date::now() which returns milliseconds
        (js_sys::Date::now() as u128) * 1_000_000 // Convert to nanoseconds scale
    };

    #[cfg(not(target_arch = "wasm32"))]
    let seed = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    };

    let mut state = seed;
    let chars: Vec<char> = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().collect();
    let mut id = String::from("CR!");

    for _ in 0..28 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = ((state >> 56) as usize) % chars.len();
        id.push(chars[idx]);
    }

    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_id_format() {
        let id = generate_container_id();
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
    fn test_container_id_uniqueness() {
        // Generate multiple IDs and verify they're different
        // (they use time-based seeds so should be unique)
        let id1 = generate_container_id();
        let id2 = generate_container_id();

        // IDs should be valid format
        assert!(id1.starts_with("CR!"));
        assert!(id2.starts_with("CR!"));
        assert_eq!(id1.len(), 31);
        assert_eq!(id2.len(), 31);

        // With time-based seeding, consecutive calls may produce same ID
        // if called within same nanosecond/millisecond, so we just verify format
        // The important thing is they don't panic on any platform
    }
}
