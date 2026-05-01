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

pub use theme::{get_accent, get_theme};

#[tauri::command]
fn get_config() -> settings::Config {
    settings::load()
}

#[tauri::command]
fn save_config(cfg: settings::Config) -> Result<(), String> {
    settings::save(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
fn scan_models_dir() -> Vec<String> {
    settings::scan_models_dir()
}

#[tauri::command]
fn get_launch_at_login(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_audio_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devs) => devs.filter_map(|d| d.name().ok()).collect(),
        Err(_)   => vec![],
    }
}

use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![get_theme, get_accent, get_config, save_config, scan_models_dir, get_launch_at_login, set_launch_at_login, list_audio_devices])
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
            let cfg = settings::load();
            if let Err(e) = settings::save(&cfg) {
                tracing::warn!("[settings] could not write config: {:?}", e);
            }

            // ── Overlay — cursor-transparent so clicks always pass through ──
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_ignore_cursor_events(true);
            }

            // ── Hotkey ─────────────────────────────────────────────────────
            // Recorder::new() pre-warms the CoreAudio stream so first keypress
            // has zero hardware-init latency.
            let recorder = Arc::new(recorder::Recorder::new()?);
            hotkey::spawn(recorder, tray_icon, app.handle().clone());

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
