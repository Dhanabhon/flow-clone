//! Safe Phase 1 CLI for inspecting the mock disk catalog.

use anyhow::Result;
use flowclone_disk::{Connection, DiskCatalogApi, Health};

fn main() -> Result<()> {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "help".into());

    match cmd.as_str() {
        "list-disks" | "ls" => list_disks(),
        "version" | "--version" | "-v" => {
            println!("flowclone {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("flowclone\n");
            eprintln!("Commands:");
            eprintln!("  list-disks    List detected mock disks");
            eprintln!("  version       Print version");
            Ok(())
        }
    }
}

fn list_disks() -> Result<()> {
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    let disks = catalog.list()?;

    for disk in disks {
        println!(
            "{:<10} {:>7}  {:<22} {}{}",
            disk.device_path,
            humansize(disk.total_bytes),
            disk.model,
            fs_or_dash(&disk.filesystem),
            badges(&disk)
        );
    }

    Ok(())
}

fn humansize(bytes: u64) -> String {
    const UNITS: &[(&str, u64)] = &[
        ("TB", 1_000_000_000_000),
        ("GB", 1_000_000_000),
        ("MB", 1_000_000),
        ("KB", 1_000),
    ];

    for (unit, scale) in UNITS {
        if bytes >= *scale {
            return format!("{:.0} {unit}", bytes as f64 / *scale as f64);
        }
    }

    format!("{bytes} B")
}

fn fs_or_dash(fs: &Option<String>) -> String {
    fs.clone().unwrap_or_else(|| "-".into())
}

fn badges(disk: &flowclone_disk::DiskInfo) -> String {
    let mut tags = String::new();
    match disk.connection {
        Connection::Usb => tags.push_str(" [usb]"),
        Connection::Thunderbolt => tags.push_str(" [tb]"),
        _ => {}
    }
    match disk.health {
        Health::Healthy => tags.push_str(" [ok]"),
        Health::Warning => tags.push_str(" [warn]"),
        Health::Failing => tags.push_str(" [fail]"),
        Health::Unknown => {}
    }
    tags
}
