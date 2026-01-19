//! KF8 Index parsing (INDX, TAGX, CNCX)
//!
//! KF8 files use several index tables:
//! - Skeleton index: file entries (HTML parts)
//! - Div index: content chunks to insert into skeletons
//! - NCX index: table of contents
//! - Other index: guide entries (cover, toc, etc.)

use std::collections::HashMap;

use std::io;

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
#[allow(dead_code)] // Fields are part of MOBI format spec
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
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        if data.len() < 192 || &data[0..4] != b"INDX" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid INDX header",
            ));
        }

        let u32_at = |offset: usize| -> u32 {
            u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
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
pub fn parse_tagx(data: &[u8]) -> io::Result<(u32, Vec<TagXEntry>)> {
    if data.len() < 12 || &data[0..4] != b"TAGX" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid TAGX section",
        ));
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
    read_record: &mut dyn FnMut(usize) -> io::Result<Vec<u8>>,
    index_record: usize,
    codec: &str,
) -> io::Result<(Vec<IndexEntry>, Cncx)> {
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
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TAGX not found"))?
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
#[allow(dead_code)] // Fields are part of MOBI format spec
pub struct SkeletonFile {
    pub file_number: usize,
    pub name: String,
    pub div_count: u32,
    pub start_pos: u32,
    pub length: u32,
}

/// Div element entry from div index
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are part of MOBI format spec
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
#[allow(dead_code)] // Fields are part of MOBI format spec
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
        let div_count = entry
            .tags
            .get(&1)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(0);
        let (start_pos, length) = entry
            .tags
            .get(&6)
            .map(|v| {
                (
                    v.first().copied().unwrap_or(0),
                    v.get(1).copied().unwrap_or(0),
                )
            })
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
        let insert_pos: u32 = entry.name.parse().unwrap_or(0);

        // Tag 2 = cncx offset for toc text
        let toc_text = entry
            .tags
            .get(&2)
            .and_then(|v| v.first())
            .and_then(|&off| cncx.get(off).cloned());

        // Tag 3 = file number
        let file_number = entry
            .tags
            .get(&3)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(0);

        // Tag 4 = sequence number
        let sequence_number = entry
            .tags
            .get(&4)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(0);

        // Tag 6 = [start_pos, length]
        let (start_pos, length) = entry
            .tags
            .get(&6)
            .map(|v| {
                (
                    v.first().copied().unwrap_or(0),
                    v.get(1).copied().unwrap_or(0),
                )
            })
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
        let pos = entry
            .tags
            .get(&1)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(0);

        // Tag 2 = length
        let length = entry
            .tags
            .get(&2)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(0);

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
            (
                v.first().copied().unwrap_or(0),
                v.get(1).copied().unwrap_or(0),
            )
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

// ============================================================================
// INDX Record Generation (for writing KF8 files)
// ============================================================================

/// Encode a variable-width integer (forward encoding, high bit set on last byte)
pub fn encint(val: u32) -> Vec<u8> {
    if val == 0 {
        return vec![0x80];
    }

    let mut result = Vec::new();
    let mut v = val;
    while v > 0 {
        result.push((v & 0x7F) as u8);
        v >>= 7;
    }

    // Set high bit on first byte (which becomes last after reverse)
    if let Some(first) = result.first_mut() {
        *first |= 0x80;
    }

    result.reverse();
    result
}

/// Tag definition for TAGX section
#[derive(Debug, Clone, Copy)]
pub struct TagDef {
    pub tag: u8,
    pub values_per_entry: u8,
    pub bitmask: u8,
    pub eof: u8,
}

/// INDX record builder
pub struct IndxBuilder {
    entries: Vec<(String, Vec<u8>)>, // (name, encoded_data)
    tagx: Vec<TagDef>,
    control_byte_count: u8,
    num_cncx: u32,
}

impl IndxBuilder {
    pub fn new(tagx: Vec<TagDef>, control_byte_count: u8) -> Self {
        Self {
            entries: Vec::new(),
            tagx,
            control_byte_count,
            num_cncx: 0,
        }
    }

    pub fn set_cncx_count(&mut self, count: u32) {
        self.num_cncx = count;
    }

    /// Add an entry with pre-encoded tag data
    pub fn add_entry(&mut self, name: String, tag_data: Vec<u8>) {
        self.entries.push((name, tag_data));
    }

    /// Build the INDX record(s)
    pub fn build(&self) -> Vec<Vec<u8>> {
        if self.entries.is_empty() {
            return vec![self.build_header_record(0, 0)];
        }

        // For simplicity, put all entries in one record (works for small indices)
        let (entry_data, idxt_offsets) = self.build_entries();
        let idxt = self.build_idxt(&idxt_offsets, entry_data.len());

        // Build the single data record
        let data_record = self.build_data_record(&entry_data, &idxt);

        // Build header record
        let header_record = self.build_header_record(self.entries.len() as u32, 1);

        vec![header_record, data_record]
    }

    fn build_header_record(&self, total_entries: u32, num_records: u32) -> Vec<u8> {
        let mut record = Vec::new();

        // INDX signature (offset 0)
        record.extend_from_slice(b"INDX");

        // Fields follow Calibre's INDEX_HEADER_FIELDS order:
        // len, nul1, type, gen, start, count, code, lng, total, ordt, ligt, nligt, ncncx

        // len: Header length (offset 4)
        record.extend_from_slice(&192u32.to_be_bytes());

        // nul1: Unknown/zero (offset 8)
        record.extend_from_slice(&0u32.to_be_bytes());

        // type: Index type (offset 12) - 2 = inflection/KF8
        record.extend_from_slice(&2u32.to_be_bytes());

        // gen: Generation/unknown (offset 16)
        record.extend_from_slice(&0u32.to_be_bytes());

        // start: IDXT offset (offset 20) - 0 for header record
        record.extend_from_slice(&0u32.to_be_bytes());

        // count: Number of data records (offset 24)
        record.extend_from_slice(&num_records.to_be_bytes());

        // code: Encoding (offset 28) - 65001 = UTF-8
        record.extend_from_slice(&65001u32.to_be_bytes());

        // lng: Language (offset 32)
        record.extend_from_slice(&0u32.to_be_bytes());

        // total: Total entries across all records (offset 36)
        record.extend_from_slice(&total_entries.to_be_bytes());

        // ordt: ORDT offset (offset 40)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ligt: LIGT offset (offset 44)
        record.extend_from_slice(&0u32.to_be_bytes());

        // nligt: Number of LIGT entries (offset 48)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ncncx: Number of CNCX records (offset 52)
        record.extend_from_slice(&self.num_cncx.to_be_bytes());

        // Unknown fields (27 u32s = 108 bytes) (offset 56-163)
        record.extend_from_slice(&[0u8; 108]);

        // ocnt (offset 164)
        record.extend_from_slice(&0u32.to_be_bytes());

        // oentries (offset 168)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ordt1 (offset 172)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ordt2 (offset 176)
        record.extend_from_slice(&0u32.to_be_bytes());

        // tagx: TAGX offset (offset 180) - points to TAGX after header
        record.extend_from_slice(&192u32.to_be_bytes());

        // Padding to reach 192 bytes (offsets 184-191)
        record.extend_from_slice(&[0u8; 8]);

        // TAGX section (after 192-byte header)
        record.extend_from_slice(&self.build_tagx());

        record
    }

    fn build_data_record(&self, entry_data: &[u8], idxt: &[u8]) -> Vec<u8> {
        let mut record = Vec::new();

        // Calculate IDXT offset
        let idxt_offset = 192 + entry_data.len();

        // INDX signature (offset 0)
        record.extend_from_slice(b"INDX");

        // Fields follow Calibre's INDEX_HEADER_FIELDS order:
        // len, nul1, type, gen, start, count, code, lng, total, ordt, ligt, nligt, ncncx

        // len: Header length (offset 4)
        record.extend_from_slice(&192u32.to_be_bytes());

        // nul1: Unknown/zero (offset 8)
        record.extend_from_slice(&0u32.to_be_bytes());

        // type: Index type (offset 12)
        record.extend_from_slice(&2u32.to_be_bytes());

        // gen: Generation/unknown (offset 16)
        record.extend_from_slice(&0u32.to_be_bytes());

        // start: IDXT offset (offset 20)
        record.extend_from_slice(&(idxt_offset as u32).to_be_bytes());

        // count: Number of entries in this record (offset 24)
        record.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());

        // code: Encoding (offset 28) - 65001 = UTF-8
        record.extend_from_slice(&65001u32.to_be_bytes());

        // lng: Language (offset 32)
        record.extend_from_slice(&0u32.to_be_bytes());

        // total: Total entries (offset 36)
        record.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());

        // ordt: ORDT offset (offset 40)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ligt: LIGT offset (offset 44)
        record.extend_from_slice(&0u32.to_be_bytes());

        // nligt: Number of LIGT entries (offset 48)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ncncx: Number of CNCX records (offset 52)
        record.extend_from_slice(&self.num_cncx.to_be_bytes());

        // Unknown fields (27 u32s = 108 bytes) (offset 56-163)
        record.extend_from_slice(&[0u8; 108]);

        // ocnt (offset 164)
        record.extend_from_slice(&0u32.to_be_bytes());

        // oentries (offset 168)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ordt1 (offset 172)
        record.extend_from_slice(&0u32.to_be_bytes());

        // ordt2 (offset 176)
        record.extend_from_slice(&0u32.to_be_bytes());

        // tagx: TAGX offset (offset 180) - 0 for data records
        record.extend_from_slice(&0u32.to_be_bytes());

        // Padding to reach 192 bytes (offsets 184-191)
        record.extend_from_slice(&[0u8; 8]);

        // Entry data
        record.extend_from_slice(entry_data);

        // IDXT
        record.extend_from_slice(idxt);

        // Pad to 4-byte boundary
        while !record.len().is_multiple_of(4) {
            record.push(0);
        }

        record
    }

    fn build_tagx(&self) -> Vec<u8> {
        let mut tagx = Vec::new();

        // TAGX signature
        tagx.extend_from_slice(b"TAGX");

        // Block size: 12 + (4 * num_tags)
        let size = 12 + (4 * self.tagx.len()) as u32;
        tagx.extend_from_slice(&size.to_be_bytes());

        // Control byte count
        tagx.extend_from_slice(&(self.control_byte_count as u32).to_be_bytes());

        // Tag definitions
        for tag in &self.tagx {
            tagx.push(tag.tag);
            tagx.push(tag.values_per_entry);
            tagx.push(tag.bitmask);
            tagx.push(tag.eof);
        }

        tagx
    }

    fn build_entries(&self) -> (Vec<u8>, Vec<u16>) {
        let mut data = Vec::new();
        let mut offsets = Vec::new();

        for (name, tag_data) in &self.entries {
            offsets.push((192 + data.len()) as u16);

            let name_bytes = name.as_bytes();
            data.push(name_bytes.len() as u8);
            data.extend_from_slice(name_bytes);
            data.extend_from_slice(tag_data);
        }

        (data, offsets)
    }

    fn build_idxt(&self, offsets: &[u16], entry_data_len: usize) -> Vec<u8> {
        let mut idxt = Vec::new();

        // IDXT signature
        idxt.extend_from_slice(b"IDXT");

        // Entry offsets (2 bytes each, big-endian)
        for &offset in offsets {
            idxt.extend_from_slice(&offset.to_be_bytes());
        }

        // Add end-of-data offset (points to end of entry data, which is where IDXT starts)
        // This is required - Calibre's reader expects n+1 offsets for n entries
        let end_offset = (192 + entry_data_len) as u16;
        idxt.extend_from_slice(&end_offset.to_be_bytes());

        // Pad to 4-byte boundary
        while !idxt.len().is_multiple_of(4) {
            idxt.push(0);
        }

        idxt
    }
}

// Skeleton index tags
const SKEL_TAG_CHUNK_COUNT: TagDef = TagDef {
    tag: 1,
    values_per_entry: 1,
    bitmask: 0x03,
    eof: 0,
};
const SKEL_TAG_GEOMETRY: TagDef = TagDef {
    tag: 6,
    values_per_entry: 2,
    bitmask: 0x0C,
    eof: 0,
};
const SKEL_TAG_EOF: TagDef = TagDef {
    tag: 0,
    values_per_entry: 0,
    bitmask: 0x00,
    eof: 1,
};

/// Build skeleton index from skeleton entries
pub fn build_skel_indx(skeletons: &[super::skeleton::SkelEntry]) -> Vec<Vec<u8>> {
    let tagx = vec![SKEL_TAG_CHUNK_COUNT, SKEL_TAG_GEOMETRY, SKEL_TAG_EOF];
    let mut builder = IndxBuilder::new(tagx, 1);

    for skel in skeletons {
        // Control byte calculation per Calibre:
        // chunk_count: 2 values / vpe=1 = 2 entries. mask=3, shift=0. 3 & (2 << 0) = 2
        // geometry: 4 values / vpe=2 = 2 entries. mask=12, shift=2. 12 & (2 << 2) = 8
        // Total: 2 | 8 = 10 = 0x0A
        let mut tag_data = vec![0x0A];

        // Chunk count (repeated twice per Calibre implementation)
        tag_data.extend(encint(skel.chunk_count as u32));
        tag_data.extend(encint(skel.chunk_count as u32));

        // Geometry: start_pos, length (repeated twice)
        tag_data.extend(encint(skel.start_pos as u32));
        tag_data.extend(encint(skel.length as u32));
        tag_data.extend(encint(skel.start_pos as u32));
        tag_data.extend(encint(skel.length as u32));

        builder.add_entry(skel.name.clone(), tag_data);
    }

    builder.build()
}

// Chunk/Fragment index tags
const CHUNK_TAG_CNCX: TagDef = TagDef {
    tag: 2,
    values_per_entry: 1,
    bitmask: 0x01,
    eof: 0,
};
const CHUNK_TAG_FILE_NUM: TagDef = TagDef {
    tag: 3,
    values_per_entry: 1,
    bitmask: 0x02,
    eof: 0,
};
const CHUNK_TAG_SEQ_NUM: TagDef = TagDef {
    tag: 4,
    values_per_entry: 1,
    bitmask: 0x04,
    eof: 0,
};
const CHUNK_TAG_GEOMETRY: TagDef = TagDef {
    tag: 6,
    values_per_entry: 2,
    bitmask: 0x08,
    eof: 0,
};
const CHUNK_TAG_EOF: TagDef = TagDef {
    tag: 0,
    values_per_entry: 0,
    bitmask: 0x00,
    eof: 1,
};

/// Build CNCX record from chunk selectors
pub fn build_cncx(selectors: &[String]) -> Vec<u8> {
    let mut cncx = Vec::new();

    for selector in selectors {
        let bytes = selector.as_bytes();
        cncx.extend(encint(bytes.len() as u32));
        cncx.extend_from_slice(bytes);
    }

    cncx
}

/// Build fragment/chunk index from chunk entries
pub fn build_chunk_indx(
    chunks: &[super::skeleton::ChunkEntry],
    cncx_offsets: &[u32],
) -> Vec<Vec<u8>> {
    let tagx = vec![
        CHUNK_TAG_CNCX,
        CHUNK_TAG_FILE_NUM,
        CHUNK_TAG_SEQ_NUM,
        CHUNK_TAG_GEOMETRY,
        CHUNK_TAG_EOF,
    ];
    let mut builder = IndxBuilder::new(tagx, 1);

    if !chunks.is_empty() {
        builder.set_cncx_count(1); // We'll have one CNCX record
    }

    for (i, chunk) in chunks.iter().enumerate() {
        // Control byte: 0x0F = all tags present
        let mut tag_data = vec![0x0F];

        // CNCX offset for selector
        let cncx_offset = cncx_offsets.get(i).copied().unwrap_or(0);
        tag_data.extend(encint(cncx_offset));

        // File number
        tag_data.extend(encint(chunk.file_number as u32));

        // Sequence number
        tag_data.extend(encint(chunk.sequence_number as u32));

        // Geometry: start_pos, length
        tag_data.extend(encint(chunk.start_pos as u32));
        tag_data.extend(encint(chunk.length as u32));

        // Entry name is insert position as 10-digit string
        let name = format!("{:010}", chunk.insert_pos);
        builder.add_entry(name, tag_data);
    }

    builder.build()
}

/// Calculate CNCX offsets for a list of selectors
pub fn calculate_cncx_offsets(selectors: &[String]) -> Vec<u32> {
    let mut offsets = Vec::new();
    let mut offset: u32 = 0;

    for selector in selectors {
        offsets.push(offset);
        // Length is VWI encoded, but we need to account for the bytes
        let len_bytes = encint(selector.len() as u32);
        offset += len_bytes.len() as u32 + selector.len() as u32;
    }

    offsets
}

// NCX (Table of Contents) index tags
const NCX_TAG_OFFSET: TagDef = TagDef {
    tag: 1,
    values_per_entry: 1,
    bitmask: 0x01,
    eof: 0,
};
const NCX_TAG_LENGTH: TagDef = TagDef {
    tag: 2,
    values_per_entry: 1,
    bitmask: 0x02,
    eof: 0,
};
const NCX_TAG_LABEL: TagDef = TagDef {
    tag: 3,
    values_per_entry: 1,
    bitmask: 0x04,
    eof: 0,
};
const NCX_TAG_DEPTH: TagDef = TagDef {
    tag: 4,
    values_per_entry: 1,
    bitmask: 0x08,
    eof: 0,
};
const NCX_TAG_PARENT: TagDef = TagDef {
    tag: 21,
    values_per_entry: 1,
    bitmask: 0x10,
    eof: 0,
};
const NCX_TAG_FIRST_CHILD: TagDef = TagDef {
    tag: 22,
    values_per_entry: 1,
    bitmask: 0x20,
    eof: 0,
};
const NCX_TAG_LAST_CHILD: TagDef = TagDef {
    tag: 23,
    values_per_entry: 1,
    bitmask: 0x40,
    eof: 0,
};
const NCX_TAG_POS_FID: TagDef = TagDef {
    tag: 6,
    values_per_entry: 2,
    bitmask: 0x80,
    eof: 0,
};
const NCX_TAG_EOF: TagDef = TagDef {
    tag: 0,
    values_per_entry: 0,
    bitmask: 0x00,
    eof: 1,
};

/// NCX entry for building table of contents
#[derive(Debug, Clone)]
pub struct NcxBuildEntry {
    /// Position in text (byte offset)
    pub pos: u32,
    /// Length of the section
    pub length: u32,
    /// Label text (for CNCX)
    pub label: String,
    /// Depth level (0 = top level)
    pub depth: u32,
    /// Index of parent entry (-1 for root entries)
    pub parent: i32,
    /// Index of first child (-1 if no children)
    pub first_child: i32,
    /// Index of last child (-1 if no children)
    pub last_child: i32,
}

/// Build NCX index for table of contents
pub fn build_ncx_indx(entries: &[NcxBuildEntry]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let tagx = vec![
        NCX_TAG_OFFSET,
        NCX_TAG_LENGTH,
        NCX_TAG_LABEL,
        NCX_TAG_DEPTH,
        NCX_TAG_PARENT,
        NCX_TAG_FIRST_CHILD,
        NCX_TAG_LAST_CHILD,
        NCX_TAG_POS_FID,
        NCX_TAG_EOF,
    ];
    let mut builder = IndxBuilder::new(tagx, 2); // 2 control bytes for NCX

    // Build CNCX with labels
    let labels: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
    let label_offsets = calculate_cncx_offsets(&labels);
    let cncx = build_cncx(&labels);

    if !entries.is_empty() {
        builder.set_cncx_count(1);
    }

    // Build entries with hierarchy information
    for (i, entry) in entries.iter().enumerate() {
        // Control byte 0 encodes which tags are present via bitmasks:
        // offset=0x01, length=0x02, label=0x04, depth=0x08,
        // parent=0x10, first_child=0x20, last_child=0x40, pos_fid=0x80
        let mut ctrl: u8 = 0x0F; // Tags 1-4 (offset, length, label, depth) always present

        // Check which hierarchy tags are present and set their bits
        let has_parent = entry.parent >= 0;
        let has_first_child = entry.first_child >= 0;
        let has_last_child = entry.last_child >= 0;

        if has_parent {
            ctrl |= 0x10; // Tag 21 (parent)
        }
        if has_first_child {
            ctrl |= 0x20; // Tag 22 (first_child)
        }
        if has_last_child {
            ctrl |= 0x40; // Tag 23 (last_child)
        }

        // Control byte 1 is unused (0x00) since all our tags fit in byte 0
        let mut tag_data = vec![ctrl, 0x00];

        // Offset (tag 1)
        tag_data.extend(encint(entry.pos));

        // Length (tag 2)
        tag_data.extend(encint(entry.length));

        // Label (tag 3) - CNCX offset
        let label_offset = label_offsets.get(i).copied().unwrap_or(0);
        tag_data.extend(encint(label_offset));

        // Depth (tag 4)
        tag_data.extend(encint(entry.depth));

        // Parent (tag 21) - if present
        if has_parent {
            tag_data.extend(encint(entry.parent as u32));
        }

        // First child (tag 22) - if present
        if has_first_child {
            tag_data.extend(encint(entry.first_child as u32));
        }

        // Last child (tag 23) - if present
        if has_last_child {
            tag_data.extend(encint(entry.last_child as u32));
        }

        // Entry name is a 4-digit zero-padded index
        let name = format!("{i:04}");
        builder.add_entry(name, tag_data);
    }

    (builder.build(), cncx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encint() {
        // 0 encodes to 0x80
        assert_eq!(encint(0), vec![0x80]);
        // 1 encodes to 0x81
        assert_eq!(encint(1), vec![0x81]);
        // 127 encodes to 0xFF
        assert_eq!(encint(127), vec![0xFF]);
        // 128 encodes to 0x01, 0x80
        assert_eq!(encint(128), vec![0x01, 0x80]);
        // 255 encodes to 0x01, 0xFF
        assert_eq!(encint(255), vec![0x01, 0xFF]);
    }

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

    #[test]
    fn test_cncx_roundtrip() {
        // Test with strings containing special characters
        let labels = vec![
            "What's in This Book?".to_string(), // curly apostrophe
            "Don't Stop".to_string(),           // straight apostrophe
            "Simple Text".to_string(),
        ];

        // Build CNCX
        let cncx_bytes = build_cncx(&labels);
        let offsets = calculate_cncx_offsets(&labels);

        println!("CNCX bytes: {:?}", cncx_bytes);
        println!("Offsets: {:?}", offsets);

        // Parse it back
        let parsed = Cncx::parse(&[cncx_bytes], "utf-8");

        println!("Parsed strings: {:?}", parsed.strings);

        // Check each label can be retrieved
        for (i, label) in labels.iter().enumerate() {
            let offset = offsets[i];
            let retrieved = parsed.get(offset);
            println!(
                "Label {}: offset={}, expected='{}', got={:?}",
                i, offset, label, retrieved
            );
            assert_eq!(
                retrieved,
                Some(label),
                "Label '{}' at offset {} not found correctly",
                label,
                offset
            );
        }
    }

    #[test]
    fn test_ncx_index_roundtrip() {
        // Build NCX entries with special characters
        let entries = vec![
            NcxBuildEntry {
                pos: 0,
                length: 1000,
                label: "What's in This Book?".to_string(),
                depth: 0,
                parent: -1,
                first_child: -1,
                last_child: -1,
            },
            NcxBuildEntry {
                pos: 1000,
                length: 500,
                label: "Don't Stop".to_string(),
                depth: 0,
                parent: -1,
                first_child: -1,
                last_child: -1,
            },
        ];

        // Build the index
        let (ncx_records, ncx_cncx) = build_ncx_indx(&entries);

        println!(
            "Built {} NCX records + CNCX ({} bytes)",
            ncx_records.len(),
            ncx_cncx.len()
        );

        // We should have 2 records: header + data
        assert_eq!(ncx_records.len(), 2, "Should have header + data record");

        // Parse the header to get TAGX
        let header = IndxHeader::parse(&ncx_records[0]).expect("Failed to parse header");
        println!(
            "Header: entry_count={}, num_cncx={}",
            header.entry_count, header.num_cncx
        );

        // Find TAGX in header record
        let tagx_start = header.tagx_offset as usize;
        let (control_byte_count, tagx) =
            parse_tagx(&ncx_records[0][tagx_start..]).expect("Failed to parse TAGX");
        println!(
            "TAGX: {} control bytes, {} tags",
            control_byte_count,
            tagx.len()
        );

        // Parse CNCX
        let cncx = Cncx::parse(&[ncx_cncx], "utf-8");
        println!("CNCX strings: {:?}", cncx.strings);

        // Parse the data record
        let data_record = &ncx_records[1];
        let data_header = IndxHeader::parse(data_record).expect("Failed to parse data header");

        // Find IDXT
        let idxt_pos = data_header.idxt_start as usize;
        assert_eq!(
            &data_record[idxt_pos..idxt_pos + 4],
            b"IDXT",
            "IDXT not found"
        );

        // Read entry positions
        let mut positions: Vec<usize> = Vec::new();
        for j in 0..data_header.entry_count as usize {
            let off = idxt_pos + 4 + j * 2;
            let pos = u16::from_be_bytes([data_record[off], data_record[off + 1]]) as usize;
            positions.push(pos);
        }
        positions.push(idxt_pos);

        println!("Entry positions: {:?}", positions);

        // Parse each entry
        let mut index_entries = Vec::new();
        for j in 0..positions.len().saturating_sub(1) {
            let start = positions[j];
            let end = positions[j + 1];
            let entry_data = &data_record[start..end];

            println!(
                "Entry {} raw data ({} bytes): {:02x?}",
                j,
                entry_data.len(),
                entry_data
            );

            let (name, consumed) = decode_string(entry_data, "utf-8");
            let tag_data = &entry_data[consumed..];

            println!(
                "  Name: '{}', tag_data ({} bytes): {:02x?}",
                name,
                tag_data.len(),
                tag_data
            );

            let tags = get_tag_map(control_byte_count, &tagx, tag_data);
            println!("  Tags: {:?}", tags);

            index_entries.push(IndexEntry { name, tags });
        }

        // Now parse as NCX entries
        let ncx_entries = parse_ncx_index(&index_entries, &cncx);

        // Verify labels
        println!("\nParsed NCX entries:");
        for (i, ncx) in ncx_entries.iter().enumerate() {
            println!(
                "  {}: text='{}', pos={}, length={}",
                i, ncx.text, ncx.pos, ncx.length
            );
        }

        assert_eq!(ncx_entries.len(), 2, "Should have 2 NCX entries");
        assert_eq!(
            ncx_entries[0].text, "What's in This Book?",
            "First label should match"
        );
        assert_eq!(
            ncx_entries[1].text, "Don't Stop",
            "Second label should match"
        );
    }
}
