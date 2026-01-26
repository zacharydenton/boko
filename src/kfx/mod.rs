//! KFX (KF10) format reader and writer.
//!
//! KFX is Amazon's latest Kindle format, successor to KF8/AZW3.
//! It uses Amazon's Ion binary format for structured data.
//!
//! ## Module structure
//!
//! - `ion` - Amazon Ion binary format parser
//! - `symbols` - KFX symbol table and enum
//! - `container` - KFX container format parsing (pure functions)

pub mod container;
pub mod ion;
pub mod symbols;
