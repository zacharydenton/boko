//! IO Abstractions for random-access byte reading.

mod byte_source;
mod adapter;

pub use byte_source::{ByteSource, FileSource, MemorySource};
pub use adapter::ByteSourceCursor;
