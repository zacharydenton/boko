/// PalmDOC LZ77 compression/decompression
///
/// Uses the palmdoc-compression crate for fast compression with hash chains.
/// Decompression is kept local for reading existing files.

/// PalmDOC LZ77 compression using fast hash-chain implementation
pub fn compress(input: &[u8]) -> Vec<u8> {
    palmdoc_compression::compress(input)
}

/// PalmDOC LZ77 decompression
///
/// The compression scheme is simple:
/// - Bytes 0x01-0x08: Copy next 'n' bytes literally
/// - Bytes 0x00, 0x09-0x7F: Literal character
/// - Bytes 0x80-0xBF: Back-reference (LZ77)
///   - Combined with next byte: distance = (val & 0x3FFF) >> 3, length = (val & 7) + 3
/// - Bytes 0xC0-0xFF: Space + (byte ^ 0x80)
pub fn decompress(input: &[u8]) -> Vec<u8> {
    // Use the crate's implementation
    palmdoc_compression::decompress(input).unwrap_or_else(|_| {
        // Fallback to manual decompression on error
        decompress_manual(input)
    })
}

/// Manual decompression implementation (fallback)
fn decompress_manual(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len() * 4);
    let mut i = 0;

    while i < input.len() {
        let c = input[i];
        i += 1;

        if c >= 1 && c <= 8 {
            // Copy next 'c' bytes literally
            let count = c as usize;
            for _ in 0..count {
                if i < input.len() {
                    output.push(input[i]);
                    i += 1;
                }
            }
        } else if c == 0 || (c >= 0x09 && c <= 0x7F) {
            // Literal character
            output.push(c);
        } else if c >= 0xC0 {
            // Space + ASCII char
            output.push(b' ');
            output.push(c ^ 0x80);
        } else if i < input.len() {
            // Back-reference (0x80-0xBF)
            let next = input[i];
            i += 1;

            let combined = ((c as u16) << 8) | (next as u16);
            let distance = ((combined & 0x3FFF) >> 3) as usize;
            let length = ((combined & 7) + 3) as usize;

            if distance > 0 && distance <= output.len() {
                for _ in 0..length {
                    let pos = output.len() - distance;
                    let byte = output[pos];
                    output.push(byte);
                }
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompress_literal() {
        // Simple literal text (bytes 0x09-0x7F are literal)
        let input = b"Hello";
        let output = decompress(input);
        assert_eq!(output, b"Hello");
    }

    #[test]
    fn test_decompress_space_ascii() {
        // 0xC0 + n = space + (n ^ 0x80)
        // 0xC0 | 'A' (0x41) = 0xC1, but actually it's stored as c >= 0xC0
        // Space + 'A' is encoded as 0xC1 (0x41 ^ 0x80 = 0xC1)
        let input = &[0xC1]; // Should decompress to " A"
        let output = decompress(input);
        assert_eq!(output, b" A");
    }

    #[test]
    fn test_roundtrip() {
        let original = b"Hello, World! This is a test of PalmDOC compression.";
        let compressed = compress(original);
        let decompressed = decompress(&compressed);
        assert_eq!(decompressed, original);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: compress then decompress yields original data
        #[test]
        fn roundtrip_compression(data in prop::collection::vec(any::<u8>(), 0..4096)) {
            let compressed = compress(&data);
            let decompressed = decompress(&compressed);
            prop_assert_eq!(decompressed, data);
        }

        /// Property: compression should not expand small data too much
        /// (worst case for PalmDoc is ~1.125x expansion)
        #[test]
        fn compression_reasonable_size(data in prop::collection::vec(any::<u8>(), 1..1024)) {
            let compressed = compress(&data);
            // PalmDoc worst case: each byte becomes escape + literal = 2 bytes
            // But in practice, it should rarely exceed 2x original size
            prop_assert!(compressed.len() <= data.len() * 2 + 16);
        }

        /// Property: empty input produces empty output
        #[test]
        fn empty_roundtrip(_seed in any::<u64>()) {
            let data: Vec<u8> = vec![];
            let compressed = compress(&data);
            let decompressed = decompress(&compressed);
            prop_assert_eq!(decompressed, data);
        }
    }
}
