/// PalmDOC LZ77 decompression
///
/// The compression scheme is simple:
/// - Bytes 0x01-0x08: Copy next 'n' bytes literally
/// - Bytes 0x00, 0x09-0x7F: Literal character
/// - Bytes 0x80-0xBF: Back-reference (LZ77)
///   - Combined with next byte: distance = (val & 0x3FFF) >> 3, length = (val & 7) + 3
/// - Bytes 0xC0-0xFF: Space + (byte ^ 0x80)

pub fn decompress(input: &[u8]) -> Vec<u8> {
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

/// PalmDOC LZ77 compression
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;

    while i < input.len() {
        // Try to find a back-reference
        if i > 10 && (input.len() - i) > 10 {
            let mut found = false;
            for chunk_len in (3..=10).rev() {
                if let Some(dist) = find_match(input, i, chunk_len) {
                    if dist <= 2047 {
                        let compound = (dist << 3) | (chunk_len - 3);
                        output.push(0x80 | ((compound >> 8) as u8));
                        output.push((compound & 0xFF) as u8);
                        i += chunk_len;
                        found = true;
                        break;
                    }
                }
            }
            if found {
                continue;
            }
        }

        let c = input[i];
        i += 1;

        // Try space + ASCII optimization
        if c == b' ' && i < input.len() {
            let next = input[i];
            if next >= 0x40 && next <= 0x7F {
                output.push(next ^ 0x80);
                i += 1;
                continue;
            }
        }

        // Literal bytes
        if c == 0 || (c > 8 && c < 0x80) {
            output.push(c);
        } else {
            // Binary data (bytes 1-8 or >= 0x80)
            let start = i - 1;
            let mut binary_data = vec![c];

            while i < input.len() && binary_data.len() < 8 {
                let next = input[i];
                if next == 0 || (next > 8 && next < 0x80) {
                    break;
                }
                binary_data.push(next);
                i += 1;
            }

            output.push(binary_data.len() as u8);
            output.extend_from_slice(&binary_data);
        }
    }

    output
}

fn find_match(data: &[u8], pos: usize, len: usize) -> Option<usize> {
    if pos < len {
        return None;
    }

    let pattern = &data[pos..pos + len];
    for i in (0..pos - len + 1).rev() {
        if &data[i..i + len] == pattern {
            return Some(pos - i);
        }
    }
    None
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
