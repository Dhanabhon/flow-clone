//! Progress reporting for clone jobs.
//!
//! The core emits [`Progress`] snapshots over a [`ProgressEmitter`]. The Tauri
//! layer subscribes to these and forwards them as events to the React UI.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

/// High-level phase of a clone job, used to drive UI state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Preparing disks and validating inputs.
    Preparing,
    /// Raw bytes are being copied source → target.
    Cloning,
    /// Post-clone verification is running.
    Verifying,
    /// Job finished successfully.
    Completed,
    /// Job failed or was aborted.
    Failed,
}

/// An immutable snapshot of clone progress at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub job_id: String,
    pub phase: Phase,
    /// Fraction completed, `0.0..=1.0`.
    pub fraction: f64,
    /// Bytes processed so far.
    pub bytes_done: u64,
    /// Total bytes to process.
    pub bytes_total: u64,
    /// Current read throughput in bytes/sec.
    pub read_speed: u64,
    /// Current write throughput in bytes/sec.
    pub write_speed: u64,
    /// Elapsed seconds.
    pub elapsed_secs: f64,
    /// Estimated remaining seconds (`None` if unknown).
    pub eta_secs: Option<f64>,
    /// Human-readable current operation, e.g. `"Reading block 12345"`.
    pub current_operation: String,
}

impl Progress {
    /// Convenience percentage in `0..=100`.
    pub fn percent(&self) -> f64 {
        (self.fraction * 100.0).clamp(0.0, 100.0)
    }
}

/// Handle used by the UI / Tauri layer to receive [`Progress`] updates.
pub type ProgressReceiver = broadcast::Receiver<Progress>;

/// Broadcasts [`Progress`] snapshots to one or more subscribers.
#[derive(Clone)]
pub struct ProgressEmitter {
    tx: Arc<broadcast::Sender<Progress>>,
}

impl ProgressEmitter {
    /// Create a new emitter. `capacity` sets the per-subscriber buffer size;
    /// slow subscribers drop intermediate updates.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx: Arc::new(tx) }
    }

    /// Subscribe to the progress stream.
    pub fn subscribe(&self) -> ProgressReceiver {
        self.tx.subscribe()
    }

    /// Emit a snapshot. Returns an error only if there are no subscribers,
    /// which is non-fatal.
    pub fn emit(&self, progress: Progress) {
        let _ = self.tx.send(progress);
    }
}

impl Default for ProgressEmitter {
    fn default() -> Self {
        Self::new(256)
    }
}
