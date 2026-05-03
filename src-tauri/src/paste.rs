// Active-application text injection.
// macOS: write to clipboard (arboard), send Cmd+V via osascript, restore prior clipboard.
// Other platforms: not supported — both entry points return a clear
// "unsupported platform" signal so the caller (and the UI banner) can react.
//
// Focused-app policy (TASK-16)
// ─────────────────────────────
// Paste targets the app that is frontmost at the moment Cmd+V is sent — *not*
// the app that was frontmost when recording started. This is intentional for a
// personal push-to-talk tool: with one in-flight job, the user almost always
// expects "wherever I am now" rather than "wherever I was a few hundred ms ago".
//
// `frontmost_app()` is a best-effort macOS helper that captures a coarse
// identifier (frontmost process name) before recording start and again
// immediately before paste. Both values are logged with the `job_id`
// allocated in `hotkey.rs`. If they differ, `hotkey.rs` emits a
// `focus-changed-before-paste` event so the UI can surface a gentle banner.
//
// We deliberately do NOT block paste on a focus mismatch — see ARCHITECTURE.md
// "Paste Target Policy". Future queueing work must revisit this rule.

#[cfg(target_os = "macos")]
use arboard::Clipboard;

/// Best-effort frontmost application identifier on macOS. Returns `None` if
/// the osascript query fails for any reason (no Accessibility permission,
/// process exit, malformed output). Callers must treat `None` as "unknown"
/// and never block paste on it.
///
/// We query the frontmost *process* name rather than bundle id because
/// `System Events` exposes process name without the extra `tell application
/// "Finder"` round-trip and without requiring Automation permission for each
/// individual app. For the personal-use focus-change observability we want,
/// the process name is sufficient.
#[cfg(target_os = "macos")]
pub fn frontmost_app() -> Option<String> {
    let out = std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to get name of first process whose frontmost is true"#,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Non-macOS stub. Logs a one-line warning the first time it is called so
/// the operator sees an honest signal in the log, then always returns `None`.
/// Callers already treat `None` as "unknown" and never block paste on it.
#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> Option<String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "[paste] frontmost_app() called on unsupported platform — returning None"
        );
    }
    None
}

#[cfg(target_os = "macos")]
pub fn paste(text: &str) -> anyhow::Result<()> {
    let mut cb = Clipboard::new()?;

    // Save prior clipboard contents (best-effort — ignore if empty/non-text).
    let prior = cb.get_text().ok();

    cb.set_text(text)?;

    // Small delay so the clipboard write is visible to the target app.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let status = std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("osascript keystroke failed: {}", status);
    }

    // Restore prior clipboard after a short delay so Cmd+V has time to land.
    std::thread::sleep(std::time::Duration::from_millis(150));
    if let Some(prev) = prior {
        let _ = cb.set_text(prev);
    }

    Ok(())
}

/// Non-macOS stub. Returns an error containing the literal string
/// `unsupported platform` so the UI banner can detect and surface the
/// condition uniformly. A real Windows path would need clipboard + Ctrl+V
/// (e.g. via `enigo`); a real Linux path would need separate X11 and
/// Wayland handling. Neither is implemented.
#[cfg(not(target_os = "macos"))]
pub fn paste(_text: &str) -> anyhow::Result<()> {
    let _ = _text;
    anyhow::bail!("unsupported platform: paste is only implemented on macOS")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `frontmost_app` must never panic and must always return a value of the
    /// declared type. On non-macOS targets the result is always `None`; on
    /// macOS in CI it may be `None` (no graphical session) or `Some(name)`.
    /// Either is acceptable — we just verify the call doesn't blow up.
    #[test]
    fn frontmost_app_does_not_panic() {
        let _ = frontmost_app();
    }

    /// On non-macOS targets, `paste()` must return an error whose message
    /// contains the literal string `unsupported platform` — the UI banner
    /// relies on that token.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn paste_returns_unsupported_platform_off_mac() {
        let err = paste("hello").unwrap_err();
        assert!(
            err.to_string().contains("unsupported platform"),
            "expected 'unsupported platform' in error, got: {}",
            err
        );
    }
}
