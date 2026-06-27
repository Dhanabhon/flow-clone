//! FlowClone desktop app — Tauri command layer.
//!
//! The desktop app is a thin presenter. It exposes a small set of Tauri
//! commands that delegate to `flowclone-core`. The UI never clones directly.

mod commands;

use flowclone_core::CloneEngine;
use std::sync::Arc;
#[cfg(debug_assertions)]
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let engine = CloneEngine::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .manage(Arc::new(engine))
        .invoke_handler(tauri::generate_handler![
            commands::list_disks,
            commands::validate_clone_plan,
            commands::start_clone_stub,
            commands::create_image_stub,
            commands::restore_image_stub,
            commands::generate_report_stub,
            commands::cancel_clone,
        ])
        .setup(|app| {
            // DevTools are disabled in release builds by default: the `devtools`
            // Cargo feature is intentionally NOT enabled on the `tauri`
            // dependency, so the inspector is stripped from production binaries.
            // In debug builds it is still available, but we don't auto-open it —
            // set FLOWCLONE_OPEN_DEVTOOLS=1 to open it on launch for debugging.
            #[cfg(debug_assertions)]
            {
                if std::env::var("FLOWCLONE_OPEN_DEVTOOLS").as_deref() == Ok("1") {
                    if let Some(window) = app.get_webview_window("main") {
                        window.open_devtools();
                    }
                }
            }
            // Touch `app` in release builds so the closure param isn't unused.
            #[cfg(not(debug_assertions))]
            let _ = app;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running FlowClone");
}
