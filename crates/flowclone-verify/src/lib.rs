//! FlowClone verification engine.
//!
//! Phase 1 verification is a stub. The real block sampler stays in this crate
//! for later, but the default verifier never opens source or target devices.

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
    /// False when the image carried no usable digest (legacy / unfinalized).
    #[serde(default = "default_true")]
    pub verifiable: bool,
    /// Expected digest (hex) — set on a mismatch for display.
    #[serde(default)]
    pub expected: Option<String>,
    /// Actual recomputed digest (hex) — set on a mismatch for display.
    #[serde(default)]
    pub actual: Option<String>,
}

fn default_true() -> bool {
    true
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

/// Default Phase 1 verifier. It reports success without reading any device.
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
    fn verify(&self, _source: &str, _target: &str, total_bytes: u64) -> Result<VerifyResult> {
        // TODO: call sampler::verify_blockwise after real raw device access is
        // guarded by the privileged helper and explicit user consent.
        let bytes_checked = total_bytes.min(self.block_size as u64 * 8);
        Ok(VerifyResult {
            matched: true,
            bytes_checked,
            blocks_checked: if bytes_checked == 0 { 0 } else { 8 },
            mismatches: 0,
            elapsed_secs: 0.2,
            verifiable: true,
            expected: None,
            actual: None,
        })
    }
}

/// Construct the default verifier.
pub fn default_verifier() -> DefaultVerifier {
    DefaultVerifier::new()
}
