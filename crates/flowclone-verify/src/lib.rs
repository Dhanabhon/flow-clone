//! FlowClone verification engine.
//!
//! After a raw clone, the verifier confirms source and target match. To keep
//! verification fast on multi-hundred-GB disks we hash blocks independently in
//! [`sampler`] and compare them in [`compare`], rather than hashing the whole
//! device in one pass.

pub mod checksum;
pub mod compare;
pub mod sampler;

use serde::{Deserialize, Serialize};

pub use compare::CompareStrategy;

/// Result of a full verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    /// True when every sampled block matched.
    pub matched: bool,
    /// Total bytes checked.
    pub bytes_checked: u64,
    /// Number of blocks sampled.
    pub blocks_checked: u64,
    /// Number of blocks that mismatched.
    pub mismatches: u64,
    /// Elapsed seconds.
    pub elapsed_secs: f64,
}

impl VerifyResult {
    /// Short human-readable summary for error messages and reports.
    pub fn summary(&self) -> String {
        if self.matched {
            format!(
                "verified {} bytes ({} blocks) in {:.2}s",
                self.bytes_checked, self.blocks_checked, self.elapsed_secs
            )
        } else {
            format!(
                "{} of {} blocks mismatched",
                self.mismatches, self.blocks_checked
            )
        }
    }
}

/// Result alias for verification.
pub type Result<T> = anyhow::Result<T>;

/// Trait abstracting the verifier so the core can mock it.
pub trait Verifier: Send + Sync {
    fn verify(&self, source: &str, target: &str, total_bytes: u64) -> Result<VerifyResult>;
}

/// Default verifier: hashes blocks of source and target and compares.
pub struct DefaultVerifier {
    block_size: usize,
}

impl DefaultVerifier {
    pub fn new() -> Self {
        Self {
            block_size: 4 * 1024 * 1024,
        }
    }
}

impl Default for DefaultVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Verifier for DefaultVerifier {
    fn verify(&self, source: &str, target: &str, total_bytes: u64) -> Result<VerifyResult> {
        sampler::verify_blockwise(source, target, total_bytes, self.block_size)
    }
}

/// Construct the default verifier.
pub fn default_verifier() -> DefaultVerifier {
    DefaultVerifier::new()
}
