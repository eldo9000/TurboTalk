use crate::diagnostic_log;
use super::keyboard_layout;
use std::ffi::c_void;

// ── Raw CGEvent FFI — bypasses the core-graphics crate wrappers because
// CGEventSourceCreate() crashes with EXC_BREAKPOINT on macOS 26 when
// called from an ad-hoc signed binary.  All three entry points accept
// NULL for the source (documented as valid by Apple).

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(
        source: *const c_void, // CGEventSourceRef — NULL is valid
        virtualKey: u16,
        keyDown: bool,
    ) -> *mut c_void; // CGEventRef

    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tapLocation: u32, event: *mut c_void);
    fn CFRelease(cf: *const c_void);
}

// CoreGraphics constants from CGEventTypes.h:
// kCGHIDEventTap = 0, kCGSessionEventTap = 1, kCGAnnotatedSessionEventTap = 2
const K_CG_SESSION_EVENT_TAP: u32 = 1;
// kCGEventFlagMaskCommand
const K_CG_EVENT_FLAG_COMMAND: u64 = 1 << 20;

pub fn post_cmd_v() {
    diagnostic_log::emergency_trace("[synth] v_keycode");
    let v_keycode = keyboard_layout::v_keycode();

    // Create both events with NULL source — avoids CGEventSourceCreate
    // which crashes on macOS 26 ad-hoc builds.
    diagnostic_log::emergency_trace("[synth] key_down event");
    let key_down = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), v_keycode, true) };
    if !key_down.is_null() {
        diagnostic_log::emergency_trace("[synth] set_flags + post key_down");
        unsafe {
            CGEventSetFlags(key_down, K_CG_EVENT_FLAG_COMMAND);
            CGEventPost(K_CG_SESSION_EVENT_TAP, key_down);
            CFRelease(key_down);
        }
    }

    diagnostic_log::emergency_trace("[synth] key_up event");
    let key_up = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), v_keycode, false) };
    if !key_up.is_null() {
        diagnostic_log::emergency_trace("[synth] post key_up");
        unsafe {
            CGEventPost(K_CG_SESSION_EVENT_TAP, key_up);
            CFRelease(key_up);
        }
    }

    diagnostic_log::emergency_trace("[synth] done");
}
