//! Windows push-to-talk via `WH_KEYBOARD_LL`.
//!
//! `rdev` installs the low-level hook with a null module handle; on packaged
//! Tauri EXEs that often binds but receives zero key events. We install the
//! same hook with `GetModuleHandleA(NULL)` and map virtual-key codes directly.

use crate::hotkey::common;
use crate::recorder::Recorder;
use parking_lot::Mutex;
use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{tray::TrayIcon, AppHandle};
use winapi::shared::minwindef::{LPARAM, LRESULT, WPARAM};
use winapi::shared::windef::HHOOK;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::um::winuser::{
    CallNextHookEx, GetMessageA, SetWindowsHookExA, UnhookWindowsHookEx, HC_ACTION,
    KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const LLKHF_EXTENDED: u32 = 0x01;

/// Map Settings hotkey names to `(vkCode, extended?)`. `extended = None` matches
/// either side; `Some(true/false)` filters on `KBDLLHOOKSTRUCT.flags`.
fn vk_match(name: &str, vk: u32, extended: bool) -> bool {
    match name {
        "left_control" => vk == 0xA2,
        "right_control" => vk == 0xA3,
        "left_option" => vk == 0xA4,
        "right_option" => vk == 0xA5,
        "left_shift" => vk == 0xA0,
        "right_shift" => vk == 0xA1,
        "left_command" => vk == 0x5B,
        "right_command" => vk == 0x5C,
        // Numpad enter shares VK_RETURN (0x0D) but sets the extended flag.
        "numpad_enter" => vk == 0x0D && extended,
        "numpad_0" => vk == 0x60,
        "numpad_decimal" => vk == 0x6E,
        "numpad_add" => vk == 0x6B,
        "numpad_subtract" => vk == 0x6D,
        "numpad_multiply" => vk == 0x6A,
        unknown => {
            tracing::warn!("[hotkey] unknown hotkey key {:?} on Windows", unknown);
            false
        }
    }
}

struct HookContext {
    recorder: Arc<Recorder>,
    tray_icon: TrayIcon,
    app: AppHandle,
    hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    down: AtomicBool,
}

static HOOK_CTX: Mutex<Option<Arc<HookContext>>> = Mutex::new(None);
static mut HOOK_HANDLE: HHOOK = std::ptr::null_mut();

impl HookContext {
    fn handle(&self, wparam: WPARAM, vk: u32, flags: u32) {
        let is_down = matches!(
            wparam as u32,
            WM_KEYDOWN | WM_SYSKEYDOWN
        );
        let is_up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
        if !is_down && !is_up {
            return;
        }

        let extended = (flags & LLKHF_EXTENDED) != 0;
        let (config_key, toggle_mode, cancel_on_esc, cancel_on_hold) = {
            let hk = self.hotkey_state.read();
            (
                hk.key.clone(),
                hk.mode == "toggle",
                hk.cancel_on_esc,
                hk.cancel_on_hold,
            )
        };

        if vk == 0x1B && is_down && cancel_on_esc {
            let s = self.recorder.state();
            if matches!(
                s,
                crate::recorder::State::Recording | crate::recorder::State::Transcribing
            ) {
                if !toggle_mode && matches!(s, crate::recorder::State::Recording) {
                    common::arm_ptt_up_suppression();
                }
                common::trigger_cancel(&self.recorder, &self.tray_icon, &self.app);
            }
            return;
        }

        if !vk_match(&config_key, vk, extended) {
            return;
        }

        if is_down {
            let was_down = self.down.swap(true, Ordering::AcqRel);
            if was_down {
                return;
            }
            tracing::debug!("[hotkey] win32 key down vk=0x{vk:02X} config={config_key}");
            if cancel_on_hold {
                common::arm_hold_cancel(&self.recorder, &self.tray_icon, &self.app, toggle_mode);
            }
            if toggle_mode {
                if self.recorder.is_recording() {
                    common::ptt_up(&self.recorder, &self.tray_icon, &self.app);
                } else {
                    common::ptt_down(&self.recorder, &self.tray_icon, &self.app);
                }
            } else {
                common::ptt_down(&self.recorder, &self.tray_icon, &self.app);
            }
        } else {
            let was_down = self.down.swap(false, Ordering::AcqRel);
            if !was_down {
                return;
            }
            tracing::debug!("[hotkey] win32 key up vk=0x{vk:02X} config={config_key}");
            common::disarm_hold_cancel();
            if !toggle_mode {
                common::ptt_up(&self.recorder, &self.tray_icon, &self.app);
            }
        }
    }
}

unsafe extern "system" fn hook_proc(code: c_int, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        if let Some(ctx) = HOOK_CTX.lock().as_ref() {
            ctx.handle(wparam, kb.vkCode, kb.flags);
        }
    }
    unsafe { CallNextHookEx(HOOK_HANDLE, code, wparam, lparam) }
}

pub fn spawn(
    recorder: Arc<Recorder>,
    tray_icon: TrayIcon,
    app: AppHandle,
    hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
) {
    let app_for_error = app.clone();

    std::thread::spawn(move || {
        {
            let hk = hotkey_state.read();
            tracing::info!(
                "[hotkey] Win32 WH_KEYBOARD_LL starting — key={} mode={}",
                hk.key,
                hk.mode
            );
        }

        let ctx = Arc::new(HookContext {
            recorder,
            tray_icon,
            app,
            hotkey_state,
            down: AtomicBool::new(false),
        });
        *HOOK_CTX.lock() = Some(ctx);

        unsafe {
            let module = GetModuleHandleA(std::ptr::null());
            let hook = SetWindowsHookExA(WH_KEYBOARD_LL, Some(hook_proc), module, 0);
            if hook.is_null() {
                let err = GetLastError();
                tracing::error!("[hotkey] SetWindowsHookExA failed: error={err}");
                *HOOK_CTX.lock() = None;
                let app_for_error = app_for_error;
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    common::emit_critical(
                        &app_for_error,
                        "ui-error",
                        common::UiError {
                            kind: "hotkey-bind-failed",
                            message: "Push-to-talk hotkey could not be bound. Restart Turbo Talk \
                                and try again."
                                .to_string(),
                            recoverable: true,
                        },
                    );
                });
                return;
            }
            HOOK_HANDLE = hook;
            tracing::info!("[hotkey] Win32 low-level keyboard hook installed");
            GetMessageA(std::ptr::null_mut(), std::ptr::null_mut(), 0, 0);
            UnhookWindowsHookEx(hook);
            HOOK_HANDLE = std::ptr::null_mut();
        }
        *HOOK_CTX.lock() = None;
    });
}

pub fn accessibility_trusted() -> bool {
    true
}
