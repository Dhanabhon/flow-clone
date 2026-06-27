//! FlowClone disk discovery & metadata.
//!
//! Phase 1 uses a mock [`DiskCatalog`] so FlowClone never touches real disks.
//! Platform-specific backends live in [`macos`], [`windows`], and [`linux`] for
//! the later privileged-access phase.

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
    /// Pick the current backend.
    ///
    /// TODO: replace this mock catalog with the platform backend once the
    /// privileged macOS helper and destructive-write safety gates are ready.
    pub fn platform_default() -> Self {
        Self::mock()
    }

    /// Mock disk catalog used by the MVP.
    pub fn mock() -> Self {
        Self(Arc::new(MockCatalog))
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

/// Deterministic Phase 1 catalog. Set `FLOWCLONE_MOCK_DISKS=one` to test the
/// image-migration path with only one external SSD.
pub struct MockCatalog;

impl DiskCatalogApi for MockCatalog {
    fn list(&self) -> Result<Vec<DiskInfo>> {
        let mut disks = vec![
            DiskInfo {
                device_path: "/dev/disk4".into(),
                bsd_name: "disk4".into(),
                model: "Samsung 970 EVO Plus".into(),
                vendor: Some("Samsung".into()),
                serial: Some("S5H9NX0R123456".into()),
                total_bytes: 512_000_000_000,
                used_bytes: Some(412_000_000_000),
                connection: Connection::Usb,
                filesystem: Some("APFS".into()),
                health: Health::Healthy,
                volume_name: Some("Macintosh Clone".into()),
                ..DiskInfo::default()
            },
            DiskInfo {
                device_path: "/dev/disk5".into(),
                bsd_name: "disk5".into(),
                model: "Kingston NV3".into(),
                vendor: Some("Kingston".into()),
                serial: Some("50026B7784A2F3D1".into()),
                total_bytes: 1_000_000_000_000,
                used_bytes: Some(0),
                connection: Connection::Usb,
                filesystem: None,
                health: Health::Healthy,
                volume_name: Some("New SSD".into()),
                ..DiskInfo::default()
            },
        ];

        if std::env::var("FLOWCLONE_MOCK_DISKS").as_deref() == Ok("one") {
            disks.truncate(1);
        }

        Ok(disks)
    }
}
