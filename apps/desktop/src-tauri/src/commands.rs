//! Tauri commands — the only bridge between React UI and Rust core.
//!
//! Commands here are deliberately thin: they marshal types and delegate to the
//! [`CloneEngine`]. No business logic lives here.

use flowclone_core::{CloneEngine, CloneJob, CloneOptions, Phase, Progress};
use flowclone_disk::{DiskCatalogApi, DiskInfo};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

const FLOW_IMAGE_FORMAT: &str = "flowclone-image";
const FLOW_IMAGE_MAGIC: &[u8] = b"FLOWCLONE_FLOWIMG_V1\n";
const FLOW_IMAGE_VERSION: u64 = 1;
const IMAGE_HEADER_LEN_BYTES: usize = 8;
const IMAGE_BLOCK_SIZE: usize = 4 * 1024 * 1024;
const MAX_IMAGE_HEADER_BYTES: u64 = 1024 * 1024;
const STUB_IMAGE_FORMAT: &str = "flowclone-stub-image";
const STUB_IMAGE_VERSION: u64 = 1;

/// Shared engine state, injected via `app.manage`.
pub type FlowCloneState = Arc<CloneEngine>;
pub type ImageCancelState = Arc<Mutex<Option<ImageCancelJob>>>;

#[derive(Clone)]
pub struct ImageCancelJob {
    job_id: String,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageValidation {
    pub format: String,
    pub version: u64,
    pub source: DiskInfo,
    pub payload_bytes: u64,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FlowImageHeader {
    format: String,
    version: u64,
    source: DiskInfo,
    payload_bytes: u64,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StubImageManifest {
    format: String,
    version: u64,
    source: DiskInfo,
    note: Option<String>,
}

/// List disks for the desktop UI.
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

/// Create a migration image file by copying raw source bytes into `.flowimg`.
#[tauri::command]
pub async fn create_image_stub(
    app: AppHandle,
    image_cancel: State<'_, ImageCancelState>,
    source_path: String,
    image_path: String,
) -> Result<String, String> {
    let image_path = image_path.trim().to_string();
    if image_path.is_empty() {
        return Err("image path is required".into());
    }
    let source = find_disk(&source_path)?;
    let raw_source_path = raw_device_path(&source.device_path);
    let required_image_bytes = flow_image_file_len(&source)?;
    flowclone_raw::ensure_free_space_for_output(&image_path, required_image_bytes)
        .map_err(|error| error.to_string())?;
    ensure_source_readable(&raw_source_path)?;

    let job_id = stub_id("image");
    let cancel = Arc::new(AtomicBool::new(false));
    let image_cancel = image_cancel.inner().clone();
    set_image_cancel(&image_cancel, job_id.clone(), cancel.clone())?;
    let total = source.total_bytes;
    let progress_app = app.clone();
    let progress_job_id = job_id.clone();
    let cleanup_job_id = job_id.clone();
    let cleanup_cancel = image_cancel.clone();
    tauri::async_runtime::spawn(async move {
        let image_path_for_copy = image_path.clone();
        let image_path_for_progress = image_path.clone();
        let source_for_copy = source.clone();
        let copy_app = progress_app.clone();
        let copy_job_id = progress_job_id.clone();
        let copy_cancel = cancel.clone();
        let result = tokio::task::spawn_blocking(move || {
            create_flow_image_file(
                &raw_source_path,
                &image_path_for_copy,
                &source_for_copy,
                &copy_cancel,
                |progress| {
                    let _ = copy_app.emit(
                        "clone://progress",
                        Progress {
                            job_id: copy_job_id.clone(),
                            phase: Phase::Cloning,
                            fraction: progress.fraction(),
                            bytes_done: progress.bytes_done,
                            bytes_total: progress.bytes_total,
                            read_speed: progress.bytes_per_sec,
                            write_speed: progress.bytes_per_sec,
                            elapsed_secs: progress.elapsed_secs,
                            eta_secs: progress.eta_secs(),
                            current_operation: format!(
                                "Creating image block {} to {}",
                                progress.block_index, image_path_for_progress
                            ),
                        },
                    );
                },
            )
        })
        .await;

        match result {
            Ok(Ok(stats)) => {
                let _ = progress_app.emit(
                    "clone://progress",
                    Progress {
                        job_id: progress_job_id,
                        phase: Phase::Completed,
                        fraction: 1.0,
                        bytes_done: stats.bytes_done,
                        bytes_total: total,
                        read_speed: stats.average_speed,
                        write_speed: stats.average_speed,
                        elapsed_secs: stats.elapsed_secs,
                        eta_secs: Some(0.0),
                        current_operation: format!("Image workflow ready at {image_path}"),
                    },
                );
            }
            Ok(Err(error)) => {
                let _ = progress_app.emit(
                    "clone://progress",
                    failed_progress(progress_job_id, total, error),
                );
            }
            Err(error) => {
                let _ = progress_app.emit(
                    "clone://progress",
                    failed_progress(progress_job_id, total, error.to_string()),
                );
            }
        }
        clear_image_cancel(&cleanup_cancel, &cleanup_job_id);
    });
    Ok(job_id)
}

fn set_image_cancel(
    state: &ImageCancelState,
    job_id: String,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut active = state
        .lock()
        .map_err(|_| "image cancel state is unavailable".to_string())?;
    *active = Some(ImageCancelJob { job_id, cancel });
    Ok(())
}

fn clear_image_cancel(state: &ImageCancelState, job_id: &str) {
    if let Ok(mut active) = state.lock() {
        if active.as_ref().is_some_and(|job| job.job_id == job_id) {
            *active = None;
        }
    }
}

fn failed_progress(job_id: String, total: u64, error: String) -> Progress {
    Progress {
        job_id,
        phase: Phase::Failed,
        fraction: 0.0,
        bytes_done: 0,
        bytes_total: total,
        read_speed: 0,
        write_speed: 0,
        elapsed_secs: 0.0,
        eta_secs: None,
        current_operation: error,
    }
}

#[derive(Debug, Clone, Copy)]
struct ImageCopyProgress {
    bytes_done: u64,
    bytes_total: u64,
    bytes_per_sec: u64,
    block_index: u64,
    elapsed_secs: f64,
}

impl ImageCopyProgress {
    fn fraction(&self) -> f64 {
        if self.bytes_total == 0 {
            0.0
        } else {
            (self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0)
        }
    }

    fn eta_secs(&self) -> Option<f64> {
        if self.bytes_done == 0 || self.bytes_done >= self.bytes_total || self.bytes_per_sec == 0 {
            None
        } else {
            Some(((self.bytes_total - self.bytes_done) as f64 / self.bytes_per_sec as f64).max(0.0))
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ImageCopyStats {
    bytes_done: u64,
    elapsed_secs: f64,
    average_speed: u64,
}

fn create_flow_image_file(
    source_path: &str,
    image_path: &str,
    source: &DiskInfo,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(ImageCopyProgress),
) -> Result<ImageCopyStats, String> {
    let mut reader = File::open(source_path)
        .map_err(|error| format!("failed to open source disk {source_path}: {error}"))?;
    let mut image = File::create(image_path)
        .map_err(|error| format!("failed to create image file: {error}"))?;
    write_flow_image_header(&mut image, source)?;

    let start = Instant::now();
    let mut buf = vec![0u8; IMAGE_BLOCK_SIZE];
    let mut bytes_done = 0u64;
    let mut block_index = 0u64;
    let mut last_emit = Instant::now();
    let mut last_emit_bytes = 0u64;

    while bytes_done < source.total_bytes {
        if cancel.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(image_path);
            return Err("cancelled".into());
        }
        let remaining = (source.total_bytes - bytes_done).min(IMAGE_BLOCK_SIZE as u64) as usize;
        let read = reader
            .read(&mut buf[..remaining])
            .map_err(|error| format!("failed to read source disk: {error}"))?;
        if read == 0 {
            return Err(format!(
                "source disk ended early: copied {bytes_done} of {} bytes",
                source.total_bytes
            ));
        }

        image
            .write_all(&buf[..read])
            .map_err(|error| format!("failed to write image file: {error}"))?;
        bytes_done += read as u64;
        block_index += 1;

        if last_emit.elapsed().as_millis() >= 100 || bytes_done == source.total_bytes {
            let elapsed = start.elapsed().as_secs_f64();
            let interval = last_emit.elapsed().as_secs_f64();
            let bytes_per_sec = if interval > 0.0 {
                ((bytes_done - last_emit_bytes) as f64 / interval) as u64
            } else {
                0
            };
            on_progress(ImageCopyProgress {
                bytes_done,
                bytes_total: source.total_bytes,
                bytes_per_sec,
                block_index,
                elapsed_secs: elapsed,
            });
            last_emit = Instant::now();
            last_emit_bytes = bytes_done;
        }
    }

    image
        .sync_all()
        .map_err(|error| format!("failed to flush image file: {error}"))?;
    let elapsed_secs = start.elapsed().as_secs_f64();
    let average_speed = if elapsed_secs > 0.0 {
        (bytes_done as f64 / elapsed_secs) as u64
    } else {
        0
    };

    Ok(ImageCopyStats {
        bytes_done,
        elapsed_secs,
        average_speed,
    })
}

fn write_flow_image_header(writer: &mut impl Write, source: &DiskInfo) -> Result<(), String> {
    let header = flow_image_header(source)?;
    let header_len = header.len() as u64;
    writer
        .write_all(FLOW_IMAGE_MAGIC)
        .map_err(|error| format!("failed to write image magic: {error}"))?;
    writer
        .write_all(&header_len.to_le_bytes())
        .map_err(|error| format!("failed to write image header length: {error}"))?;
    writer
        .write_all(header.as_bytes())
        .map_err(|error| format!("failed to write image header: {error}"))
}

fn flow_image_header(source: &DiskInfo) -> Result<String, String> {
    serde_json::to_string(&FlowImageHeader {
        format: FLOW_IMAGE_FORMAT.into(),
        version: FLOW_IMAGE_VERSION,
        source: source.clone(),
        payload_bytes: source.total_bytes,
        note: Some("Raw disk payload follows this header.".into()),
    })
    .map_err(|error| format!("failed to serialize image header: {error}"))
}

fn flow_image_file_len(source: &DiskInfo) -> Result<u64, String> {
    let header_len = flow_image_header(source)?.len() as u64;
    Ok(FLOW_IMAGE_MAGIC.len() as u64
        + IMAGE_HEADER_LEN_BYTES as u64
        + header_len
        + source.total_bytes)
}

fn raw_device_path(path: &str) -> String {
    if let Some(suffix) = path.strip_prefix("/dev/disk") {
        format!("/dev/rdisk{suffix}")
    } else {
        path.to_string()
    }
}

fn ensure_source_readable(path: &str) -> Result<(), String> {
    File::open(path)
        .map(|_| ())
        .map_err(|error| match error.kind() {
            ErrorKind::PermissionDenied => {
                format!("macOS denied access to {path}. Raw disk reads need elevated disk access.")
            }
            _ => format!("failed to open source disk {path}: {error}"),
        })
}

/// Validate a FlowClone migration image workflow file.
#[tauri::command]
pub async fn validate_image_stub(image_path: String) -> Result<ImageValidation, String> {
    validate_image_file(&image_path).await
}

/// Restore a mocked migration image. No disk is written in Phase 1.
#[tauri::command]
pub async fn restore_image_stub(image_path: String, target_path: String) -> Result<String, String> {
    validate_image_file(&image_path).await?;
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
    if let Some(image_path) = image_path.as_deref() {
        return Ok(format!(
            "# FlowClone image migration report\n\n- Source: {source_path}\n- Image: {image_path}\n- Mode: Image Migration preview\n- Result: completed\n- Restore: ready for a future target SSD\n"
        ));
    }

    Ok(format!(
        "# FlowClone direct clone report\n\n- Source: {source_path}\n- Target: {}\n- Mode: Direct Clone preview\n- Result: completed\n",
        target_path.as_deref().unwrap_or("none"),
    ))
}

/// Request cancellation of a running job.
#[tauri::command]
pub async fn cancel_clone(
    _state: State<'_, FlowCloneState>,
    image_cancel: State<'_, ImageCancelState>,
) -> Result<(), String> {
    if let Some(job) = image_cancel
        .lock()
        .map_err(|_| "image cancel state is unavailable".to_string())?
        .as_ref()
    {
        job.cancel.store(true, Ordering::SeqCst);
    }
    // The current MVP runs one job at a time; cancellation is handled via the
    // job's cancel token which will be looked up by id in a follow-up.
    Ok(())
}

/// Open macOS Privacy & Security > Full Disk Access.
#[tauri::command]
pub async fn open_full_disk_access_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
            .status()
            .map_err(|error| format!("failed to open System Settings: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("System Settings exited with status {status}"))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Full Disk Access settings are only available on macOS.".into())
    }
}

fn ensure_disk_exists(path: &str) -> Result<(), String> {
    find_disk(path).map(|_| ())
}

fn find_disk(path: &str) -> Result<DiskInfo, String> {
    let catalog = flowclone_disk::DiskCatalog::platform_default();
    match catalog.find(path).map_err(|e| e.to_string())? {
        Some(disk) => Ok(disk),
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

async fn validate_image_file(image_path: &str) -> Result<ImageValidation, String> {
    let image_path = image_path.trim();
    if image_path.is_empty() {
        return Err("image path is required".into());
    }
    let image_path = image_path.to_string();
    tokio::task::spawn_blocking(move || validate_image_path(&image_path))
        .await
        .map_err(|error| error.to_string())?
}

fn validate_image_path(image_path: &str) -> Result<ImageValidation, String> {
    let mut file =
        File::open(image_path).map_err(|error| format!("failed to read image file: {error}"))?;
    let image_len = file
        .metadata()
        .map_err(|error| format!("failed to inspect image file: {error}"))?
        .len();
    let mut magic = vec![0u8; FLOW_IMAGE_MAGIC.len()];
    let read = file
        .read(&mut magic)
        .map_err(|error| format!("failed to read image file: {error}"))?;

    if read == FLOW_IMAGE_MAGIC.len() && magic == FLOW_IMAGE_MAGIC {
        return validate_flow_image_file(file, image_len);
    }

    let contents = std::fs::read_to_string(image_path)
        .map_err(|error| format!("failed to read image file: {error}"))?;
    validate_stub_image_manifest(&contents)
}

fn validate_flow_image_file(mut file: File, image_len: u64) -> Result<ImageValidation, String> {
    let mut len_bytes = [0u8; IMAGE_HEADER_LEN_BYTES];
    file.read_exact(&mut len_bytes)
        .map_err(|error| format!("failed to read image header length: {error}"))?;
    let header_len = u64::from_le_bytes(len_bytes);
    if header_len == 0 || header_len > MAX_IMAGE_HEADER_BYTES {
        return Err(format!("invalid image header length: {header_len}"));
    }

    let mut header = vec![0u8; header_len as usize];
    file.read_exact(&mut header)
        .map_err(|error| format!("failed to read image header: {error}"))?;
    let header: FlowImageHeader = serde_json::from_slice(&header)
        .map_err(|error| format!("invalid image header: {error}"))?;

    if header.format != FLOW_IMAGE_FORMAT {
        return Err(format!("unsupported image format: {}", header.format));
    }
    if header.version != FLOW_IMAGE_VERSION {
        return Err(format!("unsupported image version: {}", header.version));
    }
    if header.source.device_path.trim().is_empty() {
        return Err("image source device path is missing".into());
    }
    if header.source.total_bytes == 0 || header.payload_bytes == 0 {
        return Err("image payload capacity is missing".into());
    }
    if header.payload_bytes != header.source.total_bytes {
        return Err("image payload size does not match source capacity".into());
    }

    let expected_len = FLOW_IMAGE_MAGIC.len() as u64
        + IMAGE_HEADER_LEN_BYTES as u64
        + header_len
        + header.payload_bytes;
    if image_len != expected_len {
        return Err(format!(
            "image file size mismatch: expected {expected_len} bytes, found {image_len}"
        ));
    }

    Ok(ImageValidation {
        format: header.format,
        version: header.version,
        source: header.source,
        payload_bytes: header.payload_bytes,
        note: header.note,
    })
}

fn validate_stub_image_manifest(contents: &str) -> Result<ImageValidation, String> {
    let manifest: StubImageManifest =
        serde_json::from_str(contents).map_err(|error| format!("invalid image file: {error}"))?;
    if manifest.format != STUB_IMAGE_FORMAT {
        return Err(format!("unsupported image format: {}", manifest.format));
    }
    if manifest.version != STUB_IMAGE_VERSION {
        return Err(format!("unsupported image version: {}", manifest.version));
    }
    if manifest.source.device_path.trim().is_empty() {
        return Err("image source device path is missing".into());
    }
    if manifest.source.total_bytes == 0 {
        return Err("image source capacity is missing".into());
    }
    Ok(ImageValidation {
        format: manifest.format,
        version: manifest.version,
        source: manifest.source,
        payload_bytes: 0,
        note: manifest.note,
    })
}

#[cfg(test)]
fn stub_image_manifest(source: &DiskInfo) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "format": STUB_IMAGE_FORMAT,
        "version": STUB_IMAGE_VERSION,
        "source": source,
        "note": "Preview file only. No disk data has been copied."
    }))
    .map_err(|error| format!("failed to serialize image manifest: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_image_manifest_contains_source_metadata() {
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.model = "External SSD".into();
        source.total_bytes = 512_000_000_000;

        let manifest = stub_image_manifest(&source).expect("stub manifest");

        assert!(manifest.contains("\"format\": \"flowclone-stub-image\""));
        assert!(manifest.contains("\"device_path\": \"/dev/disk-test\""));
        assert!(manifest.contains("\"model\": \"External SSD\""));
        assert!(manifest.contains("\"total_bytes\": 512000000000"));
    }

    #[test]
    fn validates_stub_image_manifest() {
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = 512_000_000_000;
        let manifest = stub_image_manifest(&source).expect("stub manifest");

        let validation = validate_stub_image_manifest(&manifest).expect("valid stub image");

        assert_eq!(validation.format, STUB_IMAGE_FORMAT);
        assert_eq!(validation.version, STUB_IMAGE_VERSION);
        assert_eq!(validation.source.device_path, "/dev/disk-test");
    }

    #[test]
    fn rejects_wrong_image_format() {
        let contents = r#"{
            "format": "not-flowclone",
            "version": 1,
            "source": {
                "device_path": "/dev/disk-test",
                "bsd_name": "disk-test",
                "model": "External SSD",
                "vendor": null,
                "serial": null,
                "total_bytes": 512000000000,
                "used_bytes": null,
                "connection": "usb",
                "filesystem": null,
                "read_only": false,
                "encrypted": false,
                "health": "unknown",
                "is_boot": false,
                "volume_name": null
            }
        }"#;

        let error = validate_stub_image_manifest(contents).expect_err("invalid format");

        assert!(error.contains("unsupported image format"));
    }

    #[test]
    fn create_flow_image_file_writes_payload_and_valid_header() {
        let payload = b"flowclone image payload";
        let mut source_path = std::env::temp_dir();
        source_path.push(format!("{}.source", stub_id("flowclone-test-write")));
        let mut image_path = std::env::temp_dir();
        image_path.push(format!("{}.flowimg", stub_id("flowclone-test-write")));
        std::fs::write(&source_path, payload).expect("write source file");

        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.model = "External SSD".into();
        source.total_bytes = payload.len() as u64;

        let source_path = source_path.to_string_lossy().to_string();
        let image_path = image_path.to_string_lossy().to_string();
        let mut progress = Vec::new();

        let cancel = AtomicBool::new(false);
        let stats = create_flow_image_file(&source_path, &image_path, &source, &cancel, |p| {
            progress.push(p);
        })
        .expect("create flow image");
        let validation = validate_image_path(&image_path).expect("valid image file");
        let image_bytes = std::fs::read(&image_path).expect("read image file");

        assert_eq!(stats.bytes_done, payload.len() as u64);
        assert_eq!(validation.format, FLOW_IMAGE_FORMAT);
        assert_eq!(validation.payload_bytes, payload.len() as u64);
        assert!(image_bytes.ends_with(payload));
        assert!(!progress.is_empty());

        std::fs::remove_file(source_path).expect("remove source file");
        std::fs::remove_file(image_path).expect("remove stub image file");
    }

    #[test]
    fn create_flow_image_file_cleans_up_on_cancel() {
        let payload = b"flowclone image payload";
        let mut source_path = std::env::temp_dir();
        source_path.push(format!("{}.source", stub_id("flowclone-test-cancel")));
        let mut image_path = std::env::temp_dir();
        image_path.push(format!("{}.flowimg", stub_id("flowclone-test-cancel")));
        std::fs::write(&source_path, payload).expect("write source file");

        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = payload.len() as u64;
        let cancel = AtomicBool::new(true);
        let source_path = source_path.to_string_lossy().to_string();
        let image_path = image_path.to_string_lossy().to_string();

        let error = create_flow_image_file(&source_path, &image_path, &source, &cancel, |_| {})
            .expect_err("cancelled image creation");

        assert_eq!(error, "cancelled");
        assert!(!std::path::Path::new(&image_path).exists());

        std::fs::remove_file(source_path).expect("remove source file");
    }

    #[test]
    fn raw_device_path_prefers_rdisk_on_macos_device_names() {
        assert_eq!(raw_device_path("/dev/disk6"), "/dev/rdisk6");
        assert_eq!(raw_device_path("/tmp/source.img"), "/tmp/source.img");
    }

    #[test]
    fn flow_image_file_len_includes_payload_and_header() {
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = 123;

        let len = flow_image_file_len(&source).expect("image length");

        assert!(len > source.total_bytes);
    }
}
