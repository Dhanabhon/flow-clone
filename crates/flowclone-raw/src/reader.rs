//! Raw device reader.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Buffered reader over a raw device. On macOS this is typically a
/// `/dev/rdiskN` node, opened with `O_RDONLY`.
pub struct RawReader {
    file: File,
}

impl RawReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self { file })
    }

    /// Read up to `buf.len()` bytes into `buf`. Returns `0` at EOF.
    pub fn read_block(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}
