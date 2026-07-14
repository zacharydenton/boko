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
    let s = crate::util::decode_text(bytes, Some(codec)).into_owned();

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
            // `end` comes from the (untrusted) IDXT offset table and was never
            // checked against the record length, so an out-of-range value would
            // panic slicing `rec_data[start..end]`.
            if start >= end || end > rec_data.len() {
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

/// Pad a byte vector to the next 4-byte boundary with zeros. Matches calibre's
/// `align_block` helper, used inside INDX records for the TAGX, geometry, and
/// IDXT sub-blocks.
fn align_to_4(mut block: Vec<u8>) -> Vec<u8> {
    while !block.len().is_multiple_of(4) {
        block.push(0);
    }
    block
}

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

    /// Build the INDX record(s).
    ///
    /// Returns `[header_record, data_record_1, data_record_2, ...]`. Layout
    /// matches calibre's `mobi/writer8/index.py:Index.__call__` byte-for-byte
    /// — the header record carries TAGX + a geometry block summarising each
    /// data record (last entry name + count) + a trailing IDXT, and data
    /// records use a completely different INDX header layout (8 bytes of
    /// 0xFF at offset 28).
    pub fn build(&self) -> Vec<Vec<u8>> {
        if self.entries.is_empty() {
            return vec![self.build_header_record(0, &[], &[])];
        }

        let (entry_data, idxt_offsets) = self.build_entries();
        let idxt = self.build_idxt(&idxt_offsets, entry_data.len());

        let data_record = self.build_data_record(&entry_data, &idxt);

        // Geometry summary for the header record: one entry per data record
        // giving the last entry's name and its count. Boko emits a single
        // data record, so a single summary entry.
        let last_name = self
            .entries
            .last()
            .map(|(n, _)| n.clone())
            .unwrap_or_default();
        let header_record = self.build_header_record(
            self.entries.len() as u32,
            &[(last_name, self.entries.len() as u16)],
            &[1],
        );

        vec![header_record, data_record]
    }

    fn build_header_record(
        &self,
        total_entries: u32,
        last_indices: &[(String, u16)],
        _record_counts: &[u32],
    ) -> Vec<u8> {
        let tagx = self.build_tagx();
        let tagx_aligned = align_to_4(tagx);

        // Geometry block: per data record, `[len(name)][name bytes][count u16]`.
        // Used by Kindle to know how many entries each data record contains
        // and where the alphabetical boundary lies. Calibre also packs this
        // into the header record.
        let mut geometry = Vec::new();
        let geom_start = 192 + tagx_aligned.len();
        let mut idxt_entries: Vec<u16> = Vec::with_capacity(last_indices.len());
        for (name, count) in last_indices {
            idxt_entries.push((geom_start + geometry.len()) as u16);
            let name_bytes = name.as_bytes();
            geometry.push(name_bytes.len() as u8);
            geometry.extend_from_slice(name_bytes);
            geometry.extend_from_slice(&count.to_be_bytes());
        }
        let geometry_aligned = align_to_4(geometry);

        // Inner IDXT block: 'IDXT' + 2 bytes per entry, then padded.
        let mut idxt_inner: Vec<u8> = Vec::with_capacity(4 + idxt_entries.len() * 2);
        idxt_inner.extend_from_slice(b"IDXT");
        for off in &idxt_entries {
            idxt_inner.extend_from_slice(&off.to_be_bytes());
        }
        let idxt_inner_aligned = align_to_4(idxt_inner);

        let idxt_block_offset = 192 + tagx_aligned.len() + geometry_aligned.len();
        let num_records = last_indices.len() as u32;

        let mut record = Vec::with_capacity(
            192 + tagx_aligned.len() + geometry_aligned.len() + idxt_inner_aligned.len(),
        );

        // === INDX header (192 bytes) — see calibre IndexHeader DEFINITION ===
        record.extend_from_slice(b"INDX"); // 0..4
        record.extend_from_slice(&192u32.to_be_bytes()); // 4..8 header_length
        record.extend_from_slice(&[0u8; 8]); // 8..16 unknown1 (zeros) — also identifies this as a header record
        record.extend_from_slice(&2u32.to_be_bytes()); // 16..20 index type (2 = inflection)
        record.extend_from_slice(&(idxt_block_offset as u32).to_be_bytes()); // 20..24 idxt_offset
        record.extend_from_slice(&num_records.to_be_bytes()); // 24..28 num_of_records
        record.extend_from_slice(&65001u32.to_be_bytes()); // 28..32 encoding
        record.extend_from_slice(&0xFFFFFFFFu32.to_be_bytes()); // 32..36 unknown2 = NULL
        record.extend_from_slice(&total_entries.to_be_bytes()); // 36..40 num_of_entries
        record.extend_from_slice(&0u32.to_be_bytes()); // 40..44 ordt_offset
        record.extend_from_slice(&0u32.to_be_bytes()); // 44..48 ligt_offset
        record.extend_from_slice(&0u32.to_be_bytes()); // 48..52 num_of_ordt_entries
        record.extend_from_slice(&self.num_cncx.to_be_bytes()); // 52..56 num_of_cncx
        record.extend_from_slice(&[0u8; 124]); // 56..180 unknown3 (zeros)
        record.extend_from_slice(&192u32.to_be_bytes()); // 180..184 tagx_offset
        record.extend_from_slice(&[0u8; 8]); // 184..192 unknown4 (zeros)

        // === Trailing blocks: TAGX, geometry, IDXT ===
        record.extend_from_slice(&tagx_aligned);
        record.extend_from_slice(&geometry_aligned);
        record.extend_from_slice(&idxt_inner_aligned);

        record
    }

    fn build_data_record(&self, entry_data: &[u8], idxt: &[u8]) -> Vec<u8> {
        // Calibre's data-record INDX layout differs from the header layout:
        // there are 8 bytes of 0xFF at offset 28 and zeros from 36..192.
        // See calibre/mobi/writer8/index.py:Index.__call__.
        let idxt_offset = 192 + entry_data.len();
        let record_count = self.entries.len() as u32;

        let mut record = Vec::with_capacity(192 + entry_data.len() + idxt.len());
        record.extend_from_slice(b"INDX"); // 0..4
        record.extend_from_slice(&192u32.to_be_bytes()); // 4..8 header_length
        record.extend_from_slice(&0u32.to_be_bytes()); // 8..12 unknown
        record.extend_from_slice(&1u32.to_be_bytes()); // 12..16 header type = 1 (data record)
        record.extend_from_slice(&0u32.to_be_bytes()); // 16..20 unknown
        record.extend_from_slice(&(idxt_offset as u32).to_be_bytes()); // 20..24 idxt_offset
        record.extend_from_slice(&record_count.to_be_bytes()); // 24..28 entries in this record
        record.extend_from_slice(&[0xFFu8; 8]); // 28..36 calibre writes 8 bytes of 0xFF
        record.extend_from_slice(&[0u8; 156]); // 36..192 zeros

        record.extend_from_slice(entry_data);
        record.extend_from_slice(idxt);

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

        // Add end-of-data offset
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

/// Build skeleton index records
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

    // CNCX records must be 4-byte aligned. Calibre's `CNCX` class calls
    // `align_block(buf.getvalue())` (utils.py:612, 621) on every flushed
    // record. Without this Kindle's CNCX scanner can read a misaligned
    // trailing varint and either return the wrong string or hang.
    while !cncx.len().is_multiple_of(4) {
        cncx.push(0);
    }

    cncx
}

/// Build chunk/fragment index records
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
        builder.set_cncx_count(1);
    }

    for (i, chunk) in chunks.iter().enumerate() {
        // Control byte: all tags present
        let ctrl = 0x01 | 0x02 | 0x04 | 0x08;
        let mut tag_data = vec![ctrl];

        // CNCX offset (tag 2)
        let cncx_off = cncx_offsets.get(i).copied().unwrap_or(0);
        tag_data.extend(encint(cncx_off));

        // File number (tag 3)
        tag_data.extend(encint(chunk.file_number as u32));

        // Sequence number (tag 4)
        tag_data.extend(encint(chunk.sequence_number as u32));

        // Geometry (tag 6): start_pos, length
        tag_data.extend(encint(chunk.start_pos as u32));
        tag_data.extend(encint(chunk.length as u32));

        // Entry name is insert_pos as string
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

// Guide index tags (KF8 guide reference index)
const GUIDE_TAG_TITLE: TagDef = TagDef {
    tag: 1,
    values_per_entry: 1,
    bitmask: 0x01,
    eof: 0,
};
const GUIDE_TAG_POS_FID: TagDef = TagDef {
    tag: 6,
    values_per_entry: 2,
    bitmask: 0x02,
    eof: 0,
};
const GUIDE_TAG_EOF: TagDef = TagDef {
    tag: 0,
    values_per_entry: 0,
    bitmask: 0x00,
    eof: 1,
};

/// A guide entry — maps an EPUB landmark to a Kindle navigation point.
#[derive(Debug, Clone)]
pub struct GuideBuildEntry {
    /// Guide type ("cover", "title-page", "toc", "start", "text", "notes", etc.).
    /// Used as the index entry's name; Kindle treats it as the lookup key.
    pub guide_type: String,
    /// Display label (CNCX).
    pub title: String,
    /// (fid, offset) — chunk index and offset within the chunk.
    pub pos_fid: (u32, u32),
}

/// Build the K8 guide index records (one or more INDX records + a CNCX).
pub fn build_guide_indx(entries: &[GuideBuildEntry]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let tagx = vec![GUIDE_TAG_TITLE, GUIDE_TAG_POS_FID, GUIDE_TAG_EOF];
    let mut builder = IndxBuilder::new(tagx, 1);

    let titles: Vec<String> = entries.iter().map(|e| e.title.clone()).collect();
    let title_offsets = calculate_cncx_offsets(&titles);
    let cncx = build_cncx(&titles);

    if !entries.is_empty() {
        builder.set_cncx_count(1);
    }

    for (i, entry) in entries.iter().enumerate() {
        // Both tags are mandatory for guide entries.
        let ctrl: u8 = 0x03;
        let mut tag_data = vec![ctrl];

        // Title (tag 1) — CNCX offset.
        let title_offset = title_offsets.get(i).copied().unwrap_or(0);
        tag_data.extend(encint(title_offset));

        // pos_fid (tag 6) — fid + offset.
        let (fid, off) = entry.pos_fid;
        tag_data.extend(encint(fid));
        tag_data.extend(encint(off));

        builder.add_entry(entry.guide_type.clone(), tag_data);
    }

    (builder.build(), cncx)
}

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
    /// (fid, offset) — chunk index and offset within chunk for KF8 link navigation
    pub pos_fid: Option<(u32, u32)>,
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
    // libmobi / Kindle compute control_byte_count from the TAGX itself by
    // counting EOF-terminator entries. NCX TAGX has exactly one EOF, so the
    // entry uses one control byte. Using 2 here previously produced a
    // "Wrong count of control bytes: 2 != 1" parse failure and Kindle
    // refused to open the file as corrupt.
    let mut builder = IndxBuilder::new(tagx, 1);

    // Build CNCX with labels
    let labels: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
    let label_offsets = calculate_cncx_offsets(&labels);
    let cncx = build_cncx(&labels);

    if !entries.is_empty() {
        builder.set_cncx_count(1);
    }

    for (i, entry) in entries.iter().enumerate() {
        // Control byte 0: tags 1-4 and hierarchy tags
        let mut ctrl: u8 = 0x0F; // Tags 1-4 always present

        let has_parent = entry.parent >= 0;
        let has_first_child = entry.first_child >= 0;
        let has_last_child = entry.last_child >= 0;

        if has_parent {
            ctrl |= 0x10;
        }
        if has_first_child {
            ctrl |= 0x20;
        }
        if has_last_child {
            ctrl |= 0x40;
        }
        let has_pos_fid = entry.pos_fid.is_some();
        if has_pos_fid {
            ctrl |= 0x80;
        }

        let mut tag_data = vec![ctrl];

        // Offset (tag 1)
        tag_data.extend(encint(entry.pos));

        // Length (tag 2)
        tag_data.extend(encint(entry.length));

        // Label (tag 3) - CNCX offset
        let label_offset = label_offsets.get(i).copied().unwrap_or(0);
        tag_data.extend(encint(label_offset));

        // Depth (tag 4)
        tag_data.extend(encint(entry.depth));

        // Parent (tag 21)
        if has_parent {
            tag_data.extend(encint(entry.parent as u32));
        }

        // First child (tag 22)
        if has_first_child {
            tag_data.extend(encint(entry.first_child as u32));
        }

        // Last child (tag 23)
        if has_last_child {
            tag_data.extend(encint(entry.last_child as u32));
        }

        // pos_fid (tag 6): two values — fid (chunk index), then offset in chunk
        if let Some((fid, off)) = entry.pos_fid {
            tag_data.extend(encint(fid));
            tag_data.extend(encint(off));
        }

        let name = format!("{i:04}");
        builder.add_entry(name, tag_data);
    }

    (builder.build(), cncx)
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
    fn test_cncx_roundtrip() {
        let labels = vec![
            "Chapter 1".to_string(),
            "Chapter 2".to_string(),
            "Simple Text".to_string(),
        ];

        let cncx_bytes = build_cncx(&labels);
        let offsets = calculate_cncx_offsets(&labels);

        let parsed = Cncx::parse(&[cncx_bytes], "utf-8");

        for (i, label) in labels.iter().enumerate() {
            let offset = offsets[i];
            let retrieved = parsed.get(offset);
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
        let entries = vec![
            NcxBuildEntry {
                pos: 0,
                length: 1000,
                label: "Chapter 1".to_string(),
                depth: 0,
                parent: -1,
                first_child: -1,
                last_child: -1,
                pos_fid: None,
            },
            NcxBuildEntry {
                pos: 1000,
                length: 500,
                label: "Chapter 2".to_string(),
                depth: 0,
                parent: -1,
                first_child: -1,
                last_child: -1,
                pos_fid: None,
            },
        ];

        let (ncx_records, ncx_cncx) = build_ncx_indx(&entries);

        // Should have 2 records: header + data
        assert_eq!(ncx_records.len(), 2, "Should have header + data record");

        // Parse the header
        let header = IndxHeader::parse(&ncx_records[0]).expect("Failed to parse header");

        // Find TAGX in header record
        let tagx_start = header.tagx_offset as usize;
        let (control_byte_count, tagx) =
            parse_tagx(&ncx_records[0][tagx_start..]).expect("Failed to parse TAGX");

        // Parse CNCX
        let cncx = Cncx::parse(&[ncx_cncx], "utf-8");

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

        // Parse each entry
        let mut index_entries = Vec::new();
        for j in 0..positions.len().saturating_sub(1) {
            let start = positions[j];
            let end = positions[j + 1];
            let entry_data = &data_record[start..end];

            let (name, consumed) = decode_string(entry_data, "utf-8");
            let tag_data = &entry_data[consumed..];
            let tags = get_tag_map(control_byte_count, &tagx, tag_data);

            index_entries.push(IndexEntry { name, tags });
        }

        // Parse as NCX entries
        let ncx_entries = parse_ncx_index(&index_entries, &cncx);

        assert_eq!(ncx_entries.len(), 2, "Should have 2 NCX entries");
        assert_eq!(ncx_entries[0].text, "Chapter 1", "First label should match");
        assert_eq!(
            ncx_entries[1].text, "Chapter 2",
            "Second label should match"
        );
    }

    #[test]
    fn test_skel_index_roundtrip() {
        use crate::mobi::skeleton::SkelEntry;

        let entries = vec![
            SkelEntry {
                file_number: 0,
                name: "SKEL0000000000".to_string(),
                chunk_count: 1,
                start_pos: 0,
                length: 1000,
            },
            SkelEntry {
                file_number: 1,
                name: "SKEL0000000001".to_string(),
                chunk_count: 2,
                start_pos: 1000,
                length: 2500,
            },
            SkelEntry {
                file_number: 2,
                name: "SKEL0000000002".to_string(),
                chunk_count: 1,
                start_pos: 3500,
                length: 500,
            },
        ];

        let skel_records = build_skel_indx(&entries);

        // Should have 2 records: header + data
        assert_eq!(skel_records.len(), 2, "Should have header + data record");

        // Parse the header
        let header = IndxHeader::parse(&skel_records[0]).expect("Failed to parse header");

        // Find TAGX in header record
        let tagx_start = header.tagx_offset as usize;
        let (control_byte_count, tagx) =
            parse_tagx(&skel_records[0][tagx_start..]).expect("Failed to parse TAGX");

        // Parse the data record
        let data_record = &skel_records[1];
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

        // Parse each entry
        let mut index_entries = Vec::new();
        for j in 0..positions.len().saturating_sub(1) {
            let start = positions[j];
            let end = positions[j + 1];
            let entry_data = &data_record[start..end];

            let (name, consumed) = decode_string(entry_data, "utf-8");
            let tag_data = &entry_data[consumed..];
            let tags = get_tag_map(control_byte_count, &tagx, tag_data);

            index_entries.push(IndexEntry { name, tags });
        }

        // Parse as skeleton files
        let skel_files = parse_skel_index(&index_entries);

        assert_eq!(skel_files.len(), 3, "Should have 3 skeleton files");

        // Verify first entry
        assert_eq!(skel_files[0].file_number, 0);
        assert_eq!(skel_files[0].start_pos, 0);
        assert_eq!(skel_files[0].length, 1000);

        // Verify second entry
        assert_eq!(skel_files[1].file_number, 1);
        assert_eq!(skel_files[1].start_pos, 1000);
        assert_eq!(skel_files[1].length, 2500);

        // Verify third entry
        assert_eq!(skel_files[2].file_number, 2);
        assert_eq!(skel_files[2].start_pos, 3500);
        assert_eq!(skel_files[2].length, 500);
    }
}
