//! The clone engine — orchestrates the full source → target flow.
//!
//! [`CloneEngine`] is the single entry point the Tauri layer calls. It owns the
//! ordering and safety of the whole operation. Each stage is delegated to a
//! dedicated crate:
//!
//! - [`flowclone_raw`] performs the byte copy
//! - [`flowclone_verify`] verifies the result
//! - [`flowclone_report`] builds the final report
//!
//! The engine emits [`Progress`](crate::progress::Progress) throughout so the
//! UI can stay purely presentational.

use crate::error::{CoreError, Result};
use crate::job::{CloneJob, CloneRequest, JobStatus};
use crate::progress::{Phase, Progress, ProgressEmitter};
use flowclone_disk::{DiskCatalog, DiskCatalogApi};
use flowclone_raw::{self, ProgressSink, RawProgress};
use flowclone_report::{ReportData, ReportFormat, ReportWriter};
use flowclone_verify::{Verifier, VerifyResult};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

/// Tunable options for [`CloneEngine::run`].
#[derive(Debug, Clone, Default)]
pub struct CloneOptions {
    pub block_size: Option<usize>,
    pub max_bytes_per_sec: Option<u64>,
    pub verify: bool,
}

/// Outcome of a successful clone run.
#[derive(Debug)]
pub struct CloneOutcome {
    pub job_id: String,
    pub status: JobStatus,
    pub copy: flowclone_raw::RawCopyResult,
    pub verify: Option<VerifyResult>,
}

/// Adapter that turns raw-engine progress callbacks into core [`Progress`]
/// events. Keeps the `flowclone-raw` crate free of any dependency on core.
struct RawSink {
    job_id: String,
    emitter: ProgressEmitter,
}

impl ProgressSink for RawSink {
    fn on_progress(&self, p: RawProgress) {
        let fraction = if p.bytes_total > 0 {
            (p.bytes_done as f64 / p.bytes_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let eta_secs = if p.bytes_done > 0 && p.bytes_total > p.bytes_done && p.elapsed_secs > 0.0 {
            let rate = p.bytes_done as f64 / p.elapsed_secs;
            Some(((p.bytes_total - p.bytes_done) as f64 / rate).max(0.0))
        } else {
            None
        };
        self.emitter.emit(Progress {
            job_id: self.job_id.clone(),
            phase: Phase::Cloning,
            fraction,
            bytes_done: p.bytes_done,
            bytes_total: p.bytes_total,
            read_speed: p.last_block_bytes,
            write_speed: p.last_block_bytes,
            elapsed_secs: p.elapsed_secs,
            eta_secs,
            current_operation: format!("Reading block {}", p.block_index),
        });
    }
}

/// Orchestrates the full clone workflow.
///
/// Constructed once at app startup and shared (via `Arc`) across Tauri
/// commands. It is `Clone` (cheap, internals are `Arc`).
#[derive(Clone)]
pub struct CloneEngine {
    catalog: DiskCatalog,
    raw: Arc<dyn flowclone_raw::RawEngine>,
    verifier: Arc<dyn Verifier>,
    emitter: ProgressEmitter,
}

impl CloneEngine {
    /// Build a new engine with the default platform disk catalog and the
    /// standard raw / verify backends.
    pub fn new() -> Self {
        Self {
            catalog: DiskCatalog::platform_default(),
            raw: Arc::new(flowclone_raw::default_engine()),
            verifier: Arc::new(flowclone_verify::default_verifier()),
            emitter: ProgressEmitter::default(),
        }
    }

    /// Subscribe to the live progress stream.
    pub fn progress(&self) -> crate::progress::ProgressReceiver {
        self.emitter.subscribe()
    }

    /// Resolve a [`CloneRequest`] from device paths by looking up current disk
    /// metadata. This re-reads the catalog at request time to avoid TOCTOU on
    /// stale disk info captured earlier in the UI.
    pub fn resolve_request(
        &self,
        source_path: &str,
        target_path: &str,
        options: &CloneOptions,
    ) -> Result<CloneRequest> {
        let disks = self
            .catalog
            .list()
            .map_err(|e| CoreError::Disk(e.to_string()))?;
        let source = disks
            .iter()
            .find(|d| d.device_path == source_path)
            .cloned()
            .ok_or_else(|| CoreError::SourceNotFound(source_path.into()))?;
        let target = disks
            .iter()
            .find(|d| d.device_path == target_path)
            .cloned()
            .ok_or_else(|| CoreError::TargetNotFound(target_path.into()))?;

        Ok(CloneRequest {
            source,
            target,
            max_bytes_per_sec: options.max_bytes_per_sec,
            verify: options.verify,
        })
    }

    /// Run a clone job to completion. Awaits the caller (run inside a Tauri
    /// async task).
    pub async fn run(&self, mut job: CloneJob, options: CloneOptions) -> Result<CloneOutcome> {
        job.mark_running();
        let started = Instant::now();
        let job_id = job.id.clone();
        let cancel = job.cancel_token();

        self.emit_preparing(&job_id, &job.request);

        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            job.mark_failed();
            return Err(CoreError::Cancelled(job_id));
        }

        // --- 1. Raw copy ---------------------------------------------------
        let raw_opts = flowclone_raw::RawOptions {
            block_size: options.block_size.unwrap_or(4 * 1024 * 1024),
            max_bytes_per_sec: options.max_bytes_per_sec,
        };
        let bytes_total = job.request.source.total_bytes;
        let source_path = job.request.source.device_path.clone();
        let target_path = job.request.target.device_path.clone();

        let sink = RawSink {
            job_id: job_id.clone(),
            emitter: self.emitter.clone(),
        };
        let raw = Arc::clone(&self.raw);
        let cancel_for_copy = Arc::clone(&cancel);
        let copy_result = {
            tokio::task::spawn_blocking(move || {
                raw.copy(
                    &source_path,
                    &target_path,
                    bytes_total,
                    raw_opts,
                    &cancel_for_copy,
                    &sink,
                )
            })
            .await
            .map_err(|e| CoreError::Other(e.into()))?
            .map_err(|e| CoreError::Raw(e.to_string()))?
        };

        if job.is_cancelled() {
            job.mark_failed();
            return Err(CoreError::Cancelled(job_id));
        }

        // --- 2. Verification (optional) -----------------------------------
        let verify_result = if options.verify {
            self.emit_phase(&job_id, Phase::Verifying, bytes_total);
            let verifier = Arc::clone(&self.verifier);
            let source_path = job.request.source.device_path.clone();
            let target_path = job.request.target.device_path.clone();
            let r = tokio::task::spawn_blocking(move || {
                verifier.verify(&source_path, &target_path, bytes_total)
            })
            .await
            .map_err(|e| CoreError::Other(e.into()))?
            .map_err(CoreError::Other)?;

            if !r.matched {
                job.mark_failed();
                return Err(CoreError::VerificationFailed(r.summary()));
            }
            Some(r)
        } else {
            None
        };

        // --- 3. Done -------------------------------------------------------
        job.mark_completed();
        let elapsed = started.elapsed().as_secs_f64();
        self.emit_completed(&job_id, elapsed);

        info!(job = %job_id, elapsed_s = elapsed, "clone completed");

        Ok(CloneOutcome {
            job_id,
            status: JobStatus::Completed,
            copy: copy_result,
            verify: verify_result,
        })
    }

    /// Render and write a report for a completed job.
    pub fn write_report(
        &self,
        outcome: &CloneOutcome,
        request: &CloneRequest,
        format: ReportFormat,
        dest: &Path,
    ) -> Result<()> {
        let data = ReportData {
            source: request.source.clone(),
            target: request.target.clone(),
            started_at: None,
            duration_secs: outcome.copy.elapsed_secs,
            average_speed: outcome.copy.average_speed,
            verified: outcome.verify.clone(),
            warnings: Vec::new(),
            app_version: crate::VERSION.to_string(),
        };
        ReportWriter::write(format, &data, dest).map_err(CoreError::Other)
    }

    fn emit_preparing(&self, job_id: &str, request: &CloneRequest) {
        self.emitter.emit(Progress {
            job_id: job_id.into(),
            phase: Phase::Preparing,
            fraction: 0.0,
            bytes_done: 0,
            bytes_total: request.source.total_bytes,
            read_speed: 0,
            write_speed: 0,
            elapsed_secs: 0.0,
            eta_secs: None,
            current_operation: "Preparing disks".into(),
        });
    }

    fn emit_phase(&self, job_id: &str, phase: Phase, total: u64) {
        self.emitter.emit(Progress {
            job_id: job_id.into(),
            phase,
            fraction: 0.0,
            bytes_done: 0,
            bytes_total: total,
            read_speed: 0,
            write_speed: 0,
            elapsed_secs: 0.0,
            eta_secs: None,
            current_operation: "Verifying".into(),
        });
    }

    fn emit_completed(&self, job_id: &str, elapsed: f64) {
        self.emitter.emit(Progress {
            job_id: job_id.into(),
            phase: Phase::Completed,
            fraction: 1.0,
            bytes_done: 0,
            bytes_total: 0,
            read_speed: 0,
            write_speed: 0,
            elapsed_secs: elapsed,
            eta_secs: Some(0.0),
            current_operation: "Completed".into(),
        });
    }
}

impl Default for CloneEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_request_rejects_unknown_source() {
        let engine = CloneEngine::new();
        let res = engine.resolve_request("/dev/nope", "/dev/nope2", &CloneOptions::default());
        assert!(res.is_err());
    }
}
