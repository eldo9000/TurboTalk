# TASK-76: Wire native NSPasteboard module + clipboard changeCount guard

## Goal
Replace the arboard-based paste path (which lacks a clipboard change-count guard and can clobber the user's clipboard) with the already-written native NSPasteboard module that correctly checks `changeCount` before restoring. On Windows, add `GetClipboardSequenceNumber` guard to the Win32 clipboard restore path.

## Context
TurboTalk's paste flow works like this:
1. Snapshot the current clipboard content
2. Set the transcript text on the clipboard
3. Send Cmd+V (macOS) or Ctrl+V (Windows) to the focused app
4. Wait a fixed duration (200ms macOS, 100ms Windows)
5. Restore the old clipboard snapshot

The problem: step 5 unconditionally restores the old snapshot, even if the user copied something else during the 200ms paste window. This silently destroys the user's new clipboard content.

On macOS, a complete, correct native NSPasteboard module already exists at `src-tauri/src/paste/clipboard.rs:54-176`. It has:
- `snapshot()` — captures all pasteboard formats + `changeCount`
- `restore_if_untouched()` — checks `changeCount` before restoring; if the count changed (user copied something), it skips the restore
- `clearContents` / `writeObjects` — proper native pasteboard API

But this module is **never called**. The live paste path in `paste/mod.rs:87-100` uses `arboard` instead, which has no `changeCount` guard. The comment at `clipboard.rs:5-8` says it's "available for future main-thread code paths."

The reason the native module isn't used: it calls `NSPasteboard` APIs which must run on the main thread. The paste path runs on a background worker thread (the PTT-up worker). The fix is to dispatch the snapshot/write/restore calls to the main thread.

On Windows, `paste/win_clipboard.rs:129-176` has the same problem — `restore()` unconditionally clobbers the clipboard. Windows has `GetClipboardSequenceNumber()` which serves the same role as macOS `changeCount`.

Additionally, `paste/focus_capture.rs:86-93` spawns `osascript` to activate the target app on every paste from background threads. This should use `dispatch_async` to the main queue and call `NSRunningApplication` (AppKit is already linked). But that's a separate concern — this task focuses on the clipboard guard.

## In scope
- `src-tauri/src/paste/mod.rs` — the `paste()` entry point; dispatch clipboard operations to main thread
- `src-tauri/src/paste/clipboard.rs` — the native module (already written; needs to be wired in)
- `src-tauri/src/paste/win_clipboard.rs` — add `GetClipboardSequenceNumber` guard to `restore()`
- `SESSION-STATUS.md`

## Out of scope
- The `osascript` paste-activation subprocess (related but separate — that's about process spawning, not clipboard safety; could be a follow-up task)
- Changing the paste timing (the fixed `thread::sleep(200ms)` — that's a separate optimization)
- The legacy.rs dead code (separate cleanup)
- Frontend changes

## Steps
1. Read `src-tauri/src/paste/mod.rs` completely to understand the current paste flow: it snapshots via arboard, sets text via arboard, sends Cmd+V, sleeps 200ms, restores via arboard.
2. Read `src-tauri/src/paste/clipboard.rs:54-176` (the native module) to understand the `snapshot()` / `restore_if_untouched()` API. Verify it's complete and correct.
3. In `paste/mod.rs`, for the macOS path:
   - Replace the arboard snapshot with a dispatch to the main thread that calls `native::snapshot()`.
   - Replace the arboard text-set with a dispatch to the main thread that calls the native pasteboard write.
   - After the Cmd+V + sleep, dispatch to the main thread to call `native::restore_if_untouched(snapshot)`.
4. For the dispatch mechanism: use `tauri::async_runtime::spawn_blocking` to get onto a background thread, then use a channel or `dispatch_async` to hop to the main run loop. Alternatively, Tauri 2's `app.run_on_main_thread()` may be available — check the API. The key constraint is that `NSPasteboard` calls MUST be on the main thread.
5. Keep the `arboard` path as a fallback if the native dispatch fails (e.g. main thread is blocked). The arboard path works; it just lacks the changeCount guard. A degraded paste that clobbers the clipboard is better than no paste.
6. For Windows (`paste/win_clipboard.rs`): in `restore()`, call `GetClipboardSequenceNumber()` before restoring. Compare against the sequence number captured at snapshot time. If they differ, skip the restore (the user copied something during the paste window). `GetClipboardSequenceNumber` is in `winapi`'s `winuser` feature (already a dependency).
7. Capture the sequence number in the `ClipboardSnapshot` struct alongside the format data.
8. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy`.
9. Run `npm run typecheck`.
10. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- On macOS, `paste/mod.rs` calls `native::snapshot()` / `native::restore_if_untouched()` instead of the arboard path (or falls back to arboard only on dispatch failure).
- On Windows, `win_clipboard.rs::restore()` checks `GetClipboardSequenceNumber()` before restoring.
- Copying something during the paste window does NOT get clobbered by the restore.
- Normal paste still works (transcript is pasted, clipboard is restored to the pre-paste state when no copy happened during the window).

## Notes
- The main-thread dispatch is the tricky part. `objc2` and `core-foundation` both require the main thread (or at least a thread with a CFRunLoop). Tauri's main thread is the one running the app event loop. Check if `tauri::AppHandle` has a `run_on_main_thread()` or similar in Tauri 2.
- If `run_on_main_thread` is not available, the alternative is to use `dispatch_async_f` with a `dispatch_queue_t` for the main queue (`dispatch_get_main_queue()`). This requires `libc` or `core-foundation` bindings, both already dependencies.
- The `native` module at `clipboard.rs:54-176` is behind `#[cfg(target_os = "macos")]`. Verify it compiles and the API matches what `mod.rs` needs.
- The Windows `GetClipboardSequenceNumber` function is a simple `u32` return — no clipboard open/close needed. It's safe to call from any thread.
- Test scenario: copy a URL to clipboard → dictate a sentence → during the 200ms paste window, copy a different URL → after paste completes, the clipboard should contain the second URL (the one you copied), NOT the pre-dictation snapshot.
