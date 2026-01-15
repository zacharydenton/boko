//! KF8 Index parsing (INDX, TAGX, CNCX)
//!
//! KF8 files use several index tables:
//! - Skeleton index: file entries (HTML parts)
//! - Div index: content chunks to insert into skeletons
//! - NCX index: table of contents
//! - Other index: guide entries (cover, toc, etc.)

use std::collections::HashMap;

use crate::error::{Error, Result};

/// Variable-width integer decoding (forward)
/// Each byte uses 7 bits for data, high bit indicates continuation
pub fn decint(data: &[u8]) -> (u32, usize) {
    let mut val: u32 = 0;
    let mut consumed = 0;

    for &byte in data {
        consumed += 1;
        val = (val << 7) | ((byte & 0x7F) as u32);
        if byte & 0x80 != 0 {
            break;
        }
    }

    (val, consumed)
}

/// Count number of bits set in a value
fn count_set_bits(mut n: u8) -> u8 {
    let mut count = 0;
    while n > 0 {
        count += n & 1;
        n >>= 1;
    }
    count
}

/// TAGX entry: defines how to interpret index entries
#[derive(Debug, Clone)]
pub struct TagXEntry {
    pub tag: u8,
    pub num_values: u8,
    pub bitmask: u8,
    pub eof: u8,
}

/// Parsed INDX header
#[derive(Debug)]
pub struct IndxHeader {
    pub header_type: u32,
    pub idxt_start: u32,
    pub entry_count: u32,
    pub encoding: u32,
    pub total_entries: u32,
    pub ordt_offset: u32,
    pub ligt_offset: u32,
    pub num_ligt: u32,
    pub num_cncx: u32,
    pub tagx_offset: u32,
}

impl IndxHeader {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 192 || &data[0..4] != b"INDX" {
            return Err(Error::InvalidMobi("Invalid INDX header".into()));
        }

        let u32_at = |offset: usize| -> u32 {
            u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
        };

        Ok(Self {
            header_type: u32_at(8),
            idxt_start: u32_at(20),
            entry_count: u32_at(24),
            encoding: u32_at(28),
            total_entries: u32_at(36),
            ordt_offset: u32_at(40),
            ligt_offset: u32_at(44),
            num_ligt: u32_at(48),
            num_cncx: u32_at(52),
            tagx_offset: u32_at(180),
        })
    }
}

/// Parse TAGX section from index header
pub fn parse_tagx(data: &[u8]) -> Result<(u32, Vec<TagXEntry>)> {
    if data.len() < 12 || &data[0..4] != b"TAGX" {
        return Err(Error::InvalidMobi("Invalid TAGX section".into()));
    }

    let first_entry_offset = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let control_byte_count = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

    let mut tags = Vec::new();
    let mut i = 12;
    while i + 4 <= first_entry_offset as usize && i + 4 <= data.len() {
        tags.push(TagXEntry {
            tag: data[i],
            num_values: data[i + 1],
            bitmask: data[i + 2],
            eof: data[i + 3],
        });
        i += 4;
    }

    Ok((control_byte_count, tags))
}

/// Extract tag values from an index entry
pub fn get_tag_map(
    control_byte_count: u32,
    tagx: &[TagXEntry],
    data: &[u8],
) -> HashMap<u8, Vec<u32>> {
    let mut result: HashMap<u8, Vec<u32>> = HashMap::new();

    if data.len() < control_byte_count as usize {
        return result;
    }

    let control_bytes: Vec<u8> = data[..control_byte_count as usize].to_vec();
    let mut pos = control_byte_count as usize;
    let mut control_idx = 0;

    // First pass: determine which tags are present and their counts
    struct PendingTag {
        tag: u8,
        value_count: Option<u32>,
        value_bytes: Option<u32>,
        num_values: u8,
    }

    let mut pending: Vec<PendingTag> = Vec::new();

    for entry in tagx {
        if entry.eof == 0x01 {
            control_idx += 1;
            continue;
        }

        if control_idx >= control_bytes.len() {
            break;
        }

        let value = control_bytes[control_idx] & entry.bitmask;
        if value != 0 {
            let (value_count, value_bytes) = if value == entry.bitmask {
                if count_set_bits(entry.bitmask) > 1 {
                    // Variable width value follows
                    let (vb, consumed) = decint(&data[pos..]);
                    pos += consumed;
                    (None, Some(vb))
                } else {
                    (Some(1), None)
                }
            } else {
                // Shift to get actual count
                let mut mask = entry.bitmask;
                let mut shifted_value = value;
                while mask & 1 == 0 {
                    mask >>= 1;
                    shifted_value >>= 1;
                }
                (Some(shifted_value as u32), None)
            };

            pending.push(PendingTag {
                tag: entry.tag,
                value_count,
                value_bytes,
                num_values: entry.num_values,
            });
        }
    }

    // Second pass: read actual values
    for p in pending {
        let mut values = Vec::new();

        if let Some(count) = p.value_count {
            for _ in 0..(count * p.num_values as u32) {
                if pos >= data.len() {
                    break;
                }
                let (v, consumed) = decint(&data[pos..]);
                pos += consumed;
                values.push(v);
            }
        } else if let Some(bytes) = p.value_bytes {
            let mut consumed_total = 0;
            while consumed_total < bytes as usize && pos < data.len() {
                let (v, consumed) = decint(&data[pos..]);
                pos += consumed;
                consumed_total += consumed;
                values.push(v);
            }
        }

        result.insert(p.tag, values);
    }

    result
}

/// Decode a length-prefixed string
pub fn decode_string(data: &[u8], codec: &str) -> (String, usize) {
    if data.is_empty() {
        return (String::new(), 0);
    }

    let length = data[0] as usize;
    if length == 0 || data.len() < length + 1 {
        return (String::new(), 1);
    }

    let bytes = &data[1..1 + length];
    let s = match codec {
        "utf-8" | "UTF-8" => String::from_utf8_lossy(bytes).to_string(),
        _ => String::from_utf8_lossy(bytes).to_string(), // TODO: proper CP1252
    };

    (s, length + 1)
}

/// CNCX (Compiled NCX) - string table for index entries
#[derive(Debug, Default)]
pub struct Cncx {
    pub strings: HashMap<u32, String>,
}

impl Cncx {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse CNCX records
    pub fn parse(records: &[Vec<u8>], codec: &str) -> Self {
        let mut strings = HashMap::new();
        let mut record_offset: u32 = 0;

        for raw in records {
            let mut pos = 0;
            while pos < raw.len() {
                let (length, consumed) = decint(&raw[pos..]);
                if length > 0 && pos + consumed + length as usize <= raw.len() {
                    let bytes = &raw[pos + consumed..pos + consumed + length as usize];
                    let s = match codec {
                        "utf-8" | "UTF-8" => String::from_utf8_lossy(bytes).to_string(),
                        _ => String::from_utf8_lossy(bytes).to_string(),
                    };
                    strings.insert((pos as u32) + record_offset, s);
                }
                pos += consumed + length as usize;
            }
            record_offset += 0x10000;
        }

        Self { strings }
    }

    pub fn get(&self, offset: u32) -> Option<&String> {
        self.strings.get(&offset)
    }
}

/// Index table entry (generic)
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub name: String,
    pub tags: HashMap<u8, Vec<u32>>,
}

/// Read a complete index table
pub fn read_index(
    read_record: &mut dyn FnMut(usize) -> Result<Vec<u8>>,
    index_record: usize,
    codec: &str,
) -> Result<(Vec<IndexEntry>, Cncx)> {
    let header_data = read_record(index_record)?;
    let header = IndxHeader::parse(&header_data)?;

    // Find TAGX section
    let tagx_start = if header.tagx_offset > 0 && header.tagx_offset < header_data.len() as u32 {
        header.tagx_offset as usize
    } else {
        // Search for TAGX
        header_data
            .windows(4)
            .position(|w| w == b"TAGX")
            .ok_or_else(|| Error::InvalidMobi("TAGX not found".into()))?
    };

    let (control_byte_count, tagx) = parse_tagx(&header_data[tagx_start..])?;

    // Parse CNCX records if present
    let cncx = if header.num_cncx > 0 {
        let cncx_start = index_record + header.entry_count as usize + 1;
        let mut cncx_records = Vec::new();
        for i in 0..header.num_cncx as usize {
            if let Ok(rec) = read_record(cncx_start + i) {
                cncx_records.push(rec);
            }
        }
        Cncx::parse(&cncx_records, codec)
    } else {
        Cncx::new()
    };

    // Parse index entries from subsequent records
    let mut entries = Vec::new();

    for i in 0..header.entry_count as usize {
        let rec_data = read_record(index_record + 1 + i)?;
        let rec_header = IndxHeader::parse(&rec_data)?;

        // Find IDXT position table
        let idxt_pos = rec_header.idxt_start as usize;
        if idxt_pos + 4 > rec_data.len() || &rec_data[idxt_pos..idxt_pos + 4] != b"IDXT" {
            continue;
        }

        // Read entry positions
        let mut positions: Vec<usize> = Vec::new();
        for j in 0..rec_header.entry_count as usize {
            let off = idxt_pos + 4 + j * 2;
            if off + 2 <= rec_data.len() {
                let pos = u16::from_be_bytes([rec_data[off], rec_data[off + 1]]) as usize;
                positions.push(pos);
            }
        }
        positions.push(idxt_pos); // Last entry ends at IDXT

        // Parse each entry
        for j in 0..positions.len().saturating_sub(1) {
            let start = positions[j];
            let end = positions[j + 1];
            if start >= end || start >= rec_data.len() {
                continue;
            }

            let entry_data = &rec_data[start..end];
            let (name, consumed) = decode_string(entry_data, codec);
            let tag_data = &entry_data[consumed..];
            let tags = get_tag_map(control_byte_count, &tagx, tag_data);

            entries.push(IndexEntry { name, tags });
        }
    }

    Ok((entries, cncx))
}

/// Skeleton file entry from skel index
#[derive(Debug, Clone)]
pub struct SkeletonFile {
    pub file_number: usize,
    pub name: String,
    pub div_count: u32,
    pub start_pos: u32,
    pub length: u32,
}

/// Div element entry from div index
#[derive(Debug, Clone)]
pub struct DivElement {
    pub insert_pos: u32,
    pub toc_text: Option<String>,
    pub file_number: u32,
    pub sequence_number: u32,
    pub start_pos: u32,
    pub length: u32,
}

/// NCX entry for table of contents
#[derive(Debug, Clone)]
pub struct NcxEntry {
    pub name: String,
    pub text: String,
    pub pos: u32,
    pub length: u32,
    pub level: i32,
    pub parent: i32,
    pub pos_fid: Option<(u32, u32)>,
}

/// Parse skeleton index into file entries
pub fn parse_skel_index(entries: &[IndexEntry]) -> Vec<SkeletonFile> {
    let mut files = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        // Tag 1 = div count, Tag 6 = [start_pos, length]
        let div_count = entry.tags.get(&1).and_then(|v| v.first()).copied().unwrap_or(0);
        let (start_pos, length) = entry
            .tags
            .get(&6)
            .map(|v| (v.get(0).copied().unwrap_or(0), v.get(1).copied().unwrap_or(0)))
            .unwrap_or((0, 0));

        files.push(SkeletonFile {
            file_number: i,
            name: entry.name.clone(),
            div_count,
            start_pos,
            length,
        });
    }

    files
}

/// Parse div index into element entries
pub fn parse_div_index(entries: &[IndexEntry], cncx: &Cncx) -> Vec<DivElement> {
    let mut elems = Vec::new();

    for entry in entries {
        // Parse insert_pos from entry name (it's the numeric identifier)
        let insert_pos = entry.name.parse().unwrap_or(0);

        // Tag 2 = cncx offset for toc text
        let toc_text = entry
            .tags
            .get(&2)
            .and_then(|v| v.first())
            .and_then(|&off| cncx.get(off).cloned());

        // Tag 3 = file number
        let file_number = entry.tags.get(&3).and_then(|v| v.first()).copied().unwrap_or(0);

        // Tag 4 = sequence number
        let sequence_number = entry.tags.get(&4).and_then(|v| v.first()).copied().unwrap_or(0);

        // Tag 6 = [start_pos, length]
        let (start_pos, length) = entry
            .tags
            .get(&6)
            .map(|v| (v.get(0).copied().unwrap_or(0), v.get(1).copied().unwrap_or(0)))
            .unwrap_or((0, 0));

        elems.push(DivElement {
            insert_pos,
            toc_text,
            file_number,
            sequence_number,
            start_pos,
            length,
        });
    }

    elems
}

/// Parse NCX index into TOC entries
pub fn parse_ncx_index(entries: &[IndexEntry], cncx: &Cncx) -> Vec<NcxEntry> {
    let mut ncx_entries = Vec::new();

    for entry in entries {
        // Tag 1 = position
        let pos = entry.tags.get(&1).and_then(|v| v.first()).copied().unwrap_or(0);

        // Tag 2 = length
        let length = entry.tags.get(&2).and_then(|v| v.first()).copied().unwrap_or(0);

        // Tag 3 = cncx offset for text
        let text = entry
            .tags
            .get(&3)
            .and_then(|v| v.first())
            .and_then(|&off| cncx.get(off).cloned())
            .unwrap_or_else(|| entry.name.clone());

        // Tag 4 = hierarchy level
        let level = entry
            .tags
            .get(&4)
            .and_then(|v| v.first())
            .map(|&v| v as i32)
            .unwrap_or(-1);

        // Tag 6 = pos_fid (file index, offset)
        let pos_fid = entry.tags.get(&6).map(|v| {
            (v.get(0).copied().unwrap_or(0), v.get(1).copied().unwrap_or(0))
        });

        // Tag 21 = parent index
        let parent = entry
            .tags
            .get(&21)
            .and_then(|v| v.first())
            .map(|&v| v as i32)
            .unwrap_or(-1);

        ncx_entries.push(NcxEntry {
            name: entry.name.clone(),
            text,
            pos,
            length,
            level,
            parent,
            pos_fid,
        });
    }

    ncx_entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decint() {
        // Single byte (high bit set = end)
        assert_eq!(decint(&[0x85]), (5, 1));
        // Two bytes: 0x01 (continue) 0x80 (end, value 0) = 128
        assert_eq!(decint(&[0x01, 0x80]), (128, 2));
        // Value 127
        assert_eq!(decint(&[0xFF]), (127, 1));
    }

    #[test]
    fn test_count_set_bits() {
        assert_eq!(count_set_bits(0), 0);
        assert_eq!(count_set_bits(1), 1);
        assert_eq!(count_set_bits(0b1010), 2);
        assert_eq!(count_set_bits(0xFF), 8);
    }
}
