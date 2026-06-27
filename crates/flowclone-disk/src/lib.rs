//! FlowClone disk discovery & metadata.
//!
//! Provides a platform-abstract [`DiskCatalog`] that returns [`DiskInfo`] for
//! every block device visible to the system. Platform-specific backends live in
//! [`macos`], [`windows`], and [`linux`].

pub mod linux;
pub mod macos;
pub mod model;
pub mod windows;

pub use model::{Connection, DiskInfo, Health};

use std::sync::Arc;

/// Result alias for disk operations.
pub type Result<T> = anyhow::Result<T>;

/// Trait for enumerating disks. The default implementation is selected per
/// platform by [`DiskCatalog::platform_default`].
pub trait DiskCatalogApi: Send + Sync {
    /// List all disks the backend can see.
    fn list(&self) -> Result<Vec<DiskInfo>>;

    /// Convenience: find a single disk by device path.
    fn find(&self, device_path: &str) -> Result<Option<DiskInfo>> {
        Ok(self
            .list()?
            .into_iter()
            .find(|d| d.device_path == device_path))
    }

    /// Whether raw device access currently requires the privileged helper.
    /// macOS returns `true` when not running as an admin/root process.
    fn needs_privilege(&self) -> bool {
        false
    }
}

/// Concrete, clonable catalog handle.
#[derive(Clone)]
pub struct DiskCatalog(pub Arc<dyn DiskCatalogApi>);

impl DiskCatalog {
    /// Pick the right backend for the current platform.
    pub fn platform_default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self(Arc::new(macos::MacosCatalog::new()))
        }
        #[cfg(target_os = "windows")]
        {
            Self(Arc::new(windows::WindowsCatalog::new()))
        }
        #[cfg(target_os = "linux")]
        {
            Self(Arc::new(linux::LinuxCatalog::new()))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            compile_error!("FlowClone only supports macOS, Windows, and Linux");
        }
    }
}

impl DiskCatalogApi for DiskCatalog {
    fn list(&self) -> Result<Vec<DiskInfo>> {
        self.0.list()
    }
    fn find(&self, device_path: &str) -> Result<Option<DiskInfo>> {
        self.0.find(device_path)
    }
    fn needs_privilege(&self) -> bool {
        self.0.needs_privilege()
    }
}
