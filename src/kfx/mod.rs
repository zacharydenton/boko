//! KFX (KF10) format reader and writer.
//!
//! KFX is Amazon's latest Kindle format, successor to KF8/AZW3.
//! It uses Amazon's Ion binary format for structured data.

pub(crate) mod ion;
mod reader;
pub mod style;
pub mod writer;

pub use reader::{read_kfx, read_kfx_from_reader};
pub use style::kfx_style_to_parsed;
pub use writer::{write_kfx, write_kfx_to_writer};
