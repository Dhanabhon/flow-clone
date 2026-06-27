//! Phase 1 raw device writer placeholder.

use std::io;
use std::path::Path;

/// Placeholder writer. Real disk writes are disabled until the privileged
/// helper and safety review are implemented.
pub struct RawWriter;

impl RawWriter {
    pub fn open(_path: &Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "raw disk writes are disabled in FlowClone Phase 1",
        ))
    }

    /// TODO: implement through the macOS privileged helper after safety gates
    /// are complete.
    pub fn write_block(&mut self, _buf: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "raw disk writes are disabled in FlowClone Phase 1",
        ))
    }

    /// Flush is a no-op while writes are disabled.
    pub fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
