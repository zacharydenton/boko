//! WASM bindings for browser-based ebook conversion.
//!
//! This module exposes the core conversion functions to JavaScript via wasm-bindgen.

use std::io::Cursor;
use wasm_bindgen::prelude::*;

use crate::book::{Book, Format};

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
    let mut book =
        Book::from_bytes(data, Format::Epub).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Azw3, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert EPUB to KFX (Kindle Format 10).
///
/// Takes raw EPUB bytes and returns KFX bytes.
#[wasm_bindgen]
pub fn epub_to_kfx(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Epub).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Kfx, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert AZW3 to EPUB.
///
/// Takes raw AZW3 bytes and returns EPUB bytes.
#[wasm_bindgen]
pub fn azw3_to_epub(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Azw3).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Epub, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert KFX to EPUB.
///
/// Takes raw KFX bytes and returns EPUB bytes.
#[wasm_bindgen]
pub fn kfx_to_epub(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Kfx).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Epub, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert MOBI to EPUB.
///
/// Takes raw MOBI bytes and returns EPUB bytes.
/// Handles both legacy MOBI and modern AZW3 (KF8) formats.
#[wasm_bindgen]
pub fn mobi_to_epub(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Mobi).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Epub, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert MOBI to AZW3 (upgrade legacy MOBI to KF8 format).
///
/// Takes raw MOBI bytes and returns AZW3 bytes.
#[wasm_bindgen]
pub fn mobi_to_azw3(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Mobi).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Azw3, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert EPUB to Markdown.
///
/// Takes raw EPUB bytes and returns Markdown text as UTF-8 bytes.
#[wasm_bindgen]
pub fn epub_to_markdown(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Epub).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Markdown, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert AZW3 to Markdown.
///
/// Takes raw AZW3 bytes and returns Markdown text as UTF-8 bytes.
#[wasm_bindgen]
pub fn azw3_to_markdown(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Azw3).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Markdown, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert KFX to Markdown.
///
/// Takes raw KFX bytes and returns Markdown text as UTF-8 bytes.
#[wasm_bindgen]
pub fn kfx_to_markdown(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Kfx).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Markdown, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}

/// Convert MOBI to Markdown.
///
/// Takes raw MOBI bytes and returns Markdown text as UTF-8 bytes.
#[wasm_bindgen]
pub fn mobi_to_markdown(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut book =
        Book::from_bytes(data, Format::Mobi).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let mut output = Cursor::new(Vec::new());
    book.export(Format::Markdown, &mut output)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    Ok(output.into_inner())
}
