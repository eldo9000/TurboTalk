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

#[cfg(target_os = "macos")]
pub fn paste(text: &str) -> anyhow::Result<bool> {
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

    crate::diagnostic_log::emergency_trace("[paste-tier2] focus_capture::snapshot");
    let focus = focus_capture::snapshot();
    if focus.pid > 0 {
        crate::diagnostic_log::emergency_trace("[paste-tier2] focus_capture::activate_app");
        tracing::debug!(
            "[paste] activating app pid={} ({} {})",
            focus.pid,
            focus.bundle_id.as_deref().unwrap_or("?"),
            focus.app_name.as_deref().unwrap_or("?"),
        );
        focus_capture::activate_app(&focus);
    }

    crate::diagnostic_log::emergency_trace("[paste-tier2] clipboard::snapshot");
    let pb_snapshot = clipboard::snapshot().unwrap_or_else(|e| {
        tracing::warn!("[paste] clipboard snapshot failed (continuing): {e}");
        clipboard::PasteboardSnapshot::empty()
    });

    crate::diagnostic_log::emergency_trace("[paste-tier2] clipboard::write_text");
    if let Err(e) = clipboard::write_text(text) {
        tracing::warn!("[paste] clipboard write_text failed: {e}");
    }

    crate::diagnostic_log::emergency_trace("[paste-tier2] synthetic_keys::post_cmd_v");
    synthetic_keys::post_cmd_v();

    crate::diagnostic_log::emergency_trace("[paste-tier2] ax_trusted");
    if ax_trusted {
        crate::diagnostic_log::emergency_trace("[paste-tier2] sleep 200ms");
        std::thread::sleep(std::time::Duration::from_millis(200));
        crate::diagnostic_log::emergency_trace("[paste-tier2] clipboard::restore_if_untouched");
        match clipboard::restore_if_untouched(&pb_snapshot) {
            Ok(true) => tracing::info!("[paste] Tier 2 — clipboard restored"),
            Ok(false) => tracing::warn!(
                "[paste] Tier 2 — clipboard changed during paste, restore skipped"
            ),
            Err(e) => tracing::warn!("[paste] Tier 2 — clipboard restore error: {e}"),
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
