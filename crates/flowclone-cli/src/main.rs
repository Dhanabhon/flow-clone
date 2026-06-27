//! Safe CLI for inspecting disks and development-only image creation.

use anyhow::Result;
use flowclone_disk::{Connection, DiskCatalogApi, DiskInfo, Health};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Write};
use std::time::Instant;

const FLOW_IMAGE_FORMAT: &str = "flowclone-image";
const FLOW_IMAGE_MAGIC: &[u8] = b"FLOWCLONE_FLOWIMG_V1\n";
const FLOW_IMAGE_VERSION: u64 = 1;
const IMAGE_BLOCK_SIZE: usize = 4 * 1024 * 1024;

fn main() -> Result<()> {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "help".into());

    match cmd.as_str() {
        "list-disks" | "ls" => list_disks(),
        "create-image" => create_image(),
        "version" | "--version" | "-v" => {
            println!("flowclone {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("flowclone\n");
            eprintln!("Commands:");
            eprintln!("  list-disks    List detected disks");
            eprintln!("  create-image  Create a .flowimg from a source disk");
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

fn create_image() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let source_path = arg_value(&args, "--source")?;
    let output_path = arg_value(&args, "--output")?;
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    let source = catalog
        .find(source_path)?
        .ok_or_else(|| anyhow::anyhow!("source disk not found: {source_path}"))?;
    let raw_source = raw_device_path(&source.device_path);

    eprintln!("source: {}", source.device_path);
    eprintln!("raw:    {raw_source}");
    eprintln!("output: {output_path}");

    flowclone_raw::ensure_free_space_for_output(output_path, flow_image_file_len(&source)?)?;
    create_flow_image_file(&raw_source, output_path, &source)?;
    Ok(())
}

fn arg_value<'a>(args: &'a [String], name: &str) -> Result<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required argument: {name}"))
}

#[derive(Serialize)]
struct FlowImageHeader<'a> {
    format: &'a str,
    version: u64,
    source: &'a DiskInfo,
    payload_bytes: u64,
    note: &'a str,
}

fn create_flow_image_file(source_path: &str, image_path: &str, source: &DiskInfo) -> Result<()> {
    let mut reader = File::open(source_path)
        .map_err(|error| anyhow::anyhow!("open source {source_path}: {error}"))?;
    let mut image = File::create(image_path)
        .map_err(|error| anyhow::anyhow!("create image {image_path}: {error}"))?;
    write_flow_image_header(&mut image, source)?;

    let start = Instant::now();
    let mut buf = vec![0u8; IMAGE_BLOCK_SIZE];
    let mut bytes_done = 0u64;
    let mut last_print = Instant::now();

    while bytes_done < source.total_bytes {
        let remaining = (source.total_bytes - bytes_done).min(IMAGE_BLOCK_SIZE as u64) as usize;
        let read = reader.read(&mut buf[..remaining])?;
        if read == 0 {
            anyhow::bail!(
                "source ended early: copied {bytes_done} of {} bytes",
                source.total_bytes
            );
        }
        image.write_all(&buf[..read])?;
        bytes_done += read as u64;

        if last_print.elapsed().as_secs() >= 1 || bytes_done == source.total_bytes {
            eprintln!(
                "{} / {} ({:.1}%)",
                humansize(bytes_done),
                humansize(source.total_bytes),
                (bytes_done as f64 / source.total_bytes as f64) * 100.0
            );
            last_print = Instant::now();
        }
    }

    image.sync_all()?;
    eprintln!(
        "done in {}s, wrote {}",
        start.elapsed().as_secs(),
        humansize(bytes_done)
    );
    Ok(())
}

fn write_flow_image_header(writer: &mut impl Write, source: &DiskInfo) -> Result<()> {
    let header = flow_image_header(source)?;

    writer.write_all(FLOW_IMAGE_MAGIC)?;
    writer.write_all(&(header.len() as u64).to_le_bytes())?;
    writer.write_all(&header)?;
    Ok(())
}

fn flow_image_header(source: &DiskInfo) -> Result<Vec<u8>> {
    serde_json::to_vec(&FlowImageHeader {
        format: FLOW_IMAGE_FORMAT,
        version: FLOW_IMAGE_VERSION,
        source,
        payload_bytes: source.total_bytes,
        note: "Raw disk payload follows this header.",
    })
    .map_err(Into::into)
}

fn flow_image_file_len(source: &DiskInfo) -> Result<u64> {
    let header_len = flow_image_header(source)?.len() as u64;
    Ok(FLOW_IMAGE_MAGIC.len() as u64 + 8 + header_len + source.total_bytes)
}

fn raw_device_path(path: &str) -> String {
    if let Some(suffix) = path.strip_prefix("/dev/disk") {
        format!("/dev/rdisk{suffix}")
    } else {
        path.to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arg_value_reads_named_argument() {
        let args = vec![
            "flowclone".to_string(),
            "create-image".to_string(),
            "--source".to_string(),
            "/dev/disk6".to_string(),
            "--output".to_string(),
            "/tmp/test.flowimg".to_string(),
        ];

        assert_eq!(arg_value(&args, "--source").unwrap(), "/dev/disk6");
        assert_eq!(arg_value(&args, "--output").unwrap(), "/tmp/test.flowimg");
    }

    #[test]
    fn raw_device_path_prefers_rdisk() {
        assert_eq!(raw_device_path("/dev/disk6"), "/dev/rdisk6");
        assert_eq!(raw_device_path("/tmp/source.img"), "/tmp/source.img");
    }
}
