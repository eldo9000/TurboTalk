use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use std::sync::atomic::{AtomicU16, Ordering};

// 0 means "not yet cached" — safe sentinel because keycode 0x00 is
// kVK_ANSI_A on every macOS keyboard layout and will never produce 'v'.
static V_KEYCODE_CACHE: AtomicU16 = AtomicU16::new(0);

const DEFAULT_V_KEYCODE: u16 = 0x09; // kVK_ANSI_V

/// Returns the physical keycode that produces 'v' on the current
/// keyboard layout. Thread-safe — resolves once and caches.
pub fn v_keycode() -> u16 {
    let cached = V_KEYCODE_CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    match resolve_v_keycode() {
        Ok(code) => {
            V_KEYCODE_CACHE.store(code, Ordering::Relaxed);
            code
        }
        Err(_) => DEFAULT_V_KEYCODE,
    }
}

/// Force a fresh lookup (for testing). Not cached.
pub fn resolve_v_keycode() -> anyhow::Result<u16> {
    #[allow(non_camel_case_types)]
    enum UCKeyboardLayout {}

    #[allow(non_upper_case_globals)]
    const kUCKeyActionDown: u16 = 1;
    #[allow(non_upper_case_globals)]
    const kUCKeyTranslateNoDeadKeysMask: u32 = 1 << 0;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISCopyCurrentKeyboardLayoutInputSource() -> CFTypeRef;
        fn TISGetInputSourceProperty(
            inputSource: CFTypeRef,
            propertyKey: CFStringRef,
        ) -> CFTypeRef;
        fn CFDataGetBytePtr(theData: CFTypeRef) -> *const u8;
        fn UCKeyTranslate(
            keyLayout: *const UCKeyboardLayout,
            virtualKeyCode: u16,
            keyAction: u16,
            modifierKeyState: u32,
            keyboardType: u32,
            keyTranslateOptions: u32,
            deadKeyState: *mut u32,
            maxStringLength: u32,
            actualStringLength: *mut u32,
            unicodeString: *mut u16,
        ) -> i32;
    }

    unsafe {
        let layout = TISCopyCurrentKeyboardLayoutInputSource();
        if layout.is_null() {
            anyhow::bail!("TISCopyCurrentKeyboardLayoutInputSource returned null");
        }

        let prop_key = CFString::new("TISPropertyUnicodeKeyLayoutData");
        let layout_data = TISGetInputSourceProperty(layout, prop_key.as_concrete_TypeRef());

        if layout_data.is_null() {
            CFRelease(layout);
            anyhow::bail!("TISGetInputSourceProperty returned null");
        }

        let key_layout_ptr = CFDataGetBytePtr(layout_data) as *const UCKeyboardLayout;
        if key_layout_ptr.is_null() {
            CFRelease(layout);
            anyhow::bail!("CFDataGetBytePtr returned null");
        }

        let mut unicode_string = [0u16; 4];
        let mut unicode_string_length: u32 = 0;

        for keycode in 0x00u16..=0x7Fu16 {
            let mut dead_key_state: u32 = 0;

            let status = UCKeyTranslate(
                key_layout_ptr,
                keycode,
                kUCKeyActionDown,
                0,
                0,
                kUCKeyTranslateNoDeadKeysMask,
                &mut dead_key_state,
                4,
                &mut unicode_string_length,
                unicode_string.as_mut_ptr(),
            );

            if status == 0
                && unicode_string_length == 1
                && unicode_string[0] == u16::from(b'v')
            {
                CFRelease(layout);
                return Ok(keycode);
            }
        }

        CFRelease(layout);
        Ok(DEFAULT_V_KEYCODE)
    }
}
