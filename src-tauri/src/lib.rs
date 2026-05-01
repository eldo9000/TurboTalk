// TurboTalk — personal voice dictation utility
//
// Module map (see ARCHITECTURE.md):
//   audio       mic capture (cpal)
//   recorder    3-state machine: Ready / Recording / Transcribing
//   transcribe  whisper.cpp sidecar wrapper
//   paste       active-app text injection
//   hotkey      global push-to-talk binding (rdev — Right Alt / AltGr)
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

use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_theme, get_accent])
        .setup(|app| {
            let recorder = Arc::new(recorder::Recorder::new());
            hotkey::spawn(recorder, app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running TurboTalk");
}
