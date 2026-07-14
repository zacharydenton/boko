//! Typed error handling for boko's importers, exporters, and [`Book`] API.
//!
//! [`Error`] lets callers programmatically distinguish failure classes
//! (unsupported format, malformed input, DRM protection, missing resources)
//! instead of string-matching on `io::Error`. Internal code that produces
//! plain I/O errors keeps working via `From<std::io::Error>` and `?`.
//!
//! [`Book`]: crate::Book

use crate::model::Format;

/// Errors produced by boko's importers, exporters, and `Book` API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Underlying I/O failure (file access, writer errors).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The input doesn't match any supported format, or the format doesn't
    /// support the requested direction (e.g. writing MOBI).
    #[error("unsupported format: {detail}")]
    UnsupportedFormat {
        /// Human-readable explanation of what was unsupported.
        detail: String,
    },
    /// The input claims to be `format` but its structure is invalid.
    #[error("malformed {format:?} input: {context}")]
    Malformed {
        /// The format the input claimed to be.
        format: Format,
        /// What was invalid about the input.
        context: String,
    },
    /// The input is DRM-protected / encrypted; boko does not decrypt.
    #[error("{0:?} file is DRM-protected; boko does not decrypt")]
    DrmProtected(Format),
    /// A referenced chapter, asset, or resource does not exist.
    #[error("not found: {what}")]
    NotFound {
        /// The missing chapter, asset, or resource.
        what: String,
    },
}

/// Convenience alias used throughout boko's public API.
pub type Result<T> = std::result::Result<T, Error>;

/// Compat shim: lets callers that still want `io::Result` convert back.
impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        // Explicit arms (no `_`) so a new variant forces a decision here.
        match e {
            Error::Io(io) => io,
            Error::NotFound { .. } => std::io::Error::new(std::io::ErrorKind::NotFound, e),
            Error::UnsupportedFormat { .. } => {
                std::io::Error::new(std::io::ErrorKind::Unsupported, e)
            }
            Error::DrmProtected(_) => std::io::Error::new(std::io::ErrorKind::PermissionDenied, e),
            Error::Malformed { .. } => std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_roundtrip() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let err = Error::from(io);
        let back: std::io::Error = err.into();
        assert_eq!(back.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn kind_mapping() {
        let err: std::io::Error = Error::NotFound {
            what: "chapter 3".into(),
        }
        .into();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        let err: std::io::Error = Error::UnsupportedFormat {
            detail: "mobi write".into(),
        }
        .into();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);

        let err: std::io::Error = Error::DrmProtected(Format::Azw3).into();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

        let err: std::io::Error = Error::Malformed {
            format: Format::Kfx,
            context: "bad".into(),
        }
        .into();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn display_output() {
        let err = Error::Malformed {
            format: Format::Kfx,
            context: "truncated entity table".into(),
        };
        assert_eq!(
            err.to_string(),
            "malformed Kfx input: truncated entity table"
        );
        let err = Error::NotFound {
            what: "images/cover.jpg".into(),
        };
        assert_eq!(err.to_string(), "not found: images/cover.jpg");
    }
}
