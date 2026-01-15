//! HUFF/CDIC decompression for MOBI files
//!
//! Some MOBI files use Huffman compression instead of PalmDOC LZ77.
//! This module handles the HUFF (Huffman table) and CDIC (dictionary) records.

use crate::error::{Error, Result};

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
    pub fn new(huff: &[u8], cdics: &[&[u8]]) -> Result<Self> {
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

    fn load_huff(&mut self, huff: &[u8]) -> Result<()> {
        // Check header: "HUFF\x00\x00\x00\x18"
        if huff.len() < 24 || &huff[0..8] != b"HUFF\x00\x00\x00\x18" {
            return Err(Error::InvalidMobi("Invalid HUFF header".into()));
        }

        let off1 = u32::from_be_bytes([huff[8], huff[9], huff[10], huff[11]]) as usize;
        let off2 = u32::from_be_bytes([huff[12], huff[13], huff[14], huff[15]]) as usize;

        // Load dict1: 256 entries at off1
        if huff.len() < off1 + 256 * 4 {
            return Err(Error::InvalidMobi("HUFF dict1 truncated".into()));
        }

        for i in 0..256 {
            let pos = off1 + i * 4;
            let v = u32::from_be_bytes([huff[pos], huff[pos + 1], huff[pos + 2], huff[pos + 3]]);

            let codelen = (v & 0x1f) as u8;
            let term = (v & 0x80) != 0;
            let maxcode_raw = v >> 8;

            // Calculate maxcode for this entry
            let maxcode = if codelen > 0 {
                ((maxcode_raw + 1) << (32 - codelen)).wrapping_sub(1)
            } else {
                0
            };

            self.dict1.push((codelen, term, maxcode));
        }

        // Load dict2: 64 entries at off2 (32 mincode/maxcode pairs)
        if huff.len() < off2 + 64 * 4 {
            return Err(Error::InvalidMobi("HUFF dict2 truncated".into()));
        }

        // Initialize with codelen 0
        self.mincode.push(0);
        self.maxcode.push(0);

        for i in 0..32 {
            let pos = off2 + i * 8;
            let mincode_raw =
                u32::from_be_bytes([huff[pos], huff[pos + 1], huff[pos + 2], huff[pos + 3]]);
            let maxcode_raw = u32::from_be_bytes([
                huff[pos + 4],
                huff[pos + 5],
                huff[pos + 6],
                huff[pos + 7],
            ]);

            let codelen = i + 1;
            self.mincode.push(mincode_raw << (32 - codelen));
            self.maxcode
                .push(((maxcode_raw + 1) << (32 - codelen)).wrapping_sub(1));
        }

        Ok(())
    }

    fn load_cdic(&mut self, cdic: &[u8]) -> Result<()> {
        // Check header: "CDIC\x00\x00\x00\x10"
        if cdic.len() < 16 || &cdic[0..8] != b"CDIC\x00\x00\x00\x10" {
            return Err(Error::InvalidMobi("Invalid CDIC header".into()));
        }

        let phrases = u32::from_be_bytes([cdic[8], cdic[9], cdic[10], cdic[11]]) as usize;
        let bits = u32::from_be_bytes([cdic[12], cdic[13], cdic[14], cdic[15]]) as usize;

        let n = std::cmp::min(1 << bits, phrases.saturating_sub(self.dictionary.len()));

        // Read offset table
        if cdic.len() < 16 + n * 2 {
            return Err(Error::InvalidMobi("CDIC offset table truncated".into()));
        }

        for i in 0..n {
            let off_pos = 16 + i * 2;
            let off = u16::from_be_bytes([cdic[off_pos], cdic[off_pos + 1]]) as usize;

            if 16 + off + 2 > cdic.len() {
                return Err(Error::InvalidMobi("CDIC entry truncated".into()));
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
    pub fn decompress(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let mut result = Vec::new();
        self.unpack_into(data, &mut result)?;
        Ok(result)
    }

    fn unpack_into(&mut self, data: &[u8], output: &mut Vec<u8>) -> Result<()> {
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
                return Err(Error::InvalidMobi(format!(
                    "Dictionary index {} out of bounds (len {})",
                    r,
                    self.dictionary.len()
                )));
            }

            // Get the slice, unpacking recursively if needed
            match &self.dictionary[r] {
                DictEntry::Leaf(slice) => {
                    output.extend_from_slice(slice);
                }
                DictEntry::Node(slice) => {
                    // Need to recursively unpack
                    let slice_copy = slice.clone();
                    let mut unpacked = Vec::new();
                    self.unpack_into(&slice_copy, &mut unpacked)?;
                    output.extend_from_slice(&unpacked);
                    self.dictionary[r] = DictEntry::Unpacked(unpacked);
                }
                DictEntry::Unpacked(slice) => {
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
    fn test_read_u64_be() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(read_u64_be(&data, 0), 1);

        let data2 = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(read_u64_be(&data2, 0), 0x0100000000000000);
    }
}
