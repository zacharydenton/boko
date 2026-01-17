mod headers;
mod huffcdic;
mod index;
mod palmdoc;
mod reader;
mod skeleton;
mod transform;
mod writer;
mod writer_transform;

pub use reader::{read_mobi, read_mobi_from_reader};
pub use writer::{write_mobi, write_mobi_to_writer};

/// Parse Kindle base32 encoding (0-9A-V) to number.
/// Used for kindle: URI references like kindle:embed:XXXX.
#[inline]
fn parse_base32(s: &[u8]) -> usize {
    let mut result = 0usize;
    for &b in s {
        result = result.wrapping_mul(32);
        let val = match b {
            b'0'..=b'9' => (b - b'0') as usize,
            b'A'..=b'V' => (b - b'A') as usize + 10,
            b'a'..=b'v' => (b - b'a') as usize + 10,
            _ => continue,
        };
        result = result.wrapping_add(val);
    }
    result
}
