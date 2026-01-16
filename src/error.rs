//! Error types for boko operations.

use thiserror::Error;

/// Errors that can occur during ebook reading or writing.
#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("XML parsing error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("Invalid EPUB: {0}")]
    InvalidEpub(String),

    #[error("Invalid MOBI: {0}")]
    InvalidMobi(String),

    #[error("Missing required element: {0}")]
    MissingElement(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("UTF-8 decoding error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

pub type Result<T> = std::result::Result<T, Error>;
