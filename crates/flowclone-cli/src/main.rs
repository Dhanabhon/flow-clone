//! Safe CLI for inspecting disks and development-only image creation.

use anyhow::{Context, Result};
use flowclone_disk::{Connection, DiskCatalogApi, DiskInfo, Health};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

mod used_blocks;

#[cfg(target_os = "windows")]
mod win;

const FLOW_IMAGE_FORMAT: &str = "flowclone-image";
const FLOW_IMAGE_MAGIC: &[u8] = b"FLOWCLONE_FLOWIMG_V1\n";
const FLOW_IMAGE_VERSION: u64 = 1;
const IMAGE_BLOCK_SIZE: usize = 4 * 1024 * 1024;
// Raw `\\.\PHYSICALDRIVE` reads/writes on Windows must be whole-sector multiples
// (incl. 4Kn disks). Keeping the block a 4096-multiple guarantees every full
// block is sector-aligned; only the final partial block needs special handling.
const _: () = assert!(
    IMAGE_BLOCK_SIZE.is_multiple_of(4096),
    "IMAGE_BLOCK_SIZE must be a multiple of 4096 to keep raw disk I/O sector-aligned"
);
/// v2 image magic. v2 adds a `mode` (full / used-only) and optional zstd
/// compression. Kept the same byte length as v1 so the version can be sniffed by
/// reading a fixed-size magic and comparing.
const FLOW_IMAGE_MAGIC_V2: &[u8] = b"FLOWCLONE_FLOWIMG_V2\n";
const FLOW_IMAGE_VERSION_V2: u64 = 2;
const _: () = assert!(
    FLOW_IMAGE_MAGIC.len() == FLOW_IMAGE_MAGIC_V2.len(),
    "v1 and v2 magics must be the same length so the version can be sniffed"
);
/// zstd level for `--compress`. 3 is zstd's default — a good speed/ratio balance.
const ZSTD_LEVEL: i32 = 3;

/// Payload codec recorded in a v2 image header.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Compression {
    None,
    Zstd,
}

impl Compression {
    fn as_str(self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Zstd => "zstd",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Compression::None),
            "zstd" => Ok(Compression::Zstd),
            other => anyhow::bail!("unsupported image compression: {other}"),
        }
    }
}

/// Which blocks a sparse (used-only) image actually stores, as ascending,
/// non-overlapping `[start_block, count]` runs. Blocks not covered by a run are
/// absent from the payload and restored as zeros.
#[derive(Serialize, Deserialize, Clone, Default)]
struct BlockMap {
    runs: Vec<[u64; 2]>,
}

impl BlockMap {
    /// Validate the runs for a disk of `total_blocks` blocks: each run is
    /// non-empty, in range, ascending, and non-overlapping. A bad map could make
    /// restore write blocks to the wrong place, so this is checked before use.
    fn validate(&self, total_blocks: u64) -> Result<()> {
        let mut next = 0u64;
        for &[start, count] in &self.runs {
            if count == 0 {
                anyhow::bail!("block map has an empty run");
            }
            if start < next {
                anyhow::bail!("block map runs must be ascending and non-overlapping");
            }
            let end = start
                .checked_add(count)
                .ok_or_else(|| anyhow::anyhow!("block map run overflows"))?;
            if end > total_blocks {
                anyhow::bail!("block map run {start}+{count} exceeds {total_blocks} blocks");
            }
            next = end;
        }
        Ok(())
    }

    /// Uncompressed bytes the present blocks occupy, given the block size and the
    /// image's logical size (the final block of the disk may be partial).
    fn present_bytes(&self, block_size: u64, total_bytes: u64) -> u64 {
        let total_blocks = total_bytes.div_ceil(block_size);
        let last_block = total_blocks.saturating_sub(1);
        let last_len = total_bytes - last_block * block_size;
        let mut bytes = 0u64;
        for &[start, count] in &self.runs {
            bytes += count * block_size;
            if last_block >= start && last_block < start + count {
                bytes -= block_size - last_len;
            }
        }
        bytes
    }
}

/// Answers `is_present` for monotonically increasing block indices by walking the
/// ascending block-map runs once. A `None` map means every block is present (a
/// full image).
struct PresentCursor<'a> {
    runs: Option<&'a [[u64; 2]]>,
    run: usize,
}

impl<'a> PresentCursor<'a> {
    fn new(map: Option<&'a BlockMap>) -> Self {
        Self {
            runs: map.map(|map| map.runs.as_slice()),
            run: 0,
        }
    }

    fn is_present(&mut self, block: u64) -> bool {
        let Some(runs) = self.runs else {
            return true;
        };
        while self.run < runs.len() {
            let [start, count] = runs[self.run];
            if block < start {
                return false;
            }
            if block < start + count {
                return true;
            }
            self.run += 1;
        }
        false
    }
}

/// Reject image headers larger than this — guards against a corrupt length field.
const MAX_IMAGE_HEADER_BYTES: u64 = 1024 * 1024;
/// macOS errno for "Inappropriate ioctl for device" — fsync on an unbuffered
/// raw device returns this, and it's safe to ignore.
#[cfg(not(target_os = "windows"))]
const ENOTTY: i32 = 25;
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
        "restore-image" => restore_image(),
        #[cfg(target_os = "windows")]
        "list-volumes" => list_volumes(),
        "version" | "--version" | "-v" => {
            println!("flowclone {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("flowclone\n");
            eprintln!("Commands:");
            eprintln!("  list-disks     List detected disks");
            eprintln!("  create-image   Create a .flowimg from a source disk");
            eprintln!("  restore-image  Write a .flowimg onto a target disk (ERASES it)");
            eprintln!("  version        Print version");
            Ok(())
        }
    }
}

/// Diagnostic (Windows): show which physical disk each volume maps to. This is
/// the same matching the destructive restore uses to pick volumes to dismount,
/// so it's a safe, read-only way to confirm it targets the right disk.
#[cfg(target_os = "windows")]
fn list_volumes() -> Result<()> {
    for (volume, disk) in win::volume_disk_map() {
        let disk = disk
            .map(|number| format!("PHYSICALDRIVE{number}"))
            .unwrap_or_else(|| "?".into());
        println!("{disk:<16} {volume}");
    }
    Ok(())
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
    let compress = args.iter().any(|arg| arg == "--compress");
    let used_only = args.iter().any(|arg| arg == "--used-only");
    let compression = if compress {
        Compression::Zstd
    } else {
        Compression::None
    };
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
    eprintln!("image:  {} (estimate)", humansize(required_bytes));
    eprintln!("free:   {}", humansize(space.available_bytes));
    eprintln!("note:   keep the command running until done");

    // Clear any leftover cancel sentinel / progress file from a previous run so
    // they don't immediately abort this one or show a stale bar.
    let _ = std::fs::remove_file(cancel_sentinel_path(output_path));
    let _ = std::fs::remove_file(create_progress_path(output_path));

    // macOS blocks raw reads of blocks backing a *mounted* filesystem (reads
    // return ENXIO / "Device not configured" once they reach a mounted volume),
    // so it unmounts the disk's volumes first and remounts them on drop. Windows
    // serves raw reads while mounted, so its prep is a no-op.
    // macOS needs the disk unmounted before raw reads, so compute the used-block
    // map after preparing the source.
    let _prep = prepare_source_for_read(&source.device_path)?;

    if used_only {
        match try_used_block_map(&raw_source, source.total_bytes) {
            Ok(map) => {
                let stored = map.present_bytes(IMAGE_BLOCK_SIZE as u64, source.total_bytes);
                eprintln!(
                    "mode:   used-only — storing {} of {}{}",
                    humansize(stored),
                    humansize(source.total_bytes),
                    if compress { " (zstd)" } else { "" }
                );
                create_sparse_image_file(&raw_source, output_path, &source, &map, compression)?;
                let _ = std::fs::remove_file(create_progress_path(output_path));
                return Ok(());
            }
            Err(error) if is_permission_error(&error) => {
                // The map and the full image both have to read the disk, so the
                // same permission error would just fail again. Surface it clearly
                // instead of a misleading "writing a full image" fallback.
                return Err(error.context(
                    "can't read the source disk for used-only — grant Full Disk Access \
                     (and run with admin rights)",
                ));
            }
            Err(error) => {
                eprintln!("note:   used-only unavailable ({error}); writing a full image");
            }
        }
    }

    eprintln!(
        "mode:   full image{}",
        if compress { " (zstd)" } else { "" }
    );
    if compress {
        create_compressed_image_file(&raw_source, output_path, &source)?;
    } else {
        create_flow_image_file(&raw_source, output_path, &source)?;
    }
    let _ = std::fs::remove_file(create_progress_path(output_path));
    Ok(())
}

/// Whether an error chain carries a "permission denied" I/O error. On macOS, raw
/// disk reads need root *and* Full Disk Access, so this is the usual cause.
fn is_permission_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
    })
}

/// Open the raw source read-only and work out which blocks hold real data. Any
/// failure (not GPT, unknown filesystem, parse error) propagates so the caller
/// falls back to a full image — used-only never guesses.
fn try_used_block_map(raw_source: &str, total_bytes: u64) -> Result<BlockMap> {
    // `with_context` (not `anyhow!`) so the underlying io::Error survives in the
    // chain and the caller can tell a permission error from a parse one.
    let mut file = File::open(raw_source).with_context(|| format!("open source {raw_source}"))?;
    // Detection uses one aligned 8 KiB read, which works on a raw device.
    let sector_size = used_blocks::detect_sector_size(&mut file)?;
    // The parsers do small, scattered reads, but macOS `/dev/rdiskN` only allows
    // whole-sector reads — wrap the file so those reads are aligned underneath.
    let mut reader = used_blocks::AlignedReader::new(file, sector_size);
    used_blocks::compute_used_block_map(
        &mut reader,
        total_bytes,
        IMAGE_BLOCK_SIZE as u64,
        sector_size,
    )
}

/// Undoes the disk preparation done by [`prepare_source_for_read`] /
/// [`prepare_target_for_write`] when dropped, restoring the disk's normal
/// mounted state regardless of whether the operation succeeded. An all-`None`
/// value (the default) is a no-op guard, used where a platform needs no prep.
#[derive(Default)]
struct DiskPrep {
    /// macOS: the disk to remount via `diskutil` on drop.
    #[cfg(target_os = "macos")]
    remount_device: Option<String>,
    /// Windows: the held volume locks. Held only for their `Drop`, which releases
    /// the locks and rescans the disk so its volumes re-mount (`win::VolumeLocks`).
    #[cfg(target_os = "windows")]
    _windows: Option<win::VolumeLocks>,
}

impl Drop for DiskPrep {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(device_path) = &self.remount_device {
            remount_disk(device_path);
        }
        // On Windows the `windows` field drops itself here, which releases the
        // volume locks and rescans the disk.
    }
}

/// Prepare a disk so its whole-disk device can be read raw.
#[cfg(target_os = "macos")]
fn prepare_source_for_read(device_path: &str) -> Result<DiskPrep> {
    unmount_disk(device_path)?;
    Ok(DiskPrep {
        remount_device: Some(device_path.to_string()),
    })
}

/// Prepare a disk so its whole-disk device can be written raw.
#[cfg(target_os = "macos")]
fn prepare_target_for_write(device_path: &str) -> Result<DiskPrep> {
    unmount_disk(device_path)?;
    Ok(DiskPrep {
        remount_device: Some(device_path.to_string()),
    })
}

/// Windows serves raw reads of a mounted disk, so nothing needs to be done.
#[cfg(target_os = "windows")]
fn prepare_source_for_read(_device_path: &str) -> Result<DiskPrep> {
    Ok(DiskPrep::default())
}

/// Windows rejects writes to sectors owned by a mounted filesystem, so lock and
/// dismount every volume on the target and hold the locks for the write.
#[cfg(target_os = "windows")]
fn prepare_target_for_write(device_path: &str) -> Result<DiskPrep> {
    let disk_number = win::disk_number_from_path(device_path)
        .ok_or_else(|| anyhow::anyhow!("not a physical drive path: {device_path}"))?;
    eprintln!("dismount: locking volumes on {device_path} for an exclusive write");
    let locks = win::lock_and_dismount_disk(disk_number)?;
    Ok(DiskPrep {
        _windows: Some(locks),
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn prepare_source_for_read(_device_path: &str) -> Result<DiskPrep> {
    Ok(DiskPrep::default())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn prepare_target_for_write(_device_path: &str) -> Result<DiskPrep> {
    Ok(DiskPrep::default())
}

/// Unmount every volume on a whole disk so its blocks can be read/written raw.
#[cfg(target_os = "macos")]
fn unmount_disk(device_path: &str) -> Result<()> {
    eprintln!("unmount: {device_path} (so raw I/O isn't blocked by mounts)");
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
#[cfg(target_os = "macos")]
fn remount_disk(device_path: &str) {
    let _ = std::process::Command::new("diskutil")
        .args(["mountDisk", device_path])
        .status();
}

/// Image header fields needed to restore (parsed from a `.flowimg`).
struct ImageInfo {
    /// Logical size written to the target (the full disk size, incl. absent
    /// blocks restored as zeros).
    write_bytes: u64,
    /// File offset where the (possibly compressed) payload begins.
    data_offset: u64,
    /// Codec of the payload between `data_offset` and EOF.
    compression: Compression,
    /// Present-block runs for a sparse (used-only) image; `None` = full image.
    block_map: Option<BlockMap>,
    source: DiskInfo,
}

#[derive(Deserialize)]
struct FlowImageHeaderOwned {
    format: String,
    version: u64,
    source: DiskInfo,
    payload_bytes: u64,
}

#[derive(Deserialize)]
struct FlowImageHeaderV2Owned {
    format: String,
    version: u64,
    source: DiskInfo,
    block_size: u64,
    uncompressed_bytes: u64,
    compression: String,
    mode: String,
    #[serde(default)]
    block_map: Option<BlockMap>,
}

/// Restore a `.flowimg` onto a target disk. **Destructive** — overwrites it.
///
/// Requires `--confirm-erase`; without it the command prints the plan and
/// refuses, so a stray CLI invocation can't erase a disk by accident.
fn restore_image() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let image_path = arg_value(&args, "--image")?;
    let target_path = arg_value(&args, "--target")?;
    let confirmed = args.iter().any(|arg| arg == "--confirm-erase");

    let info = read_flow_image_header(image_path)?;
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    let target = catalog
        .find(target_path)?
        .ok_or_else(|| anyhow::anyhow!("target disk not found: {target_path}"))?;
    validate_restore_target(&target, info.write_bytes)?;
    let raw_target = raw_device_path(&target.device_path);

    eprintln!("image:   {image_path}");
    eprintln!(
        "source:  {} ({})",
        info.source.model,
        humansize(info.write_bytes)
    );
    eprintln!(
        "target:  {} -> {} ({})",
        target.device_path,
        raw_target,
        humansize(target.total_bytes)
    );

    if !confirmed {
        anyhow::bail!(
            "this ERASES {} and overwrites it with the image. Re-run with --confirm-erase to proceed.",
            target.device_path
        );
    }

    // Clear any stale cancel sentinel so it doesn't abort this run immediately.
    let _ = std::fs::remove_file(cancel_sentinel_path(image_path));
    eprintln!(
        "WARNING: erasing {} and writing {} ...",
        target.device_path,
        humansize(info.write_bytes)
    );
    let _prep = prepare_target_for_write(&target.device_path)?;
    write_image_to_target(image_path, &raw_target, &info)?;
    Ok(())
}

/// Reject targets that would be unsafe or impossible to restore onto.
fn validate_restore_target(target: &DiskInfo, payload_bytes: u64) -> Result<()> {
    if target.is_boot {
        anyhow::bail!(
            "refusing to restore onto the boot disk {}",
            target.device_path
        );
    }
    if target.read_only {
        anyhow::bail!("target {} is read-only", target.device_path);
    }
    if matches!(target.connection, Connection::Internal) {
        anyhow::bail!(
            "refusing to restore onto internal disk {} (Phase 1 allows external targets only)",
            target.device_path
        );
    }
    if target.total_bytes < payload_bytes {
        anyhow::bail!(
            "target too small: {} has {} but the image needs {}",
            target.device_path,
            humansize(target.total_bytes),
            humansize(payload_bytes)
        );
    }
    Ok(())
}

/// Read + validate a `.flowimg` header (v1 or v2), returning where the payload
/// starts, how many logical bytes it expands to, and how it is compressed.
fn read_flow_image_header(image_path: &str) -> Result<ImageInfo> {
    let mut file = File::open(image_path)
        .map_err(|error| anyhow::anyhow!("open image {image_path}: {error}"))?;
    let file_len = file.metadata()?.len();

    // Both magics are the same length, so a single read sniffs the version.
    let mut magic = vec![0u8; FLOW_IMAGE_MAGIC.len()];
    file.read_exact(&mut magic)
        .map_err(|error| anyhow::anyhow!("read image header: {error}"))?;
    let is_v2 = if magic == FLOW_IMAGE_MAGIC {
        false
    } else if magic == FLOW_IMAGE_MAGIC_V2 {
        true
    } else {
        anyhow::bail!("{image_path} is not a FlowClone image (bad magic)");
    };

    let mut len_bytes = [0u8; 8];
    file.read_exact(&mut len_bytes)?;
    let header_len = u64::from_le_bytes(len_bytes);
    if header_len == 0 || header_len > MAX_IMAGE_HEADER_BYTES {
        anyhow::bail!("invalid image header length: {header_len}");
    }

    let mut header = vec![0u8; header_len as usize];
    file.read_exact(&mut header)?;
    let data_offset = FLOW_IMAGE_MAGIC.len() as u64 + 8 + header_len;

    if !is_v2 {
        let parsed: FlowImageHeaderOwned = serde_json::from_slice(&header)
            .map_err(|error| anyhow::anyhow!("invalid image header: {error}"))?;
        if parsed.format != FLOW_IMAGE_FORMAT {
            anyhow::bail!("unsupported image format: {}", parsed.format);
        }
        if parsed.version != FLOW_IMAGE_VERSION {
            anyhow::bail!("unsupported image version: {}", parsed.version);
        }
        if parsed.payload_bytes == 0 || parsed.payload_bytes != parsed.source.total_bytes {
            anyhow::bail!("invalid image payload size");
        }
        let expected = data_offset + parsed.payload_bytes;
        if file_len != expected {
            anyhow::bail!("image size mismatch: expected {expected} bytes, found {file_len}");
        }
        return Ok(ImageInfo {
            write_bytes: parsed.payload_bytes,
            data_offset,
            compression: Compression::None,
            block_map: None,
            source: parsed.source,
        });
    }

    let parsed: FlowImageHeaderV2Owned = serde_json::from_slice(&header)
        .map_err(|error| anyhow::anyhow!("invalid image header: {error}"))?;
    if parsed.format != FLOW_IMAGE_FORMAT {
        anyhow::bail!("unsupported image format: {}", parsed.format);
    }
    if parsed.version != FLOW_IMAGE_VERSION_V2 {
        anyhow::bail!("unsupported image version: {}", parsed.version);
    }
    if parsed.block_size != IMAGE_BLOCK_SIZE as u64 {
        anyhow::bail!("unsupported image block size: {}", parsed.block_size);
    }
    let compression = Compression::parse(&parsed.compression)?;
    if parsed.uncompressed_bytes == 0 || parsed.uncompressed_bytes != parsed.source.total_bytes {
        anyhow::bail!("invalid image payload size");
    }
    let total_blocks = parsed.uncompressed_bytes.div_ceil(IMAGE_BLOCK_SIZE as u64);

    // A full image stores every block and carries no map; a used-only image must
    // carry a valid one. Reject any inconsistency rather than guess.
    let block_map = match parsed.mode.as_str() {
        "full" => {
            if parsed.block_map.is_some() {
                anyhow::bail!("full image must not carry a block map");
            }
            None
        }
        "used-only" => {
            let map = parsed
                .block_map
                .ok_or_else(|| anyhow::anyhow!("used-only image is missing its block map"))?;
            map.validate(total_blocks)?;
            Some(map)
        }
        other => anyhow::bail!("unsupported image mode: {other}"),
    };

    match compression {
        // Uncompressed: the stored size equals the present blocks' size, so check
        // it against truncation.
        Compression::None => {
            let payload_len = match &block_map {
                None => parsed.uncompressed_bytes,
                Some(map) => map.present_bytes(IMAGE_BLOCK_SIZE as u64, parsed.uncompressed_bytes),
            };
            let expected = data_offset + payload_len;
            if file_len != expected {
                anyhow::bail!("image size mismatch: expected {expected} bytes, found {file_len}");
            }
        }
        // Compressed: the stored size varies and can't be predicted. A truncated
        // stream is caught at restore time — the decoder yields fewer than
        // expected and the read fails.
        Compression::Zstd => {
            if file_len <= data_offset {
                anyhow::bail!("image has no payload");
            }
        }
    }
    Ok(ImageInfo {
        write_bytes: parsed.uncompressed_bytes,
        data_offset,
        compression,
        block_map,
        source: parsed.source,
    })
}

/// Copy the image payload onto the target raw device, block by block.
fn write_image_to_target(image_path: &str, raw_target: &str, info: &ImageInfo) -> Result<()> {
    let cancel_path = cancel_sentinel_path(image_path);
    let mut image = File::open(image_path)
        .map_err(|error| anyhow::anyhow!("open image {image_path}: {error}"))?;
    image
        .seek(SeekFrom::Start(info.data_offset))
        .map_err(|error| anyhow::anyhow!("seek to image payload: {error}"))?;
    // The payload is either a raw byte stream (v1, v2 uncompressed) or a single
    // zstd stream (v2 compressed). Decompress transparently so the write loop is
    // identical for both — it always pulls `info.write_bytes` logical bytes.
    let mut payload: Box<dyn Read> = match info.compression {
        Compression::None => Box::new(image),
        Compression::Zstd => Box::new(
            zstd::Decoder::new(image)
                .map_err(|error| anyhow::anyhow!("init decompressor for {image_path}: {error}"))?,
        ),
    };
    // Read access too: Windows opens a `\\.\PHYSICALDRIVE` for raw writes with
    // GENERIC_READ | GENERIC_WRITE, and read+write is harmless on a raw Unix
    // device opened by the elevated worker.
    let mut target = OpenOptions::new()
        .read(true)
        .write(true)
        .open(raw_target)
        .map_err(|error| anyhow::anyhow!("open target {raw_target} for write: {error}"))?;

    // Restore writes to a device, so there's no growing file to poll. Publish a
    // small progress file the GUI reads instead. Clear any stale one first.
    let progress_path = restore_progress_path(image_path);
    let _ = std::fs::remove_file(&progress_path);

    // The target's logical sector size. Full blocks are always sector-aligned,
    // but the final partial block must be rounded up to a whole sector — the
    // image's payload size is a multiple of the *source* sector size, which can
    // be smaller than the target's (e.g. a 512e image onto a 4Kn disk).
    let sector = write_alignment(raw_target);

    // Write the whole disk block by block: a present block comes from the
    // payload, an absent (used-only) block is written as zeros so the target
    // matches the source. A `None` map means every block is present (full image).
    let block_size = IMAGE_BLOCK_SIZE as u64;
    let total_blocks = info.write_bytes.div_ceil(block_size);
    let mut cursor = PresentCursor::new(info.block_map.as_ref());

    let start = Instant::now();
    let mut buf = vec![0u8; IMAGE_BLOCK_SIZE];
    let mut bytes_done = 0u64;
    let mut last_print = Instant::now();

    for block_idx in 0..total_blocks {
        if std::path::Path::new(&cancel_path).exists() {
            let _ = std::fs::remove_file(&cancel_path);
            anyhow::bail!("cancelled by user");
        }
        let offset = block_idx * block_size;
        let len = (info.write_bytes - offset).min(block_size) as usize;
        if cursor.is_present(block_idx) {
            payload.read_exact(&mut buf[..len]).map_err(|error| {
                anyhow::anyhow!("read image at payload offset {offset}: {error}")
            })?;
        } else {
            buf[..len].fill(0);
        }
        // Pad the final block up to a whole sector with zeros so the raw write is
        // sector-aligned. The padding lands in slack past the image's end (the
        // target is >= the payload and a whole number of sectors), so it's safe.
        let write_len = round_up(len, sector);
        if write_len > len {
            buf[len..write_len].fill(0);
        }
        target.write_all(&buf[..write_len]).map_err(|error| {
            anyhow::anyhow!(
                "write target {raw_target} at offset {} ({}): {error}",
                offset,
                humansize(offset)
            )
        })?;
        bytes_done = offset + len as u64;

        if last_print.elapsed().as_secs() >= 1 || bytes_done == info.write_bytes {
            eprintln!(
                "{}",
                progress_line(bytes_done, info.write_bytes, start.elapsed().as_secs_f64())
            );
            let _ = std::fs::write(&progress_path, format!("{bytes_done} {}", info.write_bytes));
            last_print = Instant::now();
        }
    }
    let _ = std::fs::remove_file(&progress_path);

    // Raw devices are unbuffered, so writes are already durable and a flush isn't
    // meaningful — macOS returns ENOTTY, Windows returns "invalid function". Both
    // are benign; surface any other flush error.
    if let Err(error) = target.sync_all() {
        if !flush_error_is_benign(&error) {
            return Err(anyhow::anyhow!("flush target {raw_target}: {error}"));
        }
    }
    eprintln!(
        "done in {}, wrote {} to {}",
        format_duration(start.elapsed().as_secs()),
        humansize(bytes_done),
        raw_target
    );
    Ok(())
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
/// macOS must clear mounts to read raw; Windows reads while mounted, so no-op.
#[cfg(target_os = "macos")]
fn unmount_disk_quiet(device_path: &str) {
    let _ = std::process::Command::new("diskutil")
        .args(["unmountDisk", device_path])
        .output();
}

#[cfg(not(target_os = "macos"))]
fn unmount_disk_quiet(_device_path: &str) {}

/// Whether a flush error on a raw device handle is benign (the device is
/// unbuffered, so the flush is a no-op the kernel may reject).
#[cfg(target_os = "windows")]
fn flush_error_is_benign(error: &std::io::Error) -> bool {
    win::flush_error_is_benign(error)
}

#[cfg(not(target_os = "windows"))]
fn flush_error_is_benign(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(ENOTTY)
}

/// Round `value` up to the next multiple of `align` (`align` >= 1).
fn round_up(value: usize, align: usize) -> usize {
    debug_assert!(align >= 1);
    value.div_ceil(align) * align
}

/// The granularity raw writes to `raw_target` must use. Only real Windows
/// physical-disk devices require whole-sector writes; regular files (tests, any
/// non-device target) and other platforms' raw devices accept any length.
#[cfg(target_os = "windows")]
fn write_alignment(raw_target: &str) -> usize {
    if win::disk_number_from_path(raw_target).is_some() {
        win::logical_sector_size(raw_target) as usize
    } else {
        1
    }
}

#[cfg(not(target_os = "windows"))]
fn write_alignment(_raw_target: &str) -> usize {
    1
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

#[derive(Serialize)]
struct FlowImageHeaderV2<'a> {
    format: &'a str,
    version: u64,
    source: &'a DiskInfo,
    /// Block granularity the block map's indices count in.
    block_size: u64,
    /// Logical size restore writes to the target (the full disk). Present blocks
    /// come from the payload; absent blocks are zeros.
    uncompressed_bytes: u64,
    compression: &'a str,
    /// "full" (no map) or "used-only" (sparse, with a `block_map`).
    mode: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_map: Option<&'a BlockMap>,
    note: &'a str,
}

fn create_flow_image_file(source_path: &str, image_path: &str, source: &DiskInfo) -> Result<()> {
    let partial_path = partial_image_path(image_path);
    let cancel_path = cancel_sentinel_path(image_path);
    let mut reader = File::open(source_path)
        .map_err(|error| anyhow::anyhow!("open source {source_path}: {error}"))?;
    let mut image = File::create(&partial_path)
        .map_err(|error| anyhow::anyhow!("create image {partial_path}: {error}"))?;
    write_flow_image_header(&mut image, source)?;

    copy_disk_payload(
        &mut reader,
        &mut image,
        source,
        image_path,
        &cancel_path,
        &partial_path,
    )?;

    image.sync_all()?;
    drop(image);
    finalize_image(&partial_path, image_path)
}

/// Like [`create_flow_image_file`] but writes a v2 image whose payload is a
/// single zstd stream — a smaller file at the cost of CPU. Restore decompresses
/// transparently.
fn create_compressed_image_file(
    source_path: &str,
    image_path: &str,
    source: &DiskInfo,
) -> Result<()> {
    let partial_path = partial_image_path(image_path);
    let cancel_path = cancel_sentinel_path(image_path);
    let mut reader = File::open(source_path)
        .map_err(|error| anyhow::anyhow!("open source {source_path}: {error}"))?;
    let mut image = File::create(&partial_path)
        .map_err(|error| anyhow::anyhow!("create image {partial_path}: {error}"))?;
    // The header is written uncompressed; only the payload goes through zstd.
    write_flow_image_header_v2(&mut image, source, Compression::Zstd, None)?;
    let mut encoder = zstd::Encoder::new(image, ZSTD_LEVEL)
        .map_err(|error| anyhow::anyhow!("init compressor: {error}"))?;

    copy_disk_payload(
        &mut reader,
        &mut encoder,
        source,
        image_path,
        &cancel_path,
        &partial_path,
    )?;

    // Flush the zstd stream and recover the file handle to fsync + finalize.
    let image = encoder
        .finish()
        .map_err(|error| anyhow::anyhow!("finish compressor: {error}"))?;
    image.sync_all()?;
    drop(image);
    finalize_image(&partial_path, image_path)
}

/// Create a sparse (used-only) image: a v2 header carrying `block_map`, then only
/// the present blocks. The full-disk size is recorded in the header so restore
/// zero-fills the gaps.
fn create_sparse_image_file(
    source_path: &str,
    image_path: &str,
    source: &DiskInfo,
    block_map: &BlockMap,
    compression: Compression,
) -> Result<()> {
    let partial_path = partial_image_path(image_path);
    let cancel_path = cancel_sentinel_path(image_path);
    let mut reader = File::open(source_path)
        .map_err(|error| anyhow::anyhow!("open source {source_path}: {error}"))?;
    let mut image = File::create(&partial_path)
        .map_err(|error| anyhow::anyhow!("create image {partial_path}: {error}"))?;
    write_flow_image_header_v2(&mut image, source, compression, Some(block_map))?;

    match compression {
        Compression::None => {
            copy_present_blocks(
                &mut reader,
                &mut image,
                source,
                block_map,
                image_path,
                &cancel_path,
                &partial_path,
            )?;
            image.sync_all()?;
            drop(image);
        }
        Compression::Zstd => {
            let mut encoder = zstd::Encoder::new(image, ZSTD_LEVEL)
                .map_err(|error| anyhow::anyhow!("init compressor: {error}"))?;
            copy_present_blocks(
                &mut reader,
                &mut encoder,
                source,
                block_map,
                image_path,
                &cancel_path,
                &partial_path,
            )?;
            let image = encoder
                .finish()
                .map_err(|error| anyhow::anyhow!("finish compressor: {error}"))?;
            image.sync_all()?;
            drop(image);
        }
    }
    finalize_image(&partial_path, image_path)
}

/// Read and stream only the present blocks of a sparse image: seek to each one,
/// read it (with the same cancellation + bad-block resilience as the full path),
/// and write it to `sink`. Absent blocks are never read — that's the speedup.
fn copy_present_blocks<W: Write>(
    reader: &mut File,
    sink: &mut W,
    source: &DiskInfo,
    block_map: &BlockMap,
    image_path: &str,
    cancel_path: &str,
    partial_path: &str,
) -> Result<()> {
    let block_size = IMAGE_BLOCK_SIZE as u64;
    let total_bytes = source.total_bytes;
    let present_bytes = block_map.present_bytes(block_size, total_bytes);
    // Publish 0/total up front so the GUI shows the right denominator immediately.
    write_create_progress(image_path, 0, present_bytes);

    let start = Instant::now();
    let mut buf = vec![0u8; IMAGE_BLOCK_SIZE];
    let mut bytes_done = 0u64;
    let mut last_print = Instant::now();
    let mut bad_regions: Vec<(u64, u64)> = Vec::new();
    let mut bad_bytes = 0u64;

    for &[run_start, count] in &block_map.runs {
        for block in run_start..run_start + count {
            if std::path::Path::new(cancel_path).exists() {
                let _ = std::fs::remove_file(partial_path);
                let _ = std::fs::remove_file(cancel_path);
                let _ = std::fs::remove_file(create_progress_path(image_path));
                anyhow::bail!("cancelled by user");
            }
            let offset = block * block_size;
            let len = (total_bytes - offset).min(block_size) as usize;

            if let Err(error) = read_block_at(reader, source, offset, &mut buf[..len]) {
                // Genuinely unreadable: zero-fill, record it, and keep going so a
                // single bad sector doesn't abort the whole image.
                eprintln!(
                    "bad region at offset {} ({}): {error}; zero-filled",
                    offset,
                    humansize(offset)
                );
                buf[..len].fill(0);
                bad_regions.push((offset, len as u64));
                bad_bytes += len as u64;
                if too_damaged(bad_bytes, bad_regions.len()) {
                    anyhow::bail!(
                        "drive too damaged: {} unreadable across {} regions — aborting",
                        humansize(bad_bytes),
                        bad_regions.len()
                    );
                }
            }

            sink.write_all(&buf[..len]).map_err(|error| {
                anyhow::anyhow!("write image at source offset {offset}: {error}")
            })?;
            bytes_done += len as u64;

            if last_print.elapsed().as_secs() >= 1 || bytes_done == present_bytes {
                eprintln!(
                    "{}",
                    progress_line(bytes_done, present_bytes, start.elapsed().as_secs_f64())
                );
                write_create_progress(image_path, bytes_done, present_bytes);
                last_print = Instant::now();
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

    eprintln!(
        "done in {}, stored {}",
        format_duration(start.elapsed().as_secs()),
        humansize(bytes_done)
    );
    Ok(())
}

/// Read exactly `buf.len()` bytes at `offset`, retrying once via a disk
/// re-acquire (for USB drops) before giving up on the region.
fn read_block_at(reader: &mut File, source: &DiskInfo, offset: u64, buf: &mut [u8]) -> Result<()> {
    reader.seek(SeekFrom::Start(offset))?;
    if reader.read_exact(buf).is_ok() {
        return Ok(());
    }
    *reader = reacquire_reader(source, offset)?;
    reader
        .read_exact(buf)
        .map_err(|error| anyhow::anyhow!("{error}"))
}

/// Atomically publish a finished `.part` image as the real file.
fn finalize_image(partial_path: &str, image_path: &str) -> Result<()> {
    std::fs::rename(partial_path, image_path)
        .map_err(|error| anyhow::anyhow!("finalize image {image_path}: {error}"))
}

/// Stream the source disk into `sink` block by block, with cancellation,
/// ddrescue-style bad-block skipping, and progress. Shared by the raw (v1) and
/// compressed (v2) create paths — `sink` is the image file or a zstd encoder.
///
/// The GUI can't signal this (possibly elevated) process directly, so it drops a
/// sentinel file we poll for cancellation; `create_image` clears any stale one
/// before starting, so an existing sentinel here means "abort".
fn copy_disk_payload<W: Write>(
    reader: &mut File,
    sink: &mut W,
    source: &DiskInfo,
    image_path: &str,
    cancel_path: &str,
    partial_path: &str,
) -> Result<()> {
    let start = Instant::now();
    let mut buf = vec![0u8; IMAGE_BLOCK_SIZE];
    let mut bytes_done = 0u64;
    let mut last_print = Instant::now();
    // Publish 0/total up front so the GUI shows the right denominator immediately.
    write_create_progress(image_path, 0, source.total_bytes);

    // Bad regions that stayed unreadable after retries. ddrescue-style: zero-fill
    // and skip them so a single bad block doesn't abort the whole image, and skip
    // fast so we don't hammer (re-reading a bad block can wedge a USB bridge).
    let mut bad_regions: Vec<(u64, u64)> = Vec::new();
    let mut bad_bytes = 0u64;
    let mut retries_at_offset = 0u32;

    while bytes_done < source.total_bytes {
        if std::path::Path::new(cancel_path).exists() {
            let _ = std::fs::remove_file(partial_path);
            let _ = std::fs::remove_file(cancel_path);
            let _ = std::fs::remove_file(create_progress_path(image_path));
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
                sink.write_all(&buf[..read]).map_err(|error| {
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
                    write_create_progress(image_path, bytes_done, source.total_bytes);
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
                    *reader = reacquire_reader(source, bytes_done)?;
                    continue;
                }
                // Still unreadable: treat this block as a bad region. Zero-fill it,
                // record it, and move on instead of aborting or hammering it.
                buf[..remaining].fill(0);
                sink.write_all(&buf[..remaining]).map_err(|error| {
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
                    *reader = reacquire_reader(source, bytes_done)?;
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

/// File the elevated restore writes "<bytes_done> <total>" to, for the GUI to poll.
fn restore_progress_path(image_path: &str) -> String {
    format!("{image_path}.restore-progress")
}

/// File the create-image copy writes "<bytes_done> <total_to_store>" to, for the
/// GUI to poll. `total_to_store` is the *used* bytes for a used-only image, or
/// the full disk size otherwise — the meaningful denominator for the progress
/// bar. The on-disk `.part` size is misleading when the payload is sparse or
/// zstd-compressed, so the GUI reads this instead of stat-ing the file.
fn create_progress_path(image_path: &str) -> String {
    format!("{image_path}.create-progress")
}

/// Best-effort publish of create progress for the GUI poller. A failed write
/// only means a slightly stale bar, so it never aborts the copy.
fn write_create_progress(image_path: &str, bytes_done: u64, total: u64) {
    let _ = std::fs::write(
        create_progress_path(image_path),
        format!("{bytes_done} {total}"),
    );
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

fn write_flow_image_header_v2(
    writer: &mut impl Write,
    source: &DiskInfo,
    compression: Compression,
    block_map: Option<&BlockMap>,
) -> Result<()> {
    let header = flow_image_header_v2(source, compression, block_map)?;

    writer.write_all(FLOW_IMAGE_MAGIC_V2)?;
    writer.write_all(&(header.len() as u64).to_le_bytes())?;
    writer.write_all(&header)?;
    Ok(())
}

fn flow_image_header_v2(
    source: &DiskInfo,
    compression: Compression,
    block_map: Option<&BlockMap>,
) -> Result<Vec<u8>> {
    serde_json::to_vec(&FlowImageHeaderV2 {
        format: FLOW_IMAGE_FORMAT,
        version: FLOW_IMAGE_VERSION_V2,
        source,
        block_size: IMAGE_BLOCK_SIZE as u64,
        uncompressed_bytes: source.total_bytes,
        compression: compression.as_str(),
        mode: if block_map.is_some() {
            "used-only"
        } else {
            "full"
        },
        block_map,
        note: "Disk payload follows this header.",
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
    fn is_permission_error_detects_eperm_in_the_chain() {
        let denied =
            anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                .context("open source /dev/rdisk4");
        assert!(is_permission_error(&denied));

        // A non-permission failure (e.g. not a GPT disk) must not match, so it
        // can still fall back to a full image.
        assert!(!is_permission_error(&anyhow::anyhow!("not a GPT disk")));
    }

    fn external_target(total_bytes: u64) -> DiskInfo {
        let mut target = DiskInfo::placeholder("/dev/disk9");
        target.connection = Connection::Usb;
        target.total_bytes = total_bytes;
        target
    }

    #[test]
    fn validate_restore_target_rejects_unsafe_targets() {
        let mut boot = external_target(500);
        boot.is_boot = true;
        assert!(validate_restore_target(&boot, 100).is_err());

        let mut read_only = external_target(500);
        read_only.read_only = true;
        assert!(validate_restore_target(&read_only, 100).is_err());

        let mut internal = external_target(500);
        internal.connection = Connection::Internal;
        assert!(validate_restore_target(&internal, 100).is_err());

        // Too small.
        assert!(validate_restore_target(&external_target(50), 100).is_err());

        // Good: external, writable, big enough.
        assert!(validate_restore_target(&external_target(500), 100).is_ok());
    }

    fn write_test_image(path: &str, payload: &[u8]) {
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.model = "Test SSD".into();
        source.total_bytes = payload.len() as u64;
        let mut file = File::create(path).expect("create test image");
        write_flow_image_header(&mut file, &source).expect("write header");
        file.write_all(payload).expect("write payload");
        file.sync_all().expect("flush image");
    }

    #[test]
    fn read_flow_image_header_parses_a_valid_image() {
        let payload = b"flowclone restore payload";
        let mut image_path = std::env::temp_dir();
        image_path.push(format!(
            "flowclone-cli-readhdr-{}.flowimg",
            std::process::id()
        ));
        let image_path = image_path.to_string_lossy().to_string();
        write_test_image(&image_path, payload);

        let info = read_flow_image_header(&image_path).expect("read header");
        assert_eq!(info.write_bytes, payload.len() as u64);
        assert_eq!(info.source.model, "Test SSD");
        assert!(info.data_offset > FLOW_IMAGE_MAGIC.len() as u64);

        std::fs::remove_file(image_path).ok();
    }

    #[test]
    fn write_image_to_target_writes_exactly_the_payload() {
        let payload = b"flowclone restore payload bytes!";
        let mut image_path = std::env::temp_dir();
        image_path.push(format!(
            "flowclone-cli-restore-{}.flowimg",
            std::process::id()
        ));
        let image_path = image_path.to_string_lossy().to_string();
        let mut target_path = std::env::temp_dir();
        target_path.push(format!(
            "flowclone-cli-restore-{}.target",
            std::process::id()
        ));
        let target_path = target_path.to_string_lossy().to_string();

        write_test_image(&image_path, payload);
        // The real path opens a device node (which exists); a file target must
        // exist first because we open write-only without create.
        File::create(&target_path).expect("pre-create target");

        let info = read_flow_image_header(&image_path).expect("read header");
        write_image_to_target(&image_path, &target_path, &info).expect("restore");

        let written = std::fs::read(&target_path).expect("read target");
        assert_eq!(written, payload);

        std::fs::remove_file(image_path).ok();
        std::fs::remove_file(target_path).ok();
    }

    #[test]
    fn compression_parses_known_codecs_and_rejects_others() {
        assert!(matches!(
            Compression::parse("none").unwrap(),
            Compression::None
        ));
        assert!(matches!(
            Compression::parse("zstd").unwrap(),
            Compression::Zstd
        ));
        assert!(Compression::parse("lz4").is_err());
        assert_eq!(Compression::None.as_str(), "none");
        assert_eq!(Compression::Zstd.as_str(), "zstd");
    }

    /// Write a v2 image directly (the create path only emits v2 when compressing,
    /// so this exercises reading/restoring an uncompressed v2 — what the future
    /// used-only mode will produce).
    fn write_test_image_v2(path: &str, payload: &[u8], compression: Compression) {
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.model = "Test SSD V2".into();
        source.total_bytes = payload.len() as u64;
        let mut file = File::create(path).expect("create v2 test image");
        write_flow_image_header_v2(&mut file, &source, compression, None).expect("write v2 header");
        match compression {
            Compression::None => file.write_all(payload).expect("write payload"),
            Compression::Zstd => {
                let mut encoder = zstd::Encoder::new(&mut file, ZSTD_LEVEL).expect("encoder");
                encoder.write_all(payload).expect("compress payload");
                encoder.finish().expect("finish encoder");
            }
        }
        file.sync_all().expect("flush image");
    }

    #[test]
    fn restores_a_v2_uncompressed_image() {
        let payload = b"flowclone v2 uncompressed payload!";
        let mut image_path = std::env::temp_dir();
        image_path.push(format!(
            "flowclone-cli-v2none-{}.flowimg",
            std::process::id()
        ));
        let image_path = image_path.to_string_lossy().to_string();
        let mut target_path = std::env::temp_dir();
        target_path.push(format!(
            "flowclone-cli-v2none-{}.target",
            std::process::id()
        ));
        let target_path = target_path.to_string_lossy().to_string();

        write_test_image_v2(&image_path, payload, Compression::None);
        let info = read_flow_image_header(&image_path).expect("read v2 header");
        assert_eq!(info.write_bytes, payload.len() as u64);
        assert!(matches!(info.compression, Compression::None));

        File::create(&target_path).expect("pre-create target");
        write_image_to_target(&image_path, &target_path, &info).expect("restore");

        assert_eq!(std::fs::read(&target_path).expect("read target"), payload);

        std::fs::remove_file(image_path).ok();
        std::fs::remove_file(target_path).ok();
    }

    #[test]
    fn round_trips_a_v2_compressed_image() {
        // A short repeating pattern compresses heavily, so the image must end up
        // smaller than the raw payload, and restoring it must reproduce it byte
        // for byte.
        let payload: Vec<u8> = b"flowclone-sparse-pattern"
            .iter()
            .copied()
            .cycle()
            .take(256 * 1024)
            .collect();

        let mut source_path = std::env::temp_dir();
        source_path.push(format!(
            "flowclone-cli-v2zstd-{}.source",
            std::process::id()
        ));
        let source_path = source_path.to_string_lossy().to_string();
        let mut image_path = std::env::temp_dir();
        image_path.push(format!(
            "flowclone-cli-v2zstd-{}.flowimg",
            std::process::id()
        ));
        let image_path = image_path.to_string_lossy().to_string();
        let mut target_path = std::env::temp_dir();
        target_path.push(format!(
            "flowclone-cli-v2zstd-{}.target",
            std::process::id()
        ));
        let target_path = target_path.to_string_lossy().to_string();

        std::fs::write(&source_path, &payload).expect("write source");
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = payload.len() as u64;
        create_compressed_image_file(&source_path, &image_path, &source)
            .expect("create compressed");

        let image_len = std::fs::metadata(&image_path).expect("image meta").len();
        assert!(
            image_len < payload.len() as u64,
            "expected compression to shrink the image, got {image_len} bytes"
        );

        let info = read_flow_image_header(&image_path).expect("read header");
        assert!(matches!(info.compression, Compression::Zstd));
        assert_eq!(info.write_bytes, payload.len() as u64);

        File::create(&target_path).expect("pre-create target");
        write_image_to_target(&image_path, &target_path, &info).expect("restore");

        assert_eq!(std::fs::read(&target_path).expect("read target"), payload);

        std::fs::remove_file(source_path).ok();
        std::fs::remove_file(image_path).ok();
        std::fs::remove_file(target_path).ok();
    }

    #[test]
    fn block_map_validate_rejects_bad_runs() {
        // Ascending, in range, non-overlapping.
        assert!(BlockMap {
            runs: vec![[0, 2], [3, 1]]
        }
        .validate(4)
        .is_ok());
        // Empty run.
        assert!(BlockMap { runs: vec![[0, 0]] }.validate(4).is_err());
        // Past the end of the disk.
        assert!(BlockMap { runs: vec![[3, 2]] }.validate(4).is_err());
        // Overlapping / not ascending.
        assert!(BlockMap {
            runs: vec![[0, 2], [1, 1]]
        }
        .validate(4)
        .is_err());
    }

    #[test]
    fn block_map_present_bytes_accounts_for_a_partial_last_block() {
        let bs = IMAGE_BLOCK_SIZE as u64;
        let total = 2 * bs + 1000; // 3 blocks; the last is 1000 bytes.
        let map = BlockMap {
            runs: vec![[0, 1], [2, 1]],
        };
        assert_eq!(map.present_bytes(bs, total), bs + 1000);
    }

    /// Writes a sparse v2 image: header with the block map, then only the present
    /// blocks (sliced from `full`), optionally zstd-compressed.
    fn write_sparse_test_image(
        path: &str,
        full: &[u8],
        runs: &[[u64; 2]],
        compression: Compression,
    ) {
        let bs = IMAGE_BLOCK_SIZE;
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = full.len() as u64;
        let map = BlockMap {
            runs: runs.to_vec(),
        };

        let mut payload = Vec::new();
        for &[start, count] in runs {
            for block in start..start + count {
                let off = block as usize * bs;
                let end = (off + bs).min(full.len());
                payload.extend_from_slice(&full[off..end]);
            }
        }

        let mut file = File::create(path).expect("create sparse image");
        write_flow_image_header_v2(&mut file, &source, compression, Some(&map))
            .expect("write v2 header");
        match compression {
            Compression::None => file.write_all(&payload).expect("write payload"),
            Compression::Zstd => {
                let mut encoder = zstd::Encoder::new(&mut file, ZSTD_LEVEL).expect("encoder");
                encoder.write_all(&payload).expect("compress payload");
                encoder.finish().expect("finish encoder");
            }
        }
        file.sync_all().expect("flush image");
    }

    fn sparse_round_trip(compression: Compression, suffix: &str) {
        let bs = IMAGE_BLOCK_SIZE;
        // Three blocks: block 0 present (0xAB), block 1 absent (zeros), block 2
        // present and partial (0xCD, 1000 bytes).
        let full_len = 2 * bs + 1000;
        let mut full = vec![0u8; full_len];
        full[0..bs].fill(0xAB);
        full[2 * bs..2 * bs + 1000].fill(0xCD);
        let runs = [[0u64, 1], [2, 1]];

        let dir = std::env::temp_dir();
        let image_path = dir
            .join(format!(
                "flowclone-cli-sparse-{suffix}-{}.flowimg",
                std::process::id()
            ))
            .to_string_lossy()
            .to_string();
        let target_path = dir
            .join(format!(
                "flowclone-cli-sparse-{suffix}-{}.target",
                std::process::id()
            ))
            .to_string_lossy()
            .to_string();

        write_sparse_test_image(&image_path, &full, &runs, compression);

        let info = read_flow_image_header(&image_path).expect("read header");
        assert_eq!(info.write_bytes, full_len as u64);
        assert!(
            info.block_map.is_some(),
            "sparse image should carry a block map"
        );

        File::create(&target_path).expect("pre-create target");
        write_image_to_target(&image_path, &target_path, &info).expect("restore");

        let written = std::fs::read(&target_path).expect("read target");
        assert_eq!(written, full, "restored sparse image must match the source");

        std::fs::remove_file(image_path).ok();
        std::fs::remove_file(target_path).ok();
    }

    #[test]
    fn restores_a_sparse_image_uncompressed() {
        sparse_round_trip(Compression::None, "none");
    }

    #[test]
    fn restores_a_sparse_image_compressed() {
        sparse_round_trip(Compression::Zstd, "zstd");
    }

    /// The producer half: create a sparse image straight from a source file using
    /// a block map, then restore it and confirm present blocks survive and absent
    /// ones come back as zeros (not their old contents).
    fn create_sparse_round_trip(compression: Compression, suffix: &str) {
        let bs = IMAGE_BLOCK_SIZE;
        let full_len = 3 * bs;
        let mut source_bytes = vec![0u8; full_len];
        source_bytes[0..bs].fill(0xAB); // block 0 present
        source_bytes[bs..2 * bs].fill(0xEE); // block 1 absent — must be dropped
        source_bytes[2 * bs..3 * bs].fill(0xCD); // block 2 present
        let map = BlockMap {
            runs: vec![[0, 1], [2, 1]],
        };

        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let source_path = dir
            .join(format!("flowclone-cli-csparse-{suffix}-{pid}.source"))
            .to_string_lossy()
            .to_string();
        let image_path = dir
            .join(format!("flowclone-cli-csparse-{suffix}-{pid}.flowimg"))
            .to_string_lossy()
            .to_string();
        let target_path = dir
            .join(format!("flowclone-cli-csparse-{suffix}-{pid}.target"))
            .to_string_lossy()
            .to_string();

        std::fs::write(&source_path, &source_bytes).expect("write source");
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = full_len as u64;

        create_sparse_image_file(&source_path, &image_path, &source, &map, compression)
            .expect("create sparse");

        // The create copy publishes a progress sidecar the GUI polls. Its total
        // must be the *used* bytes (2 present blocks), not the full disk size —
        // that's what keeps the bar/ETA accurate for sparse and zstd images.
        let progress = std::fs::read_to_string(create_progress_path(&image_path))
            .expect("progress sidecar written");
        let mut parts = progress.split_whitespace();
        let done: u64 = parts.next().unwrap().parse().unwrap();
        let total: u64 = parts.next().unwrap().parse().unwrap();
        assert_eq!(total, (2 * bs) as u64, "total is used bytes, not full disk");
        assert_eq!(done, total, "final progress reaches 100%");

        let info = read_flow_image_header(&image_path).expect("read header");
        assert!(info.block_map.is_some());

        File::create(&target_path).expect("pre-create target");
        write_image_to_target(&image_path, &target_path, &info).expect("restore");

        // Block 1 was absent → restored as zeros; the dropped 0xEE must be gone.
        let mut expected = source_bytes;
        expected[bs..2 * bs].fill(0);
        assert_eq!(std::fs::read(&target_path).expect("read target"), expected);

        std::fs::remove_file(source_path).ok();
        std::fs::remove_file(create_progress_path(&image_path)).ok();
        std::fs::remove_file(image_path).ok();
        std::fs::remove_file(target_path).ok();
    }

    #[test]
    fn create_sparse_image_round_trips_uncompressed() {
        create_sparse_round_trip(Compression::None, "none");
    }

    #[test]
    fn create_sparse_image_round_trips_compressed() {
        create_sparse_round_trip(Compression::Zstd, "zstd");
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
