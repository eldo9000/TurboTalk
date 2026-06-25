# Arc Log — TASK-76: Wire native NSPasteboard module + clipboard changeCount guard

## Gate
Replace the pbcopy/pbpaste clipboard path (no changeCount guard) with the native
NSPasteboard module (full format snapshot/restore with changeCount guard) by
dispatching NSPasteboard calls to the main thread. Add GetClipboardSequenceNumber
guard on Windows.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** The pbcopy/pbpaste-based `clipboard::restore_if_untouched` always
  restores unconditionally — no changeCount check. If the user copies during the
  200ms paste window, their clipboard is silently clobbered. The native NSPasteboard
  module at `clipboard.rs::native` has a correct changeCount guard but is never used.
- **PREMISE:** Dispatching the native NSPasteboard calls to the main thread via
  `app.run_on_main_thread()` with a channel-based synchronous wait will give us the
  correct changeCount guard with a pbcopy/pbpaste fallback on dispatch failure.
- **DERIVATION:** `AppHandle::run_on_main_thread()` exists in Tauri 2 and schedules
  a closure on the main event loop. A oneshot channel bridges the async dispatch
  back to the synchronous background thread. Windows `GetClipboardSequenceNumber`
  is safe to call from any thread.
- **FALSIFICATION:** If `cargo check` fails at any macOS call site (type mismatch
  with native module), or if the Windows `GetClipboardSequenceNumber` symbol doesn't
  resolve, the premise is false.
- **FALSIF-RESULT:** `cargo check` + `cargo clippy` clean. macOS native NSPasteboard wired via `dispatch_native` helper with main-thread sync. Windows `GetClipboardSequenceNumber` guard added.
- **DISPOSITION:** CONFIRMED — dispatch 1 green. Commit 3e4075d.
