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
//! - `schema` - Bidirectional KFX ↔ IR mapping rules
//! - `tokens` - Token stream for import/export
//! - `storyline` - Storyline tokenization and IR building
//! - `transforms` - Attribute value transformers for bidirectional conversion

pub mod container;
pub mod ion;
pub mod schema;
pub mod storyline;
pub mod symbols;
pub mod tokens;
pub mod transforms;
