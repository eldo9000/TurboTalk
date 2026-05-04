//! Shared window control commands for all LibreWin apps.
//!
//! Register these in each app's `tauri::generate_handler![]` using the full path:
//! ```rust,ignore
//! tauri::generate_handler![
//!     librewin_common::window::close_window,
//!     librewin_common::window::minimize_window,
//!     librewin_common::window::toggle_maximize,
//! ]
//! ```

/// Close the focused window.
#[tauri::command]
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}

/// Minimize the focused window.
#[tauri::command]
pub fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

/// Toggle the focused window between maximized and restored.
#[tauri::command]
pub fn toggle_maximize(window: tauri::Window) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}
