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
fn save_config(
    cfg: settings::Config,
    hotkey_state: tauri::State<'_, HotkeyState>,
) -> Result<(), String> {
    settings::save(&cfg).map_err(|e| e.to_string())?;
    *hotkey_state.write() = cfg.hotkey.clone();
    Ok(())
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
fn paste_history_item(text: String, app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    // Give the previously-focused app time to regain focus before the keystroke.
    std::thread::sleep(std::time::Duration::from_millis(150));
    crate::paste::paste(&text).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_history() -> Vec<settings::HistoryEntry> {
    settings::load_history()
}

#[tauri::command]
fn save_history(entries: Vec<settings::HistoryEntry>) -> Result<(), String> {
    settings::save_history(&entries).map_err(|e| e.to_string())
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
use parking_lot::RwLock;

// Shared hotkey config — hotkey thread reads this on every event so
// settings changes take effect without restarting the app.
type HotkeyState = Arc<RwLock<settings::HotkeyConfig>>;

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
        .invoke_handler(tauri::generate_handler![get_theme, get_accent, get_config, save_config, scan_models_dir, get_launch_at_login, set_launch_at_login, list_audio_devices, load_history, save_history, paste_history_item])
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
            let level_rec = recorder.clone();
            let level_app = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
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
