// Active-application text injection.
// macOS: write to clipboard (arboard), send Cmd+V via osascript, restore prior clipboard.
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

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> Option<String> {
    None
}

pub fn paste(text: &str) -> anyhow::Result<()> {
    let mut cb = Clipboard::new()?;

    // Save prior clipboard contents (best-effort — ignore if empty/non-text).
    let prior = cb.get_text().ok();

    cb.set_text(text)?;

    // Small delay so the clipboard write is visible to the target app.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let status = std::process::Command::new("osascript")
        .args(["-e", r#"tell application "System Events" to keystroke "v" using command down"#])
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
}
