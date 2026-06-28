//! FlowClone raw clone engine.
//!
//! Phase 1 is intentionally stubbed. The crate owns the progress model and the
//! future raw I/O modules, but the default engine only simulates progress.
//!
//! Progress is reported via a caller-supplied [`ProgressSink`] so this crate
//! stays free of any dependency on the core crate (the core adapts the sink to
//! its own emitter).

pub mod buffer;
pub mod reader;
pub mod throttle;
pub mod writer;

use serde::{Deserialize, Serialize};
use std::path::Path;
#[cfg(not(target_os = "windows"))]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MIN_OUTPUT_RESERVE_BYTES: u64 = 1_000_000_000;
const MAX_OUTPUT_RESERVE_BYTES: u64 = 20_000_000_000;
const OUTPUT_RESERVE_DIVISOR: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpace {
    pub required_bytes: u64,
    pub reserve_bytes: u64,
    pub available_bytes: u64,
}

/// Ensure an image output path has enough free space plus a small safety reserve.
pub fn ensure_free_space_for_output(
    output_path: impl AsRef<Path>,
    required_bytes: u64,
) -> anyhow::Result<OutputSpace> {
    let output_path = output_path.as_ref();
    let directory = output_directory(output_path)?;
    let available_bytes =
        available_space_bytes(directory)?.saturating_add(reclaimable_output_bytes(output_path));
    check_output_space(required_bytes, available_bytes, directory)
}

fn output_directory(output_path: &Path) -> anyhow::Result<&Path> {
    let directory = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if directory.exists() {
        Ok(directory)
    } else {
        anyhow::bail!(
            "image output folder does not exist: {}",
            directory.display()
        )
    }
}

fn reclaimable_output_bytes(output_path: &Path) -> u64 {
    std::fs::metadata(output_path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

/// Free bytes available on the volume backing `path`.
///
/// Unix shells out to `df`; Windows asks the kernel via `GetDiskFreeSpaceExW`
/// (there is no `df` there). Both return the space available to the caller.
#[cfg(not(target_os = "windows"))]
fn available_space_bytes(path: &Path) -> anyhow::Result<u64> {
    let output = Command::new("df")
        .args(["-P", "-k"])
        .arg(path)
        .output()
        .map_err(|error| anyhow::anyhow!("failed to check free disk space: {error}"))?;
    if !output.status.success() {
        anyhow::bail!("failed to check free disk space for {}", path.display());
    }
    parse_df_available_kib(&String::from_utf8_lossy(&output.stdout)).map(|kib| kib * 1024)
}

#[cfg(target_os = "windows")]
fn available_space_bytes(path: &Path) -> anyhow::Result<u64> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;
    }

    // A null-terminated UTF-16 directory path; the kernel reports the free space
    // on whichever volume backs it.
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_to_caller: u64 = 0;
    // SAFETY: `wide` is a valid null-terminated UTF-16 string that outlives the
    // call; the out-pointer is a live local and the other two are allowed to be
    // null per the API contract.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let error = std::io::Error::last_os_error();
        anyhow::bail!(
            "failed to check free disk space for {}: {error}",
            path.display()
        );
    }
    Ok(free_to_caller)
}

#[cfg(not(target_os = "windows"))]
fn parse_df_available_kib(output: &str) -> anyhow::Result<u64> {
    let line = output
        .lines()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("failed to parse free disk space"))?;
    let available = line
        .split_whitespace()
        .nth(3)
        .ok_or_else(|| anyhow::anyhow!("failed to parse free disk space"))?;
    available
        .parse::<u64>()
        .map_err(|error| anyhow::anyhow!("failed to parse free disk space: {error}"))
}

fn check_output_space(
    required_bytes: u64,
    available_bytes: u64,
    directory: &Path,
) -> anyhow::Result<OutputSpace> {
    let reserve_bytes = output_reserve_bytes(required_bytes);
    let needed_bytes = required_bytes
        .checked_add(reserve_bytes)
        .ok_or_else(|| anyhow::anyhow!("image size is too large to validate"))?;
    if available_bytes < needed_bytes {
        anyhow::bail!(
            "not enough space for image in {}: need {} plus {} reserve, available {}",
            directory.display(),
            format_bytes(required_bytes),
            format_bytes(reserve_bytes),
            format_bytes(available_bytes)
        );
    }
    Ok(OutputSpace {
        required_bytes,
        reserve_bytes,
        available_bytes,
    })
}

fn output_reserve_bytes(required_bytes: u64) -> u64 {
    (required_bytes / OUTPUT_RESERVE_DIVISOR)
        .clamp(MIN_OUTPUT_RESERVE_BYTES, MAX_OUTPUT_RESERVE_BYTES)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[(&str, u64)] = &[
        ("TB", 1_000_000_000_000),
        ("GB", 1_000_000_000),
        ("MB", 1_000_000),
        ("KB", 1_000),
    ];
    for (unit, scale) in UNITS {
        if bytes >= *scale {
            return format!("{:.1} {unit}", bytes as f64 / *scale as f64);
        }
    }
    format!("{bytes} B")
}

pub use buffer::BufferPool;
pub use reader::RawReader;
pub use writer::RawWriter;

/// Tunable parameters for a raw copy.
#[derive(Debug, Clone, Copy)]
pub struct RawOptions {
    /// Block size in bytes. Defaults to 4 MiB when unset.
    pub block_size: usize,
    /// Optional throughput cap in bytes/sec.
    pub max_bytes_per_sec: Option<u64>,
}

impl Default for RawOptions {
    fn default() -> Self {
        Self {
            block_size: 4 * 1024 * 1024,
            max_bytes_per_sec: None,
        }
    }
}

/// Aggregate stats from a completed raw copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCopyResult {
    pub bytes_copied: u64,
    pub elapsed_secs: f64,
    pub average_speed: u64,
    pub blocks: u64,
}

/// A snapshot of raw-copy progress, pushed to the caller via [`ProgressSink`].
#[derive(Debug, Clone, Copy)]
pub struct RawProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub last_block_bytes: u64,
    pub block_index: u64,
    pub elapsed_secs: f64,
}

/// Receiver of raw progress. Implemented by the core crate's adapter.
pub trait ProgressSink {
    fn on_progress(&self, p: RawProgress);
}

/// A no-op sink for callers (e.g. tests) that don't care about progress.
pub struct NoProgress;
impl ProgressSink for NoProgress {
    fn on_progress(&self, _p: RawProgress) {}
}

/// Trait abstracting the raw copy so the core can swap in a mock for tests.
pub trait RawEngine: Send + Sync {
    fn copy(
        &self,
        source: &str,
        target: &str,
        total_bytes: u64,
        options: RawOptions,
        cancel: &Arc<AtomicBool>,
        sink: &dyn ProgressSink,
    ) -> anyhow::Result<RawCopyResult>;
}

/// The default Phase 1 raw engine. It never opens or writes a device.
pub struct DefaultRawEngine;

impl RawEngine for DefaultRawEngine {
    fn copy(
        &self,
        _source: &str,
        _target: &str,
        total_bytes: u64,
        options: RawOptions,
        cancel: &Arc<AtomicBool>,
        sink: &dyn ProgressSink,
    ) -> anyhow::Result<RawCopyResult> {
        let start = Instant::now();
        let blocks = 12;
        let bytes_per_tick = (total_bytes / blocks).max(options.block_size as u64);
        let mut bytes_done = 0u64;

        for block_index in 1..=blocks {
            if cancel.load(Ordering::SeqCst) {
                return Err(RawError::Cancelled.into());
            }

            bytes_done = bytes_done.saturating_add(bytes_per_tick).min(total_bytes);
            std::thread::sleep(Duration::from_millis(80));
            sink.on_progress(RawProgress {
                bytes_done,
                bytes_total: total_bytes,
                last_block_bytes: bytes_per_tick,
                block_index,
                elapsed_secs: start.elapsed().as_secs_f64(),
            });
        }

        let elapsed_secs = start.elapsed().as_secs_f64();
        let average_speed = if elapsed_secs > 0.0 {
            (bytes_done as f64 / elapsed_secs) as u64
        } else {
            0
        };

        Ok(RawCopyResult {
            bytes_copied: bytes_done,
            elapsed_secs,
            average_speed,
            blocks,
        })
    }
}

/// Construct the default stub raw engine.
pub fn default_engine() -> DefaultRawEngine {
    DefaultRawEngine
}

/// Error type for raw I/O — stringly-typed to keep the crate dependency-free.
#[derive(Debug, thiserror::Error)]
pub enum RawError {
    #[error("open source {0}: {1}")]
    OpenSource(String, String),
    #[error("open target {0}: {1}")]
    OpenTarget(String, String),
    #[error("read: {0}")]
    Read(String),
    #[error("write: {0}")]
    Write(String),
    #[error("flush: {0}")]
    Flush(String),
    #[error("cancelled")]
    Cancelled,
}

/// Future real copy loop. This is deliberately unused until the privileged
/// helper and destructive-write gates are implemented.
#[allow(dead_code)]
fn copy_raw(
    source: &str,
    target: &str,
    total_bytes: u64,
    options: RawOptions,
    cancel: &Arc<AtomicBool>,
    sink: &dyn ProgressSink,
) -> anyhow::Result<RawCopyResult> {
    use crate::RawError as E;
    let block_size = options.block_size.max(4096);
    let mut reader = RawReader::open(Path::new(source))
        .map_err(|e| E::OpenSource(source.into(), e.to_string()))?;
    let mut writer = RawWriter::open(Path::new(target))
        .map_err(|e| E::OpenTarget(target.into(), e.to_string()))?;

    let pool = BufferPool::new(2, block_size);
    let mut throttle = throttle::Throttle::new(options.max_bytes_per_sec);
    let start = Instant::now();
    let mut bytes_done = 0u64;
    let mut blocks = 0u64;
    let mut last_emit = Instant::now();

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(E::Cancelled.into());
        }

        let mut buf = pool.acquire();
        let n = reader
            .read_block(&mut buf)
            .map_err(|e| E::Read(e.to_string()))?;
        if n == 0 {
            break;
        }

        writer
            .write_block(&buf[..n])
            .map_err(|e| E::Write(e.to_string()))?;
        bytes_done += n as u64;
        blocks += 1;

        throttle.wait(n as u64);

        // Throttle progress events to ~10 Hz so the caller isn't flooded.
        if last_emit.elapsed() >= Duration::from_millis(100) {
            last_emit = Instant::now();
            sink.on_progress(RawProgress {
                bytes_done,
                bytes_total: total_bytes,
                last_block_bytes: n as u64,
                block_index: blocks,
                elapsed_secs: start.elapsed().as_secs_f64(),
            });
        }

        pool.release(buf);
    }

    writer.flush().map_err(|e| E::Flush(e.to_string()))?;

    let elapsed = start.elapsed().as_secs_f64();
    let average_speed = if elapsed > 0.0 {
        (bytes_done as f64 / elapsed) as u64
    } else {
        0
    };

    Ok(RawCopyResult {
        bytes_copied: bytes_done,
        elapsed_secs: elapsed,
        average_speed,
        blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_defaults_to_4mib() {
        let o = RawOptions::default();
        assert_eq!(o.block_size, 4 * 1024 * 1024);
    }

    #[test]
    fn no_progress_sink_is_a_valid_sink() {
        fn _accepts(_: &dyn ProgressSink) {}
        _accepts(&NoProgress);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn parses_df_available_space() {
        let output = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 1000000 1000 998000 1% /tmp\n";

        assert_eq!(parse_df_available_kib(output).unwrap(), 998000);
    }

    #[test]
    fn output_reserve_is_clamped() {
        assert_eq!(output_reserve_bytes(10_000_000), 1_000_000_000);
        assert_eq!(output_reserve_bytes(500_000_000_000), 10_000_000_000);
        assert_eq!(output_reserve_bytes(2_000_000_000_000), 20_000_000_000);
    }

    #[test]
    fn rejects_output_without_reserve_space() {
        let error = check_output_space(100_000_000_000, 101_000_000_000, Path::new("/tmp"))
            .expect_err("space should be insufficient");

        assert!(error.to_string().contains("not enough space for image"));
    }
}
