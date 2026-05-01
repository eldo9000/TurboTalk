use crate::recorder::Recorder;
use crate::tray::{self, TrayState};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};
use std::sync::Arc;
use tauri::{tray::TrayIcon, AppHandle, Emitter};

// kVK_RightOption = 0x3D
const RIGHT_OPTION_KEYCODE: i64 = 0x3D;

pub fn spawn(recorder: Arc<Recorder>, tray_icon: TrayIcon, app: AppHandle) {
    std::thread::spawn(move || {
        let tap = match CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::FlagsChanged],
            move |_proxy, _etype, event| {
                let keycode =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                if keycode == RIGHT_OPTION_KEYCODE {
                    let flags = event.get_flags();
                    if flags.contains(CGEventFlags::CGEventFlagAlternate) {
                        if let Err(e) = recorder.start() {
                            tracing::error!("[hotkey] start failed: {:?}", e);
                        }
                        let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Recording)));
                        let _ = app.emit("ptt-down", ());
                    } else {
                        let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Transcribing)));
                        let _ = app.emit("ptt-up", ());
                        match recorder.stop() {
                            Ok(Some(path)) => {
                                let app2 = app.clone();
                                let tray2 = tray_icon.clone();
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
