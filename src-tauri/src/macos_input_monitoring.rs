// macOS Input Monitoring TCC registration.
//
// Extracted from `lib.rs::run()` setup closure.  On macOS 26+ the bare
// "request access" APIs (IOHIDRequestAccess, CGRequestListenEventAccess) don't
// reliably register an ad-hoc signed bundle with TCC.  What does register it
// is actually attempting to open an IOHIDManager — the same path Karabiner,
// Logi Options+, etc. use to appear in the Input Monitoring list.
//
// This module creates the manager, schedules it on the main run loop, and
// calls Open.  Even when TCC blocks the open, the *attempt* is what causes
// the bundle to be added to Privacy & Security → Input Monitoring.  The
// manager is intentionally leaked so the registration persists for the
// process lifetime.

/// Register for Input Monitoring access by creating an IOHIDManager and
/// attempting to open it, which causes TCC to add the bundle to the Input
/// Monitoring list.  Safe to call on any platform — on non-macOS this is a
/// no-op.
pub fn register() {
    #[cfg(target_os = "macos")]
    {
        use std::os::raw::c_void;
        type CFAllocatorRef = *const c_void;
        type CFDictionaryRef = *const c_void;
        type CFRunLoopRef = *const c_void;
        type CFStringRef = *const c_void;
        type IOHIDManagerRef = *mut c_void;
        const K_IO_HID_OPTIONS_TYPE_NONE: u32 = 0;

        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            static kCFAllocatorDefault: CFAllocatorRef;
            static kCFRunLoopDefaultMode: CFStringRef;
            fn CFRunLoopGetMain() -> CFRunLoopRef;
        }
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGRequestListenEventAccess() -> bool;
        }
        #[link(name = "IOKit", kind = "framework")]
        extern "C" {
            fn IOHIDManagerCreate(
                allocator: CFAllocatorRef,
                options: u32,
            ) -> IOHIDManagerRef;
            fn IOHIDManagerSetDeviceMatching(
                manager: IOHIDManagerRef,
                matching: CFDictionaryRef,
            );
            fn IOHIDManagerScheduleWithRunLoop(
                manager: IOHIDManagerRef,
                runloop: CFRunLoopRef,
                mode: CFStringRef,
            );
            fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: u32) -> i32;
        }

        unsafe {
            // Belt-and-suspenders: the CG request also nudges TCC.
            CGRequestListenEventAccess();

            let manager =
                IOHIDManagerCreate(kCFAllocatorDefault, K_IO_HID_OPTIONS_TYPE_NONE);
            if !manager.is_null() {
                // NULL matching dict = match all devices, including keyboards.
                IOHIDManagerSetDeviceMatching(manager, std::ptr::null());
                IOHIDManagerScheduleWithRunLoop(
                    manager,
                    CFRunLoopGetMain(),
                    kCFRunLoopDefaultMode,
                );
                // The Open attempt is what causes TCC to add the
                // bundle to the Input Monitoring list. Return value
                // is ignored — we expect kIOReturnNotPermitted until
                // the user enables the toggle.
                let _ = IOHIDManagerOpen(manager, K_IO_HID_OPTIONS_TYPE_NONE);
                // Manager intentionally leaked: keeping it alive
                // preserves the TCC registration for this process.
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        // No-op on Windows/Linux.
    }
}
