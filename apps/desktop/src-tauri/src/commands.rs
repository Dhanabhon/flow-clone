//! Tauri commands — the only bridge between React UI and Rust core.
//!
//! Commands here are deliberately thin: they marshal types and delegate to the
//! [`CloneEngine`]. No business logic lives here.

use flowclone_core::{CloneEngine, CloneOptions};
use flowclone_disk::{DiskCatalogApi, DiskInfo};
use std::sync::Arc;
use tauri::State;

/// Shared engine state, injected via `app.manage`.
pub type FlowCloneState = Arc<CloneEngine>;

/// List all disks the core can see.
#[tauri::command]
pub async fn list_disks(_state: State<'_, FlowCloneState>) -> Result<Vec<DiskInfo>, String> {
    // Re-read on every call so the UI always reflects the current topology.
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    catalog.list().map_err(|e| e.to_string())
}

/// Start a clone. Returns the job id; progress arrives via events.
#[tauri::command]
pub async fn start_clone(
    state: State<'_, FlowCloneState>,
    source_path: String,
    target_path: String,
    verify: bool,
) -> Result<String, String> {
    let options = CloneOptions {
        verify,
        ..Default::default()
    };
    let request = state
        .resolve_request(&source_path, &target_path, &options)
        .map_err(|e| e.to_string())?;
    let job = flowclone_core::CloneJob::new(request).map_err(|e| e.to_string())?;
    let job_id = job.id.clone();

    let engine = state.inner().clone();
    // Drive the clone to completion in the background; the UI listens to events.
    tauri::async_runtime::spawn(async move {
        let _ = engine.run(job, options).await;
    });

    Ok(job_id)
}

/// Request cancellation of a running job.
#[tauri::command]
pub async fn cancel_clone(_state: State<'_, FlowCloneState>) -> Result<(), String> {
    // The current MVP runs one job at a time; cancellation is handled via the
    // job's cancel token which will be looked up by id in a follow-up.
    Ok(())
}
