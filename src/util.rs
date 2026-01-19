//! Utility functions with platform-specific implementations.

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

/// Generate a simple UUID v4 (random)
pub fn uuid_v4() -> String {
    let seed = time_seed_nanos();

    // Simple PRNG for UUID generation (not cryptographically secure, but fine for identifiers)
    let mut state = seed;
    let mut bytes = [0u8; 16];
    for byte in &mut bytes {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (state >> 33) as u8;
    }

    // Set version (4) and variant (2)
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}
