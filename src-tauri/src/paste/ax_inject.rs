use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use std::ptr;

const AX_SUCCESS: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
}

pub fn try_inject(text: &str) -> anyhow::Result<Option<()>> {
    let focused = unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            anyhow::bail!("AXUIElementCreateSystemWide returned null");
        }

        let attr_focused = CFString::new("AXFocusedUIElement");
        let mut focused: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(
            system,
            attr_focused.as_concrete_TypeRef(),
            &mut focused,
        );
        CFRelease(system);

        if err != AX_SUCCESS || focused.is_null() {
            return Ok(None);
        }

        focused
    };

    let _log_role = role_string(focused);

    let cf_text = CFString::new(text);

    let attr_selected = CFString::new("AXSelectedText");
    let err_selected = unsafe {
        AXUIElementSetAttributeValue(
            focused,
            attr_selected.as_concrete_TypeRef(),
            cf_text.as_CFTypeRef(),
        )
    };

    if err_selected == AX_SUCCESS {
        tracing::info!(
            "[ax_inject] injected {} chars via AXSelectedText",
            text.len()
        );
        unsafe {
            CFRelease(focused);
        }
        return Ok(Some(()));
    }

    tracing::debug!(
        "[ax_inject] AXSelectedText failed (err={}), trying AXValue",
        err_selected
    );

    let attr_value = CFString::new("AXValue");
    let err_value = unsafe {
        AXUIElementSetAttributeValue(
            focused,
            attr_value.as_concrete_TypeRef(),
            cf_text.as_CFTypeRef(),
        )
    };

    if err_value == AX_SUCCESS {
        tracing::info!(
            "[ax_inject] injected {} chars via AXValue",
            text.len()
        );
        unsafe {
            CFRelease(focused);
        }
        return Ok(Some(()));
    }

    tracing::debug!(
        "[ax_inject] AXValue also failed (err={}), element does not support AX text injection",
        err_value
    );
    unsafe {
        CFRelease(focused);
    }
    Ok(None)
}

fn role_string(element: CFTypeRef) -> Option<String> {
    unsafe {
        let attr_role = CFString::new("AXRole");
        let mut role: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(
            element,
            attr_role.as_concrete_TypeRef(),
            &mut role,
        );
        if err == AX_SUCCESS && !role.is_null() {
            let role_cf = CFString::wrap_under_create_rule(role as CFStringRef);
            let s = role_cf.to_string();
            tracing::debug!("[ax_inject] focused element role: {}", s);
            Some(s)
        } else {
            if !role.is_null() {
                CFRelease(role);
            }
            None
        }
    }
}
