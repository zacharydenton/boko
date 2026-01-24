//! Test helpers for KFX parsing.
//!
//! These utilities are used by both unit tests and integration tests
//! for verifying KFX container structure.

use std::collections::HashMap;

use super::ion::{IonParser, IonValue};

/// Parse KFX container and extract entities by type.
/// Returns map of entity_type -> [(id, payload)]
pub fn parse_kfx_container(data: &[u8]) -> HashMap<u32, Vec<(u32, Vec<u8>)>> {
    let mut entities = HashMap::new();
    if data.len() < 18 || &data[0..4] != b"CONT" {
        return entities;
    }

    let header_len = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
    let ion_magic: [u8; 4] = [0xE0, 0x01, 0x00, 0xEA];
    let mut pos = 18;

    while pos + 24 <= data.len() && data[pos..pos + 4] != ion_magic {
        let id = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
        let etype = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap());
        let offset = u64::from_le_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize;
        let length = u64::from_le_bytes(data[pos + 16..pos + 24].try_into().unwrap()) as usize;

        let start = header_len + offset;
        if start + length <= data.len() {
            entities
                .entry(etype)
                .or_insert_with(Vec::new)
                .push((id, data[start..start + length].to_vec()));
        }
        pos += 24;
    }
    entities
}

/// Parse entity payload to ION (skips ENTY header)
pub fn parse_entity_ion(payload: &[u8]) -> Option<IonValue> {
    if payload.len() < 10 || &payload[0..4] != b"ENTY" {
        return None;
    }
    let header_len = u32::from_le_bytes(payload[6..10].try_into().unwrap()) as usize;
    if header_len >= payload.len() {
        return None;
    }
    IonParser::new(&payload[header_len..]).parse().ok()
}
