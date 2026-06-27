//! Disk data model — what FlowClone knows about each disk.

use serde::{Deserialize, Serialize};

/// Coarse health classification, surfaced as a colored badge in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    #[default]
    Unknown,
    Healthy,
    Warning,
    Failing,
}

/// How the disk is attached. Drives the icon in disk cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Connection {
    #[default]
    Unknown,
    Internal,
    Usb,
    Thunderbolt,
    Firewire,
    Network,
}

/// All metadata FlowClone shows for one disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Stable device path, e.g. `/dev/disk2`. Used as the identity key.
    pub device_path: String,
    /// BSD-style device name, e.g. `disk2`.
    pub bsd_name: String,
    /// User-facing model name, e.g. `Samsung 990 Pro`.
    pub model: String,
    /// Vendor string if available.
    pub vendor: Option<String>,
    /// Serial number if available. Shown in confirmation screen.
    pub serial: Option<String>,
    /// Total capacity in bytes.
    pub total_bytes: u64,
    /// Used capacity in bytes, if known.
    pub used_bytes: Option<u64>,
    /// Connection bus type.
    pub connection: Connection,
    /// Filesystem on the disk, if any, e.g. `APFS`.
    pub filesystem: Option<String>,
    /// Whether the disk is read-only.
    pub read_only: bool,
    /// Whether the disk is encrypted.
    pub encrypted: bool,
    /// Coarse health classification.
    pub health: Health,
    /// Whether this is the current boot device. FlowClone blocks cloning it.
    pub is_boot: bool,
    /// Human-readable volume name if mounted.
    pub volume_name: Option<String>,
}

impl DiskInfo {
    /// Create a placeholder for use in tests and empty states.
    pub fn placeholder(path: impl Into<String>) -> Self {
        Self {
            device_path: path.into(),
            ..Self::default()
        }
    }
}

impl Default for DiskInfo {
    fn default() -> Self {
        Self {
            device_path: String::new(),
            bsd_name: String::new(),
            model: "Unknown disk".into(),
            vendor: None,
            serial: None,
            total_bytes: 0,
            used_bytes: None,
            connection: Connection::Unknown,
            filesystem: None,
            read_only: false,
            encrypted: false,
            health: Health::Unknown,
            is_boot: false,
            volume_name: None,
        }
    }
}
