//! macOS disk discovery.
//!
//! Uses `diskutil list -plist`, then `diskutil info -plist <disk>`, and parses
//! the property lists instead of relying on unstable command text output.

use crate::{Connection, DiskCatalogApi, DiskInfo, Health, Result};
use plist::{Dictionary, Value};
use std::{collections::HashMap, io::Cursor, process::Command};

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

        // Usage lives on the disk's *volumes*, never on the whole disk itself
        // (a whole disk reports no FileSystemName and FreeSpace == 0). Aggregate
        // each disk's mounted volumes up front so every card can show real used
        // space: APFS via `CapacityInUse`, other filesystems via `df`.
        let used_by_disk = used_bytes_by_disk(&output.stdout, &df_used_by_device());

        let mut disks = Vec::new();

        for name in names {
            match disk_info(&name) {
                Ok(mut disk) => {
                    if let Some(used) = used_by_disk.get(&disk.bsd_name) {
                        disk.used_bytes = Some((*used).min(disk.total_bytes));
                    }
                    disks.push(disk);
                }
                Err(error) if is_ignored_disk_info_error(&error) => {}
                Err(error) => tracing::warn!(%name, %error, "diskutil info failed"),
            }
        }

        Ok(disks)
    }

    fn find(&self, device_path: &str) -> Result<Option<DiskInfo>> {
        let name = device_path
            .strip_prefix("/dev/")
            .unwrap_or(device_path)
            .to_string();
        match disk_info(&name) {
            Ok(disk) => Ok(Some(disk)),
            Err(error) if is_ignored_disk_info_error(&error) => Err(error),
            Err(_) => Ok(None),
        }
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

fn is_ignored_disk_info_error(error: &anyhow::Error) -> bool {
    error.to_string().contains("virtual disk")
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
    if is_virtual_disk(dict) {
        anyhow::bail!(
            "virtual disk is not a physical source; choose the parent external disk from diskutil list"
        );
    }

    Ok(disk_from_info(dict))
}

fn is_virtual_disk(dict: &Dictionary) -> bool {
    string_value(dict, &["VirtualOrPhysical"])
        .map(|value| value.eq_ignore_ascii_case("virtual"))
        .unwrap_or(false)
        || bool_value(dict, &["Virtual"]).unwrap_or(false)
        || is_apfs_container_or_volume(dict)
}

fn is_apfs_container_or_volume(dict: &Dictionary) -> bool {
    let value = string_value(dict, &["FileSystemName", "FilesystemName", "Content"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    value == "apfs" || value.contains("apple_apfs") || value.contains("apfs container")
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
    string_value(dict, &["FileSystemName", "FilesystemName"])?;
    let total = u64_value(dict, &["TotalSize", "Size"])?;
    let free = u64_value(dict, &["FreeSpace", "AvailableSpace"])?;
    Some(total.saturating_sub(free))
}

/// Map each whole disk (`disk7`) to the sum of its mounted volumes' used bytes.
///
/// `diskutil list -plist` carries `AllDisksAndPartitions`, where APFS container
/// disks expose `APFSVolumes[].CapacityInUse` and an `APFSPhysicalStores` link
/// back to the physical partition they live on. Non-APFS partitions (NTFS,
/// exFAT, HFS, FAT) only report a `Size`, so their *used* space is taken from
/// `df` — and only appears when macOS has the volume mounted.
fn used_bytes_by_disk(list_plist: &[u8], df_used: &HashMap<String, u64>) -> HashMap<String, u64> {
    let Some(entries) = Value::from_reader(Cursor::new(list_plist))
        .ok()
        .as_ref()
        .and_then(Value::as_dictionary)
        .and_then(|root| root.get("AllDisksAndPartitions"))
        .and_then(Value::as_array)
        .cloned()
    else {
        return HashMap::new();
    };

    // Pass 1: total each APFS container's used bytes and key it by the physical
    // partition (e.g. `disk0s2`) the container is stored on.
    let mut apfs_used_by_store: HashMap<String, u64> = HashMap::new();
    for dict in entries.iter().filter_map(Value::as_dictionary) {
        let (Some(stores), Some(volumes)) = (
            dict.get("APFSPhysicalStores").and_then(Value::as_array),
            dict.get("APFSVolumes").and_then(Value::as_array),
        ) else {
            continue;
        };
        let used: u64 = volumes
            .iter()
            .filter_map(Value::as_dictionary)
            .filter_map(|volume| u64_value(volume, &["CapacityInUse"]))
            .sum();
        for store in stores.iter().filter_map(Value::as_dictionary) {
            if let Some(id) = string_value(store, &["DeviceIdentifier"]) {
                *apfs_used_by_store.entry(id).or_default() += used;
            }
        }
    }

    // Pass 2: for each physical disk, fold its partitions into a single total.
    let mut used_by_disk: HashMap<String, u64> = HashMap::new();
    for dict in entries.iter().filter_map(Value::as_dictionary) {
        // Synthesized APFS container disks carry the volumes, not the physical
        // media; skip them so usage is only attributed to physical disks.
        if dict.contains_key("APFSPhysicalStores") {
            continue;
        }
        let Some(disk_id) = string_value(dict, &["DeviceIdentifier"]) else {
            continue;
        };
        let Some(partitions) = dict.get("Partitions").and_then(Value::as_array) else {
            continue;
        };

        let mut used = 0u64;
        let mut found = false;
        for partition in partitions.iter().filter_map(Value::as_dictionary) {
            let Some(partition_id) = string_value(partition, &["DeviceIdentifier"]) else {
                continue;
            };
            if let Some(apfs_used) = apfs_used_by_store.get(&partition_id) {
                used += apfs_used;
                found = true;
            } else if let Some(volume_used) = df_used.get(&partition_id) {
                used += volume_used;
                found = true;
            }
        }
        if found {
            used_by_disk.insert(disk_id, used);
        }
    }

    used_by_disk
}

/// Used bytes per mounted volume device, keyed by BSD name (e.g. `disk7s2`).
///
/// A `df` failure is non-fatal: callers fall back to whatever the plist carried.
fn df_used_by_device() -> HashMap<String, u64> {
    Command::new("df")
        .args(["-k", "-P"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| parse_df_used(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

fn parse_df_used(output: &str) -> HashMap<String, u64> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let device = columns.next()?.strip_prefix("/dev/")?;
            let used_kib = columns.nth(1)?.parse::<u64>().ok()?;
            Some((device.to_string(), used_kib * 1024))
        })
        .collect()
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
<key>Content</key><string>GUID_partition_scheme</string>
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
        assert_eq!(disk.used_bytes, None);
        assert_eq!(disk.connection, Connection::Usb);
        assert_eq!(disk.filesystem.as_deref(), Some("GUID_partition_scheme"));
        assert!(!disk.read_only);
        assert!(disk.encrypted);
        assert_eq!(disk.health, Health::Healthy);
        assert!(!disk.is_boot);
        assert_eq!(disk.volume_name.as_deref(), Some("Macintosh Clone"));
    }

    #[test]
    fn rejects_virtual_disk_info() {
        let plist = b"<?xml version=\"1.0\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\">\n\
<plist version=\"1.0\">\n\
<dict>
<key>DeviceIdentifier</key><string>disk7</string>
<key>DeviceNode</key><string>/dev/disk7</string>
<key>VirtualOrPhysical</key><string>Virtual</string>
<key>TotalSize</key><integer>250000000000</integer>
</dict>
</plist>";

        let error = parse_disk_info_plist(plist).expect_err("virtual disk rejected");

        assert!(error.to_string().contains("virtual disk"));
        assert!(error.to_string().contains("parent external disk"));
    }

    #[test]
    fn rejects_apfs_container_or_volume_info() {
        let plist = b"<?xml version=\"1.0\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\">\n\
<plist version=\"1.0\">\n\
<dict>
<key>DeviceIdentifier</key><string>disk7</string>
<key>DeviceNode</key><string>/dev/disk7</string>
<key>FileSystemName</key><string>APFS</string>
<key>TotalSize</key><integer>250000000000</integer>
</dict>
</plist>";

        let error = parse_disk_info_plist(plist).expect_err("apfs source rejected");

        assert!(error.to_string().contains("virtual disk"));
    }

    #[test]
    fn used_bytes_requires_filesystem_usage_context() {
        let partition_scheme = Value::from_reader_xml(
            b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>
<key>TotalSize</key><integer>250</integer>
<key>FreeSpace</key><integer>50</integer>
<key>Content</key><string>GUID_partition_scheme</string>
</dict></plist>"
                .as_slice(),
        )
        .unwrap();
        assert_eq!(used_bytes(partition_scheme.as_dictionary().unwrap()), None);

        let volume = Value::from_reader_xml(
            b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>
<key>TotalSize</key><integer>250</integer>
<key>FreeSpace</key><integer>50</integer>
<key>FileSystemName</key><string>exFAT</string>
</dict></plist>"
                .as_slice(),
        )
        .unwrap();
        assert_eq!(used_bytes(volume.as_dictionary().unwrap()), Some(200));
    }

    #[test]
    fn virtual_disk_errors_are_expected_during_list_scan() {
        let error = anyhow::anyhow!("virtual disk is not a physical source");

        assert!(is_ignored_disk_info_error(&error));
    }

    #[test]
    fn parse_df_used_keys_devices_by_bsd_name() {
        let output = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
/dev/disk7s2 137030780 62437144 74593636 46% /Volumes/Local Disk\n\
map auto_home 0 0 0 100% /System/Volumes/Data/home\n";

        let used = parse_df_used(output);

        assert_eq!(used.get("disk7s2"), Some(&(62_437_144 * 1024)));
        // Non-/dev rows (autofs, etc.) are ignored.
        assert_eq!(used.len(), 1);
    }

    #[test]
    fn used_bytes_by_disk_sums_ntfs_volumes_from_df() {
        // A Windows-style disk: EFI (unmounted) + two NTFS volumes.
        let plist = br#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>AllDisksAndPartitions</key>
<array>
  <dict>
    <key>DeviceIdentifier</key><string>disk7</string>
    <key>Partitions</key>
    <array>
      <dict><key>DeviceIdentifier</key><string>disk7s1</string><key>Content</key><string>EFI</string></dict>
      <dict><key>DeviceIdentifier</key><string>disk7s2</string><key>Content</key><string>Microsoft Basic Data</string></dict>
      <dict><key>DeviceIdentifier</key><string>disk7s3</string><key>Content</key><string>Microsoft Basic Data</string></dict>
    </array>
  </dict>
</array>
</dict></plist>"#;
        let df_used = HashMap::from([
            ("disk7s2".to_string(), 60_000_000_000),
            ("disk7s3".to_string(), 10_000_000_000),
        ]);

        let used = used_bytes_by_disk(plist, &df_used);

        assert_eq!(used.get("disk7"), Some(&70_000_000_000));
    }

    #[test]
    fn used_bytes_by_disk_attributes_apfs_container_to_physical_disk() {
        // disk0 (physical) holds an Apple_APFS partition disk0s2; the synthesized
        // container disk3 lists the volumes and links back via APFSPhysicalStores.
        let plist = br#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>AllDisksAndPartitions</key>
<array>
  <dict>
    <key>DeviceIdentifier</key><string>disk0</string>
    <key>Partitions</key>
    <array>
      <dict><key>DeviceIdentifier</key><string>disk0s2</string><key>Content</key><string>Apple_APFS</string></dict>
    </array>
  </dict>
  <dict>
    <key>DeviceIdentifier</key><string>disk3</string>
    <key>APFSPhysicalStores</key>
    <array><dict><key>DeviceIdentifier</key><string>disk0s2</string></dict></array>
    <key>APFSVolumes</key>
    <array>
      <dict><key>DeviceIdentifier</key><string>disk3s1</string><key>CapacityInUse</key><integer>12000000000</integer></dict>
      <dict><key>DeviceIdentifier</key><string>disk3s5</string><key>CapacityInUse</key><integer>600000000000</integer></dict>
    </array>
  </dict>
</array>
</dict></plist>"#;

        let used = used_bytes_by_disk(plist, &HashMap::new());

        // Container volumes roll up to the physical disk; the synthesized
        // container disk is not itself reported.
        assert_eq!(used.get("disk0"), Some(&612_000_000_000));
        assert_eq!(used.get("disk3"), None);
    }

    #[test]
    fn used_bytes_by_disk_omits_disks_without_mounted_volumes() {
        let plist = br#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>AllDisksAndPartitions</key>
<array>
  <dict>
    <key>DeviceIdentifier</key><string>disk9</string>
    <key>Partitions</key>
    <array>
      <dict><key>DeviceIdentifier</key><string>disk9s1</string><key>Content</key><string>Microsoft Basic Data</string></dict>
    </array>
  </dict>
</array>
</dict></plist>"#;

        let used = used_bytes_by_disk(plist, &HashMap::new());

        assert!(!used.contains_key("disk9"));
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
