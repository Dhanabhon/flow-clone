//! Windows disk discovery.
//!
//! Enumerates physical disks through the built-in Storage cmdlets
//! (`Get-Disk` / `Get-Partition` / `Get-Volume`), mirroring how the macOS
//! backend shells out to `diskutil`. The cmdlets emit structured objects, so we
//! ask for JSON and parse it instead of scraping console text. No extra crates
//! and no elevation are needed just to *list* disks.

use crate::{Connection, DiskCatalogApi, DiskInfo, Health, Result};
use serde::Deserialize;
use std::os::windows::process::CommandExt;
use std::process::Command;

/// `CREATE_NO_WINDOW` — keep PowerShell from flashing a console window when the
/// GUI refreshes the disk list.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// One disk as projected by the PowerShell query below.
#[derive(Deserialize)]
struct PsDisk {
    number: i64,
    model: Option<String>,
    serial: Option<String>,
    size: u64,
    used: u64,
    bus: Option<String>,
    boot: bool,
    system: bool,
    readonly: bool,
    health: Option<String>,
    fs: Option<String>,
    label: Option<String>,
    /// Comma-joined drive letters, e.g. `"C"` or `"L,M"`.
    letters: Option<String>,
}

/// Builds a JSON array of disks. Volume usage is aggregated per disk so each
/// card can show real used space; partitions without a mounted volume (e.g. a
/// brand-new SSD) simply contribute nothing.
const LIST_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$out = foreach ($d in Get-Disk) {
    $used = [uint64]0
    $fs = $null
    $label = $null
    $letters = New-Object System.Collections.Generic.List[string]
    foreach ($p in (Get-Partition -DiskNumber $d.Number)) {
        if ($p.DriveLetter) { $letters.Add([string]$p.DriveLetter) }
        $v = $p | Get-Volume
        if ($v) {
            if ($v.Size -gt 0) { $used += [uint64]($v.Size - $v.SizeRemaining) }
            if (-not $fs -and $v.FileSystemType) { $fs = [string]$v.FileSystemType }
            if (-not $label -and $v.FileSystemLabel) { $label = [string]$v.FileSystemLabel }
        }
    }
    [PSCustomObject]@{
        number   = [int]$d.Number
        model    = [string]$d.FriendlyName
        serial   = ([string]$d.SerialNumber).Trim()
        size     = [uint64]$d.Size
        used     = $used
        bus      = [string]$d.BusType
        boot     = [bool]$d.IsBoot
        system   = [bool]$d.IsSystem
        readonly = [bool]$d.IsReadOnly
        health   = [string]$d.HealthStatus
        fs       = $fs
        label    = $label
        letters  = ($letters -join ',')
    }
}
ConvertTo-Json -InputObject @($out) -Depth 4 -Compress
"#;

/// Windows disk catalog backed by the Storage cmdlets.
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
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                LIST_SCRIPT,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Get-Disk failed: {}", stderr.trim());
        }

        Ok(parse_disks(&output.stdout))
    }
}

/// Parse the PowerShell JSON. `ConvertTo-Json` collapses a single-element array
/// to a bare object, so accept either shape; anything unparseable yields an
/// empty list rather than failing the whole refresh.
fn parse_disks(stdout: &[u8]) -> Vec<DiskInfo> {
    let text = String::from_utf8_lossy(stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Vec::new();
    }

    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let raw: Vec<PsDisk> = match value {
        serde_json::Value::Array(_) => serde_json::from_value(value).unwrap_or_default(),
        serde_json::Value::Object(_) => serde_json::from_value::<PsDisk>(value)
            .map(|d| vec![d])
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    raw.into_iter().map(map_disk).collect()
}

fn map_disk(d: PsDisk) -> DiskInfo {
    let model = d
        .model
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown disk".into());

    let serial = d
        .serial
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let filesystem = d.fs.filter(|s| !s.is_empty());
    let label = d.label.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let letters: Vec<String> = d
        .letters
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}:"))
        .collect();

    // Make the disk recognizable in the UI: prefer the volume label, then fall
    // back to the drive letter(s) the user actually sees in Explorer.
    let volume_name = match (label, letters.is_empty()) {
        (Some(name), false) => Some(format!("{name} ({})", letters.join(", "))),
        (Some(name), true) => Some(name),
        (None, false) => Some(letters.join(", ")),
        (None, true) => None,
    };

    DiskInfo {
        device_path: format!("\\\\.\\PHYSICALDRIVE{}", d.number),
        bsd_name: format!("PhysicalDrive{}", d.number),
        model,
        vendor: None,
        serial,
        total_bytes: d.size,
        used_bytes: Some(d.used),
        connection: map_bus(d.bus.as_deref()),
        filesystem,
        read_only: d.readonly,
        encrypted: false,
        health: map_health(d.health.as_deref()),
        is_boot: d.boot || d.system,
        volume_name,
    }
}

/// Map a Windows `BusType` string to FlowClone's [`Connection`].
fn map_bus(bus: Option<&str>) -> Connection {
    match bus.unwrap_or("").to_ascii_lowercase().as_str() {
        "usb" => Connection::Usb,
        "1394" => Connection::Firewire,
        "iscsi" | "fibre channel" => Connection::Network,
        "sata" | "ata" | "nvme" | "scsi" | "sas" | "raid" | "spaces"
        | "file backed virtual" => Connection::Internal,
        _ => Connection::Unknown,
    }
}

/// Map a Windows `HealthStatus` string to FlowClone's [`Health`].
fn map_health(health: Option<&str>) -> Health {
    match health.unwrap_or("").to_ascii_lowercase().as_str() {
        "healthy" => Health::Healthy,
        "warning" => Health::Warning,
        "unhealthy" => Health::Failing,
        _ => Health::Unknown,
    }
}
