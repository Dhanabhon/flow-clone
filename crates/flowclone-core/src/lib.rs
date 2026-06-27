//! FlowClone core — clone orchestration engine.
//!
//! This crate is the **most important module** in FlowClone. It orchestrates
//! the entire cloning workflow:
//!
//! 1. Validate source / target
//! 2. Create a [`CloneJob`]
//! 3. Start the raw clone (delegated to [`flowclone_raw`])
//! 4. Emit [`Progress`] updates
//! 5. Run verification (delegated to [`flowclone_verify`])
//! 6. Generate a report (delegated to [`flowclone_report`])
//!
//! The UI never clones directly — it calls into this crate via Tauri commands.

pub mod clone;
pub mod error;
pub mod job;
pub mod progress;

pub use clone::{CloneEngine, CloneOptions};
pub use error::{CoreError, Result};
pub use job::{CloneJob, CloneRequest, JobId, JobStatus};
pub use progress::{Phase, Progress, ProgressEmitter};

/// Crate-wide version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
