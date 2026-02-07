use std::fs::File;
use std::io;
#[cfg(all(not(unix), not(windows)))]
use std::io::{Read, Seek, SeekFrom};

/// A thread-safe, random-access source of bytes.
/// Allows multiple threads to read different parts of the source simultaneously.
pub trait ByteSource: Send + Sync {
    /// Returns the total length of the source.
    fn len(&self) -> u64;

    /// Returns true if the source is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reads bytes starting at `offset` into the provided buffer.
    /// Returns the number of bytes read (must be exactly `buf.len()` or error).
    /// This must NOT modify any internal cursor position.
    fn read_at_into(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;

    /// Reads exactly `len` bytes starting at `offset`.
    /// This must NOT modify any internal cursor position.
    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let read = self.read_at_into(offset, &mut buf)?;
        if read != len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough data",
            ));
        }
        Ok(buf)
    }
}

// --- Implementation: Local File ---

pub struct FileSource {
    file: File, // internal file handle
    len: u64,
}

impl FileSource {
    pub fn new(file: File) -> io::Result<Self> {
        let len = file.metadata()?.len();
        Ok(Self { file, len })
    }
}

#[cfg(unix)]
impl ByteSource for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at_into(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        use std::os::unix::fs::FileExt; // Enables pread
        self.file.read_exact_at(buf, offset)?;
        Ok(buf.len())
    }
}

#[cfg(windows)]
impl ByteSource for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at_into(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        use std::os::windows::fs::FileExt;
        let read = self.file.seek_read(buf, offset)?;
        if read != buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough data",
            ));
        }
        Ok(read)
    }
}

#[cfg(all(not(unix), not(windows)))]
impl ByteSource for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at_into(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        // Fallback for WASM and other platforms
        let mut file_clone = self.file.try_clone()?;
        file_clone.seek(SeekFrom::Start(offset))?;
        file_clone.read_exact(buf)?;
        Ok(buf.len())
    }
}

// --- Implementation: In-Memory ---

/// An in-memory ByteSource backed by a `Vec<u8>`.
pub struct MemorySource {
    data: Vec<u8>,
}

impl MemorySource {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl ByteSource for MemorySource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at_into(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let offset = offset as usize;
        if offset > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "offset beyond end of data",
            ));
        }
        let end = (offset + buf.len()).min(self.data.len());
        if end - offset < buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough data",
            ));
        }
        buf.copy_from_slice(&self.data[offset..end]);
        Ok(buf.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_source_read_at_into() {
        let source = MemorySource::new(b"hello world".to_vec());
        let mut buf = [0u8; 5];
        let read = source.read_at_into(6, &mut buf).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf, b"world");
    }

    #[test]
    fn test_memory_source_read_at() {
        let source = MemorySource::new(b"abcdef".to_vec());
        let data = source.read_at(1, 3).unwrap();
        assert_eq!(&data, b"bcd");
    }
}
