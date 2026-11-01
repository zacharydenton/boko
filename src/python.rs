//! Python bindings for in-memory ebook conversion and inspection.

use std::io::{Cursor, ErrorKind};

use pyo3::exceptions::{PyKeyError, PyPermissionError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::{Book, Error, Format, TocEntry};

fn parse_format(name: &str) -> PyResult<Format> {
    match name.to_ascii_lowercase().as_str() {
        "epub" => Ok(Format::Epub),
        "azw3" => Ok(Format::Azw3),
        "mobi" | "azw" => Ok(Format::Mobi),
        "kfx" => Ok(Format::Kfx),
        "markdown" | "md" => Ok(Format::Markdown),
        _ => Err(PyValueError::new_err(format!("unknown format: {name}"))),
    }
}

fn py_err(error: Error) -> PyErr {
    match error {
        // Byte parsers also report malformed input through io::Error.
        Error::Io(error)
            if matches!(
                error.kind(),
                ErrorKind::InvalidData | ErrorKind::UnexpectedEof
            ) =>
        {
            PyValueError::new_err(error.to_string())
        }
        Error::Io(error) => error.into(),
        Error::UnsupportedFormat { .. } | Error::Malformed { .. } => {
            PyValueError::new_err(error.to_string())
        }
        Error::DrmProtected(_) => PyPermissionError::new_err(error.to_string()),
        Error::NotFound { .. } => PyKeyError::new_err(error.to_string()),
    }
}

/// Convert ebook bytes. Inputs: epub, azw3, mobi/azw, kfx. Outputs: epub, azw3, kfx, markdown/md.
///
/// Returns bytes (UTF-8 for Markdown). Raises ValueError for invalid formats or malformed
/// books, PermissionError for DRM, and OSError for underlying I/O failures.
#[pyfunction]
fn convert(py: Python<'_>, data: &[u8], from_format: &str, to_format: &str) -> PyResult<Vec<u8>> {
    let from = parse_format(from_format)?;
    let to = parse_format(to_format)?;
    if !from.can_import() {
        return Err(PyValueError::new_err(format!(
            "format not supported as input: {from_format}"
        )));
    }
    if !to.can_export() {
        return Err(PyValueError::new_err(format!(
            "format not supported as output: {to_format}"
        )));
    }

    py.detach(|| {
        let book = Book::from_bytes(data, from)?;
        let mut output = Cursor::new(Vec::new());
        book.export(to, &mut output)?;
        Ok(output.into_inner())
    })
    .map_err(py_err)
}

/// Inspect ebook bytes, returning title, authors, language, chapters, and toc_entries.
///
/// Accepts the same input formats and raises the same exceptions as convert.
#[pyfunction]
fn book_info<'py>(py: Python<'py>, data: &[u8], format: &str) -> PyResult<Bound<'py, PyDict>> {
    let from = parse_format(format)?;
    if !from.can_import() {
        return Err(PyValueError::new_err(format!(
            "format not supported as input: {format}"
        )));
    }
    let book = py.detach(|| Book::from_bytes(data, from)).map_err(py_err)?;
    let meta = book.metadata();

    fn count_toc(entries: &[TocEntry]) -> usize {
        entries
            .iter()
            .map(|entry| 1 + count_toc(&entry.children))
            .sum()
    }

    let info = PyDict::new(py);
    info.set_item("title", &meta.title)?;
    info.set_item("authors", &meta.authors)?;
    info.set_item("language", &meta.language)?;
    info.set_item("chapters", book.spine().len())?;
    info.set_item("toc_entries", count_toc(book.toc()))?;
    Ok(info)
}

/// Native ebook conversion and metadata inspection, powered by Boko.
#[pymodule]
mod boko {
    #[pymodule_export]
    use super::{book_info, convert};
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::exceptions::PyFileNotFoundError;

    #[test]
    fn errors_keep_their_python_types() {
        Python::initialize();
        Python::attach(|py| {
            let error = py_err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
            assert!(error.is_instance_of::<PyFileNotFoundError>(py));
            for kind in [ErrorKind::InvalidData, ErrorKind::UnexpectedEof] {
                let error = py_err(Error::Io(std::io::Error::from(kind)));
                assert!(error.is_instance_of::<PyValueError>(py));
            }
            let error = py_err(Error::UnsupportedFormat {
                detail: "test".into(),
            });
            assert!(error.is_instance_of::<PyValueError>(py));
            let error = py_err(Error::Malformed {
                format: Format::Epub,
                context: "test".into(),
            });
            assert!(error.is_instance_of::<PyValueError>(py));
            let error = py_err(Error::DrmProtected(Format::Kfx));
            assert!(error.is_instance_of::<PyPermissionError>(py));
            let error = py_err(Error::NotFound {
                what: "test".into(),
            });
            assert!(error.is_instance_of::<PyKeyError>(py));
        });
    }
}
