use anyhow;
use winapi::shared::minwindef::UINT;
use winapi::shared::ntdef::HANDLE;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::winbase::{
    GlobalAlloc, GlobalFree, GlobalLock, GlobalSize, GlobalUnlock, GHND,
};
use winapi::um::winuser::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    OpenClipboard, SetClipboardData, CF_UNICODETEXT,
};

/// Full snapshot of every clipboard format at a point in time.
pub struct ClipboardSnapshot {
    pub formats: Vec<ClipboardFormat>,
}

pub struct ClipboardFormat {
    pub format: u32,
    pub data: Vec<u8>,
}

// ── Drop guard ────────────────────────────────────────────────────────────

/// Opens the clipboard on construction, closes it on drop.
struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> anyhow::Result<Self> {
        match unsafe { OpenClipboard(std::ptr::null_mut()) } {
            0 => anyhow::bail!("OpenClipboard failed: {}", unsafe { GetLastError() }),
            _ => Ok(Self),
        }
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

// ─── Public API ───────────────────────────────────────────────────────────

/// Save every format currently on the clipboard.
pub fn snapshot() -> anyhow::Result<ClipboardSnapshot> {
    let _guard = ClipboardGuard::open()?;

    let mut formats: Vec<ClipboardFormat> = Vec::new();
    let mut format: UINT = 0;

    loop {
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }

        let handle = unsafe { GetClipboardData(format) };
        if handle.is_null() {
            continue;
        }

        let size = unsafe { GlobalSize(handle as _) };
        if size == 0 {
            continue;
        }

        let ptr = unsafe { GlobalLock(handle as _) };
        if ptr.is_null() {
            continue;
        }

        let mut data = vec![0u8; size];
        unsafe {
            std::ptr::copy_nonoverlapping(ptr as *const u8, data.as_mut_ptr(), data.len());
            GlobalUnlock(handle as _);
        }

        formats.push(ClipboardFormat { format, data });
    }

    Ok(ClipboardSnapshot { formats })
}

/// Clear the clipboard and write `text` as CF_UNICODETEXT.
pub fn write_text(text: &str) -> anyhow::Result<()> {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let byte_size = (utf16.len() + 1) * std::mem::size_of::<u16>();

    let handle = unsafe { GlobalAlloc(GHND, byte_size) };
    if handle.is_null() {
        anyhow::bail!("GlobalAlloc failed: {}", unsafe { GetLastError() });
    }

    let ptr = unsafe { GlobalLock(handle) };
    if ptr.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        anyhow::bail!("GlobalLock failed: {}", unsafe { GetLastError() });
    }

    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr as *mut u16, utf16.len());
        *(ptr as *mut u16).add(utf16.len()) = 0;
        GlobalUnlock(handle);
    }

    let _guard = ClipboardGuard::open()?;

    unsafe {
        EmptyClipboard();
    }

    let ret = unsafe { SetClipboardData(CF_UNICODETEXT, handle as HANDLE) };
    if ret.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        anyhow::bail!("SetClipboardData failed: {}", unsafe { GetLastError() });
    }

    Ok(())
}

/// Restore a previously-saved clipboard snapshot.
pub fn restore(snapshot: &ClipboardSnapshot) -> anyhow::Result<()> {
    let _guard = ClipboardGuard::open()?;

    unsafe {
        EmptyClipboard();
    }

    for fmt in &snapshot.formats {
        let handle = unsafe { GlobalAlloc(GHND, fmt.data.len()) };
        if handle.is_null() {
            tracing::warn!(
                "[win_clipboard] GlobalAlloc failed for format {}",
                fmt.format
            );
            continue;
        }

        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            unsafe {
                GlobalFree(handle);
            }
            tracing::warn!(
                "[win_clipboard] GlobalLock failed for format {}",
                fmt.format
            );
            continue;
        }

        unsafe {
            std::ptr::copy_nonoverlapping(fmt.data.as_ptr(), ptr as *mut u8, fmt.data.len());
            GlobalUnlock(handle);
        }

        let ret = unsafe { SetClipboardData(fmt.format, handle as HANDLE) };
        if ret.is_null() {
            tracing::warn!(
                "[win_clipboard] SetClipboardData failed for format {}: {}",
                fmt.format,
                unsafe { GetLastError() }
            );
            unsafe {
                GlobalFree(handle);
            }
        }
    }

    Ok(())
}
