//! KFX (KF10) format reader and writer.
//!
//! KFX is Amazon's latest Kindle format, successor to KF8/AZW3.
//! It uses Amazon's Ion binary format for structured data.
//!
//! ## Module structure
//!
//! - `ion` - Amazon Ion binary format parser and writer
//! - `symbols` - KFX symbol table and enum
//! - `container` - KFX container format parsing (pure functions)
//! - `schema` - Bidirectional KFX ↔ IR mapping rules
//! - `tokens` - Token stream for import/export
//! - `storyline` - Storyline tokenization and IR building
//! - `transforms` - Attribute value transformers for bidirectional conversion
//! - `fragment` - KFX fragment representation
//! - `serialization` - Binary container format serialization
//! - `context` - Export context for central state management

pub mod container;
pub mod context;
pub mod fragment;
pub mod ion;
pub mod schema;
pub mod serialization;
pub mod storyline;
pub mod symbols;
pub mod tokens;
pub mod transforms;
