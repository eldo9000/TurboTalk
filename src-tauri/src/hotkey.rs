use crate::recorder::Recorder;
use crate::tray::{self, TrayState};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};
use std::sync::Arc;
use tauri::{tray::TrayIcon, AppHandle, Emitter};

fn key_for_name(name: &str) -> (i64, CGEventFlags) {
    match name {
        "right_control" => (0x3E, CGEventFlags::CGEventFlagControl),
        "right_command" => (0x36, CGEventFlags::CGEventFlagCommand),
        "right_shift"   => (0x3C, CGEventFlags::CGEventFlagShift),
        _               => (0x3D, CGEventFlags::CGEventFlagAlternate), // right_option (default)
    }
}

fn ptt_down(recorder: &Recorder, tray_icon: &TrayIcon, app: &AppHandle) {
    if let Err(e) = recorder.start() {
        tracing::error!("[hotkey] start failed: {:?}", e);
    }
    let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Recording)));
    let _ = app.emit("ptt-down", ());
}

fn ptt_up(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
    let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Transcribing)));
    let _ = app.emit("ptt-up", ());
    match recorder.stop() {
        Ok(Some(path)) => {
            let app2  = app.clone();
            let tray2 = tray_icon.clone();
            let rec2  = recorder.clone();
            std::thread::spawn(move || {
                match crate::transcribe::run(&path) {
                    Ok(text) => {
                        tracing::info!("[transcribe] {:?}", text);
                        let _ = app2.emit("transcript", text.clone());
                        if let Err(e) = crate::paste::paste(&text) {
                            tracing::error!("[paste] {:?}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("[transcribe] {:?}", e);
                        let _ = app2.emit("transcript", "");
                    }
                }
                let _ = tray2.set_icon(Some(tray::make_icon(TrayState::Idle)));
                drop(rec2);
            });
        }
        Ok(None) => {
            let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Idle)));
        }
        Err(e) => {
            tracing::error!("[hotkey] stop failed: {:?}", e);
            let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Idle)));
        }
    }
}

pub fn spawn(
    recorder: Arc<Recorder>,
    tray_icon: TrayIcon,
    app: AppHandle,
    hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
) {
    std::thread::spawn(move || {
        let tap = match CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::FlagsChanged],
            move |_proxy, _etype, event| {
                let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                let flags   = event.get_flags();

                // Read current config (RwLock read — nanoseconds, uncontended)
                let (target_keycode, target_flag, toggle_mode) = {
                    let hk = hotkey_state.read();
                    let (kc, f) = key_for_name(&hk.key);
                    (kc, f, hk.mode == "toggle")
                };

                if keycode == target_keycode {
                    let is_key_down = flags.contains(target_flag);
                    if toggle_mode {
                        if is_key_down {
                            if recorder.is_recording() {
                                ptt_up(&recorder, &tray_icon, &app);
                            } else {
                                ptt_down(&recorder, &tray_icon, &app);
                            }
                        }
                    } else {
                        if is_key_down {
                            ptt_down(&recorder, &tray_icon, &app);
                        } else {
                            ptt_up(&recorder, &tray_icon, &app);
                        }
                    }
                }
                None
            },
        ) {
            Ok(t) => t,
            Err(()) => {
                tracing::error!(
                    "[hotkey] CGEventTap failed — grant Accessibility permission in \
                     System Settings → Privacy & Security → Accessibility, then restart"
                );
                return;
            }
        };

        let source = tap
            .mach_port
            .create_runloop_source(0)
            .expect("[hotkey] create_runloop_source failed");
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        CFRunLoop::run_current();
    });
}
