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
