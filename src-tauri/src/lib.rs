// TurboTalk — personal voice dictation utility
//
// Module map (see ARCHITECTURE.md):
//   audio       mic capture (cpal)
//   recorder    3-state machine: Ready / Recording / Transcribing
//   transcribe  whisper.cpp sidecar wrapper
//   paste       active-app text injection
//   hotkey      global push-to-talk binding (CGEventTap — Right Alt)
//   cleanup     LLM postprocessor (Chaperone Layer)
//   settings    config persistence

pub mod audio;
pub mod cleanup;
pub mod hotkey;
pub mod paste;
pub mod recorder;
pub mod settings;
pub mod theme;
pub mod transcribe;
pub mod tray;
pub mod vad;

pub use theme::{get_accent, get_theme};

/// Unified error channel for the frontend. Any backend error path that the
/// user should see (failed save, malformed config, dropped history entry, etc.)
/// goes through here so the frontend has a single listener and uniform UX.
///
/// Payload shape is intentionally simple:
///   { kind: String, message: String, recoverable: bool }
pub fn emit_ui_error(
    app: &tauri::AppHandle,
    kind: &str,
    message: impl Into<String>,
    recoverable: bool,
) {
    use tauri::Emitter;
    let payload = serde_json::json!({
        "kind": kind,
        "message": message.into(),
        "recoverable": recoverable,
    });
    if let Err(e) = app.emit("ui-error", payload) {
        tracing::warn!("[ui-error] failed to emit {}: {}", kind, e);
    }
}

#[tauri::command]
#[specta::specta]
fn get_config() -> settings::Config {
    settings::load()
}

#[tauri::command]
#[specta::specta]
fn save_config(
    cfg: settings::Config,
    hotkey_state: tauri::State<'_, HotkeyState>,
) -> Result<(), String> {
    settings::save(&cfg).map_err(|e| e.to_string())?;
    *hotkey_state.write() = cfg.hotkey.clone();
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn scan_models_dir() -> Vec<String> {
    settings::scan_models_dir()
}

#[tauri::command]
#[specta::specta]
fn get_launch_at_login(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn copy_history_item(text: String) -> Result<(), String> {
    use arboard::Clipboard;
    let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn load_history(app: tauri::AppHandle) -> Vec<settings::HistoryEntry> {
    let result = settings::load_history_detailed();
    if result.dropped > 0 {
        emit_ui_error(
            &app,
            "history-load-malformed",
            format!("{} history entr{} skipped (malformed)",
                result.dropped,
                if result.dropped == 1 { "y was" } else { "ies were" },
            ),
            true,
        );
    }
    let cfg = settings::load();
    let original_len = result.entries.len();
    let filtered = settings::filter_history_by_policy(result.entries, &cfg.history_auto_delete);
    if filtered.len() < original_len {
        let _ = settings::save_history(&filtered);
    }
    filtered
}

#[tauri::command]
#[specta::specta]
fn save_history(
    entries: Vec<settings::HistoryEntry>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if let Err(e) = settings::save_history(&entries) {
        let msg = e.to_string();
        emit_ui_error(
            &app,
            "history-save",
            format!("Couldn't save history: {}", msg),
            true,
        );
        return Err(msg);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn download_model(
    url: String,
    name: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;

    // Build the destination path — create the directory if it doesn't exist yet.
    // (canonical_models_dir() requires the dir to already exist, so we build manually.)
    let mut dir = dirs::home_dir()
        .ok_or_else(|| "Could not locate home directory".to_string())?;
    dir.push(".config/librewin/turbotalk/models");
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.bin", name));

    let client = reqwest::Client::builder()
        .user_agent("TurboTalk/0.0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let mut resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let total = resp.content_length();
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(&dest).await.map_err(|e| e.to_string())?;

    let _ = app.emit("download-progress", serde_json::json!({ "name": &name, "pct": 0u8 }));

    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(e) = file.write_all(&chunk).await {
                    drop(file);
                    let _ = tokio::fs::remove_file(&dest).await;
                    return Err(e.to_string());
                }
                downloaded += chunk.len() as u64;
                let pct = total
                    .filter(|&t| t > 0)
                    .map(|t| ((downloaded * 100) / t).min(99) as u8)
                    .unwrap_or(0);
                let _ = app.emit("download-progress", serde_json::json!({ "name": &name, "pct": pct }));
            }
            Ok(None) => break,
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&dest).await;
                return Err(format!("Download interrupted: {}", e));
            }
        }
    }

    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);

    let canonical = dest.canonicalize().map_err(|e| e.to_string())?;
    Ok(canonical.to_string_lossy().into_owned())
}

#[tauri::command]
#[specta::specta]
fn list_audio_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devs) => devs.filter_map(|d| d.name().ok()).collect(),
        Err(_)   => vec![],
    }
}

use std::sync::Arc;
use parking_lot::RwLock;

// Shared hotkey config — hotkey thread reads this on every event so
// settings changes take effect without restarting the app.
type HotkeyState = Arc<RwLock<settings::HotkeyConfig>>;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};

/// Position `win` centered horizontally, just below the macOS menu bar.
fn center_top(win: &tauri::WebviewWindow) {
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let screen_w = monitor.size().width as f64 / scale;
    let win_w = win
        .outer_size()
        .map(|s| s.width as f64 / scale)
        .unwrap_or(440.0);
    let x = (screen_w - win_w) / 2.0;
    let _ = win.set_position(tauri::LogicalPosition::new(x, 28.0));
}

/// Build the tauri-specta type-export descriptor. Lives in its own function
/// so the `#[test]` regenerator below can call it without standing up the
/// full Tauri runtime.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
        get_config,
        save_config,
        scan_models_dir,
        get_launch_at_login,
        set_launch_at_login,
        list_audio_devices,
        download_model,
        load_history,
        save_history,
        copy_history_item,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    // ── Typed Rust↔TS contract ─────────────────────────────────────────────
    // Every command crossing the IPC boundary that the frontend talks to is
    // listed in `specta_builder()`. In debug builds, `.export()` writes
    // `src/bindings.ts` so adding/removing/renaming a field on `Config` (or
    // any sub-struct) shows up as a TypeScript compile error in the
    // frontend. `get_theme`/`get_accent` stay free-form because the
    // frontend reaches them through `@libre/ui`.
    let specta_builder = specta_builder();

    #[cfg(debug_assertions)]
    {
        use specta_typescript::Typescript;
        if let Err(e) = specta_builder.export(
            Typescript::default()
                .header("// AUTO-GENERATED by tauri-specta. Do not edit by hand.\n// Run `cargo test --manifest-path src-tauri/Cargo.toml export_bindings`\n// (or launch the app in dev) to regenerate.\n"),
            "../src/bindings.ts",
        ) {
            tracing::warn!("[specta] failed to export bindings.ts: {:?}", e);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_theme, get_accent,
            get_config, save_config, scan_models_dir,
            get_launch_at_login, set_launch_at_login, list_audio_devices,
            download_model,
            load_history, save_history, copy_history_item
        ])
        .setup(|app| {
            // ── Tray icon ──────────────────────────────────────────────────
            let show_item = MenuItem::with_id(app, "show", "Show TurboTalk", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let tray_icon: TrayIcon = TrayIconBuilder::new()
                .icon(tray::make_icon(tray::TrayState::Idle))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("TurboTalk")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            center_top(&win);
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                        let _ = app.emit("open-history", ());
                    }
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // ── Config (write defaults on first run) ───────────────────────
            let cfg_result = settings::load_detailed();
            let cfg = cfg_result.config;
            if let Some(err_msg) = cfg_result.parse_error {
                emit_ui_error(
                    &app.handle().clone(),
                    "config-parse",
                    err_msg,
                    true,
                );
            }
            if let Err(e) = settings::save(&cfg) {
                tracing::warn!("[settings] could not write config: {:?}", e);
            }

            // ── Shared hotkey state — updated live when settings are saved ──
            let hotkey_state: HotkeyState = Arc::new(RwLock::new(cfg.hotkey.clone()));
            app.manage(hotkey_state.clone());

            // ── Overlay — cursor-transparent so clicks always pass through ──
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_ignore_cursor_events(true);
            }

            // ── Hotkey ─────────────────────────────────────────────────────
            // Stream opens on first keypress; always re-queries the config device
            // so built-in mic / AirPods switches work without restarting.
            let recorder = Arc::new(recorder::Recorder::new()?);

            // Emit live audio level to the overlay at 20 Hz while recording.
            // Same thread also services the device-lost edge: if the cpal
            // error callback flagged it, we cancel the recorder, reset the
            // tray, and emit `device-lost` to the frontend so the overlay
            // clears and the main window can surface a banner.
            let level_rec = recorder.clone();
            let level_app = app.handle().clone();
            let level_tray = tray_icon.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if level_rec.device_lost() {
                    tracing::warn!("[lib] observed device-lost flag — cancelling recorder");
                    level_rec.cancel();
                    let _ = level_tray.set_icon(Some(tray::make_icon(tray::TrayState::Idle)));
                    let _ = level_app.emit("device-lost", ());
                    // `recording-discarded` keeps the overlay's existing
                    // catch-all listener happy without the frontend needing
                    // to learn a new clear-overlay path.
                    let _ = level_app.emit("recording-discarded", ());
                }
                if level_rec.is_recording() {
                    let _ = level_app.emit("audio-level", level_rec.level());
                }
            });

            hotkey::spawn(recorder, tray_icon, app.handle().clone(), hotkey_state);

            Ok(())
        })
        // Close button hides to tray instead of quitting
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running TurboTalk");
}

#[cfg(test)]
mod tests {
    use super::specta_builder;

    /// Regenerate `src/bindings.ts` on demand. Run with:
    ///   cargo test --manifest-path src-tauri/Cargo.toml export_bindings
    ///
    /// We also run it implicitly in dev builds via `run()`, but a unit test
    /// is the cleanest way to refresh the file from CI / the worker without
    /// launching a full Tauri window.
    #[test]
    fn export_bindings() {
        use specta_typescript::Typescript;
        specta_builder()
            .export(
                Typescript::default().header(
                    "// AUTO-GENERATED by tauri-specta. Do not edit by hand.\n\
                     // Run `cargo test --manifest-path src-tauri/Cargo.toml export_bindings`\n\
                     // (or launch the app in dev) to regenerate.\n",
                ),
                "../src/bindings.ts",
            )
            .expect("specta export failed");
    }
}
