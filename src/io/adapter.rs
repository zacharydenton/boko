use super::byte_source::ByteSource;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;

/// Wraps an `Arc<ByteSource>` into a stateful `Read + Seek` stream.
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
        let read = self
            .inner
            .read_at_into(self.position, &mut buf[..max_read])?;

        // 3. Advance cursor
        self.position += read as u64;

        Ok(read)
    }
}

impl Seek for ByteSourceCursor {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "Seek out of range");

        // All arithmetic checked: offsets can come from pathological archive
        // metadata, and an i64 add here would abort under the release
        // overflow checks (or a >i64::MAX Start would wrap negative).
        let new_pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::End(p) => {
                let total_len = i64::try_from(self.inner.len()).map_err(|_| invalid())?;
                u64::try_from(total_len.checked_add(p).ok_or_else(invalid)?)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Seek before 0"))?
            }
            SeekFrom::Current(p) => {
                let current = i64::try_from(self.position).map_err(|_| invalid())?;
                u64::try_from(current.checked_add(p).ok_or_else(invalid)?)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Seek before 0"))?
            }
        };

        self.position = new_pos;
        Ok(self.position)
    }
}
