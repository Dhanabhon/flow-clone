//! macOS disk discovery.
//!
//! Uses `diskutil info -plist -all` and parses the property list. Parsing the
//! plist avoids depending on `ioreg` text output, which is unstable.

use crate::{DiskCatalogApi, DiskInfo, Result};
use std::process::Command;

/// macOS disk catalog backed by `diskutil`.
pub struct MacosCatalog;

impl MacosCatalog {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskCatalogApi for MacosCatalog {
    fn list(&self) -> Result<Vec<DiskInfo>> {
        let output = Command::new("diskutil").args(["list", "-plist"]).output()?;

        if !output.status.success() {
            tracing::warn!("diskutil list failed; returning empty disk set");
            return Ok(Vec::new());
        }

        // Full plist parsing lands with a plist dependency in a follow-up.
        // For now we parse the minimal `WholeDisks` / device keys from the
        // plist text so the catalog is functional end-to-end.
        let plist = String::from_utf8_lossy(&output.stdout);
        Ok(parse_diskutil_plist(&plist))
    }

    fn needs_privilege(&self) -> bool {
        // Writing to raw devices (/dev/rdisk*) requires root or an admin auth
        // prompt. The privileged helper handles this in a later phase.
        !is_root()
    }
}

fn is_root() -> bool {
    // SAFETY: getuid is always safe to call.
    unsafe { libc_getuid() == 0 }
}

// Avoid adding a libc dependency just for getuid; declare it ourselves.
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// Best-effort extraction of whole-disk device paths from a `diskutil list
/// -plist` payload. Returns placeholders with the path set; richer metadata
/// (model, serial, size) is filled in by per-disk `diskutil info -plist`.
fn parse_diskutil_plist(plist: &str) -> Vec<DiskInfo> {
    let mut disks = Vec::new();
    for line in plist.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix("<string>/dev/disk")
            .and_then(|s| s.strip_suffix("</string>"))
        {
            let cleaned = name.trim_end_matches('>');
            let bsd = format!("disk{cleaned}");
            disks.push(DiskInfo {
                device_path: format!("/dev/{bsd}"),
                bsd_name: bsd,
                ..DiskInfo::default()
            });
        }
    }
    // Deduplicate by device_path; diskutil lists each disk many times.
    disks.dedup_by(|a, b| a.device_path == b.device_path);
    disks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Connection, Health};

    #[test]
    fn parses_a_device_line() {
        // Matches the real `diskutil list -plist` shape: each string on its
        // own line, surrounded by many other plist keys.
        let plist = "<?xml version=\"1.0\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\">\n\
<plist version=\"1.0\">\n\
<array>\n\
<string>/dev/disk0</string>\n\
<string>/dev/disk2</string>\n\
</array>\n\
</plist>";
        let disks = parse_diskutil_plist(plist);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[0].device_path, "/dev/disk0");
        assert_eq!(disks[0].bsd_name, "disk0");
        assert_eq!(disks[1].device_path, "/dev/disk2");
    }

    #[test]
    fn ignores_non_device_strings() {
        let plist = "<array><string>disk0s1</string><string>APFS</string></array>";
        let disks = parse_diskutil_plist(plist);
        assert!(disks.is_empty());
    }

    #[test]
    fn unknown_health_is_default() {
        let info = DiskInfo::default();
        assert_eq!(info.health, Health::Unknown);
    }

    #[test]
    fn connection_is_unknown_by_default() {
        let info = DiskInfo::default();
        assert_eq!(info.connection, Connection::Unknown);
    }
}
