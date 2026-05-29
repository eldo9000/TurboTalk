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
//
// Clipboard / paste success
// ──────────────────────────
// We query the macOS Accessibility API for the focused element role before
// paste purely for diagnostics — many modern editors (Cursor, Zed, VS Code,
// Electron webviews) accept Cmd+V fine but expose AXWebArea, AXGroup, or no
// focused element at all. A pre-paste AX role check therefore produces
// constant false "paste miss" reports even when injection succeeded.
//
// Success is defined by a successful osascript Cmd+V keystroke; the prior
// clipboard is always restored afterward (same policy as the Windows branch).

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
        tracing::warn!("[paste] frontmost_app() called on unsupported platform — returning None");
    }
    None
}

/// Queries the macOS Accessibility API for the role of the currently focused
/// UI element in the frontmost application. Returns `None` if AX is
/// unavailable, the process is not trusted for accessibility, or no element
/// has focus.
#[cfg(target_os = "macos")]
fn focused_ax_role() -> Option<String> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};
    use std::ptr;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
    }

    const AX_SUCCESS: i32 = 0;

    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }

        let attr_focused = CFString::new("AXFocusedUIElement");
        let mut focused: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(
            system,
            attr_focused.as_concrete_TypeRef(),
            &mut focused,
        );
        CFRelease(system);

        if err != AX_SUCCESS || focused.is_null() {
            return None;
        }

        let attr_role = CFString::new("AXRole");
        let mut role_ref: CFTypeRef = ptr::null();
        let err2 = AXUIElementCopyAttributeValue(
            focused,
            attr_role.as_concrete_TypeRef(),
            &mut role_ref,
        );
        CFRelease(focused);

        if err2 != AX_SUCCESS || role_ref.is_null() {
            return None;
        }

        // AXRole is always a CFString; wrap_under_create_rule takes the +1 retain.
        let role = CFString::wrap_under_create_rule(role_ref as CFStringRef);
        Some(role.to_string())
    }
}

/// Pastes `text` into the frontmost application.
///
/// Returns `Ok(true)` when the osascript Cmd+V keystroke succeeds — original
/// clipboard is restored. Returns `Err` on a hard failure (osascript non-zero
/// exit). AX role is logged at debug level for diagnostics only.
#[cfg(target_os = "macos")]
pub fn paste(text: &str) -> anyhow::Result<bool> {
    let mut cb = Clipboard::new()?;
    let prior = cb.get_text().ok();

    let ax_role = focused_ax_role();
    tracing::debug!("[paste] AX focused role before paste: {:?}", ax_role);

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
        // Hard osascript failure — restore clipboard and bail.
        if let Some(prev) = prior {
            let _ = cb.set_text(prev);
        }
        anyhow::bail!("osascript keystroke failed: {}", status);
    }

    std::thread::sleep(std::time::Duration::from_millis(150));

    if let Some(prev) = prior {
        let _ = cb.set_text(prev);
    }
    Ok(true)
}

/// Windows + Linux/X11 paste implementation.
///
/// Writes `text` to the system clipboard via `arboard`, synthesizes a native
/// `Ctrl+V` via `enigo`, then restores the prior clipboard contents. The
/// 50 ms / 150 ms sleeps mirror the macOS branch. Always returns `Ok(true)`
/// — AX-based miss detection is not available on non-macOS platforms.
///
/// Wayland is not supported in the beta — under `XDG_SESSION_TYPE=wayland`
/// we return an error containing the literal substring `unsupported platform`
/// (the test in this file and the UI banner both grep for that token).
#[cfg(not(target_os = "macos"))]
pub fn paste(text: &str) -> anyhow::Result<bool> {
    use arboard::Clipboard;
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    // Wayland detection (Linux only). On Windows this env var is unset, so the
    // check is a no-op.
    #[cfg(target_os = "linux")]
    {
        if let Ok(session) = std::env::var("XDG_SESSION_TYPE") {
            if session.eq_ignore_ascii_case("wayland") {
                tracing::warn!(
                    "[paste] XDG_SESSION_TYPE=wayland — paste injection not supported in beta"
                );
                anyhow::bail!(
                    "unsupported platform: paste under Wayland is not supported in this beta"
                );
            }
        }
    }

    let mut cb = Clipboard::new()?;
    let prior = cb.get_text().ok();

    cb.set_text(text)?;

    // Small delay so the clipboard write is visible to the target app.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("enigo init failed: {e}"))?;

    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| anyhow::anyhow!("enigo Ctrl press failed: {e}"))?;
    let click_res = enigo.key(Key::Unicode('v'), Direction::Click);
    // Always release Ctrl even if the 'v' click failed — don't leave modifier stuck.
    let release_res = enigo.key(Key::Control, Direction::Release);

    click_res.map_err(|e| anyhow::anyhow!("enigo 'v' click failed: {e}"))?;
    release_res.map_err(|e| anyhow::anyhow!("enigo Ctrl release failed: {e}"))?;

    // Restore prior clipboard after a short delay so Ctrl+V has time to land.
    std::thread::sleep(std::time::Duration::from_millis(150));
    if let Some(prev) = prior {
        let _ = cb.set_text(prev);
    }

    Ok(true)
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

    /// Under Linux/Wayland, `paste()` must return an error whose message
    /// contains the literal string `unsupported platform` — the UI banner
    /// relies on that token. We force-set `XDG_SESSION_TYPE=wayland` for
    /// the duration of the test so the runtime detection branch fires.
    ///
    /// Note: env vars are process-wide. This test sets and restores
    /// `XDG_SESSION_TYPE` around the call. It runs only on Linux because
    /// the Wayland code path is gated to that target.
    #[cfg(target_os = "linux")]
    #[test]
    fn paste_returns_unsupported_platform_under_wayland() {
        let prior = std::env::var("XDG_SESSION_TYPE").ok();
        std::env::set_var("XDG_SESSION_TYPE", "wayland");

        let err = paste("hello").unwrap_err();
        let msg = err.to_string();

        // Restore env var before asserting so a panic doesn't pollute the
        // process for other tests.
        match prior {
            Some(v) => std::env::set_var("XDG_SESSION_TYPE", v),
            None => std::env::remove_var("XDG_SESSION_TYPE"),
        }

        assert!(
            msg.contains("unsupported platform"),
            "expected 'unsupported platform' in error, got: {msg}"
        );
    }
}
