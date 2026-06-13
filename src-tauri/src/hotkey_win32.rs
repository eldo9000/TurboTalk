//! Windows push-to-talk via `GetAsyncKeyState` polling.
//!
//! `WH_KEYBOARD_LL` (and `rdev` on top of it) often installs successfully but
//! receives zero events in packaged Tauri builds on some Windows setups. Polling
//! the configured VK at ~125 Hz is reliable for modifier-style PTT keys.

use crate::hotkey::common;
use crate::recorder::Recorder;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{tray::TrayIcon, AppHandle};
use winapi::um::winuser::GetAsyncKeyState;

const POLL_INTERVAL: Duration = Duration::from_millis(8);

static LISTENER_ALIVE: AtomicBool = AtomicBool::new(false);
static POLL_LOOPS: AtomicU64 = AtomicU64::new(0);
static MATCHED_DOWN_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_MATCHED_VK: AtomicU32 = AtomicU32::new(0);
static LISTENER_STARTED_MS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct HotkeyProbe {
    pub method: String,
    pub listener_alive: bool,
    pub poll_loops: u64,
    pub matched_down_count: u64,
    pub last_matched_vk: u32,
    pub listener_started_ms: u64,
}

pub fn diagnostic_probe() -> HotkeyProbe {
    HotkeyProbe {
        method: "GetAsyncKeyState polling (8ms)".into(),
        listener_alive: LISTENER_ALIVE.load(Ordering::Relaxed),
        poll_loops: POLL_LOOPS.load(Ordering::Relaxed),
        matched_down_count: MATCHED_DOWN_COUNT.load(Ordering::Relaxed),
        last_matched_vk: LAST_MATCHED_VK.load(Ordering::Relaxed),
        listener_started_ms: LISTENER_STARTED_MS.load(Ordering::Relaxed),
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn vk_down(vk: u32) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// VK codes to poll for each Settings hotkey name.
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
        // VK_F13–VK_F24 (0x7C–0x87) — spare keys; no default OS action.
        // Third-party pedals / macro keyboards: map to one of these in
        // your device software, then select the same key in TurboTalk.
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
        // Mouse extra buttons — users must disable native back/forward action
        // in their mouse software to avoid double-firing.
        "mouse_middle"  => &[0x04], // VK_MBUTTON
        "mouse_back"    => &[0x05], // VK_XBUTTON1
        "mouse_forward" => &[0x06], // VK_XBUTTON2
        _ => &[],
    }
}

fn is_configured_key_down(name: &str) -> Option<u32> {
    for &vk in vk_codes_for_name(name) {
        if vk_down(vk) {
            return Some(vk);
        }
    }
    None
}

struct PollContext {
    recorder: Arc<Recorder>,
    tray_icon: TrayIcon,
    app: AppHandle,
    hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    down: AtomicBool,
    esc_down: AtomicBool,
}

impl PollContext {
    fn tick(&self) {
        POLL_LOOPS.fetch_add(1, Ordering::Relaxed);

        let (config_key, toggle_mode, cancel_on_esc, cancel_on_hold) = {
            let hk = self.hotkey_state.read();
            (
                hk.key.clone(),
                hk.mode == "toggle",
                hk.cancel_on_esc,
                hk.cancel_on_hold,
            )
        };

        if cancel_on_esc && vk_down(0x1B) {
            if !self.esc_down.swap(true, Ordering::AcqRel) {
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
            }
            return;
        }
        self.esc_down.store(false, Ordering::Relaxed);

        let Some(vk) = is_configured_key_down(&config_key) else {
            if self.down.swap(false, Ordering::AcqRel) && !toggle_mode {
                tracing::debug!("[hotkey] win32 poll key up config={config_key}");
                common::disarm_hold_cancel();
                common::ptt_up(&self.recorder, &self.tray_icon, &self.app);
            }
            return;
        };

        let was_down = self.down.load(Ordering::Acquire);
        if !was_down {
            self.down.store(true, Ordering::Release);
            MATCHED_DOWN_COUNT.fetch_add(1, Ordering::Relaxed);
            LAST_MATCHED_VK.store(vk, Ordering::Relaxed);
            tracing::info!(
                "[hotkey] win32 poll key down vk=0x{vk:02X} config={config_key}"
            );
            if cancel_on_hold && common::should_arm_hold_cancel(&self.recorder) {
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
        }
    }
}

static POLL_CTX: Mutex<Option<Arc<PollContext>>> = Mutex::new(None);

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
                "[hotkey] Win32 GetAsyncKeyState poller starting — key={} mode={}",
                hk.key,
                hk.mode
            );
            if vk_codes_for_name(&hk.key).is_empty() {
                tracing::warn!("[hotkey] unknown hotkey key {:?} on Windows", hk.key);
            }
        }

        LISTENER_STARTED_MS.store(epoch_ms(), Ordering::Relaxed);
        LISTENER_ALIVE.store(true, Ordering::Relaxed);

        let ctx = Arc::new(PollContext {
            recorder,
            tray_icon,
            app,
            hotkey_state,
            down: AtomicBool::new(false),
            esc_down: AtomicBool::new(false),
        });
        *POLL_CTX.lock() = Some(ctx.clone());

        tracing::info!("[hotkey] Win32 key poller running");

        while LISTENER_ALIVE.load(Ordering::Relaxed) {
            ctx.tick();
            std::thread::sleep(POLL_INTERVAL);
        }

        *POLL_CTX.lock() = None;
        tracing::info!("[hotkey] Win32 key poller stopped");
    });
}

pub fn accessibility_trusted() -> bool {
    true
}
