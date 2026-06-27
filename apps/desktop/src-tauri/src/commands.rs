//! Tauri commands — the only bridge between React UI and Rust core.
//!
//! Commands here are deliberately thin: they marshal types and delegate to the
//! [`CloneEngine`]. No business logic lives here.

use flowclone_core::{CloneEngine, CloneJob, CloneOptions, Phase, Progress};
use flowclone_disk::{DiskCatalogApi, DiskInfo};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Shared engine state, injected via `app.manage`.
pub type FlowCloneState = Arc<CloneEngine>;

/// List mock disks for the Phase 1 desktop UI.
#[tauri::command]
pub async fn list_disks(_state: State<'_, FlowCloneState>) -> Result<Vec<DiskInfo>, String> {
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    catalog.list().map_err(|e| e.to_string())
}

/// Validate a direct clone plan without starting it.
#[tauri::command]
pub async fn validate_clone_plan(
    state: State<'_, FlowCloneState>,
    source_path: String,
    target_path: String,
    verify: bool,
) -> Result<(), String> {
    let options = CloneOptions {
        verify,
        ..Default::default()
    };
    let request = state
        .resolve_request(&source_path, &target_path, &options)
        .map_err(|e| e.to_string())?;
    CloneJob::new(request).map_err(|e| e.to_string())?;
    Ok(())
}

/// Start a mocked clone. Returns the job id; progress arrives via events.
#[tauri::command]
pub async fn start_clone_stub(
    app: AppHandle,
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
    let job = CloneJob::new(request).map_err(|e| e.to_string())?;
    let job_id = job.id.clone();

    let engine = state.inner().clone();
    let mut progress = engine.progress();
    let progress_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok(progress) = progress.recv().await {
            let done = matches!(progress.phase, Phase::Completed | Phase::Failed);
            let _ = progress_app.emit("clone://progress", &progress);
            if done {
                break;
            }
        }
    });

    let run_app = app.clone();
    let run_job_id = job_id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = engine.run(job, options).await {
            let _ = run_app.emit(
                "clone://progress",
                Progress {
                    job_id: run_job_id,
                    phase: Phase::Failed,
                    fraction: 0.0,
                    bytes_done: 0,
                    bytes_total: 0,
                    read_speed: 0,
                    write_speed: 0,
                    elapsed_secs: 0.0,
                    eta_secs: None,
                    current_operation: error.to_string(),
                },
            );
        }
    });

    Ok(job_id)
}

/// Create a mocked migration image. No file is written in Phase 1.
#[tauri::command]
pub async fn create_image_stub(
    app: AppHandle,
    source_path: String,
    image_path: String,
) -> Result<String, String> {
    if image_path.trim().is_empty() {
        return Err("image path is required".into());
    }
    ensure_disk_exists(&source_path)?;

    let job_id = stub_id("image");
    let _ = app.emit(
        "clone://progress",
        Progress {
            job_id: job_id.clone(),
            phase: Phase::Completed,
            fraction: 1.0,
            bytes_done: 0,
            bytes_total: 0,
            read_speed: 0,
            write_speed: 0,
            elapsed_secs: 0.1,
            eta_secs: Some(0.0),
            current_operation: format!("Mock image ready at {image_path}"),
        },
    );
    Ok(job_id)
}

/// Restore a mocked migration image. No disk is written in Phase 1.
#[tauri::command]
pub async fn restore_image_stub(image_path: String, target_path: String) -> Result<String, String> {
    if image_path.trim().is_empty() {
        return Err("image path is required".into());
    }
    ensure_disk_exists(&target_path)?;
    Ok(stub_id("restore"))
}

/// Generate a report preview for the completed mock workflow.
#[tauri::command]
pub async fn generate_report_stub(
    source_path: String,
    target_path: Option<String>,
    image_path: Option<String>,
) -> Result<String, String> {
    Ok(format!(
        "# FlowClone report\n\n- Source: {source_path}\n- Target: {}\n- Image: {}\n- Mode: mocked Phase 1 workflow\n- Result: completed\n",
        target_path.as_deref().unwrap_or("none"),
        image_path.as_deref().unwrap_or("none")
    ))
}

/// Request cancellation of a running job.
#[tauri::command]
pub async fn cancel_clone(_state: State<'_, FlowCloneState>) -> Result<(), String> {
    // The current MVP runs one job at a time; cancellation is handled via the
    // job's cancel token which will be looked up by id in a follow-up.
    Ok(())
}

fn ensure_disk_exists(path: &str) -> Result<(), String> {
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    match catalog.find(path).map_err(|e| e.to_string())? {
        Some(_) => Ok(()),
        None => Err(format!("disk not found: {path}")),
    }
}

fn stub_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos:x}")
}
