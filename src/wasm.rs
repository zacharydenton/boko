//! WASM bindings for browser-based ebook conversion.
//!
//! This module exposes the core conversion functions to JavaScript via wasm-bindgen.

use std::io::Cursor;
use wasm_bindgen::prelude::*;

use crate::model::{Book, Format, TocEntry};

/// Initialize panic hook for better error messages in the browser console.
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "wasm")]
    console_error_panic_hook::set_once();
}

/// Parse a format name (as used by the JS API) into a [`Format`].
fn parse_format(name: &str) -> Result<Format, JsValue> {
    match name.to_ascii_lowercase().as_str() {
        "epub" => Ok(Format::Epub),
        "azw3" => Ok(Format::Azw3),
        "mobi" | "azw" => Ok(Format::Mobi),
        "kfx" => Ok(Format::Kfx),
        "markdown" | "md" => Ok(Format::Markdown),
        _ => Err(JsValue::from_str(&format!("unknown format: {name}"))),
    }
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Convert an ebook from one format to another.
///
/// `from` and `to` are format names: `"epub"`, `"azw3"`, `"mobi"`, `"kfx"`,
/// or `"markdown"` (`"md"`). Any importable `from` (EPUB, AZW3, MOBI, KFX)
/// can be converted to any exportable `to` (EPUB, AZW3, KFX, Markdown).
///
/// Takes the raw input bytes and returns the converted output bytes
/// (UTF-8 text for Markdown).
#[wasm_bindgen]
pub fn convert(data: &[u8], from: &str, to: &str) -> Result<Vec<u8>, JsValue> {
    let from = parse_format(from)?;
    let to = parse_format(to)?;

    if !from.can_import() {
        return Err(JsValue::from_str(&format!(
            "format not supported as input: {from:?}"
        )));
    }
    if !to.can_export() {
        return Err(JsValue::from_str(&format!(
            "format not supported as output: {to:?}"
        )));
    }

    let book = Book::from_bytes(data, from).map_err(js_err)?;

    let mut output = Cursor::new(Vec::new());
    book.export(to, &mut output).map_err(js_err)?;

    Ok(output.into_inner())
}

/// Inspect an ebook's metadata without converting it.
///
/// `from` is the input format name (see [`convert`]). Returns a JSON string:
/// `{"title": ..., "authors": [...], "language": ..., "chapters": n, "toc_entries": n}`.
/// Call `JSON.parse` on the result in JavaScript.
#[wasm_bindgen]
pub fn book_info(data: &[u8], from: &str) -> Result<JsValue, JsValue> {
    let from = parse_format(from)?;
    if !from.can_import() {
        return Err(JsValue::from_str(&format!(
            "format not supported as input: {from:?}"
        )));
    }

    let book = Book::from_bytes(data, from).map_err(js_err)?;
    let meta = book.metadata();

    fn count_toc(entries: &[TocEntry]) -> usize {
        entries.iter().map(|e| 1 + count_toc(&e.children)).sum()
    }

    fn json_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    let authors: Vec<String> = meta.authors.iter().map(|a| json_string(a)).collect();
    let json = format!(
        "{{\"title\":{},\"authors\":[{}],\"language\":{},\"chapters\":{},\"toc_entries\":{}}}",
        json_string(&meta.title),
        authors.join(","),
        json_string(&meta.language),
        book.spine().len(),
        count_toc(book.toc()),
    );

    Ok(JsValue::from_str(&json))
}
