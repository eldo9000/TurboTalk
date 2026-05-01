// TurboTalk — personal voice dictation utility
//
// Module map (see ARCHITECTURE.md):
//   audio       mic capture (cpal)
//   recorder    3-state machine: Ready / Recording / Transcribing
//   transcribe  whisper.cpp sidecar wrapper
//   paste       active-app text injection
//   hotkey      global push-to-talk binding
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

pub use theme::{get_accent, get_theme};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![get_theme, get_accent])
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running TurboTalk");
}
