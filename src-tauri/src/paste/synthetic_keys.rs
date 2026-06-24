use super::keyboard_layout;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

pub fn post_cmd_v() {
    let v_keycode = keyboard_layout::v_keycode();

    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(s) => s,
        Err(()) => {
            tracing::error!("[synthetic_keys] CGEventSource creation failed");
            return;
        }
    };

    if let Ok(key_down) = CGEvent::new_keyboard_event(source.clone(), v_keycode, true) {
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);
        key_down.post(CGEventTapLocation::HID);
    }

    if let Ok(key_up) = CGEvent::new_keyboard_event(source, v_keycode, false) {
        key_up.post(CGEventTapLocation::HID);
    }
}
