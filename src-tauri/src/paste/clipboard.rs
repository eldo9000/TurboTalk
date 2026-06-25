// macOS clipboard module.
//
// Paste() is called from a background thread (hotkey.rs:ptt_up spawns one).
// NSPasteboard is documented as main-thread-only, but arboard calls it
// directly from whichever thread the caller is on.  On macOS 26 this
// throws an uncaught NSInternalInconsistencyException → SIGTRAP.
//
// We avoid NSPasteboard entirely by using /usr/bin/pbcopy and
// /usr/bin/pbpaste subprocesses, which are safe from any thread.
// Subprocess overhead (~2 ms) is negligible in the dictation cycle.
//
// The native NSPasteboard code in `native` submodule is kept for the
// hypothetical future where we run the entire paste flow on the main
// thread and want full format snapshot/restore.

use anyhow::Result;
use std::process::{Command, Stdio};
use std::io::Write;

/// Snapshot of the clipboard contents at paste time.
/// On background threads (the common case), this only captures plain text.
pub struct PasteboardSnapshot {
    prior_text: Option<String>,
}

impl PasteboardSnapshot {
    pub fn empty() -> Self {
        Self { prior_text: None }
    }
}

/// Read the current clipboard text via pbpaste.
fn read_clipboard() -> Option<String> {
    let output = Command::new("/usr/bin/pbpaste").output().ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        if text.is_empty() { None } else { Some(text) }
    } else {
        None
    }
}

/// Write text to the clipboard via pbcopy.
fn write_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("/usr/bin/pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("pbcopy spawn failed: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("pbcopy stdin not available"))?
        .write_all(text.as_bytes())
        .map_err(|e| anyhow::anyhow!("pbcopy write failed: {e}"))?;
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("pbcopy wait failed: {e}"))?;
    if !status.success() {
        anyhow::bail!("pbcopy failed with status {status}");
    }
    Ok(())
}

/// Save the current pasteboard contents.
pub fn snapshot() -> Result<PasteboardSnapshot> {
    let prior = read_clipboard();
    Ok(PasteboardSnapshot { prior_text: prior })
}

/// Clear the pasteboard and write plain text.
pub fn write_text(text: &str) -> Result<()> {
    write_clipboard(text)
}

/// Restore the prior clipboard contents.
pub fn restore_if_untouched(snapshot: &PasteboardSnapshot) -> Result<bool> {
    if let Some(prior) = &snapshot.prior_text {
        write_clipboard(prior)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Native NSPasteboard implementation for main-thread use only.
/// Provides full format snapshot/restore with changeCount guard.
#[cfg(target_os = "macos")]
pub mod native {
    use super::*;
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use std::ffi::CString;
    use std::os::raw::{c_char, c_void};

    #[derive(Clone)]
    pub struct PasteboardSnapshot {
        pub change_count: i64,
        pub items: Vec<PasteboardItem>,
    }

    impl PasteboardSnapshot {
        pub fn empty() -> Self {
            Self { change_count: -1, items: Vec::new() }
        }
    }

    #[derive(Clone)]
    pub struct PasteboardItem {
        pub types: Vec<String>,
        pub data: Vec<Vec<u8>>,
    }

    fn ns_string(s: &str) -> *mut AnyObject {
        let c_str = CString::new(s).expect("ns_string: null byte in string");
        unsafe { msg_send![objc2::class!(NSString), stringWithUTF8String: c_str.as_ptr()] }
    }

    fn nsstring_to_string(ns: *mut AnyObject) -> String {
        unsafe {
            let utf8: *const c_char = msg_send![ns, UTF8String];
            if utf8.is_null() { return String::new(); }
            std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
        }
    }

    fn nsdata_from_bytes(bytes: &[u8]) -> *mut AnyObject {
        unsafe {
            msg_send![objc2::class!(NSData), dataWithBytes: bytes.as_ptr() as *const c_void, length: bytes.len()]
        }
    }

    fn nsdata_to_vec(data: *mut AnyObject) -> Vec<u8> {
        unsafe {
            let ptr: *const c_void = msg_send![data, bytes];
            let len: usize = msg_send![data, length];
            if ptr.is_null() || len == 0 { return Vec::new(); }
            std::slice::from_raw_parts(ptr as *const u8, len).to_vec()
        }
    }

    pub fn snapshot() -> Result<PasteboardSnapshot> {
        unsafe {
            let pb: *mut AnyObject = msg_send![objc2::class!(NSPasteboard), generalPasteboard];
            if pb.is_null() { anyhow::bail!("NSPasteboard returned null"); }
            let change_count: i64 = msg_send![pb, changeCount];

            let items_obj: *mut AnyObject = msg_send![pb, pasteboardItems];
            let items = if items_obj.is_null() { Vec::new() } else {
                let count: usize = msg_send![items_obj, count];
                let mut items = Vec::with_capacity(count);
                for i in 0..count {
                    let item: *mut AnyObject = msg_send![items_obj, objectAtIndex: i];
                    let types_array: *mut AnyObject = msg_send![item, types];
                    let type_count: usize = msg_send![types_array, count];
                    let mut types = Vec::with_capacity(type_count);
                    let mut data = Vec::with_capacity(type_count);
                    for j in 0..type_count {
                        let uti: *mut AnyObject = msg_send![types_array, objectAtIndex: j];
                        let type_name = nsstring_to_string(uti);
                        let data_obj: *mut AnyObject = msg_send![item, dataForType: uti];
                        let bytes = if data_obj.is_null() { Vec::new() } else { nsdata_to_vec(data_obj) };
                        types.push(type_name);
                        data.push(bytes);
                    }
                    items.push(PasteboardItem { types, data });
                }
                items
            };
            Ok(PasteboardSnapshot { change_count, items })
        }
    }

    pub fn write_text(text: &str) -> Result<()> {
        unsafe {
            let pb: *mut AnyObject = msg_send![objc2::class!(NSPasteboard), generalPasteboard];
            if pb.is_null() { anyhow::bail!("NSPasteboard returned null"); }
            let () = msg_send![pb, clearContents];
            let ns_text = ns_string(text);
            let ns_type = ns_string("public.utf8-plain-text");
            let ok: i8 = msg_send![pb, setString: ns_text, forType: ns_type];
            if ok == 0 { anyhow::bail!("setString:forType: returned NO"); }
            Ok(())
        }
    }

    pub fn restore_if_untouched(snapshot: &PasteboardSnapshot) -> Result<bool> {
        unsafe {
            let pb: *mut AnyObject = msg_send![objc2::class!(NSPasteboard), generalPasteboard];
            if pb.is_null() { anyhow::bail!("NSPasteboard returned null"); }
            let current: i64 = msg_send![pb, changeCount];
            if current != snapshot.change_count { return Ok(false); }
            let () = msg_send![pb, clearContents];
            if snapshot.items.is_empty() { return Ok(true); }
            let arr: *mut AnyObject = msg_send![objc2::class!(NSMutableArray), array];
            for item in &snapshot.items {
                let ns_item: *mut AnyObject = msg_send![objc2::class!(NSPasteboardItem), alloc];
                let ns_item: *mut AnyObject = msg_send![ns_item, init];
                let () = msg_send![ns_item, autorelease];
                for (type_name, bytes) in item.types.iter().zip(item.data.iter()) {
                    let ns_type = ns_string(type_name);
                    let ns_data = nsdata_from_bytes(bytes);
                    let () = msg_send![ns_item, setData: ns_data, forType: ns_type];
                }
                let () = msg_send![arr, addObject: ns_item];
            }
            let ok: i8 = msg_send![pb, writeObjects: arr];
            if ok == 0 { anyhow::bail!("writeObjects: returned NO"); }
            Ok(true)
        }
    }
}
