//! Raw device writer.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

/// Writer over a raw device. Opens the target node with write access.
pub struct RawWriter {
    file: File,
}

impl RawWriter {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).open(path)?;
        Ok(Self { file })
    }

    /// Write a full block. Errors propagate as `io::Error`.
    pub fn write_block(&mut self, buf: &[u8]) -> io::Result<()> {
        self.file.write_all(buf)
    }

    /// Flush any kernel buffer to the device.
    pub fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
