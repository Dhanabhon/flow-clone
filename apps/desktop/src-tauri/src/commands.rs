//! Tauri commands — the only bridge between React UI and Rust core.
//!
//! Commands here are deliberately thin: they marshal types and delegate to the
//! [`CloneEngine`]. No business logic lives here.

use flowclone_core::{CloneEngine, CloneJob, CloneOptions, Phase, Progress};
use flowclone_disk::{DiskCatalogApi, DiskInfo};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
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
/// Seconds the partial image can stop growing before we treat it as an
/// interruption (disconnect / power loss) in the UI.
const ELEVATED_STALL_SECS: f64 = 5.0;

/// Shared engine state, injected via `app.manage`.
pub type FlowCloneState = Arc<CloneEngine>;
pub type ImageCancelState = Arc<Mutex<Option<ImageCancelJob>>>;

#[derive(Clone)]
pub struct ImageCancelJob {
    job_id: String,
    cancel: Arc<AtomicBool>,
    // Set for elevated jobs. The root CLI copy can't be stopped via the
    // in-process flag, so cancellation drops a sentinel file it polls instead.
    image_path: Option<String>,
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
///
/// Reading a whole disk means opening `/dev/rdiskN`, which only root/operator
/// can do. When the GUI can open it directly (running elevated) the copy runs
/// in-process with live progress and cancellation. Otherwise it falls back to
/// the trusted `flowclone` CLI behind a native macOS admin prompt, polling the
/// growing `.part` file for progress.
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

    // Record the in-flight job so an unexpected exit (power loss, crash) can be
    // detected and surfaced on the next launch. Terminal events clear it.
    write_pending_marker(&image_path, &source);

    let image_cancel = image_cancel.inner().clone();
    match File::open(&raw_source_path) {
        Ok(_) => spawn_inprocess_image_copy(app, image_cancel, source, raw_source_path, image_path),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            spawn_elevated_image_copy(app, image_cancel, source, image_path)
        }
        Err(error) => Err(format!(
            "failed to open source disk {raw_source_path}: {error}"
        )),
    }
}

/// In-process raw copy. Used when the process can already read the raw device.
fn spawn_inprocess_image_copy(
    app: AppHandle,
    image_cancel: ImageCancelState,
    source: DiskInfo,
    raw_source_path: String,
    image_path: String,
) -> Result<String, String> {
    let job_id = stub_id("image");
    let cancel = Arc::new(AtomicBool::new(false));
    set_image_cancel(&image_cancel, job_id.clone(), cancel.clone(), None)?;
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
        clear_pending_marker();
    });
    Ok(job_id)
}

/// Elevated raw copy via the `flowclone` CLI behind a macOS admin prompt.
///
/// The privileged copy runs as a separate root process, so progress is observed
/// by polling the CLI's `.part` file rather than via in-process callbacks. The
/// background root copy cannot be interrupted mid-write in Phase 1; cancellation
/// only detaches the UI poller (the privileged helper will own real cancellation
/// later).
fn spawn_elevated_image_copy(
    app: AppHandle,
    image_cancel: ImageCancelState,
    source: DiskInfo,
    image_path: String,
) -> Result<String, String> {
    let cli = resolve_cli_binary()?;
    let job_id = stub_id("image");
    let cancel = Arc::new(AtomicBool::new(false));
    set_image_cancel(
        &image_cancel,
        job_id.clone(),
        cancel.clone(),
        Some(image_path.clone()),
    )?;

    let total = source.total_bytes;
    let header_bytes = image_header_overhead(&source)?;
    let partial_path = partial_image_path(&image_path);
    let device_path = source.device_path.clone();
    let cleanup_job_id = job_id.clone();
    let cleanup_cancel = image_cancel.clone();

    // Progress poller: watch the partial file grow while the root copy runs.
    let poll_app = app.clone();
    let poll_job_id = job_id.clone();
    let poll_cancel = cancel.clone();
    let poll_partial = partial_path.clone();
    let poll_image_path = image_path.clone();
    tauri::async_runtime::spawn(async move {
        let start = Instant::now();
        let mut last_bytes = 0u64;
        let mut last_tick = Instant::now();
        let mut speed = 0u64;
        let mut last_growth_bytes = 0u64;
        let mut last_growth_at = Instant::now();
        loop {
            if poll_cancel.load(Ordering::SeqCst) {
                break;
            }
            let elapsed = start.elapsed().as_secs_f64();
            let progress = match std::fs::metadata(&poll_partial) {
                Ok(metadata) => {
                    let bytes_done = metadata.len().saturating_sub(header_bytes).min(total);
                    if bytes_done > last_growth_bytes {
                        last_growth_bytes = bytes_done;
                        last_growth_at = Instant::now();
                    }
                    // Speed/ETA from how fast the partial file grows between ticks.
                    let interval = last_tick.elapsed().as_secs_f64();
                    if interval >= 0.8 {
                        speed = (bytes_done.saturating_sub(last_bytes) as f64 / interval) as u64;
                        last_bytes = bytes_done;
                        last_tick = Instant::now();
                    }
                    let fraction = if total == 0 {
                        0.0
                    } else {
                        (bytes_done as f64 / total as f64).clamp(0.0, 1.0)
                    };
                    // The file stopped growing: the CLI is recovering from a
                    // disconnect (cable/power). Surface that instead of a frozen bar.
                    let stalled = bytes_done > 0
                        && bytes_done < total
                        && last_growth_at.elapsed().as_secs_f64() >= ELEVATED_STALL_SECS;
                    let eta_secs = if !stalled && speed > 0 && bytes_done < total {
                        Some(((total - bytes_done) as f64 / speed as f64).max(0.0))
                    } else {
                        None
                    };
                    Progress {
                        job_id: poll_job_id.clone(),
                        phase: Phase::Cloning,
                        fraction,
                        bytes_done,
                        bytes_total: total,
                        read_speed: if stalled { 0 } else { speed },
                        write_speed: if stalled { 0 } else { speed },
                        elapsed_secs: elapsed,
                        eta_secs,
                        current_operation: if stalled {
                            "Interrupted; reconnecting to disk".to_string()
                        } else {
                            format!(
                                "Creating image block {} to {}",
                                bytes_done / IMAGE_BLOCK_SIZE as u64,
                                poll_image_path
                            )
                        },
                    }
                }
                // No partial file yet: the CLI is still waiting for the admin
                // password prompt (or unmounting). Emit a heartbeat so the screen
                // shows elapsed time and tells the user to approve the prompt,
                // rather than appearing frozen at "Preparing image".
                Err(_) => Progress {
                    job_id: poll_job_id.clone(),
                    phase: Phase::Cloning,
                    fraction: 0.0,
                    bytes_done: 0,
                    bytes_total: total,
                    read_speed: 0,
                    write_speed: 0,
                    elapsed_secs: elapsed,
                    eta_secs: None,
                    current_operation: "Waiting for administrator authorization".to_string(),
                },
            };
            let _ = poll_app.emit("clone://progress", progress);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    // Runner: launch the elevated CLI and report the terminal state.
    let run_app = app.clone();
    let run_job_id = job_id.clone();
    let run_cancel = cancel.clone();
    let run_image_path = image_path.clone();
    tauri::async_runtime::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            run_elevated(
                &cli,
                &[
                    "create-image",
                    "--source",
                    &device_path,
                    "--output",
                    &run_image_path,
                ],
                "elevated image creation failed",
            )
        })
        .await;
        run_cancel.store(true, Ordering::SeqCst);

        match result {
            Ok(Ok(())) => {
                let _ = run_app.emit(
                    "clone://progress",
                    Progress {
                        job_id: run_job_id,
                        phase: Phase::Completed,
                        fraction: 1.0,
                        bytes_done: total,
                        bytes_total: total,
                        read_speed: 0,
                        write_speed: 0,
                        elapsed_secs: 0.0,
                        eta_secs: Some(0.0),
                        current_operation: format!("Image workflow ready at {image_path}"),
                    },
                );
            }
            Ok(Err(error)) => {
                let _ = run_app.emit(
                    "clone://progress",
                    failed_progress(run_job_id, total, error),
                );
            }
            Err(error) => {
                let _ = run_app.emit(
                    "clone://progress",
                    failed_progress(run_job_id, total, error.to_string()),
                );
            }
        }
        clear_image_cancel(&cleanup_cancel, &cleanup_job_id);
        clear_pending_marker();
    });

    Ok(job_id)
}

/// Bytes the `.flowimg` header occupies before the raw payload begins.
fn image_header_overhead(source: &DiskInfo) -> Result<u64, String> {
    let header_len = flow_image_header(source)?.len() as u64;
    Ok(FLOW_IMAGE_MAGIC.len() as u64 + IMAGE_HEADER_LEN_BYTES as u64 + header_len)
}

/// Locate the `flowclone` CLI binary that performs the privileged raw read.
fn resolve_cli_binary() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("FLOWCLONE_CLI") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }
    // The CLI sits next to the app executable: a Tauri sidecar in a bundled app
    // (Contents/MacOS/flowclone), or `target/<profile>/flowclone` in dev.
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        let exact = dir.join("flowclone");
        if exact.exists() {
            return Ok(exact);
        }
        // The sidecar may keep its target-triple suffix (e.g.
        // flowclone-aarch64-apple-darwin); match that without picking the app
        // binary (flowclone-desktop).
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if is_cli_binary_name(&entry.file_name().to_string_lossy()) {
                    return Ok(entry.path());
                }
            }
        }
    }
    Err("FlowClone CLI not found. Build it with `cargo build -p flowclone-cli` (dev) or `scripts/build-sidecar.sh` (release), or set FLOWCLONE_CLI.".into())
}

/// Whether a filename is the `flowclone` CLI or a triple-suffixed sidecar of it,
/// excluding the app binary (`flowclone-desktop`).
fn is_cli_binary_name(name: &str) -> bool {
    let name = name.strip_suffix(".exe").unwrap_or(name);
    name == "flowclone"
        || (name.starts_with("flowclone-")
            && (name.contains("-apple-darwin")
                || name.contains("-pc-windows-")
                || name.contains("-unknown-linux-")))
}

/// Run the bundled CLI as root via a native macOS admin prompt.
#[cfg(target_os = "macos")]
fn run_elevated(cli: &Path, args: &[&str], fail_prefix: &str) -> Result<(), String> {
    let mut shell_command = posix_quote(&cli.to_string_lossy());
    for arg in args {
        shell_command.push(' ');
        shell_command.push_str(&posix_quote(arg));
    }
    let apple_script = format!(
        "do shell script {} with administrator privileges",
        applescript_quote(&shell_command)
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&apple_script)
        .output()
        .map_err(|error| format!("failed to launch admin prompt: {error}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("-128") || stderr.contains("User canceled") {
        return Err("Admin authorization was cancelled.".into());
    }
    Err(format!("{fail_prefix}: {}", stderr.trim()))
}

/// Single-quote a value for safe use as one `/bin/sh` argument.
#[cfg(target_os = "macos")]
fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Escape a string for use as an AppleScript double-quoted string literal.
#[cfg(target_os = "macos")]
fn applescript_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Run the bundled CLI elevated via a Windows UAC prompt.
///
/// PowerShell's `Start-Process -Verb RunAs` raises the consent dialog, runs the
/// CLI as administrator, and (with `-Wait`) blocks until it finishes — the same
/// shape as the macOS `osascript` path. An elevated `RunAs` process can't have
/// its stdio redirected, so progress/cancel still flow through the CLI's files;
/// here we only need its success/failure, taken from the exit code.
#[cfg(target_os = "windows")]
fn run_elevated(cli: &Path, args: &[&str], fail_prefix: &str) -> Result<(), String> {
    let file = ps_single_quote(&cli.to_string_lossy());
    let arg_list = if args.is_empty() {
        "@()".to_string()
    } else {
        let joined = args
            .iter()
            .map(|arg| ps_runas_arg(arg))
            .collect::<Vec<_>>()
            .join(",");
        format!("@({joined})")
    };
    // `exit 1223` == ERROR_CANCELLED: Start-Process throws when the user declines
    // the UAC prompt. Otherwise propagate the CLI's own exit code.
    // `-WindowStyle Hidden` keeps the elevated CLI's freshly-created console from
    // flashing up a terminal window (it writes progress to files, not a console).
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         try {{ $p = Start-Process -FilePath {file} -ArgumentList {arg_list} -Verb RunAs -WindowStyle Hidden -Wait -PassThru; \
         if ($null -eq $p.ExitCode) {{ exit 0 }} else {{ exit $p.ExitCode }} }} \
         catch {{ exit 1223 }}"
    );

    // `CREATE_NO_WINDOW` stops this PowerShell itself from popping a console
    // window when spawned by the GUI (which has no console of its own).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("failed to launch elevation prompt: {error}"))?;

    match output.status.code() {
        Some(0) => Ok(()),
        Some(1223) => Err("Admin authorization was cancelled.".into()),
        Some(code) => Err(format!(
            "{fail_prefix}: the elevated helper exited with code {code}"
        )),
        None => Err(format!("{fail_prefix}: the elevated helper was terminated")),
    }
}

/// Single-quote a value as one PowerShell string literal.
#[cfg(target_os = "windows")]
fn ps_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Encode one CLI argument for `Start-Process -ArgumentList`: PowerShell joins
/// the array with spaces and adds no quoting, so wrap the token in double quotes
/// (surviving spaces in paths) and then express that as a PS string literal.
#[cfg(target_os = "windows")]
fn ps_runas_arg(value: &str) -> String {
    let quoted_token = format!("\"{}\"", value.replace('"', "\\\""));
    format!("'{}'", quoted_token.replace('\'', "''"))
}

/// Desktop builds only ship for macOS and Windows; other targets have no
/// elevation path.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run_elevated(_cli: &Path, _args: &[&str], _fail_prefix: &str) -> Result<(), String> {
    Err("elevated disk access is not supported on this platform".into())
}

fn set_image_cancel(
    state: &ImageCancelState,
    job_id: String,
    cancel: Arc<AtomicBool>,
    image_path: Option<String>,
) -> Result<(), String> {
    let mut active = state
        .lock()
        .map_err(|_| "image cancel state is unavailable".to_string())?;
    *active = Some(ImageCancelJob {
        job_id,
        cancel,
        image_path,
    });
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
    let partial_path = partial_image_path(image_path);
    let mut reader = File::open(source_path)
        .map_err(|error| format!("failed to open source disk {source_path}: {error}"))?;
    let mut image = File::create(&partial_path)
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
            let _ = std::fs::remove_file(&partial_path);
            return Err("cancelled".into());
        }
        let remaining = (source.total_bytes - bytes_done).min(IMAGE_BLOCK_SIZE as u64) as usize;
        let read = reader.read(&mut buf[..remaining]).map_err(|error| {
            format!("failed to read source disk at offset {bytes_done} bytes: {error}")
        })?;
        if read == 0 {
            return Err(format!(
                "source disk ended early: copied {bytes_done} of {} bytes",
                source.total_bytes
            ));
        }

        image.write_all(&buf[..read]).map_err(|error| {
            format!("failed to write image file at source offset {bytes_done} bytes: {error}")
        })?;
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
    drop(image);
    std::fs::rename(&partial_path, image_path)
        .map_err(|error| format!("failed to finalize image file: {error}"))?;
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

fn partial_image_path(image_path: &str) -> String {
    format!("{image_path}.part")
}

/// Sentinel file the elevated CLI polls to know it should abort.
fn cancel_sentinel_path(image_path: &str) -> String {
    format!("{image_path}.cancel")
}

/// Marker recording an in-flight image job, kept in the user's home so it
/// survives a crash or power loss (unlike the temp dir, which can be cleared).
fn pending_marker_path() -> Option<PathBuf> {
    // `HOME` on Unix; `USERPROFILE` is the Windows equivalent.
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(
        PathBuf::from(home)
            .join(".flowclone")
            .join("pending-image.json"),
    )
}

fn write_pending_marker(image_path: &str, source: &DiskInfo) {
    let Some(path) = pending_marker_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let marker = serde_json::json!({
        "image_path": image_path,
        "source_model": source.model,
        "total_bytes": source.total_bytes,
    });
    let _ = std::fs::write(&path, marker.to_string());
}

fn clear_pending_marker() {
    if let Some(path) = pending_marker_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// An image job that was running when the app last exited unexpectedly.
#[derive(Debug, Clone, Serialize)]
pub struct PendingImage {
    pub image_path: String,
    pub source_model: String,
    pub bytes_done: u64,
    pub total_bytes: u64,
}

/// Report an interrupted image job (power loss / crash), if one is left behind.
#[tauri::command]
pub async fn pending_image_job() -> Result<Option<PendingImage>, String> {
    let Some(path) = pending_marker_path() else {
        return Ok(None);
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
        clear_pending_marker();
        return Ok(None);
    };
    let image_path = value
        .get("image_path")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if image_path.is_empty() {
        clear_pending_marker();
        return Ok(None);
    }

    // If the final image exists, the job actually completed — nothing pending.
    if std::path::Path::new(&image_path).exists() {
        clear_pending_marker();
        return Ok(None);
    }
    // Without a partial file there is nothing to recover or clean up.
    let Ok(partial) = std::fs::metadata(partial_image_path(&image_path)) else {
        clear_pending_marker();
        return Ok(None);
    };

    Ok(Some(PendingImage {
        image_path,
        source_model: value
            .get("source_model")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        bytes_done: partial.len(),
        total_bytes: value
            .get("total_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    }))
}

/// Delete the partial file from an interrupted job and clear the marker.
#[tauri::command]
pub async fn discard_pending_image() -> Result<(), String> {
    if let Some(path) = pending_marker_path() {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(image_path) = value.get("image_path").and_then(|v| v.as_str()) {
                    let _ = std::fs::remove_file(partial_image_path(image_path));
                    let _ = std::fs::remove_file(cancel_sentinel_path(image_path));
                }
            }
        }
    }
    clear_pending_marker();
    Ok(())
}

/// Keep the partial file but stop reminding the user about the interrupted job.
#[tauri::command]
pub async fn dismiss_pending_image() -> Result<(), String> {
    clear_pending_marker();
    Ok(())
}

fn raw_device_path(path: &str) -> String {
    if let Some(suffix) = path.strip_prefix("/dev/disk") {
        format!("/dev/rdisk{suffix}")
    } else {
        path.to_string()
    }
}

/// Validate a FlowClone migration image workflow file.
#[tauri::command]
pub async fn validate_image_stub(image_path: String) -> Result<ImageValidation, String> {
    validate_image_file(&image_path).await
}

/// Restore a `.flowimg` onto a target disk via the elevated CLI. **Destructive.**
#[tauri::command]
pub async fn restore_image_stub(
    app: AppHandle,
    image_cancel: State<'_, ImageCancelState>,
    image_path: String,
    target_path: String,
) -> Result<String, String> {
    let image_path = image_path.trim().to_string();
    let validation = validate_image_file(&image_path).await?;
    if validation.payload_bytes == 0 {
        return Err("this image has no raw payload to restore".into());
    }
    let target = find_disk(&target_path)?;
    // Defense in depth — the CLI re-validates, but reject obvious mistakes here.
    if target.is_boot {
        return Err(format!(
            "refusing to restore onto the boot disk {}",
            target.device_path
        ));
    }
    if target.read_only {
        return Err(format!("target {} is read-only", target.device_path));
    }
    if matches!(target.connection, flowclone_disk::Connection::Internal) {
        return Err(format!(
            "refusing to restore onto internal disk {} (external targets only)",
            target.device_path
        ));
    }
    if target.total_bytes < validation.payload_bytes {
        return Err(format!(
            "target {} is too small for this image",
            target.device_path
        ));
    }

    let image_cancel = image_cancel.inner().clone();
    spawn_elevated_restore(
        app,
        image_cancel,
        image_path,
        target.device_path,
        validation.payload_bytes,
    )
}

/// Path the CLI writes restore progress to; mirrors `flowclone-cli`.
fn restore_progress_path(image_path: &str) -> String {
    format!("{image_path}.restore-progress")
}

/// Read "<bytes_done> <total>" the elevated restore publishes.
fn read_restore_progress(path: &str) -> Option<(u64, u64)> {
    let contents = std::fs::read_to_string(path).ok()?;
    let mut parts = contents.split_whitespace();
    let done = parts.next()?.parse::<u64>().ok()?;
    let total = parts.next()?.parse::<u64>().ok()?;
    Some((done, total))
}

/// Run the elevated CLI restore, polling its progress file for the UI.
fn spawn_elevated_restore(
    app: AppHandle,
    image_cancel: ImageCancelState,
    image_path: String,
    target_device: String,
    total: u64,
) -> Result<String, String> {
    let cli = resolve_cli_binary()?;
    let job_id = stub_id("restore");
    let cancel = Arc::new(AtomicBool::new(false));
    set_image_cancel(
        &image_cancel,
        job_id.clone(),
        cancel.clone(),
        Some(image_path.clone()),
    )?;

    let progress_path = restore_progress_path(&image_path);
    let cleanup_job_id = job_id.clone();
    let cleanup_cancel = image_cancel.clone();

    // Poller: read the CLI's progress file (no growing target file to stat).
    let poll_app = app.clone();
    let poll_job_id = job_id.clone();
    let poll_cancel = cancel.clone();
    let poll_progress_path = progress_path.clone();
    tauri::async_runtime::spawn(async move {
        let start = Instant::now();
        let mut last_bytes = 0u64;
        let mut last_tick = Instant::now();
        let mut speed = 0u64;
        let mut last_growth_bytes = 0u64;
        let mut last_growth_at = Instant::now();
        loop {
            if poll_cancel.load(Ordering::SeqCst) {
                break;
            }
            let elapsed = start.elapsed().as_secs_f64();
            let progress = match read_restore_progress(&poll_progress_path) {
                Some((bytes_done, file_total)) => {
                    let total = if file_total > 0 { file_total } else { total };
                    if bytes_done > last_growth_bytes {
                        last_growth_bytes = bytes_done;
                        last_growth_at = Instant::now();
                    }
                    let interval = last_tick.elapsed().as_secs_f64();
                    if interval >= 0.8 {
                        speed = (bytes_done.saturating_sub(last_bytes) as f64 / interval) as u64;
                        last_bytes = bytes_done;
                        last_tick = Instant::now();
                    }
                    let fraction = if total == 0 {
                        0.0
                    } else {
                        (bytes_done as f64 / total as f64).clamp(0.0, 1.0)
                    };
                    let stalled = bytes_done > 0
                        && bytes_done < total
                        && last_growth_at.elapsed().as_secs_f64() >= ELEVATED_STALL_SECS;
                    let eta_secs = if !stalled && speed > 0 && bytes_done < total {
                        Some(((total - bytes_done) as f64 / speed as f64).max(0.0))
                    } else {
                        None
                    };
                    Progress {
                        job_id: poll_job_id.clone(),
                        phase: Phase::Cloning,
                        fraction,
                        bytes_done,
                        bytes_total: total,
                        read_speed: if stalled { 0 } else { speed },
                        write_speed: if stalled { 0 } else { speed },
                        elapsed_secs: elapsed,
                        eta_secs,
                        current_operation: if stalled {
                            "Interrupted; reconnecting to disk".to_string()
                        } else {
                            "Restoring to disk".to_string()
                        },
                    }
                }
                None => Progress {
                    job_id: poll_job_id.clone(),
                    phase: Phase::Cloning,
                    fraction: 0.0,
                    bytes_done: 0,
                    bytes_total: total,
                    read_speed: 0,
                    write_speed: 0,
                    elapsed_secs: elapsed,
                    eta_secs: None,
                    current_operation: "Waiting for administrator authorization".to_string(),
                },
            };
            let _ = poll_app.emit("clone://progress", progress);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    // Runner: launch the elevated CLI restore and report the terminal state.
    let run_app = app.clone();
    let run_job_id = job_id.clone();
    let run_cancel = cancel.clone();
    let run_image_path = image_path.clone();
    tauri::async_runtime::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            run_elevated(
                &cli,
                &[
                    "restore-image",
                    "--image",
                    &run_image_path,
                    "--target",
                    &target_device,
                    "--confirm-erase",
                ],
                "elevated restore failed",
            )
        })
        .await;
        run_cancel.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&progress_path);

        match result {
            Ok(Ok(())) => {
                let _ = run_app.emit(
                    "clone://progress",
                    Progress {
                        job_id: run_job_id,
                        phase: Phase::Completed,
                        fraction: 1.0,
                        bytes_done: total,
                        bytes_total: total,
                        read_speed: 0,
                        write_speed: 0,
                        elapsed_secs: 0.0,
                        eta_secs: Some(0.0),
                        current_operation: "Restore workflow ready".to_string(),
                    },
                );
            }
            Ok(Err(error)) => {
                let _ = run_app.emit(
                    "clone://progress",
                    failed_progress(run_job_id, total, error),
                );
            }
            Err(error) => {
                let _ = run_app.emit(
                    "clone://progress",
                    failed_progress(run_job_id, total, error.to_string()),
                );
            }
        }
        clear_image_cancel(&cleanup_cancel, &cleanup_job_id);
    });

    Ok(job_id)
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
        // Stops the in-process copy and the progress poller.
        job.cancel.store(true, Ordering::SeqCst);
        // The elevated copy runs as a separate root process that ignores the
        // flag above, so drop a sentinel file it polls and aborts on.
        if let Some(image_path) = &job.image_path {
            let _ = std::fs::write(cancel_sentinel_path(image_path), b"cancel");
        }
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

/// Safely eject / power down a disk so it can be unplugged. Not destructive and
/// does not require elevation; the UI only offers it for external disks.
#[tauri::command]
pub async fn eject_disk(device_path: String) -> Result<(), String> {
    eject_device(&device_path)
}

#[cfg(target_os = "macos")]
fn eject_device(device_path: &str) -> Result<(), String> {
    let output = Command::new("diskutil")
        .args(["eject", device_path])
        .output()
        .map_err(|error| format!("failed to run diskutil eject: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(target_os = "windows")]
fn eject_device(device_path: &str) -> Result<(), String> {
    // device_path looks like `\\.\PhysicalDriveN` / `PhysicalDriveN`; eject its
    // volumes via the Shell "Eject" verb (the user-level Safely Remove path).
    let number: String = device_path
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if number.is_empty() {
        return Err(format!("could not parse disk number from {device_path}"));
    }
    let script = format!(
        "$ls=(Get-Partition -DiskNumber {number} -ErrorAction SilentlyContinue).DriveLetter | ? {{$_}}; \
         $sh=New-Object -comObject Shell.Application; \
         foreach($l in $ls){{$sh.Namespace(17).ParseName(\"$l`:\").InvokeVerb('Eject')}}"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|error| format!("failed to run eject: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn eject_device(_device_path: &str) -> Result<(), String> {
    Err("eject is not supported on this platform yet".into())
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
    fn create_flow_image_file_only_finalizes_complete_images() {
        let payload = b"short source";
        let mut source_path = std::env::temp_dir();
        source_path.push(format!("{}.source", stub_id("flowclone-test-short")));
        let mut image_path = std::env::temp_dir();
        image_path.push(format!("{}.flowimg", stub_id("flowclone-test-short")));
        std::fs::write(&source_path, payload).expect("write source file");

        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = payload.len() as u64 + 1;
        let source_path = source_path.to_string_lossy().to_string();
        let image_path = image_path.to_string_lossy().to_string();
        let partial_path = partial_image_path(&image_path);
        let cancel = AtomicBool::new(false);

        let error = create_flow_image_file(&source_path, &image_path, &source, &cancel, |_| {})
            .expect_err("incomplete source should fail");

        assert!(error.contains("source disk ended early"));
        assert!(!std::path::Path::new(&image_path).exists());
        assert!(std::path::Path::new(&partial_path).exists());

        std::fs::remove_file(source_path).expect("remove source file");
        std::fs::remove_file(partial_path).expect("remove partial image file");
    }

    #[test]
    fn is_cli_binary_name_matches_cli_and_sidecars_not_the_app() {
        assert!(is_cli_binary_name("flowclone"));
        assert!(is_cli_binary_name("flowclone.exe"));
        assert!(is_cli_binary_name("flowclone-aarch64-apple-darwin"));
        assert!(is_cli_binary_name("flowclone-universal-apple-darwin"));
        assert!(is_cli_binary_name("flowclone-x86_64-pc-windows-msvc.exe"));
        // The app binary and unrelated names must not match.
        assert!(!is_cli_binary_name("flowclone-desktop"));
        assert!(!is_cli_binary_name("FlowClone"));
        assert!(!is_cli_binary_name("flowclonex"));
    }

    #[test]
    fn raw_device_path_prefers_rdisk_on_macos_device_names() {
        assert_eq!(raw_device_path("/dev/disk6"), "/dev/rdisk6");
        assert_eq!(raw_device_path("/tmp/source.img"), "/tmp/source.img");
    }

    #[test]
    fn posix_quote_neutralizes_single_quotes_and_spaces() {
        assert_eq!(posix_quote("/dev/disk6"), "'/dev/disk6'");
        assert_eq!(
            posix_quote("/Volumes/My Disk/img.flowimg"),
            "'/Volumes/My Disk/img.flowimg'"
        );
        // A path that tries to break out of the quotes is neutralized.
        assert_eq!(posix_quote("a'; rm -rf /; '"), "'a'\\''; rm -rf /; '\\'''");
    }

    #[test]
    fn applescript_quote_escapes_backslash_then_quotes() {
        assert_eq!(applescript_quote("plain"), "\"plain\"");
        // Backslashes are doubled and double-quotes escaped, in that order.
        assert_eq!(applescript_quote("a\\b\"c"), "\"a\\\\b\\\"c\"");
    }

    #[test]
    fn flow_image_file_len_includes_payload_and_header() {
        let mut source = DiskInfo::placeholder("/dev/disk-test");
        source.total_bytes = 123;

        let len = flow_image_file_len(&source).expect("image length");

        assert!(len > source.total_bytes);
    }
}
