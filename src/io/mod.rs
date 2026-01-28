//! IO Abstractions for random-access byte reading.

mod adapter;
mod byte_source;

pub use adapter::ByteSourceCursor;
pub use byte_source::{ByteSource, FileSource, MemorySource};
