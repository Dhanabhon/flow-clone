//! FlowClone desktop app — Tauri command layer.
//!
//! The desktop app is a thin presenter. It exposes a small set of Tauri
//! commands that delegate to `flowclone-core`. The UI never clones directly.

mod commands;

use flowclone_core::CloneEngine;
use std::sync::{Arc, Mutex};
use tauri::menu::{
    AboutMetadata, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::Emitter;
#[cfg(debug_assertions)]
use tauri::Manager;

/// Build the application menu.
///
/// Tauri's default menu labels every item with the crate name
/// (`flowclone-desktop`). This replaces it so the app menu reads "FlowClone",
/// adds a "Check For Update..." item (no behavior yet), and keeps the standard
/// Edit menu so text inputs still get Cut/Copy/Paste/Select All.
fn build_menu<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
) -> tauri::Result<tauri::menu::Menu<R>> {
    let about_metadata = AboutMetadata {
        name: Some("FlowClone".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        // Embed the app icon so the About panel shows the FlowClone logo instead
        // of the generic icon shown when running unbundled (e.g. `pnpm dev`).
        icon: Some(tauri::include_image!("icons/icon.png")),
        ..Default::default()
    };

    // `SubmenuBuilder::about` auto-labels the item "About {crate name}", so set
    // the label explicitly to force "About FlowClone".
    let about = PredefinedMenuItem::about(handle, Some("About FlowClone"), Some(about_metadata))?;

    // No handler is wired yet — the item is intentionally inert for now.
    let check_for_update =
        MenuItemBuilder::with_id("check_for_update", "Check For Update...").build(handle)?;

    let app_menu = SubmenuBuilder::new(handle, "FlowClone")
        .item(&about)
        .separator()
        .item(&check_for_update)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let edit_menu = SubmenuBuilder::new(handle, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let window_menu = SubmenuBuilder::new(handle, "Window")
        .minimize()
        .separator()
        .close_window()
        .build()?;

    MenuBuilder::new(handle)
        .items(&[&app_menu, &edit_menu, &window_menu])
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let engine = CloneEngine::new();
    let image_cancel: commands::ImageCancelState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .menu(build_menu)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .manage(Arc::new(engine))
        .manage(image_cancel)
        .invoke_handler(tauri::generate_handler![
            commands::list_disks,
            commands::validate_clone_plan,
            commands::start_clone_stub,
            commands::create_image_stub,
            commands::validate_image_stub,
            commands::restore_image_stub,
            commands::generate_report_stub,
            commands::cancel_clone,
            commands::open_full_disk_access_settings,
            commands::pending_image_job,
            commands::discard_pending_image,
            commands::dismiss_pending_image,
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
            // Refresh the disk list event-driven instead of polling: emit a
            // `disks://changed` event whenever storage attaches or detaches.
            let watch_handle = app.handle().clone();
            flowclone_disk::platform_disk_watcher().start(Box::new(move || {
                let _ = watch_handle.emit("disks://changed", ());
            }));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running FlowClone");
}
