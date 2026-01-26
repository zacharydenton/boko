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

// ============================================================================
// Image Dimension Extraction
// ============================================================================

/// Extract image dimensions from raw image data.
///
/// Supports PNG, JPEG, and GIF formats by parsing header bytes.
/// Returns `(width, height)` or `None` if format is unrecognized.
///
/// # Examples
///
/// ```ignore
/// let png_data = include_bytes!("../tests/fixtures/image.png");
/// if let Some((w, h)) = extract_image_dimensions(png_data) {
///     println!("Image is {}x{}", w, h);
/// }
/// ```
pub fn extract_image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 {
        return None;
    }

    // PNG: width/height at bytes 16-23 in IHDR chunk
    if data.len() >= 24 && data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47 {
        let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((width, height));
    }

    // JPEG: Need to parse SOF markers
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        return extract_jpeg_dimensions(data);
    }

    // GIF: width/height at bytes 6-9 (little-endian)
    if data.len() >= 10 && data[0] == 0x47 && data[1] == 0x49 && data[2] == 0x46 {
        let width = u16::from_le_bytes([data[6], data[7]]) as u32;
        let height = u16::from_le_bytes([data[8], data[9]]) as u32;
        return Some((width, height));
    }

    None
}

/// Extract dimensions from JPEG data by parsing SOF markers.
fn extract_jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }

        let marker = data[i + 1];

        // SOF markers (Start of Frame) - various encoding types
        if matches!(marker, 0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF) {
            if i + 9 < data.len() {
                let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                return Some((width, height));
            }
        }

        // Skip to next marker
        if i + 3 < data.len() {
            let length = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            i += 2 + length;
        } else {
            break;
        }
    }
    None
}

/// Detect MIME type from file extension or magic bytes.
///
/// Returns a static string like "image/jpeg", "image/png", etc.
pub fn detect_mime_type(filename: &str, data: &[u8]) -> Option<&'static str> {
    let filename_lower = filename.to_lowercase();

    // Check by extension first
    if filename_lower.ends_with(".jpg") || filename_lower.ends_with(".jpeg") {
        return Some("image/jpeg");
    }
    if filename_lower.ends_with(".png") {
        return Some("image/png");
    }
    if filename_lower.ends_with(".gif") {
        return Some("image/gif");
    }
    if filename_lower.ends_with(".svg") {
        return Some("image/svg+xml");
    }
    if filename_lower.ends_with(".webp") {
        return Some("image/webp");
    }

    // Check magic bytes
    if data.len() >= 4 {
        if data[0] == 0xFF && data[1] == 0xD8 {
            return Some("image/jpeg");
        }
        if data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47 {
            return Some("image/png");
        }
        if data[0] == 0x47 && data[1] == 0x49 && data[2] == 0x46 {
            return Some("image/gif");
        }
    }

    None
}

// ============================================================================
// Encoding Detection
// ============================================================================

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
