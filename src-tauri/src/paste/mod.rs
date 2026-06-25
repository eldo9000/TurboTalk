// Text injection for the frontmost application.
//
// Platform strategy:
//   macOS: Three-tier — direct AX injection (native text fields),
//          clipboard + Cmd+V (webviews/Electron), clipboard-only fallback.
//   Windows: Clipboard snapshot + window activation + Ctrl+V via enigo.
//   Linux: Legacy arboard + enigo (unchanged).
//
// This module supersedes the old monolithic paste.rs (now legacy.rs).

mod legacy;

// macOS modules
#[cfg(target_os = "macos")]
pub mod ax_inject;
#[cfg(target_os = "macos")]
pub mod clipboard;
#[cfg(target_os = "macos")]
pub mod focus_capture;
#[cfg(target_os = "macos")]
pub mod keyboard_layout;
#[cfg(target_os = "macos")]
pub mod synthetic_keys;

// Windows modules
#[cfg(target_os = "windows")]
pub mod win_clipboard;
#[cfg(target_os = "windows")]
pub mod win_focus;
#[cfg(target_os = "windows")]
pub mod win_paste;

// Re-export helpers still used by hotkey.rs and other callers.
pub use legacy::frontmost_app;

// ── Paste entry point ───────────────────────────────────────────────────────

/// Dispatch a closure to the main thread and return its result.
/// Uses an mpsc channel to sync the result back to the calling thread.
/// Only valid on macOS where AppKit requires main-thread dispatch.
#[cfg(target_os = "macos")]
fn dispatch_native<F, T>(app: &tauri::AppHandle, f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    app
        .run_on_main_thread(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| anyhow::anyhow!("run_on_main_thread failed: {e:?}"))?;
    rx.recv()
        .map_err(|e| anyhow::anyhow!("main thread result recv failed: {e}"))?
}

#[cfg(target_os = "macos")]
pub fn paste(text: &str, app: &tauri::AppHandle) -> anyhow::Result<bool> {
    let ax_trusted = accessibility_trusted();

    // ── Tier 1: Direct AX text injection ──────────────────────────────────
    //
    // For native text fields (NSTextView, NSTextField, etc.): set the focused
    // element's AXSelectedText or AXValue attribute directly. No clipboard
    // involvement, no keystrokes. Requires AXIsProcessTrusted().
    //
    // Falls through silently if the element doesn't support AX text
    // manipulation (most web views, Electron apps, code editors).

    if ax_trusted {
        match ax_inject::try_inject(text) {
            Ok(Some(())) => {
                tracing::info!(
                    "[paste] Tier 1 (AX injection) succeeded — {} chars",
                    text.len()
                );
                return Ok(true);
            }
            Ok(None) => {
                tracing::debug!("[paste] Tier 1 (AX injection) not supported by element, falling through");
            }
            Err(e) => {
                tracing::warn!("[paste] Tier 1 (AX injection) error: {e}, falling through");
            }
        }
    } else {
        tracing::debug!("[paste] AX not trusted, skipping Tier 1");
    }

    // ── Tier 2: Clipboard + Cmd+V ────────────────────────────────────────
    //
    // Standard pasteboard path: save clipboard, write transcript, activate the
    // target app, send Cmd+V, then restore clipboard with changeCount guard.
    //
    // Clipboard operations use native NSPasteboard on the main thread via
    // dispatch_native(), falling back to pbcopy/pbpaste on failure.

    let focus = focus_capture::snapshot();
    if focus.pid > 0 {
        tracing::debug!(
            "[paste] activating app pid={} ({} {})",
            focus.pid,
            focus.bundle_id.as_deref().unwrap_or("?"),
            focus.app_name.as_deref().unwrap_or("?"),
        );
        focus_capture::activate_app(&focus);
    }

    // Try native NSPasteboard module with main-thread dispatch.
    // If native snapshot fails, fall back to pbcopy for writing and skip restore.
    let native_snapshot: Option<clipboard::native::PasteboardSnapshot> =
        match dispatch_native(app, clipboard::native::snapshot) {
            Ok(s) => {
                // Native snapshot succeeded — also write via native.
                let text_owned = text.to_string();
                if let Err(e) = dispatch_native(app, move || {
                    clipboard::native::write_text(&text_owned)
                }) {
                    tracing::warn!("[paste] native write failed (falling back to pbcopy): {e}");
                    let _ = clipboard::write_text(text);
                }
                Some(s)
            }
            Err(e) => {
                tracing::warn!(
                    "[paste] native snapshot failed (falling back to pbcopy): {e}"
                );
                if let Err(e) = clipboard::write_text(text) {
                    tracing::warn!("[paste] pbcopy write failed: {e}");
                }
                None
            }
        };

    synthetic_keys::post_cmd_v();

    if ax_trusted {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Some(snapshot) = native_snapshot {
            match dispatch_native(app, move || {
                clipboard::native::restore_if_untouched(&snapshot)
            }) {
                Ok(true) => {
                    tracing::info!("[paste] Tier 2 — clipboard restored (changeCount guard)")
                }
                Ok(false) => {
                    tracing::warn!(
                        "[paste] Tier 2 — clipboard changed, restore skipped"
                    )
                }
                Err(e) => {
                    tracing::warn!("[paste] Tier 2 — native restore error: {e}")
                }
            }
        } else {
            tracing::warn!("[paste] Tier 2 — no native snapshot, clipboard left as-is");
        }
        Ok(true)
    } else {
        tracing::info!(
            "[paste] Tier 2 — no AX trust, text left on clipboard ({} bytes)",
            text.len()
        );
        Ok(false)
    }
}

#[cfg(target_os = "macos")]
fn accessibility_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

#[cfg(target_os = "windows")]
pub fn paste(text: &str) -> anyhow::Result<bool> {
    win_paste::paste(text)
}

#[cfg(target_os = "linux")]
pub fn paste(text: &str) -> anyhow::Result<bool> {
    legacy::paste(text)
}
