//! Pure-Rust PalmDOC compression/decompression.
//!
//! PalmDOC uses a simple LZ77-style compression scheme with a sliding window.

use std::io;

/// Decompress PalmDOC data.
pub fn decompress(input: &[u8]) -> io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() * 2);
    let mut i = 0;

    while i < input.len() {
        let byte = input[i];
        i += 1;

        match byte {
            // Literal byte
            0x00 | 0x09..=0x7F => {
                output.push(byte);
            }
            // Copy next 1-8 bytes literally
            0x01..=0x08 => {
                let count = byte as usize;
                if i + count > input.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "PalmDoc: unexpected end of input",
                    ));
                }
                output.extend_from_slice(&input[i..i + count]);
                i += count;
            }
            // Back-reference: copy from sliding window
            0x80..=0xBF => {
                if i >= input.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "PalmDoc: unexpected end of input",
                    ));
                }
                let next = input[i] as usize;
                i += 1;

                // Distance and length encoded in two bytes
                let distance = (((byte as usize) & 0x3F) << 5) | (next >> 3);
                let length = (next & 0x07) + 3;

                if distance > output.len() || distance == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("PalmDoc: invalid back-reference distance {}", distance),
                    ));
                }

                let start = output.len() - distance;
                for j in 0..length {
                    output.push(output[start + (j % distance)]);
                }
            }
            // Space + character
            0xC0..=0xFF => {
                output.push(b' ');
                output.push(byte ^ 0x80);
            }
        }
    }

    Ok(output)
}

/// Hash function for 3-byte sequences
#[inline]
fn hash3(data: &[u8]) -> usize {
    ((data[0] as usize) << 16 | (data[1] as usize) << 8 | (data[2] as usize)) % HASH_SIZE
}

const HASH_SIZE: usize = 4096;
const MAX_DISTANCE: usize = 2047;

/// Compress data using PalmDOC compression with hash-based match finding.
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;

    // Hash table: maps hash -> most recent position with that hash
    let mut hash_table = [0usize; HASH_SIZE];
    // Chain: for each position, points to previous position with same hash
    let mut chain = vec![0usize; input.len()];

    while i < input.len() {
        let mut best_match_len = 0;
        let mut best_match_dist = 0;

        // Look for matches using hash table
        if i + 3 <= input.len() {
            let h = hash3(&input[i..]);
            let mut pos = hash_table[h];

            // Follow the hash chain to find matches
            let min_pos = i.saturating_sub(MAX_DISTANCE);
            let mut chain_len = 0;
            while pos >= min_pos && pos < i && chain_len < 16 {
                let dist = i - pos;
                if dist <= MAX_DISTANCE {
                    // Check match length
                    let mut len = 0;
                    let max_len = (input.len() - i).min(10);
                    while len < max_len && input[pos + len] == input[i + len] {
                        len += 1;
                    }
                    if len >= 3 && len > best_match_len {
                        best_match_len = len;
                        best_match_dist = dist;
                        if len == 10 {
                            break; // Max match length reached
                        }
                    }
                }
                if pos == 0 {
                    break;
                }
                pos = chain[pos];
                chain_len += 1;
            }

            // Update hash table and chain
            chain[i] = hash_table[h];
            hash_table[h] = i;
        }

        if best_match_len >= 3 {
            // Encode back-reference
            let len_code = (best_match_len - 3).min(7);
            let dist_hi = (best_match_dist >> 5) & 0x3F;
            let dist_lo = best_match_dist & 0x1F;

            output.push(0x80 | dist_hi as u8);
            output.push(((dist_lo << 3) | len_code) as u8);

            // Update hash table for skipped positions
            for j in 1..best_match_len {
                if i + j + 3 <= input.len() {
                    let h = hash3(&input[i + j..]);
                    chain[i + j] = hash_table[h];
                    hash_table[h] = i + j;
                }
            }
            i += best_match_len;
        } else if input[i] == b' ' && i + 1 < input.len() && input[i + 1] >= 0x40 && input[i + 1] < 0x80 {
            // Space + printable ASCII -> single byte encoding
            output.push(input[i + 1] ^ 0x80);
            i += 2;
        } else if input[i] == 0 || (input[i] >= 0x09 && input[i] <= 0x7F) {
            // Literal byte
            output.push(input[i]);
            i += 1;
        } else {
            // Byte needs literal escape (0x01-0x08 prefix)
            let mut literal_run = Vec::new();
            while i < input.len()
                && literal_run.len() < 8
                && !(input[i] == 0 || (input[i] >= 0x09 && input[i] <= 0x7F))
            {
                literal_run.push(input[i]);
                i += 1;
            }
            if !literal_run.is_empty() {
                output.push(literal_run.len() as u8);
                output.extend_from_slice(&literal_run);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let original = b"Hello, this is a test of PalmDOC compression. This text has some repetition. This text has some repetition.";
        let compressed = compress(original);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_space_encoding() {
        let original = b"Hello World";
        let compressed = compress(original);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }
}
