//! Utility functions with platform-specific implementations.

use std::borrow::Cow;

/// Get a time-based seed value for pseudo-random number generation.
///
/// On native platforms, uses `SystemTime::now()`.
/// On WASM, uses `js_sys::Date::now()`.
#[cfg(not(target_arch = "wasm32"))]
pub fn time_seed_nanos() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(12345)
}

#[cfg(target_arch = "wasm32")]
pub fn time_seed_nanos() -> u64 {
    // js_sys::Date::now() returns milliseconds as f64
    (js_sys::Date::now() * 1_000_000.0) as u64
}

/// Get current time as seconds since Unix epoch.
///
/// On native platforms, uses `SystemTime::now()`.
/// On WASM, uses `js_sys::Date::now()`.
#[cfg(not(target_arch = "wasm32"))]
pub fn time_now_secs() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
pub fn time_now_secs() -> u32 {
    // js_sys::Date::now() returns milliseconds as f64
    (js_sys::Date::now() / 1000.0) as u32
}

/// Decode bytes to a string, handling various encodings.
///
/// This function:
/// 1. First tries UTF-8 (handles BOM automatically via encoding_rs)
/// 2. If malformed, tries the hint encoding (from `<?xml encoding="..."?>`)
/// 3. Falls back to Windows-1252 (common in old ebooks)
///
/// # Arguments
///
/// * `bytes` - The raw bytes to decode
/// * `hint_encoding` - Optional encoding name from XML declaration or document metadata
///
/// # Returns
///
/// The decoded string. Uses `Cow<str>` to avoid allocation when the input is valid UTF-8.
///
/// # Examples
///
/// ```ignore
/// // Valid UTF-8
/// let utf8_bytes = "Hello, World!".as_bytes();
/// assert_eq!(decode_text(utf8_bytes, None), "Hello, World!");
///
/// // With encoding hint (e.g., from XML declaration)
/// let bytes = b"Hello";
/// assert_eq!(decode_text(bytes, Some("utf-8")), "Hello");
/// ```
pub fn decode_text<'a>(bytes: &'a [u8], hint_encoding: Option<&str>) -> Cow<'a, str> {
    // Try UTF-8 first (handles BOM automatically)
    let (result, _encoding, malformed) = encoding_rs::UTF_8.decode(bytes);

    if !malformed {
        return result;
    }

    // If UTF-8 failed, try the hint encoding
    if let Some(name) = hint_encoding {
        if let Some(encoding) = encoding_rs::Encoding::for_label(name.as_bytes()) {
            let (result, _, _) = encoding.decode(bytes);
            return result;
        }
    }

    // Fallback: Windows-1252 (common in old ebooks, superset of ISO-8859-1)
    let (result, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
    result
}

/// Extract encoding from XML declaration.
///
/// Parses `<?xml ... encoding="..." ?>` to extract the encoding name.
///
/// # Arguments
///
/// * `bytes` - The raw bytes (only the first ~100 bytes are checked)
///
/// # Returns
///
/// The encoding name if found, or `None`.
pub fn extract_xml_encoding(bytes: &[u8]) -> Option<&str> {
    // Only check the first 100 bytes for the XML declaration
    let check_len = bytes.len().min(100);
    let prefix = &bytes[..check_len];

    // Look for <?xml
    let xml_start = prefix.windows(5).position(|w| w == b"<?xml")?;
    let after_xml = &prefix[xml_start..];

    // Look for encoding="..." or encoding='...'
    let enc_pos = after_xml
        .windows(9)
        .position(|w| w.eq_ignore_ascii_case(b"encoding="))?;
    let after_enc = &after_xml[enc_pos + 9..];

    if after_enc.is_empty() {
        return None;
    }

    let quote = after_enc[0];
    if quote != b'"' && quote != b'\'' {
        return None;
    }

    let value_start = 1;
    let value_end = after_enc[value_start..]
        .iter()
        .position(|&b| b == quote)?
        + value_start;

    std::str::from_utf8(&after_enc[value_start..value_end]).ok()
}
