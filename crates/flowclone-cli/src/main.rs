//! Safe CLI for inspecting disks and development-only image creation.

use anyhow::Result;
use flowclone_disk::{Connection, DiskCatalogApi, DiskInfo, Health};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

const FLOW_IMAGE_FORMAT: &str = "flowclone-image";
const FLOW_IMAGE_MAGIC: &[u8] = b"FLOWCLONE_FLOWIMG_V1\n";
const FLOW_IMAGE_VERSION: u64 = 1;
const IMAGE_BLOCK_SIZE: usize = 4 * 1024 * 1024;
/// Wait between attempts to re-acquire a disk that dropped off the bus.
const READ_RECOVERY_WAIT: Duration = Duration::from_secs(3);
/// Attempts to re-find the disk after one drop before giving up on it.
const MAX_READ_RECOVERY_ATTEMPTS: u32 = 20;
/// Re-read a failing offset this many times before declaring it a bad region and
/// skipping it. Kept low so we don't hammer a bad block (which can wedge a USB
/// bridge into not re-enumerating).
const READ_RETRIES_BEFORE_SKIP: u32 = 1;
/// Abort if more than this much is unreadable — the drive is too damaged to image.
const MAX_BAD_REGION_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB
/// Abort if there are more than this many distinct bad regions.
const MAX_BAD_REGIONS: usize = 4096;

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

    let required_bytes = flow_image_file_len(&source)?;
    let space = flowclone_raw::ensure_free_space_for_output(output_path, required_bytes)?;
    eprintln!("image:  {}", humansize(required_bytes));
    eprintln!("free:   {}", humansize(space.available_bytes));
    eprintln!("note:   this writes a full raw image; keep the command running until done");

    // Clear any leftover cancel sentinel from a previous run so it doesn't
    // immediately abort this one.
    let _ = std::fs::remove_file(cancel_sentinel_path(output_path));

    // macOS blocks raw reads of blocks backing a *mounted* filesystem (reads
    // return ENXIO / "Device not configured" once they reach a mounted volume).
    // Unmount the disk's volumes first; the whole-disk device stays available
    // for raw reads. Remount afterward so the disk reappears for the user.
    unmount_disk(&source.device_path)?;
    let result = create_flow_image_file(&raw_source, output_path, &source);
    remount_disk(&source.device_path);
    result?;
    Ok(())
}

/// Unmount every volume on a whole disk so its blocks can be read raw.
fn unmount_disk(device_path: &str) -> Result<()> {
    eprintln!("unmount: {device_path} (so raw reads aren't blocked by mounts)");
    let output = std::process::Command::new("diskutil")
        .args(["unmountDisk", device_path])
        .output()
        .map_err(|error| anyhow::anyhow!("failed to run diskutil unmountDisk: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "could not unmount {device_path}: {}. Close any apps using the disk and retry.",
            stderr.trim()
        );
    }
    Ok(())
}

/// Best-effort remount so the disk's volumes reappear after imaging.
fn remount_disk(device_path: &str) {
    let _ = std::process::Command::new("diskutil")
        .args(["mountDisk", device_path])
        .status();
}

/// Re-acquire the source after it dropped off the bus, ready to resume at `offset`.
///
/// USB enclosures can disconnect under sustained reads and re-enumerate, often at
/// a new `/dev/diskN`. Re-find the disk by serial, unmount it (it remounts on
/// reattach), reopen the raw device, and seek back to where the copy left off.
fn reacquire_reader(source: &DiskInfo, offset: u64) -> Result<File> {
    for attempt in 1..=MAX_READ_RECOVERY_ATTEMPTS {
        // Try immediately first (the device may still be present, e.g. after a
        // skip), then wait between subsequent attempts for it to re-enumerate.
        if let Some(disk) = find_source_again(source) {
            unmount_disk_quiet(&disk.device_path);
            let raw = raw_device_path(&disk.device_path);
            match File::open(&raw) {
                Ok(mut reader) => match reader.seek(SeekFrom::Start(offset)) {
                    Ok(_) => {
                        eprintln!("recovery: resumed on {raw} at {}", humansize(offset));
                        return Ok(reader);
                    }
                    Err(error) => eprintln!("recovery: seek {raw} failed: {error}"),
                },
                Err(error) => eprintln!("recovery: open {raw} failed: {error}"),
            }
        }
        if attempt < MAX_READ_RECOVERY_ATTEMPTS {
            eprintln!(
                "recovery: waiting for the disk to reappear (attempt {attempt}/{MAX_READ_RECOVERY_ATTEMPTS})..."
            );
            std::thread::sleep(READ_RECOVERY_WAIT);
        }
    }
    anyhow::bail!("the disk did not come back after {MAX_READ_RECOVERY_ATTEMPTS} attempts")
}

/// Find the source disk again by serial, falling back to model + capacity.
fn find_source_again(source: &DiskInfo) -> Option<DiskInfo> {
    flowclone_disk::DiskCatalog::platform_default()
        .list()
        .ok()?
        .into_iter()
        .find(|candidate| same_disk(candidate, source))
}

fn same_disk(candidate: &DiskInfo, source: &DiskInfo) -> bool {
    match (candidate.serial.as_deref(), source.serial.as_deref()) {
        (Some(found), Some(want)) if !want.is_empty() => found == want,
        _ => candidate.total_bytes == source.total_bytes && candidate.model == source.model,
    }
}

/// Unmount without failing the caller — used while recovering from a drop.
fn unmount_disk_quiet(device_path: &str) {
    let _ = std::process::Command::new("diskutil")
        .args(["unmountDisk", device_path])
        .output();
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
    let partial_path = partial_image_path(image_path);
    // The GUI can't signal this root process directly, so it drops a sentinel
    // file we poll to support cancellation. `create_image` clears any stale one
    // before starting, so an existing sentinel here means "abort".
    let cancel_path = cancel_sentinel_path(image_path);
    let mut reader = File::open(source_path)
        .map_err(|error| anyhow::anyhow!("open source {source_path}: {error}"))?;
    let mut image = File::create(&partial_path)
        .map_err(|error| anyhow::anyhow!("create image {partial_path}: {error}"))?;
    write_flow_image_header(&mut image, source)?;

    let start = Instant::now();
    let mut buf = vec![0u8; IMAGE_BLOCK_SIZE];
    let mut bytes_done = 0u64;
    let mut last_print = Instant::now();

    // Bad regions that stayed unreadable after retries. ddrescue-style: zero-fill
    // and skip them so a single bad block doesn't abort the whole image, and skip
    // fast so we don't hammer (re-reading a bad block can wedge a USB bridge).
    let mut bad_regions: Vec<(u64, u64)> = Vec::new();
    let mut bad_bytes = 0u64;
    let mut retries_at_offset = 0u32;

    while bytes_done < source.total_bytes {
        if std::path::Path::new(&cancel_path).exists() {
            let _ = std::fs::remove_file(&partial_path);
            let _ = std::fs::remove_file(&cancel_path);
            anyhow::bail!("cancelled by user");
        }
        let remaining = (source.total_bytes - bytes_done).min(IMAGE_BLOCK_SIZE as u64) as usize;
        match reader.read(&mut buf[..remaining]) {
            Ok(0) => {
                anyhow::bail!(
                    "source ended early: copied {bytes_done} of {} bytes",
                    source.total_bytes
                );
            }
            Ok(read) => {
                retries_at_offset = 0;
                image.write_all(&buf[..read]).map_err(|error| {
                    anyhow::anyhow!(
                        "write image {partial_path} at source offset {} ({}): {error}",
                        bytes_done,
                        humansize(bytes_done)
                    )
                })?;
                bytes_done += read as u64;

                if last_print.elapsed().as_secs() >= 1 || bytes_done == source.total_bytes {
                    eprintln!(
                        "{}",
                        progress_line(
                            bytes_done,
                            source.total_bytes,
                            start.elapsed().as_secs_f64()
                        )
                    );
                    last_print = Instant::now();
                }
            }
            Err(error) => {
                eprintln!(
                    "read error at offset {} ({}): {error}",
                    bytes_done,
                    humansize(bytes_done)
                );
                retries_at_offset += 1;
                // The drive may have just dropped off the bus and re-enumerated;
                // re-acquire and retry the same offset a small number of times.
                if retries_at_offset <= READ_RETRIES_BEFORE_SKIP {
                    reader = reacquire_reader(source, bytes_done)?;
                    continue;
                }
                // Still unreadable: treat this block as a bad region. Zero-fill it,
                // record it, and move on instead of aborting or hammering it.
                buf[..remaining].fill(0);
                image.write_all(&buf[..remaining]).map_err(|error| {
                    anyhow::anyhow!("write zero-fill at offset {bytes_done}: {error}")
                })?;
                bad_regions.push((bytes_done, remaining as u64));
                bad_bytes += remaining as u64;
                eprintln!(
                    "bad region: zero-filled {} at offset {} ({}) and continuing",
                    humansize(remaining as u64),
                    bytes_done,
                    humansize(bytes_done)
                );
                bytes_done += remaining as u64;
                retries_at_offset = 0;

                if too_damaged(bad_bytes, bad_regions.len()) {
                    anyhow::bail!(
                        "drive too damaged: {} unreadable across {} regions — aborting",
                        humansize(bad_bytes),
                        bad_regions.len()
                    );
                }
                if bytes_done < source.total_bytes {
                    reader = reacquire_reader(source, bytes_done)?;
                }
            }
        }
    }

    if !bad_regions.is_empty() {
        write_bad_region_log(image_path, &bad_regions);
        eprintln!(
            "WARNING: {} unreadable across {} region(s) were zero-filled; see {}",
            humansize(bad_bytes),
            bad_regions.len(),
            bad_region_log_path(image_path)
        );
    }

    image.sync_all()?;
    drop(image);
    std::fs::rename(&partial_path, image_path)
        .map_err(|error| anyhow::anyhow!("finalize image {image_path}: {error}"))?;
    eprintln!(
        "done in {}, wrote {}",
        format_duration(start.elapsed().as_secs()),
        humansize(bytes_done)
    );
    Ok(())
}

fn partial_image_path(image_path: &str) -> String {
    format!("{image_path}.part")
}

/// Sentinel file the GUI drops to ask this (possibly elevated) process to abort.
fn cancel_sentinel_path(image_path: &str) -> String {
    format!("{image_path}.cancel")
}

/// Whether the accumulated unreadable regions mean the drive is too damaged to
/// produce a meaningful image and we should stop.
fn too_damaged(bad_bytes: u64, bad_regions: usize) -> bool {
    bad_bytes > MAX_BAD_REGION_BYTES || bad_regions > MAX_BAD_REGIONS
}

/// Sidecar log listing the regions that were zero-filled in the image.
fn bad_region_log_path(image_path: &str) -> String {
    format!("{image_path}.badblocks.txt")
}

fn write_bad_region_log(image_path: &str, regions: &[(u64, u64)]) {
    let mut text = String::from(
        "# FlowClone unreadable regions (zero-filled)\n# offset_bytes\tlength_bytes\n",
    );
    for (offset, len) in regions {
        text.push_str(&format!("{offset}\t{len}\n"));
    }
    let _ = std::fs::write(bad_region_log_path(image_path), text);
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

fn progress_line(bytes_done: u64, bytes_total: u64, elapsed_secs: f64) -> String {
    let speed = if elapsed_secs > 0.0 {
        (bytes_done as f64 / elapsed_secs) as u64
    } else {
        0
    };
    let eta = if speed > 0 && bytes_done < bytes_total {
        format!(
            "ETA {}",
            format_duration((bytes_total - bytes_done) / speed)
        )
    } else if bytes_done >= bytes_total {
        "done".into()
    } else {
        "ETA --".into()
    };

    format!(
        "{} / {} ({}, {}/s, {eta})",
        humansize(bytes_done),
        humansize(bytes_total),
        progress_percent(bytes_done, bytes_total),
        humansize(speed)
    )
}

fn progress_percent(bytes_done: u64, bytes_total: u64) -> String {
    if bytes_total == 0 {
        return "0.0%".into();
    }
    let percent = (bytes_done as f64 / bytes_total as f64) * 100.0;
    if percent > 0.0 && percent < 1.0 {
        format!("{percent:.3}%")
    } else {
        format!("{percent:.1}%")
    }
}

fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
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

    #[test]
    fn progress_line_keeps_small_percent_visible() {
        let line = progress_line(113_000_000, 250_000_000_000, 3.0);

        assert!(line.contains("0.045%"));
        assert!(line.contains("MB/s"));
        assert!(line.contains("ETA"));
    }

    #[test]
    fn create_flow_image_file_only_finalizes_complete_images() {
        let payload = b"short source";
        let mut source_path = std::env::temp_dir();
        source_path.push(format!("flowclone-cli-short-{}.source", std::process::id()));
        let mut image_path = std::env::temp_dir();
        image_path.push(format!(
            "flowclone-cli-short-{}.flowimg",
            std::process::id()
        ));
        std::fs::write(&source_path, payload).expect("write source file");

        let mut source = DiskInfo::placeholder("/tmp/source.img");
        source.total_bytes = payload.len() as u64 + 1;
        let source_path = source_path.to_string_lossy().to_string();
        let image_path = image_path.to_string_lossy().to_string();
        let partial_path = partial_image_path(&image_path);

        let error = create_flow_image_file(&source_path, &image_path, &source)
            .expect_err("incomplete source should fail");

        assert!(error.to_string().contains("source ended early"));
        assert!(!std::path::Path::new(&image_path).exists());
        assert!(std::path::Path::new(&partial_path).exists());

        std::fs::remove_file(source_path).expect("remove source file");
        std::fs::remove_file(partial_path).expect("remove partial image");
    }

    #[test]
    fn create_flow_image_file_aborts_on_cancel_sentinel() {
        let payload = b"flowclone cancel payload";
        let mut source_path = std::env::temp_dir();
        source_path.push(format!(
            "flowclone-cli-cancel-{}.source",
            std::process::id()
        ));
        let mut image_path = std::env::temp_dir();
        image_path.push(format!(
            "flowclone-cli-cancel-{}.flowimg",
            std::process::id()
        ));
        std::fs::write(&source_path, payload).expect("write source file");

        let mut source = DiskInfo::placeholder("/tmp/source.img");
        source.total_bytes = payload.len() as u64;
        let source_path = source_path.to_string_lossy().to_string();
        let image_path = image_path.to_string_lossy().to_string();
        let partial_path = partial_image_path(&image_path);
        let cancel_path = cancel_sentinel_path(&image_path);

        // Drop the sentinel before copying; the loop should abort and clean up.
        std::fs::write(&cancel_path, b"cancel").expect("write sentinel");

        let error = create_flow_image_file(&source_path, &image_path, &source)
            .expect_err("cancel sentinel should abort");

        assert!(error.to_string().contains("cancelled by user"));
        assert!(!std::path::Path::new(&image_path).exists());
        assert!(!std::path::Path::new(&partial_path).exists());
        assert!(!std::path::Path::new(&cancel_path).exists());

        std::fs::remove_file(source_path).expect("remove source file");
    }

    #[test]
    fn too_damaged_trips_on_byte_or_region_caps() {
        assert!(!too_damaged(0, 0));
        assert!(!too_damaged(MAX_BAD_REGION_BYTES, MAX_BAD_REGIONS));
        assert!(too_damaged(MAX_BAD_REGION_BYTES + 1, 1));
        assert!(too_damaged(0, MAX_BAD_REGIONS + 1));
    }

    #[test]
    fn same_disk_matches_by_serial_then_model_and_size() {
        let mut want = DiskInfo::placeholder("/dev/disk7");
        want.serial = Some("EC6606SERIAL".into());
        want.model = "EC-6606".into();
        want.total_bytes = 250_000_000_000;

        // Same serial, different device node (re-enumerated) → match.
        let mut reattached = DiskInfo::placeholder("/dev/disk9");
        reattached.serial = Some("EC6606SERIAL".into());
        assert!(same_disk(&reattached, &want));

        // Different serial → no match even with same size/model.
        let mut other = DiskInfo::placeholder("/dev/disk9");
        other.serial = Some("OTHER".into());
        other.model = "EC-6606".into();
        other.total_bytes = 250_000_000_000;
        assert!(!same_disk(&other, &want));

        // No serial anywhere → fall back to model + capacity.
        let mut no_serial_want = DiskInfo::placeholder("/dev/disk7");
        no_serial_want.serial = None;
        no_serial_want.model = "EC-6606".into();
        no_serial_want.total_bytes = 250_000_000_000;
        let mut no_serial_found = DiskInfo::placeholder("/dev/disk9");
        no_serial_found.serial = None;
        no_serial_found.model = "EC-6606".into();
        no_serial_found.total_bytes = 250_000_000_000;
        assert!(same_disk(&no_serial_found, &no_serial_want));
    }
}
