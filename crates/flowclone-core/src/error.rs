//! Error types for the core crate.

use thiserror::Error;

/// Result alias used across the core crate.
pub type Result<T> = std::result::Result<T, CoreError>;

/// Errors produced while orchestrating a clone.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("source and target must be different devices")]
    SameDevice,

    #[error("target disk ({target_bytes}) is smaller than source disk ({source_bytes})")]
    TargetTooSmall {
        source_bytes: u64,
        target_bytes: u64,
    },

    #[error("source disk not found: {0}")]
    SourceNotFound(String),

    #[error("target disk not found: {0}")]
    TargetNotFound(String),

    #[error("job {0} is already running")]
    AlreadyRunning(String),

    #[error("job {0} was cancelled")]
    Cancelled(String),

    #[error("verification failed: {0}")]
    VerificationFailed(String),

    #[error("disk error: {0}")]
    Disk(String),

    #[error("raw I/O error: {0}")]
    Raw(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
