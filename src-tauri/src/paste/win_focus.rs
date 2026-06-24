use winapi::shared::minwindef::TRUE;
use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::winuser::{
    AttachThreadInput, BringWindowToTop, GetForegroundWindow, GetLastError,
    GetWindowThreadProcessId, SetFocus, SetForegroundWindow,
};

/// Capture the currently foreground window's HWND.
/// Returns 0 if no window is foreground.
pub fn foreground_hwnd() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

/// Get the PID of the process owning the foreground window.
pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
}

/// Bring a foreground window to the front reliably.
/// Uses the AttachThreadInput pattern so SetForegroundWindow actually works.
/// hwnd: the handle returned by foreground_hwnd() or GetForegroundWindow().
pub fn activate_hwnd(hwnd: isize) -> anyhow::Result<()> {
    unsafe {
        let fore_hwnd = GetForegroundWindow();
        let fore_thread = GetWindowThreadProcessId(fore_hwnd, std::ptr::null_mut());
        let app_thread = GetCurrentThreadId();

        if fore_thread != app_thread {
            AttachThreadInput(fore_thread, app_thread, TRUE);
            let ok = SetForegroundWindow(hwnd as _);
            if ok == 0 {
                tracing::warn!(
                    "[win_focus] SetForegroundWindow failed (hwnd={}, fore_thread={}, app_thread={}, err={})",
                    hwnd,
                    fore_thread,
                    app_thread,
                    GetLastError(),
                );
            }
            BringWindowToTop(hwnd as _);
            SetFocus(hwnd as _);
            AttachThreadInput(fore_thread, app_thread, 0);
        } else {
            let ok = SetForegroundWindow(hwnd as _);
            if ok == 0 {
                tracing::warn!(
                    "[win_focus] SetForegroundWindow failed (same thread, hwnd={}, err={})",
                    hwnd,
                    GetLastError(),
                );
            }
            BringWindowToTop(hwnd as _);
        }
    }
    Ok(())
}
