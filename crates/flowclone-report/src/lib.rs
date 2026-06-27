//! FlowClone report generation.
//!
//! Produces either Markdown or JSON summaries of a completed clone job, ready
//! for the user to export from the success screen.

pub mod json;
pub mod markdown;

use flowclone_disk::DiskInfo;
use flowclone_verify::VerifyResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use json as json_format;
pub use markdown as markdown_format;

/// Output format for a report.
#[derive(Debug, Clone, Copy)]
pub enum ReportFormat {
    Markdown,
    Json,
}

/// Everything a report needs to render. Built by the core after a clone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportData {
    pub source: DiskInfo,
    pub target: DiskInfo,
    /// ISO-8601 timestamp of when the clone started, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub duration_secs: f64,
    pub average_speed: u64,
    pub verified: Option<VerifyResult>,
    pub warnings: Vec<String>,
    pub app_version: String,
}

/// Writes a [`ReportData`] to disk in the requested format.
pub struct ReportWriter;

impl ReportWriter {
    /// Render `data` to `dest` in `format`.
    pub fn write(format: ReportFormat, data: &ReportData, dest: &Path) -> anyhow::Result<()> {
        let content = match format {
            ReportFormat::Markdown => markdown::render(data),
            ReportFormat::Json => json::render(data)?,
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, content)?;
        Ok(())
    }
}
