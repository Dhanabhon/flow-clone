//! Strategies for comparing verification samples.
//!
//! Currently the only strategy is a direct block-by-block hash compare, but the
//! enum is here so we can add statistical / sampled modes later without
//! changing call sites.

/// How verification should compare source and target.
#[derive(Debug, Clone, Copy, Default)]
pub enum CompareStrategy {
    /// Hash every block end-to-end (MVP default).
    #[default]
    Full,
    /// Hash a deterministic random sample of blocks for a fast spot check.
    Sampled { fraction: f64 },
}
