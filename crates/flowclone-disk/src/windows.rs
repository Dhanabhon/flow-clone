//! Windows disk discovery (placeholder for cross-platform builds).
//!
//! A real backend will use the Windows `IOCTL_STORAGE_*` family via the
//! `windows` crate. For now it returns an empty list so the crate compiles
//! on non-macOS hosts.

use crate::{DiskCatalogApi, DiskInfo, Result};

/// Windows disk catalog.
pub struct WindowsCatalog;

impl WindowsCatalog {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskCatalogApi for WindowsCatalog {
    fn list(&self) -> Result<Vec<DiskInfo>> {
        Ok(Vec::new())
    }
}
