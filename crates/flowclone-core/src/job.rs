//! Clone job model — the request, the running job, and its lifecycle.

use crate::error::{CoreError, Result};
use flowclone_disk::DiskInfo;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Opaque identifier for a clone job.
pub type JobId = String;

/// Lifecycle status of a [`CloneJob`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Inputs validated, ready to start.
    Pending,
    /// Currently running.
    Running,
    /// Finished and verified.
    Completed,
    /// Failed or aborted.
    Failed,
}

/// A validated request to clone `source` into `target`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneRequest {
    pub source: DiskInfo,
    pub target: DiskInfo,
    /// Optional cap on throughput, in bytes/sec.
    pub max_bytes_per_sec: Option<u64>,
    /// Run post-clone verification.
    pub verify: bool,
}

impl CloneRequest {
    /// Validate the request before a job is created.
    ///
    /// These guards exist to make it impossible to start a destructive
    /// operation on obviously-wrong inputs. See `docs/SAFETY.md`.
    pub fn validate(&self) -> Result<()> {
        if self.source.device_path == self.target.device_path {
            return Err(CoreError::SameDevice);
        }
        if self.target.total_bytes < self.source.total_bytes {
            return Err(CoreError::TargetTooSmall {
                source_bytes: self.source.total_bytes,
                target_bytes: self.target.total_bytes,
            });
        }
        Ok(())
    }
}

/// Options that tune how a clone is executed.
#[derive(Debug, Clone, Default)]
pub struct CloneOptions {
    pub block_size: Option<usize>,
    pub max_bytes_per_sec: Option<u64>,
    pub verify: bool,
}

/// A live clone job.
pub struct CloneJob {
    pub id: JobId,
    pub request: CloneRequest,
    status: JobStatus,
    started: Option<Instant>,
    cancel_flag: Arc<AtomicBool>,
}

impl CloneJob {
    /// Create a new pending job. Does **not** start cloning.
    pub fn new(request: CloneRequest) -> Result<Self> {
        request.validate()?;
        Ok(Self {
            id: format!("job-{}", short_id()),
            request,
            status: JobStatus::Pending,
            started: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn status(&self) -> JobStatus {
        self.status
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.map(|t| t.elapsed()).unwrap_or_default()
    }

    /// Handle used to cooperatively cancel a running clone.
    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel_flag)
    }

    /// Mark the job as started.
    pub(crate) fn mark_running(&mut self) {
        self.status = JobStatus::Running;
        if self.started.is_none() {
            self.started = Some(Instant::now());
        }
    }

    /// Mark the job as completed.
    pub(crate) fn mark_completed(&mut self) {
        self.status = JobStatus::Completed;
    }

    /// Mark the job as failed.
    pub(crate) fn mark_failed(&mut self) {
        self.status = JobStatus::Failed;
    }

    /// Request cooperative cancellation.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }
}

/// Short, job-local random id. Not cryptographically unique — only for display.
fn short_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{now:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowclone_disk::DiskInfo;

    fn disk(path: &str, bytes: u64) -> DiskInfo {
        DiskInfo {
            device_path: path.into(),
            total_bytes: bytes,
            ..DiskInfo::default()
        }
    }

    #[test]
    fn rejects_same_device() {
        let req = CloneRequest {
            source: disk("/dev/disk0", 100),
            target: disk("/dev/disk0", 100),
            max_bytes_per_sec: None,
            verify: false,
        };
        assert!(matches!(req.validate(), Err(CoreError::SameDevice)));
    }

    #[test]
    fn rejects_target_too_small() {
        let req = CloneRequest {
            source: disk("/dev/disk0", 500),
            target: disk("/dev/disk1", 250),
            max_bytes_per_sec: None,
            verify: false,
        };
        assert!(matches!(
            req.validate(),
            Err(CoreError::TargetTooSmall { .. })
        ));
    }
}
