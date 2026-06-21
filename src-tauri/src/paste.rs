// Active-application text injection.
// macOS: write to clipboard (arboard), send Cmd+V via CGEventPost when
// Accessibility trust is available, restore prior clipboard. `frontmost_app()` uses
// NSWorkspace via objc2 instead of osascript (also sub-millisecond).
// Other platforms: not supported — both entry points return a clear
// "unsupported platform" signal so the caller (and the UI banner) can react.
//
// Focused-app policy
// ────────────────────
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
// Cmd+V is sent via CGEventPost (sub-millisecond native keystroke injection).
// If macOS does not trust this process for Accessibility, keyboard injection
// can be silently dropped; in that case we leave the transcript on the
// clipboard and return Ok(false) so the caller can surface an honest message.

#[cfg(target_os = "macos")]
use arboard::Clipboard;

#[cfg(target_os = "macos")]
fn set_clipboard_text_verified(cb: &mut Clipboard, text: &str) -> anyhow::Result<usize> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    cb.set_text(text)?;
    let readback_len = cb.get_text().ok().map(|s| s.len()).unwrap_or(0);
    if readback_len > 0 {
        tracing::info!("[paste] clipboard write verified via arboard ({} bytes)", readback_len);
        return Ok(readback_len);
    }

    tracing::warn!("[paste] arboard clipboard readback was empty; retrying with pbcopy");
    let mut child = Command::new("/usr/bin/pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("pbcopy spawn failed: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| anyhow::anyhow!("pbcopy write failed: {e}"))?;
    }
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("pbcopy wait failed: {e}"))?;
    if !status.success() {
        anyhow::bail!("pbcopy failed with status {status}");
    }

    let readback_len = cb.get_text().ok().map(|s| s.len()).unwrap_or(0);
    tracing::info!(
        "[paste] clipboard write verified after pbcopy fallback ({} bytes)",
        readback_len
    );
    if readback_len == 0 {
        anyhow::bail!("clipboard write readback is still empty after pbcopy fallback");
    }
    Ok(readback_len)
}

#[cfg(target_os = "macos")]
fn accessibility_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    unsafe { AXIsProcessTrusted() }
}

/// Best-effort frontmost application identifier on macOS. Returns `None` if
/// the native NSWorkspace query fails for any reason. Callers must treat
/// `None` as "unknown" and never block paste on it.
///
/// Uses `NSWorkspace.sharedWorkspace.frontmostApplication.localizedName`
/// via the `objc2` crate — a sub-millisecond native call.
/// The process name is sufficient for focus-change observability.
#[cfg(target_os = "macos")]
pub fn frontmost_app() -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    unsafe {
        let workspace: *mut AnyObject = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let frontmost: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if frontmost.is_null() {
            return None;
        }
        let name: *mut AnyObject = msg_send![frontmost, localizedName];
        if name.is_null() {
            return None;
        }
        // localizedName returns an autoreleased NSString*; convert via UTF8String.
        let utf8: *const std::os::raw::c_char = msg_send![name, UTF8String];
        if utf8.is_null() {
            return None;
        }
        let c_str = std::ffi::CStr::from_ptr(utf8);
        Some(c_str.to_string_lossy().into_owned())
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
        let err =
            AXUIElementCopyAttributeValue(system, attr_focused.as_concrete_TypeRef(), &mut focused);
        CFRelease(system);

        if err != AX_SUCCESS || focused.is_null() {
            return None;
        }

        let attr_role = CFString::new("AXRole");
        let mut role_ref: CFTypeRef = ptr::null();
        let err2 =
            AXUIElementCopyAttributeValue(focused, attr_role.as_concrete_TypeRef(), &mut role_ref);
        CFRelease(focused);

        if err2 != AX_SUCCESS || role_ref.is_null() {
            return None;
        }

        // AXRole is always a CFString; wrap_under_create_rule takes the +1 retain.
        let role = CFString::wrap_under_create_rule(role_ref as CFStringRef);
        Some(role.to_string())
    }
}

/// Pastes `text` into the frontmost application via native CGEventPost.
///
/// Writes to clipboard, synthesizes Cmd+V via `CGEvent::new_keyboard_event` +
/// `CGEventPost` (sub-millisecond native keystroke injection), then restores
/// the prior clipboard.
///
/// Returns `Ok(true)` when the keystroke is posted, or `Ok(false)` when
/// macOS is expected to block keystroke injection and the text was copied
/// for a manual Cmd+V instead. AX role is logged at debug level for
/// diagnostics only.
#[cfg(target_os = "macos")]
pub fn paste(text: &str) -> anyhow::Result<bool> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let mut cb = Clipboard::new()?;
    let prior = cb.get_text().ok();

    let ax_role = focused_ax_role();
    tracing::debug!("[paste] AX focused role before paste: {:?}", ax_role);

    let clipboard_len = set_clipboard_text_verified(&mut cb, text)?;
    let ax_trusted = accessibility_trusted();

    // kVK_ANSI_V from Carbon/HIToolbox/Events.h
    const V_KEYCODE: u16 = 0x09;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| anyhow::anyhow!("CGEventSource creation failed"))?;

    // When AX is trusted, post at the HID level (hardware-level injection).
    // When AX is false, try the Session tap as a best-effort path — it does
    // not require Accessibility trust on all macOS builds. If the OS drops
    // the event silently, the caller's Ok(false) path will show the clipboard
    // banner and Cmd+V still works.
    let tap = if ax_trusted {
        CGEventTapLocation::HID
    } else {
        CGEventTapLocation::Session
    };

    if let Ok(key_down) = CGEvent::new_keyboard_event(source.clone(), V_KEYCODE, true) {
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);
        key_down.post(tap);
    }
    if let Ok(key_up) = CGEvent::new_keyboard_event(source, V_KEYCODE, false) {
        key_up.post(tap);
    }

    if !ax_trusted {
        tracing::info!(
            "[paste] AX trust false; tried Session-tap Cmd+V, transcript also in clipboard ({} bytes)",
            clipboard_len
        );
        // Leave prior clipboard unrestored so manual Cmd+V still works if the
        // Session-tap event was dropped by the OS.
        return Ok(false);
    }

    // Delay to let the paste land before restoring clipboard.
    // Heavyweight apps (Electron, Xcode, etc.) with a busy main thread
    // can miss the paste at shorter intervals — they may read the restored
    // clipboard content instead.
    std::thread::sleep(std::time::Duration::from_millis(500));

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
