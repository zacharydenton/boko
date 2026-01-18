//! WASM bindings for browser-based ebook conversion.
//!
//! This module exposes the core conversion functions to JavaScript via wasm-bindgen.

use std::io::Cursor;
use wasm_bindgen::prelude::*;

use crate::epub::{read_epub_from_reader, write_epub_to_writer};
use crate::kfx::{read_kfx_from_reader, write_kfx_to_writer};
use crate::mobi::{read_mobi_from_reader, write_mobi_to_writer};

/// Initialize panic hook for better error messages in the browser console.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "wasm")]
    console_error_panic_hook::set_once();
}

/// Convert EPUB to AZW3 (KF8 format).
///
/// Takes raw EPUB bytes and returns AZW3 bytes.
#[wasm_bindgen]
pub fn epub_to_azw3(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let cursor = Cursor::new(data);
    let book = read_epub_from_reader(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Vec::new();
    write_mobi_to_writer(&book, &mut output).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output)
}

/// Convert MOBI/AZW3 to EPUB.
///
/// Takes raw MOBI or AZW3 bytes and returns EPUB bytes.
/// The reader handles both legacy MOBI and modern AZW3 (KF8) formats.
#[wasm_bindgen]
pub fn mobi_to_epub(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let cursor = Cursor::new(data);
    let book = read_mobi_from_reader(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    write_epub_to_writer(&book, &mut output).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert MOBI to AZW3 (upgrade legacy MOBI to KF8 format).
///
/// Takes raw MOBI bytes and returns AZW3 bytes.
/// This is useful for upgrading old MOBI files to the modern KF8 format.
#[wasm_bindgen]
pub fn mobi_to_azw3(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let cursor = Cursor::new(data);
    let book = read_mobi_from_reader(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Vec::new();
    write_mobi_to_writer(&book, &mut output).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output)
}

/// Convert EPUB to KFX (latest Kindle format).
///
/// Takes raw EPUB bytes and returns KFX bytes.
#[wasm_bindgen]
pub fn epub_to_kfx(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let cursor = Cursor::new(data);
    let book = read_epub_from_reader(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Vec::new();
    write_kfx_to_writer(&book, &mut output).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output)
}

/// Convert KFX to EPUB.
///
/// Takes raw KFX bytes and returns EPUB bytes.
#[wasm_bindgen]
pub fn kfx_to_epub(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let cursor = Cursor::new(data);
    let book = read_kfx_from_reader(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    write_epub_to_writer(&book, &mut output).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert KFX to AZW3.
///
/// Takes raw KFX bytes and returns AZW3 bytes.
#[wasm_bindgen]
pub fn kfx_to_azw3(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let cursor = Cursor::new(data);
    let book = read_kfx_from_reader(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Vec::new();
    write_mobi_to_writer(&book, &mut output).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output)
}

/// Convert MOBI/AZW3 to KFX.
///
/// Takes raw MOBI or AZW3 bytes and returns KFX bytes.
#[wasm_bindgen]
pub fn mobi_to_kfx(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let cursor = Cursor::new(data);
    let book = read_mobi_from_reader(cursor).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Vec::new();
    write_kfx_to_writer(&book, &mut output).map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output)
}
