use crate::diagnostic_log;
use super::keyboard_layout;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

pub fn post_cmd_v() {
    diagnostic_log::emergency_trace("[synth] v_keycode");
    let v_keycode = keyboard_layout::v_keycode();

    diagnostic_log::emergency_trace("[synth] CGEventSource::new");
    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(s) => s,
        Err(()) => {
            tracing::error!("[synthetic_keys] CGEventSource creation failed");
            diagnostic_log::emergency_trace("[synth] CGEventSource failed");
            return;
        }
    };

    diagnostic_log::emergency_trace("[synth] key_down event");
    if let Ok(key_down) = CGEvent::new_keyboard_event(source.clone(), v_keycode, true) {
        diagnostic_log::emergency_trace("[synth] set_flags");
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);
        diagnostic_log::emergency_trace("[synth] post key_down");
        key_down.post(CGEventTapLocation::HID);
    }

    diagnostic_log::emergency_trace("[synth] key_up event");
    if let Ok(key_up) = CGEvent::new_keyboard_event(source, v_keycode, false) {
        diagnostic_log::emergency_trace("[synth] post key_up");
        key_up.post(CGEventTapLocation::HID);
    }

    diagnostic_log::emergency_trace("[synth] done");
}
