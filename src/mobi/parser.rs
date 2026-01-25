//! Pure MOBI parsing functions (no IO).

use std::io;

pub use super::headers::{Compression, Encoding, ExthHeader, MobiHeader, NULL_INDEX};
pub use super::huffcdic::HuffCdicReader;
pub use super::index::{
    Cncx, DivElement, IndexEntry, NcxEntry, SkeletonFile,
    parse_div_index, parse_ncx_index, parse_skel_index, read_index,
};

/// PDB (Palm Database) header info extracted from bytes.
#[derive(Debug)]
pub struct PdbInfo {
    pub name: String,
    pub num_records: u16,
    /// Record offsets within the file.
    pub record_offsets: Vec<u32>,
}

impl PdbInfo {
    /// Parse PDB header from first 78+ bytes of file.
    /// Returns PdbInfo and total bytes consumed.
    pub fn parse(data: &[u8]) -> io::Result<(Self, usize)> {
        if data.len() < 78 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PDB header too short",
            ));
        }

        // Bytes 0-31: Database name (null-terminated)
        let name_end = data[..32].iter().position(|&b| b == 0).unwrap_or(32);
        let name = String::from_utf8_lossy(&data[..name_end]).to_string();

        // Bytes 60-67: Type/Creator should be "BOOKMOBI" or "TEXtREAd"
        let ident = &data[60..68];
        if ident != b"BOOKMOBI" && !ident.eq_ignore_ascii_case(b"TEXTREAD") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unknown book type: {:?}", String::from_utf8_lossy(ident)),
            ));
        }

        // Bytes 76-77: Number of records
        let num_records = u16::from_be_bytes([data[76], data[77]]);

        // Record info list (8 bytes per record, starting at byte 78)
        let records_start = 78;
        let records_len = num_records as usize * 8;

        if data.len() < records_start + records_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PDB header truncated",
            ));
        }

        let mut record_offsets = Vec::with_capacity(num_records as usize);
        for i in 0..num_records as usize {
            let pos = records_start + i * 8;
            let offset = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            record_offsets.push(offset);
        }

        let total_consumed = records_start + records_len;
        Ok((
            Self {
                name,
                num_records,
                record_offsets,
            },
            total_consumed,
        ))
    }

    /// Get the byte range for a record.
    pub fn record_range(&self, index: usize, file_len: u64) -> io::Result<(u64, u64)> {
        if index >= self.record_offsets.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Record index {index} out of bounds"),
            ));
        }

        let start = self.record_offsets[index] as u64;
        let end = if index + 1 < self.record_offsets.len() {
            self.record_offsets[index + 1] as u64
        } else {
            file_len
        };

        Ok((start, end))
    }
}

/// Detected MOBI format variant.
#[derive(Debug, Clone, Copy)]
pub enum MobiFormat {
    /// Pure KF8 (AZW3) - version 8
    Kf8,
    /// Combo file with both MOBI6 and KF8 sections
    Combo { kf8_record_offset: usize },
    /// Legacy MOBI6 - single HTML stream
    Mobi6,
}

impl MobiFormat {
    /// Record offset for KF8 content (0 for pure files, >0 for combo)
    pub fn record_offset(&self) -> usize {
        match self {
            MobiFormat::Kf8 => 0,
            MobiFormat::Combo { kf8_record_offset } => *kf8_record_offset,
            MobiFormat::Mobi6 => 0,
        }
    }

    pub fn is_kf8(&self) -> bool {
        matches!(self, MobiFormat::Kf8 | MobiFormat::Combo { .. })
    }
}

/// Detect format from headers.
pub fn detect_format(mobi: &MobiHeader, exth: Option<&ExthHeader>) -> MobiFormat {
    // Pure KF8: version 8
    if mobi.mobi_version == 8 {
        return MobiFormat::Kf8;
    }

    // Check for combo file: EXTH 121 points to KF8 boundary
    if let Some(kf8_idx) = exth.and_then(|e| e.kf8_boundary) {
        if kf8_idx > 0 {
            return MobiFormat::Combo {
                kf8_record_offset: kf8_idx as usize,
            };
        }
    }

    MobiFormat::Mobi6
}

/// Parse EXTH header if present.
pub fn parse_exth(record0: &[u8], header: &MobiHeader) -> Option<ExthHeader> {
    if header.has_exth() && header.header_length > 0 {
        let exth_start = 16 + header.header_length as usize;
        if exth_start < record0.len() {
            return ExthHeader::parse(&record0[exth_start..], header.encoding).ok();
        }
    }
    None
}

/// Parse FDST (Flow Descriptor Table) from record bytes.
pub fn parse_fdst(data: &[u8]) -> io::Result<Vec<(usize, usize)>> {
    if data.len() < 12 || &data[0..4] != b"FDST" {
        return Ok(Vec::new());
    }

    let sec_start = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let num_sections = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;

    let mut flows = Vec::with_capacity(num_sections);
    for i in 0..num_sections {
        let offset = sec_start + i * 8;
        if offset + 8 > data.len() {
            break;
        }

        let start = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let end = u32::from_be_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;

        flows.push((start, end));
    }

    Ok(flows)
}

/// Strip trailing multibyte extra data from text records.
///
/// MOBI text records can have trailing data appended. The extra_flags field
/// indicates which types are present. We need to strip this data before
/// decompression.
pub fn strip_trailing_data(record: &[u8], flags: u16) -> &[u8] {
    if flags == 0 || record.is_empty() {
        return record;
    }

    let mut end = record.len();

    // Process trailing data entries based on flags (skip bit 0, handled separately)
    // Iterate through bits 1-15 by right-shifting
    let mut shifted_flags = flags >> 1;
    while shifted_flags != 0 {
        if shifted_flags & 1 != 0 {
            if end == 0 {
                break;
            }
            // Read variable-length size from end of record
            // VWI format: read backward, low 7 bits are value, high bit SET means stop
            let mut size = 0usize;
            let mut shift = 0;
            let mut pos = end;
            while pos > 0 {
                pos -= 1;
                let byte = record[pos];
                size |= ((byte & 0x7F) as usize) << shift;
                shift += 7;
                // High bit SET means this is the last byte of the VWI
                if byte & 0x80 != 0 || shift >= 28 {
                    break;
                }
            }
            if size > 0 && size <= end {
                end -= size;
            }
        }
        shifted_flags >>= 1;
    }

    // Handle multibyte overlap flag (bit 0) - processed LAST
    if flags & 1 != 0 && end > 0 {
        let overlap = (record[end - 1] & 3) as usize + 1;
        if overlap <= end {
            end -= overlap;
        }
    }

    &record[..end]
}

/// Detect image type from magic bytes.
pub fn detect_image_type(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }

    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if data.starts_with(b"GIF8") {
        Some("image/gif")
    } else if data.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

/// Detect font type from magic bytes / structure.
pub fn detect_font_type(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }

    // TrueType / OpenType
    if data.starts_with(&[0x00, 0x01, 0x00, 0x00]) || data.starts_with(b"OTTO") {
        return Some("ttf");
    }
    // WOFF
    if data.starts_with(b"wOFF") {
        return Some("woff");
    }

    None
}

/// Check if record is metadata/structure (not an image).
/// Based on 4-byte FourCC signatures used in MOBI/KF8 format.
pub fn is_metadata_record(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    matches!(
        &data[..4],
        b"FLIS" | b"FCIS" | b"SRCS" | b"BOUN" | b"FDST" | b"DATP"
        | b"AUDI" | b"VIDE" | b"RESC" | b"CMET" | b"PAGE" | b"CONT"
        | b"CRES" | b"FONT" | b"INDX"
    ) || data.starts_with(b"BOUNDARY")
}

/// A simple TOC node for intermediate representation.
/// Importers convert this to `crate::book::TocEntry`.
#[derive(Debug, Clone)]
pub struct TocNode {
    pub title: String,
    pub href: String,
    pub children: Vec<TocNode>,
}

/// Build hierarchical TOC from NCX entries.
///
/// Takes a closure `href_fn` that generates the href for each NCX entry.
/// This allows different importers to use their own href format:
/// - MOBI6: `content.html#filepos{pos}`
/// - KF8/AZW3: `part{file_number:04}.html`
pub fn build_toc_from_ncx<F>(ncx: &[NcxEntry], mut href_fn: F) -> Vec<TocNode>
where
    F: FnMut(&NcxEntry) -> String,
{
    use quick_xml::escape::unescape;
    use std::collections::HashMap;

    if ncx.is_empty() {
        return Vec::new();
    }

    // Build flat entries
    let entries: Vec<TocNode> = ncx
        .iter()
        .map(|entry| {
            let href = href_fn(entry);
            let title = unescape(&entry.text)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| entry.text.clone());
            TocNode {
                title,
                href,
                children: Vec::new(),
            }
        })
        .collect();

    // If no parent relationships, return flat list
    if ncx.iter().all(|e| e.parent < 0) {
        return entries;
    }

    // Build hierarchy using parent indices
    let mut entries: Vec<Option<TocNode>> = entries.into_iter().map(Some).collect();
    let mut roots: Vec<usize> = Vec::new();
    let mut children_map: HashMap<usize, Vec<usize>> = HashMap::new();

    for (i, ncx_entry) in ncx.iter().enumerate() {
        if ncx_entry.parent < 0 {
            roots.push(i);
        } else {
            children_map
                .entry(ncx_entry.parent as usize)
                .or_default()
                .push(i);
        }
    }

    fn take_with_children(
        idx: usize,
        entries: &mut [Option<TocNode>],
        children_map: &HashMap<usize, Vec<usize>>,
    ) -> Option<TocNode> {
        let mut entry = entries[idx].take()?;
        if let Some(children_indices) = children_map.get(&idx) {
            for &child_idx in children_indices {
                if let Some(child) = take_with_children(child_idx, entries, children_map) {
                    entry.children.push(child);
                }
            }
        }
        Some(entry)
    }

    roots
        .into_iter()
        .filter_map(|idx| take_with_children(idx, &mut entries, &children_map))
        .collect()
}
