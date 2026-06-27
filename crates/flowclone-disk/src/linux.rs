//! Linux disk discovery (placeholder for cross-platform builds).
//!
//! The real backend will read `/sys/block` and `lsblk --json`. For now it
//! returns an empty list so the crate compiles on non-macOS hosts.

use crate::{DiskCatalogApi, DiskInfo, Result};

/// Linux disk catalog.
pub struct LinuxCatalog;

impl LinuxCatalog {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskCatalogApi for LinuxCatalog {
    fn list(&self) -> Result<Vec<DiskInfo>> {
        Ok(Vec::new())
    }
}
