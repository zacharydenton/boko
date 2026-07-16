//! HUFF/CDIC decompression for MOBI files
//!
//! Some MOBI files use Huffman compression instead of PalmDOC LZ77.
//! This module handles the HUFF (Huffman table) and CDIC (dictionary) records.

use std::io;

/// Maximum HUFF/CDIC dictionary-node recursion depth. Real dictionaries nest
/// only a handful of levels; this guards against a crafted CDIC record.
const MAX_HUFF_DEPTH: usize = 32;

/// Maximum bytes a single text record may decompress to. HUFF/CDIC can amplify
/// hugely (a small Huffman record referencing deeply-nested dictionary nodes),
/// so bound the output per record to stop a decompression bomb. Real records
/// decompress to at most tens of KB; this ceiling is far above that.
const MAX_DECOMPRESSED_RECORD: usize = 16 * 1024 * 1024;

/// Total decompressed bytes allowed across every text record of one book,
/// proportional to the compressed input size. Real HUFF/CDIC ratios are well
/// under 10x; 64x plus a fixed floor leaves generous headroom while stopping
/// the per-record cap from being multiplied across 65k tiny records into
/// terabyte-scale output from a sub-megabyte file.
pub fn total_text_budget(compressed_len: u64) -> usize {
    let proportional = compressed_len.saturating_mul(64);
    usize::try_from(proportional)
        .unwrap_or(usize::MAX)
        .saturating_add(4 * 1024 * 1024)
}

/// Charge `n` bytes against the remaining decompression budget, erroring if the
/// record would exceed [`MAX_DECOMPRESSED_RECORD`].
fn take_budget(budget: &mut usize, n: usize) -> io::Result<()> {
    match budget.checked_sub(n) {
        Some(rem) => {
            *budget = rem;
            Ok(())
        }
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HUFF/CDIC record exceeds decompressed size limit",
        )),
    }
}

/// Dictionary entry: (slice data, is_leaf flag)
#[derive(Clone)]
enum DictEntry {
    Leaf(Vec<u8>),
    Node(Vec<u8>),
    Unpacked(Vec<u8>),
}

/// HUFF/CDIC decompressor
pub struct HuffCdicReader {
    /// dict1: 256 entries of (codelen, term, maxcode)
    dict1: Vec<(u8, bool, u32)>,
    /// mincode for each code length (1-32)
    mincode: Vec<u32>,
    /// maxcode for each code length (1-32)
    maxcode: Vec<u32>,
    /// Dictionary entries from CDIC records
    dictionary: Vec<DictEntry>,
}

impl HuffCdicReader {
    /// Create a new reader from HUFF and CDIC records
    pub fn new(huff: &[u8], cdics: &[&[u8]]) -> io::Result<Self> {
        let mut reader = Self {
            dict1: Vec::with_capacity(256),
            mincode: Vec::with_capacity(33),
            maxcode: Vec::with_capacity(33),
            dictionary: Vec::new(),
        };

        reader.load_huff(huff)?;
        for cdic in cdics {
            reader.load_cdic(cdic)?;
        }

        Ok(reader)
    }

    fn load_huff(&mut self, huff: &[u8]) -> io::Result<()> {
        // Check header: "HUFF\x00\x00\x00\x18"
        if huff.len() < 24 || &huff[0..8] != b"HUFF\x00\x00\x00\x18" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid HUFF header",
            ));
        }

        let off1 = u32::from_be_bytes([huff[8], huff[9], huff[10], huff[11]]) as usize;
        let off2 = u32::from_be_bytes([huff[12], huff[13], huff[14], huff[15]]) as usize;

        // Load dict1: 256 entries at off1
        if off1.checked_add(256 * 4).is_none_or(|end| huff.len() < end) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HUFF dict1 truncated",
            ));
        }

        for i in 0..256 {
            let pos = off1 + i * 4;
            let v = u32::from_be_bytes([huff[pos], huff[pos + 1], huff[pos + 2], huff[pos + 3]]);

            let codelen = (v & 0x1f) as u8;
            let term = (v & 0x80) != 0;
            let maxcode_raw = v >> 8;

            // A zero code length would make the decode loop consume zero bits
            // and emit zero bytes per iteration — an infinite loop on hostile
            // input (the output budget never fires because nothing is
            // emitted). Real HUFF tables always use 1..=32-bit codes, so
            // reject codelen == 0 outright. (The 0x1f mask already caps the
            // value at 31, so > 32 is impossible.)
            if codelen == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HUFF dict1 entry has zero code length",
                ));
            }

            // Calculate maxcode for this entry
            let maxcode = ((maxcode_raw + 1) << (32 - codelen)).wrapping_sub(1);

            self.dict1.push((codelen, term, maxcode));
        }

        // Load dict2: 64 entries at off2 (32 mincode/maxcode pairs)
        if off2.checked_add(64 * 4).is_none_or(|end| huff.len() < end) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HUFF dict2 truncated",
            ));
        }

        // Initialize with codelen 0
        self.mincode.push(0);
        self.maxcode.push(0);

        for i in 0..32 {
            let pos = off2 + i * 8;
            let mincode_raw =
                u32::from_be_bytes([huff[pos], huff[pos + 1], huff[pos + 2], huff[pos + 3]]);
            let maxcode_raw =
                u32::from_be_bytes([huff[pos + 4], huff[pos + 5], huff[pos + 6], huff[pos + 7]]);

            let codelen = i + 1;
            self.mincode.push(mincode_raw << (32 - codelen));
            // `maxcode_raw` is a full 32-bit field here, so `+ 1` can overflow;
            // wrap it (matches the intended modular arithmetic) instead of
            // panicking under overflow-checks.
            self.maxcode
                .push((maxcode_raw.wrapping_add(1) << (32 - codelen)).wrapping_sub(1));
        }

        Ok(())
    }

    fn load_cdic(&mut self, cdic: &[u8]) -> io::Result<()> {
        // Check header: "CDIC\x00\x00\x00\x10"
        if cdic.len() < 16 || &cdic[0..8] != b"CDIC\x00\x00\x00\x10" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid CDIC header",
            ));
        }

        let phrases = u32::from_be_bytes([cdic[8], cdic[9], cdic[10], cdic[11]]) as usize;
        let bits = u32::from_be_bytes([cdic[12], cdic[13], cdic[14], cdic[15]]) as usize;

        // `bits` is untrusted; `1 << bits` overflows (panics under
        // overflow-checks) for `bits >= usize::BITS`. Saturate instead — the
        // count is bounded by the phrase count and the offset-table check below.
        let entry_cap = u32::try_from(bits)
            .ok()
            .and_then(|b| 1usize.checked_shl(b))
            .unwrap_or(usize::MAX);
        let n = std::cmp::min(entry_cap, phrases.saturating_sub(self.dictionary.len()));

        // Read offset table
        if n.checked_mul(2)
            .and_then(|b| b.checked_add(16))
            .is_none_or(|end| cdic.len() < end)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CDIC offset table truncated",
            ));
        }

        for i in 0..n {
            let off_pos = 16 + i * 2;
            let off = u16::from_be_bytes([cdic[off_pos], cdic[off_pos + 1]]) as usize;

            if 16 + off + 2 > cdic.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CDIC entry truncated",
                ));
            }

            let blen = u16::from_be_bytes([cdic[16 + off], cdic[16 + off + 1]]);
            let slice_len = (blen & 0x7fff) as usize;
            let is_leaf = (blen & 0x8000) != 0;

            let slice_start = 16 + off + 2;
            let slice_end = std::cmp::min(slice_start + slice_len, cdic.len());
            let slice = cdic[slice_start..slice_end].to_vec();

            if is_leaf {
                self.dictionary.push(DictEntry::Leaf(slice));
            } else {
                self.dictionary.push(DictEntry::Node(slice));
            }
        }

        Ok(())
    }

    /// Decompress a text record
    /// Decompress one text record, charging output bytes against both the
    /// per-record cap and `shared_budget` (the whole-book allowance from
    /// [`total_text_budget`]). The shared budget is what stops a crafted
    /// file with thousands of maximally-amplifying records from producing
    /// terabytes of output 16 MiB at a time.
    pub fn decompress(&mut self, data: &[u8], shared_budget: &mut usize) -> io::Result<Vec<u8>> {
        let mut result = Vec::new();
        let allowed = MAX_DECOMPRESSED_RECORD.min(*shared_budget);
        let mut budget = allowed;
        self.unpack_into(data, &mut result, 0, &mut budget)?;
        *shared_budget -= allowed - budget;
        Ok(result)
    }

    fn unpack_into(
        &mut self,
        data: &[u8],
        output: &mut Vec<u8>,
        depth: usize,
        budget: &mut usize,
    ) -> io::Result<()> {
        // Node entries unpack recursively; cap the depth so a crafted CDIC
        // dictionary can't drive unbounded recursion into a stack overflow.
        if depth > MAX_HUFF_DEPTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HUFF/CDIC dictionary nested too deep",
            ));
        }

        let bitsleft = data.len() * 8;
        let mut bits_remaining = bitsleft as i64;

        // Pad data for safe reading
        let mut padded = data.to_vec();
        padded.extend_from_slice(&[0u8; 8]);

        let mut pos = 0usize;
        let mut x = read_u64_be(&padded, pos);
        let mut n: i32 = 32;

        while bits_remaining > 0 {
            if n <= 0 {
                pos += 4;
                x = read_u64_be(&padded, pos);
                n += 32;
            }

            let code = ((x >> n) & 0xFFFFFFFF) as u32;

            // Look up in dict1 using top 8 bits
            let (mut codelen, term, mut maxcode) = self.dict1[(code >> 24) as usize];

            if !term {
                // Need to find the right code length
                while codelen < 32 && code < self.mincode[codelen as usize] {
                    codelen += 1;
                }
                if codelen < 33 {
                    maxcode = self.maxcode[codelen as usize];
                }
            }

            n -= codelen as i32;
            bits_remaining -= codelen as i64;

            if bits_remaining < 0 {
                break;
            }

            // Calculate dictionary index
            let r = if codelen > 0 {
                (maxcode.wrapping_sub(code) >> (32 - codelen)) as usize
            } else {
                0
            };

            if r >= self.dictionary.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Dictionary index {} out of bounds (len {})",
                        r,
                        self.dictionary.len()
                    ),
                ));
            }

            // Get the slice, unpacking recursively if needed
            match &self.dictionary[r] {
                DictEntry::Leaf(slice) => {
                    take_budget(budget, slice.len())?;
                    output.extend_from_slice(slice);
                }
                DictEntry::Node(slice) => {
                    // Need to recursively unpack. The recursive call charges the
                    // shared budget for every leaf byte it emits, so copying the
                    // result into `output` afterwards is not double-counted.
                    let slice_copy = slice.clone();
                    let mut unpacked = Vec::new();
                    self.unpack_into(&slice_copy, &mut unpacked, depth + 1, budget)?;
                    output.extend_from_slice(&unpacked);
                    self.dictionary[r] = DictEntry::Unpacked(unpacked);
                }
                DictEntry::Unpacked(slice) => {
                    take_budget(budget, slice.len())?;
                    output.extend_from_slice(slice);
                }
            }
        }

        Ok(())
    }
}

fn read_u64_be(data: &[u8], pos: usize) -> u64 {
    if pos + 8 <= data.len() {
        u64::from_be_bytes([
            data[pos],
            data[pos + 1],
            data[pos + 2],
            data[pos + 3],
            data[pos + 4],
            data[pos + 5],
            data[pos + 6],
            data[pos + 7],
        ])
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_cdic_survives_huge_bits_field() {
        // A crafted CDIC with `bits = 0xFFFF_FFFF` used to panic on `1 << bits`
        // (shift overflow). It must now be handled without panicking.
        let mut reader = HuffCdicReader {
            dict1: Vec::new(),
            mincode: Vec::new(),
            maxcode: Vec::new(),
            dictionary: Vec::new(),
        };
        let mut cdic = Vec::new();
        cdic.extend_from_slice(b"CDIC\x00\x00\x00\x10");
        cdic.extend_from_slice(&0u32.to_be_bytes()); // phrases = 0
        cdic.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // bits (hostile)
        assert!(reader.load_cdic(&cdic).is_ok());
    }

    #[test]
    fn test_read_u64_be() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(read_u64_be(&data, 0), 1);

        let data2 = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(read_u64_be(&data2, 0), 0x0100000000000000);
    }

    /// Build a minimal valid HUFF record with uniform 8-bit codes.
    /// All 256 byte values map through 8-bit Huffman codes to identity.
    fn make_huff() -> Vec<u8> {
        let off1: u32 = 24;
        let off2: u32 = 24 + 256 * 4; // 1048

        let mut huff = vec![0u8; 24];
        huff[0..8].copy_from_slice(b"HUFF\x00\x00\x00\x18");
        huff[8..12].copy_from_slice(&off1.to_be_bytes());
        huff[12..16].copy_from_slice(&off2.to_be_bytes());

        // Table 1: 256 entries. All uniform 8-bit, term=true.
        // Entry = (maxcode_raw << 8) | 0x80 | codelen
        //       = (255 << 8) | 0x80 | 8 = 0x0000FF88
        let entry = 0x0000FF88u32;
        for _ in 0..256 {
            huff.extend_from_slice(&entry.to_be_bytes());
        }

        // Table 2: 32 pairs of (mincode_raw, maxcode_raw)
        for i in 0..32u32 {
            if i + 1 == 8 {
                huff.extend_from_slice(&0u32.to_be_bytes()); // min=0
                huff.extend_from_slice(&255u32.to_be_bytes()); // max=255
            } else {
                huff.extend_from_slice(&[0u8; 8]); // unused
            }
        }

        huff
    }

    /// Build a minimal valid CDIC record with 4 single-byte leaf entries.
    fn make_cdic() -> Vec<u8> {
        let num_phrases: u32 = 4;
        let bits: u32 = 2; // 1 << 2 = 4 entries max per CDIC

        let mut cdic = vec![0u8; 16];
        cdic[0..8].copy_from_slice(b"CDIC\x00\x00\x00\x10");
        cdic[8..12].copy_from_slice(&num_phrases.to_be_bytes());
        cdic[12..16].copy_from_slice(&bits.to_be_bytes());

        // Offset table: 4 entries x 2 bytes = 8 bytes
        // Entry data starts at offset 8 (relative to byte 16)
        // Each entry is 3 bytes: u16 length_flags + 1 byte data
        let offset_table_size = 4 * 2; // 8
        for i in 0..4u16 {
            let offset = offset_table_size as u16 + i * 3;
            cdic.extend_from_slice(&offset.to_be_bytes());
        }

        // Entries: each is leaf (0x8001) + one byte
        for i in 0..4u8 {
            cdic.extend_from_slice(&0x8001u16.to_be_bytes()); // leaf, len=1
            cdic.push(i);
        }

        cdic
    }

    #[test]
    fn test_load_huff_valid() {
        let huff = make_huff();
        let mut reader = HuffCdicReader {
            dict1: Vec::new(),
            mincode: Vec::new(),
            maxcode: Vec::new(),
            dictionary: Vec::new(),
        };
        assert!(reader.load_huff(&huff).is_ok());
        assert_eq!(reader.dict1.len(), 256);
    }

    #[test]
    fn test_load_huff_rejects_zero_codelen() {
        // A dict1 entry with codelen == 0 used to make the decode loop consume
        // zero bits and emit zero bytes forever (the output budget never fires
        // because nothing is emitted) — an infinite loop on hostile input.
        // Craft a HUFF record whose dict1 entries are all terminal with
        // codelen == 0; load_huff must reject it up front.
        let mut huff = make_huff();
        // Overwrite every dict1 entry (at off1 = 24) with codelen = 0, term set:
        // (maxcode_raw << 8) | 0x80 | 0 = 0x0000FF80
        let bad_entry = 0x0000_FF80u32.to_be_bytes();
        for i in 0..256 {
            let pos = 24 + i * 4;
            huff[pos..pos + 4].copy_from_slice(&bad_entry);
        }

        let mut reader = HuffCdicReader {
            dict1: Vec::new(),
            mincode: Vec::new(),
            maxcode: Vec::new(),
            dictionary: Vec::new(),
        };
        let err = reader.load_huff(&huff).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("zero code length"),
            "Expected zero-code-length error, got: {err}"
        );

        // The full constructor path must also fail (this is the path that
        // previously produced a reader whose decompress() spun forever).
        let cdic = make_cdic();
        assert!(HuffCdicReader::new(&huff, &[cdic.as_slice()]).is_err());
    }

    #[test]
    fn test_load_huff_rejects_bad_magic() {
        let mut reader = HuffCdicReader {
            dict1: Vec::new(),
            mincode: Vec::new(),
            maxcode: Vec::new(),
            dictionary: Vec::new(),
        };
        // JPEG SOI marker — not HUFF
        assert!(reader.load_huff(b"\xFF\xD8\xFF\xE0JFIF").is_err());
    }

    #[test]
    fn test_load_cdic_valid() {
        let cdic = make_cdic();
        let mut reader = HuffCdicReader {
            dict1: Vec::new(),
            mincode: Vec::new(),
            maxcode: Vec::new(),
            dictionary: Vec::new(),
        };
        assert!(reader.load_cdic(&cdic).is_ok());
        assert_eq!(reader.dictionary.len(), 4);
    }

    #[test]
    fn test_load_cdic_rejects_jpeg_bytes() {
        // This is exactly what happens with the off-by-one bug:
        // the loop reads one record past the CDICs, hits a JPEG image,
        // and load_cdic() fails because 0xFF 0xD8 != "CDIC".
        let jpeg = b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00";
        let mut reader = HuffCdicReader {
            dict1: Vec::new(),
            mincode: Vec::new(),
            maxcode: Vec::new(),
            dictionary: Vec::new(),
        };
        let err = reader.load_cdic(jpeg).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("Invalid CDIC header"),
            "Expected 'Invalid CDIC header', got: {}",
            err
        );
    }

    #[test]
    fn test_reader_new_with_valid_cdics() {
        let huff = make_huff();
        let cdic = make_cdic();
        // Correct usage: only CDIC records passed
        let result = HuffCdicReader::new(&huff, &[cdic.as_slice()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reader_new_with_poison_record_fails() {
        // Simulates the off-by-one bug: the caller passes an extra
        // non-CDIC record (JPEG image bytes) after the real CDICs.
        let huff = make_huff();
        let cdic = make_cdic();
        let jpeg = b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x01\x00";

        match HuffCdicReader::new(&huff, &[cdic.as_slice(), jpeg.as_slice()]) {
            Err(e) => assert!(
                e.to_string().contains("Invalid CDIC header"),
                "Expected 'Invalid CDIC header', got: {e}"
            ),
            Ok(_) => panic!("Should fail when JPEG bytes are passed as CDIC"),
        }
    }
}
