//! Windows push-to-talk via `WH_KEYBOARD_LL` low-level keyboard hook.
//!
//! Installs a global low-level keyboard hook on a dedicated message-pumping
//! thread. The hook callback processes each keyboard event, matches the
//! configured PTT hotkey and Escape cancel, and dispatches to the
//! `Controller` layer.

use super::common;
use super::Controller;
use crate::recorder::Recorder;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{tray::TrayIcon, AppHandle};
use winapi::shared::minwindef::{DWORD, HHOOK, HINSTANCE, LPARAM, LRESULT, UINT, WPARAM};
use winapi::um::winuser::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

// LLKHF_* constants — winapi 0.3 does not expose these; values from
// Windows SDK `winuser.h`.
const LLKHF_EXTENDED: u32 = 0x0001;
const LLKHF_INJECTED: u32 = 0x0010;
const LLKHF_ALTDOWN: u32 = 0x0020;
const LLKHF_UP: u32 = 0x0080;

/// Hook handle stored for cleanup inside the hook thread.
static HOOK_HANDLE: AtomicUsize = AtomicUsize::new(0);

/// Thread ID stored so an external caller can post WM_QUIT to shut down
/// the hook thread cleanly.
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

static LISTENER_ALIVE: AtomicBool = AtomicBool::new(false);
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static KEY_EVENT_COUNT: AtomicU64 = AtomicU64::new(0);
static ESC_CANCEL_COUNT: AtomicU64 = AtomicU64::new(0);
static LISTENER_STARTED_MS: AtomicU64 = AtomicU64::new(0);

/// True while the PTT key is physically held. Prevents the first
/// WM_KEYDOWN from re-entering on autorepeat.
static PTT_KEY_HELD: AtomicBool = AtomicBool::new(false);

/// Escape key dedup same as PTT_KEY_HELD.
static ESC_KEY_HELD: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct HotkeyProbe {
    pub method: String,
    pub listener_alive: bool,
    pub hook_installed: bool,
    pub key_event_count: u64,
    pub esc_cancel_count: u64,
    pub listener_started_ms: u64,
}

pub fn diagnostic_probe() -> HotkeyProbe {
    HotkeyProbe {
        method: "WH_KEYBOARD_LL low-level hook (message pump)".into(),
        listener_alive: LISTENER_ALIVE.load(Ordering::Relaxed),
        hook_installed: HOOK_INSTALLED.load(Ordering::Relaxed),
        key_event_count: KEY_EVENT_COUNT.load(Ordering::Relaxed),
        esc_cancel_count: ESC_CANCEL_COUNT.load(Ordering::Relaxed),
        listener_started_ms: LISTENER_STARTED_MS.load(Ordering::Relaxed),
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// VK codes for each Settings hotkey name.
fn vk_codes_for_name(name: &str) -> &'static [u32] {
    match name {
        "left_control" => &[0xA2],
        "right_control" => &[0xA3],
        "left_option" => &[0xA4],
        "right_option" => &[0xA5],
        "left_shift" => &[0xA0],
        "right_shift" => &[0xA1],
        "left_command" => &[0x5B],
        "right_command" => &[0x5C],
        // VK_F13–VK_F24 (0x7C–0x87)
        "f13" => &[0x7C],
        "f14" => &[0x7D],
        "f15" => &[0x7E],
        "f16" => &[0x7F],
        "f17" => &[0x80],
        "f18" => &[0x81],
        "f19" => &[0x82],
        "f20" => &[0x83],
        "f21" => &[0x84],
        "f22" => &[0x85],
        "f23" => &[0x86],
        "f24" => &[0x87],
        // Mouse buttons — listed for completeness but not supported by
        // WH_KEYBOARD_LL. A separate WH_MOUSE_LL hook would be needed.
        "mouse_middle" => &[0x04],
        "mouse_back" => &[0x05],
        "mouse_forward" => &[0x06],
        _ => &[],
    }
}

/// True when the key name maps to a keyboard VK (not a mouse button).
fn is_keyboard_key(name: &str) -> bool {
    !matches!(name, "mouse_middle" | "mouse_back" | "mouse_forward")
}

struct HookContext {
    recorder: Arc<Recorder>,
    tray_icon: TrayIcon,
    app: AppHandle,
    hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
}

/// The context is stored behind a Mutex so the hook callback can read it.
/// Since both spawn() and the callback run on the same thread, contention
/// is zero in practice.
static HOOK_CTX: Mutex<Option<Arc<HookContext>>> = Mutex::new(None);

/// Low-level keyboard hook callback. Runs on the hook thread during
/// `GetMessageW` processing.
///
/// # Safety
///
/// `lParam` must point to a valid `KBDLLHOOKSTRUCT` when `code >= 0`.
/// Must call `CallNextHookEx` with the original parameters.
unsafe extern "system" fn hook_callback(code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param);
    }

    KEY_EVENT_COUNT.fetch_add(1, Ordering::Relaxed);

    let kb = &*(l_param as *const KBDLLHOOKSTRUCT);
    let vk = kb.vkCode;

    // Skip synthetic/injected keystrokes to avoid reacting to automation
    // or software that programmatically generates keystrokes.
    if (kb.flags & LLKHF_INJECTED) != 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param);
    }

    let ctx_guard = HOOK_CTX.lock();
    let Some(ctx) = ctx_guard.as_ref() else {
        return CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param);
    };

    let (config_key, cancel_on_esc, cancel_on_hold) = {
        let hk = ctx.hotkey_state.read();
        (hk.key.clone(), hk.cancel_on_esc, hk.cancel_on_hold)
    };

    // ── Escape cancel ───────────────────────────────────────────────────
    if cancel_on_esc && vk == 0x1B {
        match w_param as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if !ESC_KEY_HELD.swap(true, Ordering::AcqRel) {
                    ESC_CANCEL_COUNT.fetch_add(1, Ordering::Relaxed);
                    let controller = {
                        let hk = ctx.hotkey_state.read();
                        Controller::from_mode(&hk.mode, &ctx.recorder, &ctx.tray_icon, &ctx.app)
                    };
                    controller.cancel_if_busy();
                }
            }
            WM_KEYUP | WM_SYSKEYUP => {
                ESC_KEY_HELD.store(false, Ordering::Release);
            }
            _ => {}
        }
        return CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param);
    }

    // ── PTT hotkey match ────────────────────────────────────────────────
    let vks = vk_codes_for_name(&config_key);
    if vks.is_empty() || !vks.contains(&vk) {
        return CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param);
    }

    let controller = {
        let hk = ctx.hotkey_state.read();
        Controller::from_mode(&hk.mode, &ctx.recorder, &ctx.tray_icon, &ctx.app)
    };

    match w_param as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if !PTT_KEY_HELD.swap(true, Ordering::AcqRel) {
                if cancel_on_hold {
                    controller.arm_hold_cancel_if_busy();
                }
                controller.press();
            }
        }
        WM_KEYUP | WM_SYSKEYUP => {
            if PTT_KEY_HELD.swap(false, Ordering::AcqRel) {
                controller.release();
            }
        }
        _ => {}
    }

    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

pub fn spawn(
    recorder: Arc<Recorder>,
    tray_icon: TrayIcon,
    app: AppHandle,
    hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
) {
    std::thread::spawn(move || {
        {
            let hk = hotkey_state.read();
            tracing::info!(
                "[hotkey] Win32 WH_KEYBOARD_LL hook starting — key={} mode={}",
                hk.key,
                hk.mode,
            );
            if vk_codes_for_name(&hk.key).is_empty() {
                tracing::warn!("[hotkey] unknown hotkey key {:?} on Windows", hk.key);
            }
            if !is_keyboard_key(&hk.key) {
                tracing::warn!(
                    "[hotkey] mouse button key {:?} is not captured by WH_KEYBOARD_LL; \
                     PTT will not respond to this key",
                    hk.key,
                );
            }
        }

        LISTENER_STARTED_MS.store(epoch_ms(), Ordering::Relaxed);

        *HOOK_CTX.lock() = Some(Arc::new(HookContext {
            recorder,
            tray_icon: tray_icon.clone(),
            app: app.clone(),
            hotkey_state,
        }));

        HOOK_THREAD_ID.store(
            unsafe { winapi::um::processthreadsapi::GetCurrentThreadId() },
            Ordering::Relaxed,
        );

        // Install the low-level keyboard hook.
        // dwThreadId = 0 (global hook); hmod = NULL since the hook proc
        // lives in our own module.
        let hook = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_callback), 0 as HINSTANCE, 0)
        };

        if hook.is_null() {
            tracing::error!("[hotkey] SetWindowsHookExW(WH_KEYBOARD_LL) failed");
            *HOOK_CTX.lock() = None;
            return;
        }

        HOOK_HANDLE.store(hook as usize, Ordering::Relaxed);
        HOOK_INSTALLED.store(true, Ordering::Relaxed);
        LISTENER_ALIVE.store(true, Ordering::Relaxed);
        tracing::info!("[hotkey] Win32 WH_KEYBOARD_LL hook installed");

        // ── Message pump ─────────────────────────────────────────────────
        // `GetMessageW` blocks until a message arrives; the hook callback
        // fires synchronously inside this call for each keyboard event.
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let ret = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            match ret {
                0 => break,                         // WM_QUIT
                -1 => {
                    tracing::error!("[hotkey] GetMessageW returned -1");
                    break;
                }
                _ => {} // message processed, continue
            }
        }

        // ── Cleanup ──────────────────────────────────────────────────────
        let hook_ptr = HOOK_HANDLE.load(Ordering::Relaxed) as HHOOK;
        if !hook_ptr.is_null() {
            UnhookWindowsHookEx(hook_ptr);
        }
        *HOOK_CTX.lock() = None;
        LISTENER_ALIVE.store(false, Ordering::Relaxed);
        HOOK_INSTALLED.store(false, Ordering::Relaxed);
        tracing::info!("[hotkey] Win32 WH_KEYBOARD_LL hook stopped");
    });
}

pub fn accessibility_trusted() -> bool {
    true
}

pub fn iohid_listener_running() -> bool {
    false
}
