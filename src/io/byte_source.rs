use std::fs::File;
use std::io;
#[cfg(not(unix))]
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

    /// Reads exactly `len` bytes starting at `offset`.
    /// This must NOT modify any internal cursor position.
    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>>;
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
    fn len(&self) -> u64 { self.len }

    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        use std::os::unix::fs::FileExt; // Enables pread
        let mut buffer = vec![0u8; len];
        self.file.read_exact_at(&mut buffer, offset)?;
        Ok(buffer)
    }
}

#[cfg(not(unix))]
impl ByteSource for FileSource {
    fn len(&self) -> u64 { self.len }

    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        // Fallback for Windows/WASM where pread might not be available directly on File
        // We clone the file handle (which is usually a cheap FD clone) or use a Mutex.
        // For high-performance Windows, prefer `std::os::windows::fs::FileExt::seek_read`

        let mut file_clone = self.file.try_clone()?;
        file_clone.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0u8; len];
        file_clone.read_exact(&mut buffer)?;
        Ok(buffer)
    }
}

// --- Implementation: In-Memory ---

/// An in-memory ByteSource backed by a Vec<u8>.
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

    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let offset = offset as usize;
        if offset > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "offset beyond end of data",
            ));
        }
        let end = (offset + len).min(self.data.len());
        if end - offset < len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough data",
            ));
        }
        Ok(self.data[offset..end].to_vec())
    }
}
