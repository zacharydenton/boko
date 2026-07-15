//! Pure MOBI parsing functions (no IO).

use std::io;

pub use super::headers::{Compression, Encoding, ExthHeader, MobiHeader, NULL_INDEX};
pub use super::huffcdic::HuffCdicReader;
pub use super::index::{
    DivElement, NcxEntry, SkeletonFile, parse_div_index, parse_ncx_index, parse_skel_index,
    read_index,
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
            let offset =
                u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
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

        // Record offsets come straight from the (untrusted) PDB record table.
        // A descending or out-of-file range would underflow `end - start` at the
        // call sites, so reject it here rather than panicking downstream.
        if start > end || end > file_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Record {index} has invalid range {start}..{end}"),
            ));
        }

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

    // `num_sections` is untrusted; each entry needs 8 bytes, so it can't exceed
    // the record size. Clamp the reservation so a bogus count (e.g. 0xFFFFFFFF)
    // can't request a multi-gigabyte allocation before the per-entry bounds
    // check runs.
    let mut flows = Vec::with_capacity(num_sections.min(data.len() / 8));
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
///
/// Recognises raw font signatures (TrueType / OpenType / WOFF) as well as the
/// Kindle `FONT` FourCC container, which wraps a font with optional XOR
/// obfuscation and zlib compression. For `FONT` containers, the actual font
/// type is only known after `decode_font_record`; the returned `"otf"` is a
/// safe default for the asset filename — readers identify fonts by their
/// `@font-face` `src:` MIME, not by extension.
pub fn detect_font_type(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }

    // Kindle FONT container (wraps actual font with optional XOR + zlib).
    if data.starts_with(b"FONT") {
        return Some("otf");
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

/// Maximum bytes a single FONT record may decompress to. Even the largest
/// real embedded fonts (full CJK families) are well under this; a crafted
/// zlib stream ("font bomb") hits the cap and errors instead of exhausting
/// memory. Mirrors `util::MAX_DECOMPRESSED_ENTRY` / `MAX_DECOMPRESSED_RECORD`.
const MAX_FONT_DECOMPRESSED: usize = 32 * 1024 * 1024;

/// Decode a Kindle `FONT` container record into raw font bytes.
///
/// AZW3 and MOBI ebooks may embed fonts wrapped in a `FONT` FourCC container
/// with an optional XOR-obfuscated prefix (first 1040 bytes) and optional
/// zlib compression. The container layout is:
///
/// | offset | size | field |
/// |-------:|-----:|-------|
/// | 0      | 4    | Magic `FONT` |
/// | 4      | 4    | Uncompressed size (big-endian `u32`) |
/// | 8      | 4    | Flags (`u32`): bit 0 = zlib-compressed, bit 1 = XOR-obfuscated |
/// | 12     | 4    | Data offset (start of font payload, big-endian `u32`) |
/// | 16     | 4    | XOR key length (big-endian `u32`) |
/// | 20     | 4    | XOR key offset (big-endian `u32`) |
/// | 24..   | …    | XOR key (when present) followed by font payload |
///
/// Returns the decoded raw font bytes (typically OTF/TTF/WOFF), suitable for
/// writing directly into an EPUB `fonts/` asset entry.
pub fn decode_font_record(data: &[u8]) -> io::Result<Vec<u8>> {
    if data.len() < 24 || !data.starts_with(b"FONT") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Not a FONT record",
        ));
    }

    let flags = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let data_offset = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let xor_key_len = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let xor_key_offset = u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as usize;

    let is_compressed = flags & 0x0001 != 0;
    let is_obfuscated = flags & 0x0002 != 0;

    if data_offset > data.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "FONT data offset beyond record",
        ));
    }

    let mut font_data = data[data_offset..].to_vec();

    // Deobfuscate: XOR the first 1040 bytes with the key (Amazon's chosen window).
    if is_obfuscated && xor_key_len > 0 {
        let Some(key_end) = xor_key_offset.checked_add(xor_key_len) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "FONT XOR key offset overflow",
            ));
        };
        if key_end > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "FONT XOR key beyond record",
            ));
        }
        let xor_key = &data[xor_key_offset..key_end];
        let deobfuscate_len = 1040.min(font_data.len());
        for (i, byte) in font_data.iter_mut().enumerate().take(deobfuscate_len) {
            *byte ^= xor_key[i % xor_key_len];
        }
    }

    // Decompress. The zlib stream is untrusted, so cap the output (same
    // `.take(cap + 1)` pattern as `util::bounded_inflate`, which handles raw
    // DEFLATE; FONT records are zlib-wrapped): a tiny crafted record could
    // otherwise inflate to gigabytes. No real font approaches this cap.
    if is_compressed {
        use std::io::Read;
        let mut decoder =
            flate2::read::ZlibDecoder::new(&font_data[..]).take(MAX_FONT_DECOMPRESSED as u64 + 1);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("FONT zlib error: {e}"))
        })?;
        if decompressed.len() > MAX_FONT_DECOMPRESSED {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "FONT record decompresses beyond size limit",
            ));
        }
        Ok(decompressed)
    } else {
        Ok(font_data)
    }
}

/// Check if record is metadata/structure (not an image or font).
/// Based on 4-byte FourCC signatures used in MOBI/KF8 format.
///
/// Note: `FONT` is intentionally NOT classified as metadata — it is a
/// container for extractable font data, decoded by `decode_font_record`.
pub fn is_metadata_record(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    matches!(
        &data[..4],
        b"FLIS"
            | b"FCIS"
            | b"SRCS"
            | b"BOUN"
            | b"FDST"
            | b"DATP"
            | b"AUDI"
            | b"VIDE"
            | b"RESC"
            | b"CMET"
            | b"PAGE"
            | b"CONT"
            | b"CRES"
            | b"INDX"
    ) || data.starts_with(b"BOUNDARY")
}

/// A simple TOC node for intermediate representation.
/// Importers convert this to `crate::model::TocEntry`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fdst_ignores_bogus_section_count() {
        // Header claims 0xFFFF_FFFF sections but the record is tiny. The old
        // eager `Vec::with_capacity(num_sections)` would try to reserve ~64 GiB;
        // now it must return a small result without panicking or OOM.
        let mut data = Vec::new();
        data.extend_from_slice(b"FDST");
        data.extend_from_slice(&12u32.to_be_bytes()); // sec_start
        data.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // num_sections (lie)
        let flows = parse_fdst(&data).unwrap();
        assert!(flows.is_empty());
    }

    /// Build a FONT record (no obfuscation) wrapping the given zlib payload.
    fn make_font_record(compressed: &[u8]) -> Vec<u8> {
        let mut rec = Vec::with_capacity(24 + compressed.len());
        rec.extend_from_slice(b"FONT");
        rec.extend_from_slice(&0u32.to_be_bytes()); // uncompressed size (unused)
        rec.extend_from_slice(&1u32.to_be_bytes()); // flags: bit 0 = compressed
        rec.extend_from_slice(&24u32.to_be_bytes()); // data offset
        rec.extend_from_slice(&0u32.to_be_bytes()); // xor key len
        rec.extend_from_slice(&0u32.to_be_bytes()); // xor key offset
        rec.extend_from_slice(compressed);
        rec
    }

    fn zlib_compress(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn test_decode_font_record_compressed_roundtrip() {
        let font = b"OTTO fake font bytes";
        let record = make_font_record(&zlib_compress(font));
        let decoded = decode_font_record(&record).unwrap();
        assert_eq!(decoded, font);
    }

    #[test]
    fn test_decode_font_record_rejects_decompression_bomb() {
        // A few KB of zlib-compressed zeros that inflate past the 32 MB cap.
        // Unbounded read_to_end would previously balloon this into memory;
        // it must now error cleanly at the cap.
        let bomb = zlib_compress(&vec![0u8; MAX_FONT_DECOMPRESSED + 1]);
        assert!(
            bomb.len() < MAX_FONT_DECOMPRESSED / 100,
            "bomb should be tiny relative to its expansion"
        );
        let record = make_font_record(&bomb);
        let err = decode_font_record(&record).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("size limit"),
            "Expected size-limit error, got: {err}"
        );
    }

    fn make_pdb_header(name: &str, num_records: u16, offsets: &[u32]) -> Vec<u8> {
        let mut data = vec![0u8; 78 + num_records as usize * 8];

        // Name (null-terminated, max 32 bytes)
        let name_bytes = name.as_bytes();
        data[..name_bytes.len().min(31)].copy_from_slice(&name_bytes[..name_bytes.len().min(31)]);

        // Type/Creator = "BOOKMOBI"
        data[60..68].copy_from_slice(b"BOOKMOBI");

        // Number of records
        data[76..78].copy_from_slice(&num_records.to_be_bytes());

        // Record offsets
        for (i, &offset) in offsets.iter().enumerate() {
            let pos = 78 + i * 8;
            data[pos..pos + 4].copy_from_slice(&offset.to_be_bytes());
        }

        data
    }

    #[test]
    fn test_pdb_info_parse() {
        let data = make_pdb_header("TestBook", 3, &[100, 200, 300]);

        let (pdb, consumed) = PdbInfo::parse(&data).unwrap();
        assert_eq!(pdb.name, "TestBook");
        assert_eq!(pdb.num_records, 3);
        assert_eq!(pdb.record_offsets, vec![100, 200, 300]);
        assert_eq!(consumed, 78 + 3 * 8);
    }

    #[test]
    fn test_pdb_info_record_range() {
        let data = make_pdb_header("Book", 3, &[100, 200, 500]);
        let (pdb, _) = PdbInfo::parse(&data).unwrap();

        // Middle record
        let (start, end) = pdb.record_range(1, 1000).unwrap();
        assert_eq!(start, 200);
        assert_eq!(end, 500);

        // Last record uses file_len
        let (start, end) = pdb.record_range(2, 1000).unwrap();
        assert_eq!(start, 500);
        assert_eq!(end, 1000);

        // Out of bounds
        assert!(pdb.record_range(5, 1000).is_err());
    }

    #[test]
    fn test_pdb_info_record_range_rejects_descending_offsets() {
        // A crafted PDB whose record offsets descend would underflow `end - start`
        // at the call sites. record_range must reject it instead of panicking.
        let data = make_pdb_header("Book", 3, &[500, 200, 100]);
        let (pdb, _) = PdbInfo::parse(&data).unwrap();

        assert!(pdb.record_range(0, 1000).is_err()); // 500 > 200
        assert!(pdb.record_range(1, 1000).is_err()); // 200 > 100
    }

    #[test]
    fn test_pdb_info_record_range_rejects_offset_past_file() {
        // Last-record end is file_len; a start beyond the file is invalid.
        let data = make_pdb_header("Book", 2, &[100, 5000]);
        let (pdb, _) = PdbInfo::parse(&data).unwrap();

        assert!(pdb.record_range(1, 1000).is_err()); // start 5000 > file_len 1000
    }

    #[test]
    fn test_pdb_info_invalid_type() {
        let mut data = make_pdb_header("Book", 1, &[100]);
        data[60..68].copy_from_slice(b"NOTABOOK");

        assert!(PdbInfo::parse(&data).is_err());
    }

    #[test]
    fn test_pdb_info_too_short() {
        let data = vec![0u8; 50];
        assert!(PdbInfo::parse(&data).is_err());
    }

    #[test]
    fn test_parse_fdst() {
        let mut data = Vec::new();
        data.extend_from_slice(b"FDST");
        data.extend_from_slice(&12u32.to_be_bytes()); // section start offset
        data.extend_from_slice(&2u32.to_be_bytes()); // 2 sections

        // Section 1: 0-1000
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&1000u32.to_be_bytes());

        // Section 2: 1000-2500
        data.extend_from_slice(&1000u32.to_be_bytes());
        data.extend_from_slice(&2500u32.to_be_bytes());

        let flows = parse_fdst(&data).unwrap();
        assert_eq!(flows, vec![(0, 1000), (1000, 2500)]);
    }

    #[test]
    fn test_parse_fdst_empty() {
        // Not FDST signature
        let data = b"NOTFDST";
        let flows = parse_fdst(data).unwrap();
        assert!(flows.is_empty());

        // Too short
        let flows = parse_fdst(&[]).unwrap();
        assert!(flows.is_empty());
    }

    #[test]
    fn test_strip_trailing_data_no_flags() {
        let record = b"hello world";
        assert_eq!(strip_trailing_data(record, 0), record.as_slice());
    }

    #[test]
    fn test_strip_trailing_data_multibyte_overlap() {
        // Flag bit 0: multibyte overlap
        // Last byte & 3 + 1 = overlap count
        let mut record = b"hello world".to_vec();
        record.push(0x02); // overlap = (2 & 3) + 1 = 3

        let stripped = strip_trailing_data(&record, 0x01);
        // 12 bytes total, overlap = 3, so 12 - 3 = 9 bytes remain
        assert_eq!(stripped, b"hello wor");
    }

    #[test]
    fn test_strip_trailing_data_empty() {
        let empty: &[u8] = &[];
        assert_eq!(strip_trailing_data(empty, 0xFF), empty);
    }

    #[test]
    fn test_detect_image_type() {
        // JPEG
        assert_eq!(
            detect_image_type(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );

        // PNG
        assert_eq!(
            detect_image_type(&[0x89, b'P', b'N', b'G']),
            Some("image/png")
        );

        // GIF
        assert_eq!(detect_image_type(b"GIF89a"), Some("image/gif"));

        // BMP
        assert_eq!(detect_image_type(b"BM\x00\x00"), Some("image/bmp"));

        // Unknown
        assert_eq!(detect_image_type(b"????"), None);

        // Too short
        assert_eq!(detect_image_type(&[0xFF, 0xD8]), None);
    }

    #[test]
    fn test_detect_font_type() {
        // TrueType
        assert_eq!(detect_font_type(&[0x00, 0x01, 0x00, 0x00]), Some("ttf"));

        // OpenType
        assert_eq!(detect_font_type(b"OTTO"), Some("ttf"));

        // WOFF
        assert_eq!(detect_font_type(b"wOFF"), Some("woff"));

        // Kindle FONT container — default to "otf" extension; actual type is
        // known only after decode_font_record.
        assert_eq!(detect_font_type(b"FONT\x00\x00\x00\x00"), Some("otf"));

        // Unknown
        assert_eq!(detect_font_type(b"????"), None);

        // Too short
        assert_eq!(detect_font_type(&[0x00]), None);
    }

    #[test]
    fn test_is_metadata_record() {
        assert!(is_metadata_record(b"FLIS...."));
        assert!(is_metadata_record(b"FCIS...."));
        assert!(is_metadata_record(b"FDST...."));
        assert!(is_metadata_record(b"INDX...."));
        assert!(is_metadata_record(b"BOUNDARY"));

        // FONT records are NOT metadata — they wrap extractable font data
        // that `decode_font_record` unpacks.
        assert!(!is_metadata_record(b"FONT...."));

        assert!(!is_metadata_record(b"\x89PNG"));
        assert!(!is_metadata_record(b"\xFF\xD8\xFF\xE0"));
        assert!(!is_metadata_record(b"abc")); // too short
    }

    /// Build a synthetic FONT container record from constituent parts.
    fn build_font_record(flags: u32, xor_key: &[u8], payload: &[u8]) -> Vec<u8> {
        // Header layout: 4 (magic) + 4 (uncomp_size) + 4 (flags)
        //              + 4 (data_offset) + 4 (xor_key_len) + 4 (xor_key_offset) = 24
        let xor_key_offset: u32 = 24;
        let data_offset: u32 = 24 + xor_key.len() as u32;
        let uncompressed_size: u32 = payload.len() as u32;

        let mut record = Vec::new();
        record.extend_from_slice(b"FONT");
        record.extend_from_slice(&uncompressed_size.to_be_bytes());
        record.extend_from_slice(&flags.to_be_bytes());
        record.extend_from_slice(&data_offset.to_be_bytes());
        record.extend_from_slice(&(xor_key.len() as u32).to_be_bytes());
        record.extend_from_slice(&xor_key_offset.to_be_bytes());
        record.extend_from_slice(xor_key);
        record.extend_from_slice(payload);
        record
    }

    #[test]
    fn test_decode_font_record_uncompressed_plain() {
        let font_bytes = b"OTTO\x00\x10\x00\x00testfontdata";
        let record = build_font_record(0, &[], font_bytes);
        assert_eq!(decode_font_record(&record).unwrap(), font_bytes);
    }

    #[test]
    fn test_decode_font_record_zlib_compressed() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let font_bytes = b"OTTOreal-decompressed-font-payload-bytes";
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(font_bytes).unwrap();
        let compressed = encoder.finish().unwrap();

        // flags bit 0 = compressed
        let record = build_font_record(0x0001, &[], &compressed);
        assert_eq!(decode_font_record(&record).unwrap(), font_bytes);
    }

    #[test]
    fn test_decode_font_record_xor_obfuscated() {
        let mut font_bytes = b"OTTOfont-data".to_vec();
        let xor_key = [0xAA, 0xBB, 0xCC, 0xDD];

        // Pre-apply XOR to the first min(1040, len) bytes — the on-disk form.
        let n = font_bytes.len().min(1040);
        for (i, byte) in font_bytes.iter_mut().enumerate().take(n) {
            *byte ^= xor_key[i % xor_key.len()];
        }

        // flags bit 1 = obfuscated
        let record = build_font_record(0x0002, &xor_key, &font_bytes);
        let decoded = decode_font_record(&record).unwrap();
        assert_eq!(&decoded[..4], b"OTTO");
        assert_eq!(decoded, b"OTTOfont-data");
    }

    #[test]
    fn test_decode_font_record_xor_and_zlib() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        // Real-world case: payload is first compressed, then the first 1040
        // compressed bytes are XOR-masked. Decoder must reverse in the
        // opposite order: XOR-unmask then decompress.
        let font_bytes = b"OTTOreal-zlib-and-xor-payload-bytes-for-testing".to_vec();
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&font_bytes).unwrap();
        let mut compressed = encoder.finish().unwrap();

        let xor_key = [0x11, 0x22, 0x33, 0x44, 0x55];
        let n = compressed.len().min(1040);
        for (i, byte) in compressed.iter_mut().enumerate().take(n) {
            *byte ^= xor_key[i % xor_key.len()];
        }

        // flags = compressed (0x0001) | obfuscated (0x0002) = 0x0003
        let record = build_font_record(0x0003, &xor_key, &compressed);
        assert_eq!(decode_font_record(&record).unwrap(), font_bytes);
    }

    #[test]
    fn test_decode_font_record_rejects_wrong_magic() {
        let mut record = b"NOPE".to_vec();
        record.extend_from_slice(&[0u8; 20]);
        assert!(decode_font_record(&record).is_err());
    }

    #[test]
    fn test_decode_font_record_rejects_truncated() {
        assert!(decode_font_record(b"FONT").is_err());
        assert!(decode_font_record(b"FONT\x00\x00").is_err());
    }

    #[test]
    fn test_decode_font_record_rejects_offset_beyond_record() {
        let mut record = Vec::new();
        record.extend_from_slice(b"FONT");
        record.extend_from_slice(&100u32.to_be_bytes()); // uncomp size
        record.extend_from_slice(&0u32.to_be_bytes()); // flags
        record.extend_from_slice(&9999u32.to_be_bytes()); // data offset (beyond)
        record.extend_from_slice(&0u32.to_be_bytes()); // xor key len
        record.extend_from_slice(&0u32.to_be_bytes()); // xor key offset
        assert!(decode_font_record(&record).is_err());
    }

    #[test]
    fn test_build_toc_from_ncx_flat() {
        let ncx = vec![
            NcxEntry {
                name: "0000".to_string(),
                text: "Chapter 1".to_string(),
                pos: 0,
                length: 1000,
                level: 0,
                parent: -1,
                pos_fid: None,
            },
            NcxEntry {
                name: "0001".to_string(),
                text: "Chapter 2".to_string(),
                pos: 1000,
                length: 1000,
                level: 0,
                parent: -1,
                pos_fid: None,
            },
        ];

        let toc = build_toc_from_ncx(&ncx, |e| format!("ch{}.html", e.pos));

        assert_eq!(toc.len(), 2);
        assert_eq!(toc[0].title, "Chapter 1");
        assert_eq!(toc[0].href, "ch0.html");
        assert_eq!(toc[1].title, "Chapter 2");
        assert_eq!(toc[1].href, "ch1000.html");
    }

    #[test]
    fn test_build_toc_from_ncx_nested() {
        let ncx = vec![
            NcxEntry {
                name: "0000".to_string(),
                text: "Part 1".to_string(),
                pos: 0,
                length: 2000,
                level: 0,
                parent: -1,
                pos_fid: None,
            },
            NcxEntry {
                name: "0001".to_string(),
                text: "Chapter 1.1".to_string(),
                pos: 0,
                length: 1000,
                level: 1,
                parent: 0,
                pos_fid: None,
            },
            NcxEntry {
                name: "0002".to_string(),
                text: "Chapter 1.2".to_string(),
                pos: 1000,
                length: 1000,
                level: 1,
                parent: 0,
                pos_fid: None,
            },
        ];

        let toc = build_toc_from_ncx(&ncx, |e| format!("#{}", e.pos));

        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Part 1");
        assert_eq!(toc[0].children.len(), 2);
        assert_eq!(toc[0].children[0].title, "Chapter 1.1");
        assert_eq!(toc[0].children[1].title, "Chapter 1.2");
    }

    #[test]
    fn test_build_toc_from_ncx_empty() {
        let toc = build_toc_from_ncx(&[], |_| String::new());
        assert!(toc.is_empty());
    }

    #[test]
    fn test_build_toc_from_ncx_unescapes_html() {
        let ncx = vec![NcxEntry {
            name: "0000".to_string(),
            text: "Tom &amp; Jerry".to_string(),
            pos: 0,
            length: 100,
            level: 0,
            parent: -1,
            pos_fid: None,
        }];

        let toc = build_toc_from_ncx(&ncx, |_| "#0".to_string());
        assert_eq!(toc[0].title, "Tom & Jerry");
    }
}
