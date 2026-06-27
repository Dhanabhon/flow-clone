//! macOS disk discovery.
//!
//! Uses `diskutil list -plist`, then `diskutil info -plist <disk>`, and parses
//! the property lists instead of relying on unstable command text output.

use crate::{Connection, DiskCatalogApi, DiskInfo, Health, Result};
use plist::{Dictionary, Value};
use std::{io::Cursor, process::Command};

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
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("diskutil list failed: {}", stderr.trim());
        }

        let names = parse_whole_disk_names(&output.stdout)?;
        let mut disks = Vec::new();

        for name in names {
            match disk_info(&name) {
                Ok(disk) => disks.push(disk),
                Err(error) => tracing::warn!(%name, %error, "diskutil info failed"),
            }
        }

        Ok(disks)
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

fn disk_info(name: &str) -> Result<DiskInfo> {
    let output = Command::new("diskutil")
        .args(["info", "-plist", name])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("diskutil info failed for {name}");
    }

    parse_disk_info_plist(&output.stdout)
}

fn parse_whole_disk_names(plist: &[u8]) -> Result<Vec<String>> {
    let value = Value::from_reader(Cursor::new(plist))?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| anyhow::anyhow!("diskutil list plist root is not a dictionary"))?;
    let disks = dict
        .get("WholeDisks")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("diskutil list plist missing WholeDisks"))?;

    let mut names = disks
        .iter()
        .filter_map(Value::as_string)
        .filter(|name| name.starts_with("disk"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    Ok(names)
}

fn parse_disk_info_plist(plist: &[u8]) -> Result<DiskInfo> {
    let value = Value::from_reader(Cursor::new(plist))?;
    let dict = value
        .as_dictionary()
        .ok_or_else(|| anyhow::anyhow!("diskutil info plist root is not a dictionary"))?;

    Ok(disk_from_info(dict))
}

fn disk_from_info(dict: &Dictionary) -> DiskInfo {
    let bsd_name = string_value(dict, &["DeviceIdentifier", "ParentWholeDisk"])
        .unwrap_or_else(|| "disk".into());
    let device_path =
        string_value(dict, &["DeviceNode"]).unwrap_or_else(|| format!("/dev/{bsd_name}"));

    DiskInfo {
        device_path,
        bsd_name,
        model: string_value(
            dict,
            &[
                "MediaName",
                "IORegistryEntryName",
                "DeviceModel",
                "VolumeName",
                "DeviceIdentifier",
            ],
        )
        .unwrap_or_else(|| "Unknown disk".into()),
        vendor: string_value(dict, &["DeviceVendor", "VendorName"]),
        serial: string_value(dict, &["SerialNumber", "DeviceSerial", "MediaUUID"]),
        total_bytes: u64_value(dict, &["TotalSize", "Size"]).unwrap_or(0),
        used_bytes: used_bytes(dict),
        connection: connection_from_dict(dict),
        filesystem: string_value(dict, &["FileSystemName", "FilesystemName", "Content"]),
        read_only: bool_value(dict, &["ReadOnly", "MediaReadOnly"]).unwrap_or(false),
        encrypted: bool_value(dict, &["CoreStorageEncrypted", "Encrypted"]).unwrap_or(false),
        health: health_from_dict(dict),
        is_boot: bool_value(dict, &["Bootable", "BootDevice"]).unwrap_or(false),
        volume_name: string_value(dict, &["VolumeName"]),
    }
}

fn string_value(dict: &Dictionary, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| dict.get(key).and_then(Value::as_string))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn bool_value(dict: &Dictionary, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| dict.get(key).and_then(Value::as_boolean))
}

fn u64_value(dict: &Dictionary, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| dict.get(key).and_then(Value::as_unsigned_integer))
}

fn used_bytes(dict: &Dictionary) -> Option<u64> {
    let total = u64_value(dict, &["TotalSize", "Size"])?;
    let free = u64_value(dict, &["FreeSpace", "AvailableSpace"])?;
    Some(total.saturating_sub(free))
}

fn connection_from_dict(dict: &Dictionary) -> Connection {
    let value = string_value(dict, &["BusProtocol", "Protocol", "DeviceProtocol"])
        .unwrap_or_default()
        .to_ascii_lowercase();

    if value.contains("usb") {
        Connection::Usb
    } else if value.contains("thunderbolt") {
        Connection::Thunderbolt
    } else if value.contains("firewire") {
        Connection::Firewire
    } else if value.contains("network") {
        Connection::Network
    } else if bool_value(dict, &["Internal"]).unwrap_or(false) {
        Connection::Internal
    } else {
        Connection::Unknown
    }
}

fn health_from_dict(dict: &Dictionary) -> Health {
    match string_value(dict, &["SMARTStatus", "SmartStatus"])
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "verified" => Health::Healthy,
        "failing" => Health::Failing,
        "not supported" | "unsupported" => Health::Unknown,
        value if value.contains("fail") => Health::Failing,
        value if value.contains("warn") => Health::Warning,
        value if value.contains("verified") => Health::Healthy,
        _ => Health::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_whole_disk_names() {
        let plist = b"<?xml version=\"1.0\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\">\n\
<plist version=\"1.0\">\n\
<dict>
<key>WholeDisks</key>
<array>
<string>disk0</string>
<string>disk2</string>
<string>disk0</string>
</array>
</dict>
</plist>";
        let disks = parse_whole_disk_names(plist).unwrap();
        assert_eq!(disks, ["disk0", "disk2"]);
    }

    #[test]
    fn parses_disk_info_metadata() {
        let plist = b"<?xml version=\"1.0\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\">\n\
<plist version=\"1.0\">\n\
<dict>
<key>DeviceIdentifier</key><string>disk4</string>
<key>DeviceNode</key><string>/dev/disk4</string>
<key>MediaName</key><string>Samsung 970 EVO Plus</string>
<key>DeviceVendor</key><string>Samsung</string>
<key>SerialNumber</key><string>S5H9NX0R123456</string>
<key>TotalSize</key><integer>512000000000</integer>
<key>FreeSpace</key><integer>100000000000</integer>
<key>BusProtocol</key><string>USB</string>
<key>FileSystemName</key><string>APFS</string>
<key>ReadOnly</key><false/>
<key>CoreStorageEncrypted</key><true/>
<key>SMARTStatus</key><string>Verified</string>
<key>Bootable</key><false/>
<key>VolumeName</key><string>Macintosh Clone</string>
</dict>
</plist>";
        let disk = parse_disk_info_plist(plist).unwrap();
        assert_eq!(disk.device_path, "/dev/disk4");
        assert_eq!(disk.bsd_name, "disk4");
        assert_eq!(disk.model, "Samsung 970 EVO Plus");
        assert_eq!(disk.vendor.as_deref(), Some("Samsung"));
        assert_eq!(disk.serial.as_deref(), Some("S5H9NX0R123456"));
        assert_eq!(disk.total_bytes, 512_000_000_000);
        assert_eq!(disk.used_bytes, Some(412_000_000_000));
        assert_eq!(disk.connection, Connection::Usb);
        assert_eq!(disk.filesystem.as_deref(), Some("APFS"));
        assert!(!disk.read_only);
        assert!(disk.encrypted);
        assert_eq!(disk.health, Health::Healthy);
        assert!(!disk.is_boot);
        assert_eq!(disk.volume_name.as_deref(), Some("Macintosh Clone"));
    }

    #[test]
    fn maps_failing_smart_status() {
        let mut dict = Dictionary::new();
        dict.insert("SMARTStatus".into(), Value::String("Failing".into()));
        assert_eq!(health_from_dict(&dict), Health::Failing);
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
