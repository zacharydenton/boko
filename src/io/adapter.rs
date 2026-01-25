use super::byte_source::ByteSource;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;

/// Wraps an Arc<ByteSource> into a stateful `Read + Seek` stream.
/// Used to pass our ByteSource into libraries like `zip::ZipArchive`.
pub struct ByteSourceCursor {
    inner: Arc<dyn ByteSource>,
    position: u64,
}

impl ByteSourceCursor {
    pub fn new(inner: Arc<dyn ByteSource>) -> Self {
        Self { inner, position: 0 }
    }
}

impl Read for ByteSourceCursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let total_len = self.inner.len();
        
        // 1. Clamp read to file bounds
        if self.position >= total_len {
            return Ok(0);
        }
        let max_read = (total_len - self.position).min(buf.len() as u64) as usize;
        
        // 2. Fetch data (stateless)
        let data = self.inner.read_at(self.position, max_read)?;
        
        // 3. Copy to buffer and advance cursor
        buf[..data.len()].copy_from_slice(&data);
        self.position += data.len() as u64;
        
        Ok(data.len())
    }
}

impl Seek for ByteSourceCursor {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let total_len = self.inner.len() as i64;
        let current_pos = self.position as i64;
        
        let new_pos = match pos {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(p) => total_len + p,
            SeekFrom::Current(p) => current_pos + p,
        };

        if new_pos < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Seek before 0"));
        }

        self.position = new_pos as u64;
        Ok(self.position)
    }
}
