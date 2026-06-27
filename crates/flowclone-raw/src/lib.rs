//! FlowClone raw read/write engine.
//!
//! Owns the low-level byte stream from a source device to a target device.
//! The engine:
//!
//! - reads from the raw source in fixed-size blocks (`reader`)
//! - writes them to the raw target (`writer`)
//! - reuses a small pool of buffers to avoid per-block allocation (`buffer`)
//! - optionally caps throughput (`throttle`)
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// The default production raw engine.
pub struct DefaultRawEngine;

impl RawEngine for DefaultRawEngine {
    fn copy(
        &self,
        source: &str,
        target: &str,
        total_bytes: u64,
        options: RawOptions,
        cancel: &Arc<AtomicBool>,
        sink: &dyn ProgressSink,
    ) -> anyhow::Result<RawCopyResult> {
        copy_raw(source, target, total_bytes, options, cancel, sink)
    }
}

/// Construct the default raw engine.
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

/// Core copy loop. Opens the raw devices, then pumps blocks source → target
/// while reporting progress and honouring cancellation.
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
}
