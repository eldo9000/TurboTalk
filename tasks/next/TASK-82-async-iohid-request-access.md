# TASK-82: Async IOHIDRequestAccess dispatch

## Goal
Move the synchronous `IOHIDRequestAccess` / `CGRequestListenEventAccess` TCC calls in `request_input_monitoring_permission` off the Tauri command thread by spawning them in a background thread — matching the pattern already used by `request_microphone_permission`.

## Context
Scroll Reverser dispatches `IOHIDRequestAccess` on a background queue (`dispatch_async(dispatch_get_global_queue(...))`) so the TCC permission prompt dialog doesn't block the main thread. TurboTalk currently calls both `CGRequestListenEventAccess()` and `IOHIDRequestAccess()` synchronously inside the Tauri command `request_input_monitoring_permission` at `src-tauri/src/permissions.rs:285-291`.

These are FFI calls into Apple frameworks. The TCC dialog appears asynchronously, but the actual FFI call might block briefly while TCC checks its database and prepares to show the dialog. Running it on the command thread means the frontend sees a spinner for the duration.

The fix: make `request_input_monitoring_permission` async, spawn the TCC calls in `std::thread::spawn` (same pattern as `request_microphone_permission` at `permissions.rs:226-250`), and return the result via a oneshot channel. The frontend already `await`s this command (`Onboarding.svelte:143`), so making it async requires no frontend changes.

## In scope
- `src-tauri/src/permissions.rs` — `request_input_monitoring_permission` function (lines 269-298)
- Verify the frontend (`src/Onboarding.svelte`) already uses `await` — no changes needed
- `SESSION-STATUS.md`

## Out of scope
- Changing the permission logic itself (still calls both CGRequestListenEventAccess + IOHIDRequestAccess as belt-and-suspenders)
- The `request_microphone_permission` function (already async)
- Any frontend UI changes

## Steps
1. Read `src-tauri/src/permissions.rs:213-259` to understand the async + thread-spawn + oneshot pattern used by `request_microphone_permission`.
2. Convert `request_input_monitoring_permission` from sync `fn` to `async fn`. Keep the `#[tauri::command]` and `#[specta::specta]` attributes.
3. Inside the async body, on macOS: spawn `std::thread::spawn(move || { ... })` that:
   - Links `CoreGraphics` and `IOKit` frameworks inside the thread closure (the `extern "C"` blocks need to stay inside the `#[cfg(target_os = "macos")]` block)
   - Calls `CGRequestListenEventAccess()` and `IOHIDRequestAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT)` inside `unsafe {}`
   - Signals a `tokio::sync::oneshot::channel::<()>` sender
4. After spawning, `await` the oneshot receiver with a 5-second timeout (shorter than the mic's 30s — the IM prompt is a simple TCC dialog, not a full AVFoundation capture request).
5. After the thread completes (or times out), call `input_monitoring_status()` and return the result.
6. Update the function doc comment to note it's now async.
7. Run `cargo check --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml -- -W clippy::all`.
8. Run `npm run typecheck` — the frontend bindings will regenerate, but since the command signature (return type `PermissionStatus`) is unchanged, no frontend code needs modification.
9. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass with no new warnings.
- `request_input_monitoring_permission` is `async fn` and spawns TCC calls in a background thread.
- The frontend's `await commands.requestInputMonitoringPermission()` continues to work without changes.
- No warning is emitted for the unused `() ` oneshot result — handle it with `let _ = rx.await;` or similar.

## Notes
- The `extern "C"` blocks for `CGRequestListenEventAccess` and `IOHIDRequestAccess` can stay inside the `#[cfg(target_os = "macos")]` block at the top of the function body, OR move inside the spawned thread. Either way works; prefer keeping them at the function level for readability, and only the actual `unsafe { ... }` call site goes inside the thread.
- The oneshot channel pattern requires `use tokio::sync::oneshot;` at the top of the file (it's already imported in the `request_microphone_permission` function's inner scope — you may need to add it to the file-level imports or keep it scoped).
- Do NOT use `block2::RcBlock` — that's for AVFoundation's block-based API. The IM TCC calls are plain C functions, no blocks needed.
- The `input_monitoring_status()` call after the thread completes doesn't need to be inside the thread — it's just reading the TCC database, which is fast.
