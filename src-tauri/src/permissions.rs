// First-launch readiness checks + System Settings deep-links.
//
// Three things gate the app from being usable:
//   1. macOS Input Monitoring — needed by newer macOS releases for global
//      keyboard listening. Granting takes effect after restarting the listener.
//   2. macOS Microphone — TCC permission for cpal to capture audio.
//      Native prompt fires on `requestAccess`; granting takes effect live.
//   3. At least one local model exists in the canonical models dir.
//
// macOS Accessibility is still reported separately because automatic Cmd+V
// paste depends on it. On ad-hoc builds it can remain false even when System
// Settings shows Turbo Talk enabled, so it must not block dictation readiness.
//
// `check_readiness` returns the current state of all three so the frontend
// can render an onboarding wizard and re-poll while it's open. Each step's
// "Open Settings" button calls `open_system_settings` with a pane key.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by `reset_onboarding`, cleared by `clear_force_onboarding`.
/// Signals the frontend to show the welcome screen even when fully ready.
static FORCE_ONBOARDING: AtomicBool = AtomicBool::new(false);

/// True while the welcome screen (splash + onboarding) is active.
/// Starts true; cleared when setup completes (all gates green).
/// The hotkey reads this to silently suppress dictation during setup.
static ONBOARDING_ACTIVE: AtomicBool = AtomicBool::new(true);

/// Avoid flooding the logs while the onboarding UI polls readiness.
#[cfg(target_os = "macos")]
static WARNED_AX_FALLBACK: AtomicBool = AtomicBool::new(false);

/// Check whether the onboarding/splash screen is still active.
/// Called by the hotkey listener to suppress dictation during setup.
pub fn onboarding_active() -> bool {
    ONBOARDING_ACTIVE.load(Ordering::Acquire)
}

/// Clear the onboarding-active flag. Called when:
///   - Startup readiness is immediately green (no onboarding needed), or
///   - The user completes the onboarding wizard.
pub fn clear_onboarding_active() {
    ONBOARDING_ACTIVE.store(false, Ordering::Release);
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Readiness {
    pub accessibility: PermissionStatus,
    pub automatic_paste: PermissionStatus,
    pub input_monitoring: PermissionStatus,
    pub microphone: PermissionStatus,
    pub model_present: bool,
    /// Host OS id (`macos`, `windows`, `linux`, …) for platform-aware onboarding UI.
    pub platform: String,
    /// True iff all dictation gates pass — frontend uses this as the
    /// "show onboarding vs. show main UI" switch.
    pub ready: bool,
    /// Debug override: true when `reset_onboarding` was called this session.
    /// Frontend shows onboarding regardless of `ready` while this is set.
    pub force_onboarding: bool,
}

// ── Accessibility ───────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn automatic_paste_status() -> PermissionStatus {
    if crate::hotkey::accessibility_trusted() {
        PermissionStatus::Granted
    } else {
        if !WARNED_AX_FALLBACK.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "[permissions] AXIsProcessTrusted() returned false; \
                 relying on IOHIDManager keyboard fallback (Input Monitoring) for hotkey"
        );
    }
        PermissionStatus::Denied
    }
}

#[cfg(target_os = "macos")]
fn accessibility_status() -> PermissionStatus {
    // This field is the historical "can the app pass setup?" signal. The
    // IOHID keyboard listener now provides hotkeys through Input Monitoring,
    // so Accessibility is no longer a dictation gate on ad-hoc builds.
    let _ = automatic_paste_status();
    PermissionStatus::Granted
}

#[cfg(not(target_os = "macos"))]
fn automatic_paste_status() -> PermissionStatus {
    PermissionStatus::Unsupported
}

#[cfg(not(target_os = "macos"))]
fn accessibility_status() -> PermissionStatus {
    PermissionStatus::Unsupported
}

// ── Input Monitoring (IOKit HID TCC) ────────────────────────────────────────

#[cfg(target_os = "macos")]
fn input_monitoring_status() -> PermissionStatus {
    // Values from IOKit.framework/Headers/hidsystem/IOHIDLib.h.
    const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
    const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
    const K_IOHID_ACCESS_TYPE_DENIED: u32 = 1;
    const K_IOHID_ACCESS_TYPE_UNKNOWN: u32 = 2;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
    }

    let status = unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) };
    match status {
        K_IOHID_ACCESS_TYPE_GRANTED => PermissionStatus::Granted,
        K_IOHID_ACCESS_TYPE_DENIED => PermissionStatus::Denied,
        // TCC can transiently return Unknown after a binary update while it
        // re-verifies the code signature. If the IOHID listener is already
        // running, Input Monitoring is effectively granted — override Unknown
        // so the welcome screen doesn't re-appear on every rebuild.
        K_IOHID_ACCESS_TYPE_UNKNOWN if crate::hotkey::iohid_listener_running() => {
            PermissionStatus::Granted
        }
        K_IOHID_ACCESS_TYPE_UNKNOWN => PermissionStatus::NotDetermined,
        _ => PermissionStatus::Denied,
    }
}

#[cfg(not(target_os = "macos"))]
fn input_monitoring_status() -> PermissionStatus {
    PermissionStatus::Unsupported
}

// ── Microphone (AVFoundation) ───────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn microphone_status() -> PermissionStatus {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    // SAFETY: AVMediaTypeAudio is a static NSString constant exported by
    // AVFoundation; the binding types it as Option for the (impossible)
    // case that the framework couldn't load it. The class method takes a
    // non-null reference and returns a primitive enum value; no ownership
    // transfer is involved.
    let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
        return PermissionStatus::Unsupported;
    };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };
    // AVAuthorizationStatus is a struct(NSInteger) with associated consts,
    // not a Rust enum, so equality is the only pattern available.
    if status == AVAuthorizationStatus::Authorized {
        PermissionStatus::Granted
    } else if status == AVAuthorizationStatus::NotDetermined {
        PermissionStatus::NotDetermined
    } else {
        PermissionStatus::Denied
    }
}

#[cfg(not(target_os = "macos"))]
fn microphone_status() -> PermissionStatus {
    // Windows (and Linux): no OS-level mic permission API to query.
    // Returning Unsupported is correct here because:
    // 1. check_readiness() treats Unsupported as ok (non-blocking),
    //    so the readiness gate does not require a permission check.
    // 2. The native mic prompt fires automatically when cpal opens the
    //    input stream — no explicit requestAccess call is needed.
    // 3. If cpal fails (denied mic), the error surfaces via audio.rs's
    //    platform-aware mic_permission_help_text() message.
    PermissionStatus::Unsupported
}

/// Trigger the native macOS microphone prompt. Resolves to the new status
/// once the user dismisses the alert. If permission is already determined
/// (granted or denied), AVFoundation calls back synchronously with the
/// existing value — no second prompt is shown.
#[tauri::command]
#[specta::specta]
pub async fn request_microphone_permission() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        use tokio::sync::oneshot;

        let (tx, rx) = oneshot::channel::<()>();
        // The block must not survive across the `.await` (RcBlock is !Send).
        // Issue the AVFoundation request from a dedicated OS thread; the
        // completion handler signals the oneshot on whatever queue
        // AVFoundation chose. The async function only holds the receiver,
        // which is Send.
        std::thread::spawn(move || {
            use block2::RcBlock;
            use objc2::runtime::Bool;
            use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
            use std::sync::{Arc, Mutex};

            let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
                let _ = tx.send(());
                return;
            };
            let tx = Arc::new(Mutex::new(Some(tx)));
            let tx_for_block = tx.clone();
            let block = RcBlock::new(move |_granted: Bool| {
                if let Some(sender) = tx_for_block.lock().unwrap().take() {
                    let _ = sender.send(());
                }
            });
            // SAFETY: requestAccessForMediaType retains the block until it
            // invokes the completion handler on an arbitrary queue. The
            // RcBlock is dropped at the end of this thread; AVFoundation
            // retains its own reference for as long as it needs.
            unsafe {
                AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &block);
            }
        });
        // 30s upper bound covers "user walked away from the prompt".
        let _ = tokio::time::timeout(std::time::Duration::from_secs(30), rx).await;
        microphone_status()
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus::Unsupported
    }
}

/// Trigger the native macOS Input Monitoring prompt. This adds Turbo Talk to
/// Privacy & Security → Input Monitoring so the user can enable keyboard-event
/// listening for the packaged app. If the prompt was already denied, macOS will
/// not show it again; the caller should deep-link to System Settings.
///
/// Uses CGRequestListenEventAccess (CoreGraphics, macOS 12+) which is the
/// correct TCC path for Input Monitoring on modern macOS. Falls back to
/// IOHIDRequestAccess for older systems.
#[tauri::command]
#[specta::specta]
pub fn request_input_monitoring_permission() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            // Added macOS 12.0 — requests Input Monitoring (listen-event) TCC
            // access and adds the bundle to the Privacy list on first call.
            fn CGRequestListenEventAccess() -> bool;
        }
        #[link(name = "IOKit", kind = "framework")]
        extern "C" {
            fn IOHIDRequestAccess(request_type: u32) -> bool;
        }

        unsafe {
            // Primary: CoreGraphics TCC path (macOS 12+)
            CGRequestListenEventAccess();
            // Belt-and-suspenders: IOKit path for older macOS
            const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
            IOHIDRequestAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT);
        }
        input_monitoring_status()
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus::Unsupported
    }
}

// ── Model presence ──────────────────────────────────────────────────────────

fn whisper_model_present() -> bool {
    let Some(dir) = crate::settings::canonical_models_dir() else {
        return false;
    };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return false;
    };
    rd.flatten()
        .any(|e| e.path().extension().is_some_and(|ext| ext == "bin"))
}

fn model_present() -> bool {
    use crate::settings::BackendFamily;

    let cfg = crate::settings::load();
    match cfg.backend {
        BackendFamily::Whisper => whisper_model_present(),
        BackendFamily::Parakeet => {
            let variant = crate::settings::resolve_backend_variant(&cfg);
            crate::transcribe_backends::parakeet::variant_dir(&variant)
                .and_then(|d| {
                    crate::transcribe_backends::parakeet::validate_parakeet_model_dir(&d).ok()
                })
                .is_some()
        }
    }
}

// ── Public commands ─────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn check_readiness() -> Readiness {
    let accessibility = accessibility_status();
    let automatic_paste = automatic_paste_status();
    let input_monitoring = input_monitoring_status();
    let microphone = microphone_status();
    let model_present = model_present();
    let force_onboarding = FORCE_ONBOARDING.load(Ordering::SeqCst);
    // Unsupported means the permission is not applicable on this platform
    // (e.g. all three return Unsupported on Windows). Treat it as non-blocking
    // so `ready` reflects "can the app run" rather than "did every permission
    // pass a macOS TCC check."
    fn ok(s: PermissionStatus) -> bool {
        matches!(s, PermissionStatus::Granted | PermissionStatus::Unsupported)
    }
    // Input Monitoring: if the IOHID listener is already running, the hotkey
    // works regardless of what IOHIDCheckAccess reports. TCC can show Denied
    // for a freshly-rebuilt binary (stale code-hash entry) while the listener
    // is actively delivering events.
    let input_monitoring_ok =
        ok(input_monitoring) || crate::hotkey::iohid_listener_running();
    // Microphone: only block on explicit Denied. NotDetermined means cpal has
    // not yet prompted, or AVFoundation's TCC view of a cpal-granted permission
    // is stale. Either way the audio stream will open (or prompt) on first use.
    let microphone_ok = !matches!(microphone, PermissionStatus::Denied);
    let ready = ok(accessibility) && input_monitoring_ok && microphone_ok && model_present;
    tracing::info!(
        "[readiness] accessibility={:?} input_monitoring={:?}(ok={}) microphone={:?}(ok={}) model_present={} iohid_running={} ready={}",
        accessibility, input_monitoring, input_monitoring_ok, microphone, microphone_ok,
        model_present, crate::hotkey::iohid_listener_running(), ready
    );
    Readiness {
        accessibility,
        automatic_paste,
        input_monitoring,
        microphone,
        model_present,
        platform: std::env::consts::OS.to_string(),
        ready,
        force_onboarding,
    }
}

/// Debug command: set the in-memory force-onboarding flag so the frontend
/// shows the welcome screen immediately. Also re-enables hotkey suppression
/// while onboarding is active.
#[tauri::command]
#[specta::specta]
pub fn reset_onboarding() {
    FORCE_ONBOARDING.store(true, Ordering::SeqCst);
    ONBOARDING_ACTIVE.store(true, Ordering::Release);
}

/// Called by the frontend when onboarding completes, to clear the force flag
/// and enable the hotkey for dictation.
#[tauri::command]
#[specta::specta]
pub fn clear_force_onboarding() {
    FORCE_ONBOARDING.store(false, Ordering::SeqCst);
    clear_onboarding_active();
}

/// Called by the frontend when all setup gates are green (or onboarding
/// completes). Clears both the debug force flag and the onboarding-active
/// state so the hotkey can arm.
#[tauri::command]
#[specta::specta]
pub fn set_setup_complete() {
    FORCE_ONBOARDING.store(false, Ordering::SeqCst);
    clear_onboarding_active();
}

/// Open a specific pane in macOS System Settings. `pane` is one of:
///   "accessibility" | "microphone" | "input_monitoring"
/// Anything else returns an error so the frontend sees a typed failure
/// instead of silently launching the wrong pane.
#[tauri::command]
#[specta::specta]
pub fn open_system_settings(pane: String) -> Result<(), String> {
    let url = match pane.as_str() {
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        "microphone" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        "input_monitoring" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
        other => return Err(format!("unknown settings pane: {}", other)),
    };
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("failed to open System Settings: {}", e))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err("System Settings deep-link only supported on macOS".into())
    }
}

/// Restart the app. Used by the onboarding flow after the user grants
/// Accessibility — `AXIsProcessTrusted()` caches per-process, so a relaunch
/// is the only way to re-evaluate.
#[tauri::command]
#[specta::specta]
pub fn restart_app(app: tauri::AppHandle) {
    #[cfg(not(debug_assertions))]
    {
        app.restart();
    }

    #[cfg(debug_assertions)]
    {
        match std::env::current_exe() {
            Ok(exe) => {
                tracing::info!(
                    "[permissions] restart_app: spawning {} and exiting in dev mode",
                    exe.display()
                );
                let _ = std::process::Command::new(&exe).spawn();
                std::process::exit(0);
            }
            Err(e) => {
                tracing::error!("[permissions] restart_app: failed to get current exe: {e}");
                use tauri::Emitter;
                let _ = app.emit(
                    "ui-error",
                    serde_json::json!({
                        "kind": "restart-dev-mode",
                        "message": format!("Restart failed: {e}"),
                        "recoverable": true
                    }),
                );
            }
        }
    }
}

/// Reset the TCC permission entry for Turbo Talk so the onboarding wizard
/// can clear stale entries left by a previous install. After this call the
/// bundle is no longer in the Privacy list; the caller should immediately
/// re-run the relevant registration path (IOHIDManager open for input
/// monitoring, AXIsProcessTrustedWithOptions for accessibility) so macOS
/// re-adds a fresh, correctly-bound entry.
///
/// `service` must be one of: "accessibility" | "input_monitoring"
#[tauri::command]
#[specta::specta]
pub fn reset_tcc_entry(service: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let tcc_service = match service.as_str() {
            "accessibility" => "Accessibility",
            "input_monitoring" => "ListenEvent",
            other => return Err(format!("unknown permission service: {other}")),
        };
        let status = std::process::Command::new("tccutil")
            .arg("reset")
            .arg(tcc_service)
            .arg("com.turbotalk.dictation")
            .status()
            .map_err(|e| format!("tccutil failed to launch: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "tccutil reset exited with code {}",
                status.code().unwrap_or(-1)
            ))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = service;
        Err("Permission reset only supported on macOS".into())
    }
}

/// Trigger the native macOS Accessibility prompt. Side-effect: auto-adds
/// the app to the Privacy & Security → Accessibility list (toggled off) so
/// the user has something to enable. Called from the onboarding flow on
/// step 1 before deep-linking to System Settings.
///
/// Returns the trust status as observed at call time. AXIsProcessTrusted
/// caches per-process, so this will keep returning `Denied` until the user
/// grants and the app is restarted.
#[tauri::command]
#[specta::specta]
pub fn prompt_for_accessibility() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        use core_foundation::base::TCFType;
        use core_foundation::boolean::CFBoolean;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
        }

        let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::true_value();
        let opts = CFDictionary::from_CFType_pairs(&[(key, value)]);

        // SAFETY: AXIsProcessTrustedWithOptions takes a borrowed
        // CFDictionaryRef. The dictionary lives for the duration of this
        // call; the function does not retain it beyond the call.
        let trusted = unsafe {
            AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef() as *const std::ffi::c_void)
        };
        if trusted {
            PermissionStatus::Granted
        } else {
            PermissionStatus::Denied
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus::Unsupported
    }
}
