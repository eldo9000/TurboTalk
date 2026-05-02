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

/// Emit a UI-critical event. Logs at warn-level if the emit fails (e.g. no
/// frontend listener is registered yet). Non-critical signals like
/// `audio-level` can keep using fire-and-forget `let _ = app.emit(...)`.
fn emit_critical<P: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: P) {
    if let Err(e) = app.emit(event, payload) {
        tracing::warn!("[hotkey] failed to emit {}: {:?}", event, e);
    }
}

fn ptt_down(recorder: &Recorder, tray_icon: &TrayIcon, app: &AppHandle) {
    if let Err(e) = recorder.start() {
        // Illegal transition or audio error — do NOT emit ptt-down, do NOT
        // change tray icon. Frontend stays in its current state.
        tracing::warn!("[hotkey] start ignored: {}", e);
        return;
    }
    let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Recording)));
    emit_critical(app, "ptt-down", ());
}

fn ptt_up(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
    match recorder.stop() {
        Ok(Some(path)) => {
            let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Transcribing)));
            emit_critical(app, "ptt-up", ());
            let app2  = app.clone();
            let tray2 = tray_icon.clone();
            let rec2  = recorder.clone();
            std::thread::spawn(move || {
                match crate::transcribe::run(&path) {
                    Ok(text) => {
                        tracing::info!("[transcribe] {:?}", text);
                        emit_critical(&app2, "transcript", text.clone());
                        if let Err(e) = crate::paste::paste(&text) {
                            tracing::error!("[paste] {:?}", e);
                            // Surface to UI so the user knows the transcript
                            // was processed but never reached the focused app.
                            // Keep the message short and actionable.
                            let msg = "Couldn't paste — check Accessibility permission".to_string();
                            emit_critical(&app2, "paste-error", msg);
                        }
                    }
                    Err(e) => {
                        tracing::error!("[transcribe] {:?}", e);
                        let msg = format!("{}", e);
                        emit_critical(&app2, "transcript-error", msg);
                    }
                }
                let _ = tray2.set_icon(Some(tray::make_icon(TrayState::Idle)));
                drop(rec2);
            });
        }
        Ok(None) => {
            // Silence trim discarded all samples. Tell the frontend to clear
            // its overlay — otherwise it stays stuck on "Transcribing…".
            let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Idle)));
            emit_critical(app, "recording-discarded", ());
        }
        Err(e) => {
            // Illegal transition (e.g. stop while not Recording) — do NOT
            // emit ptt-up or any other event. Frontend stays as-is.
            tracing::warn!("[hotkey] stop ignored: {}", e);
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
                    } else if is_key_down {
                        ptt_down(&recorder, &tray_icon, &app);
                    } else {
                        ptt_up(&recorder, &tray_icon, &app);
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
