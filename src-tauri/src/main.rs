// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Single-instance guard: exit silently if another TurboTalk is running.
    // Uses a named mutex (Windows) or a PID file (macOS/Linux).
    #[cfg(target_os = "windows")]
    {
        use std::ffi::CString;
        let name = CString::new("TurboTalk-SingleInstance-Mutex").unwrap();
        let handle = unsafe {
            winapi::um::synchapi::CreateMutexA(
                std::ptr::null_mut(),
                1i32,
                name.as_ptr(),
            )
        };
        if handle.is_null() {
            eprintln!("[main] CreateMutexA failed — exiting");
            return;
        }
        if unsafe { winapi::um::errhandlingapi::GetLastError() } == 183u32 {
            eprintln!("[main] Another instance is already running — exiting");
            unsafe { let _ = winapi::um::handleapi::CloseHandle(handle); }
            return;
        }
        // Handle stays open for the process lifetime to hold the mutex.
    }

    // Panic hook: if any thread panics and the default handler runs (which
    // eventually calls abort()), we want to see the location + backtrace on
    // stderr before the process dies.  Without this, panics in background
    // threads (e.g. the tracing-appender NonBlocking writer) vanish without
    // a trace — no .ips, no log line.
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<non-string payload>");
        eprintln!("=== TURBOTALK PANIC ({loc}) ===");
        eprintln!("{payload}");
        // Best-effort backtrace. On nightly `Backtrace::force_capture()`
        // would be preferred, but stable `Backtrace::capture()` already
        // reports enough for the handful of frames that matter.
        let bt = std::backtrace::Backtrace::capture();
        if bt.status() == std::backtrace::BacktraceStatus::Captured {
            eprintln!("backtrace:\n{bt}");
        } else {
            eprintln!("(backtrace unavailable)");
        }
        eprintln!("=== END TURBOTALK PANIC ===");
    }));

    turbotalk_lib::run();
}
