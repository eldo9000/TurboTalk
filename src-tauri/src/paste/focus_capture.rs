// macOS Accessibility-API focus capture and window activation.
//
// Provides rich focus snapshots (PID, bundle ID, app name) and
// reliable app activation before paste, superseding the old
// NSWorkspace-only frontmost_app() pattern.

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

const AX_SUCCESS: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementGetPid(element: CFTypeRef, pid: *mut i32) -> i32;
}

/// Rich snapshot of the focused application at a point in time.
pub struct FocusSnapshot {
    pub pid: i32,
    pub bundle_id: Option<String>,
    pub app_name: Option<String>,
}

/// Capture the currently focused application.
///
/// Uses AX API to get the focused element, extracts PID, then uses
/// NSRunningApplication for bundle ID and display name.
pub fn snapshot() -> FocusSnapshot {
    let pid = match focused_pid() {
        Some(pid) => pid,
        None => {
            return FocusSnapshot { pid: 0, bundle_id: None, app_name: None };
        }
    };

    FocusSnapshot {
        pid,
        bundle_id: bundle_id_for_pid(pid),
        app_name: app_name_for_pid(pid),
    }
}

/// Bring the captured application to the foreground.
///
/// Uses NSRunningApplication on the main thread, or osascript from
/// background threads (which is always the case during dictation).
pub fn activate_app(snapshot: &FocusSnapshot) {
    if snapshot.pid <= 0 {
        return;
    }

    // NSRunningApplication is an AppKit class — main thread only.
    // The paste worker runs on a background thread, so we use osascript
    // which is safe to call from any thread.
    if is_main_thread() {
        unsafe {
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![
                objc2::class!(NSRunningApplication),
                runningApplicationWithProcessIdentifier: snapshot.pid
            ];
            if app.is_null() { return; }

            let responds: i8 = objc2::msg_send![
                app,
                respondsToSelector: objc2::sel!(yieldActivationToApplication:)
            ];
            if responds != 0 {
                let _: () = objc2::msg_send![app, yieldActivationToApplication: app];
            }

            const NS_ACTIVATE_IGNORING_OTHER_APPS: u64 = 1 << 0;
            let _: () = objc2::msg_send![app, activateWithOptions: NS_ACTIVATE_IGNORING_OTHER_APPS];
        }
    } else {
        // On background threads, tell System Events via osascript to activate
        // the target app by PID.
        let script = format!(
            "tell application \"System Events\" to set frontmost of \
             (first process whose unix id is {}) to true",
            snapshot.pid
        );
        match std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
        {
            Ok(out) if out.status.success() => {
                tracing::debug!(
                    "[paste] activated app pid={} via osascript",
                    snapshot.pid
                );
            }
            Ok(out) => {
                tracing::warn!(
                    "[paste] osascript activate failed for pid={}: {}",
                    snapshot.pid,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Err(e) => {
                tracing::warn!("[paste] osascript spawn failed: {e}");
            }
        }
    }
}

#[link(name = "System", kind = "dylib")]
extern "C" {
    fn pthread_main_np() -> i32;
}

fn is_main_thread() -> bool {
    unsafe { pthread_main_np() != 0 }
}

/// Best-effort bundle ID for the frontmost app.
pub fn frontmost_bundle_id() -> Option<String> {
    let pid = focused_pid()?;
    bundle_id_for_pid(pid)
}

/// Re-export the old-style frontmost_app() for backward compat during migration.
pub fn frontmost_app_name() -> Option<String> {
    super::legacy::frontmost_app()
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// AX-focused element → PID. Returns `None` if AX is unavailable, untrusted,
/// or no element has focus.
fn focused_pid() -> Option<i32> {
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

        let mut pid: i32 = 0;
        let pid_err = AXUIElementGetPid(focused, &mut pid);
        CFRelease(focused);

        if pid_err != AX_SUCCESS || pid == 0 {
            return None;
        }

        Some(pid)
    }
}

unsafe fn nsstring_to_string(ns: *mut objc2::runtime::AnyObject) -> Option<String> {
    if ns.is_null() {
        return None;
    }
    let utf8: *const c_char = objc2::msg_send![ns, UTF8String];
    if utf8.is_null() {
        return None;
    }
    Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
}

unsafe fn running_app(pid: i32) -> Option<*mut objc2::runtime::AnyObject> {
    let app: *mut objc2::runtime::AnyObject = objc2::msg_send![
        objc2::class!(NSRunningApplication),
        runningApplicationWithProcessIdentifier: pid
    ];
    if app.is_null() { None } else { Some(app) }
}

fn bundle_id_for_pid(pid: i32) -> Option<String> {
    if !is_main_thread() { return None; }
    unsafe {
        let app = running_app(pid)?;
        let ident: *mut objc2::runtime::AnyObject =
            objc2::msg_send![app, bundleIdentifier];
        nsstring_to_string(ident)
    }
}

fn app_name_for_pid(pid: i32) -> Option<String> {
    if !is_main_thread() { return None; }
    unsafe {
        let app = running_app(pid)?;
        let name: *mut objc2::runtime::AnyObject =
            objc2::msg_send![app, localizedName];
        nsstring_to_string(name)
    }
}
