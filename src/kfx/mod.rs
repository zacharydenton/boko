//! KFX (KF10) format reader.
//!
//! KFX is Amazon's latest Kindle format, successor to KF8/AZW3.
//! It uses Amazon's Ion binary format for structured data.

mod ion;
mod reader;

pub use reader::{read_kfx, read_kfx_from_reader};
