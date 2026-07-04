# TASK-84: Runtime Input Monitoring permission loss detection

## Goal
Detect when Input Monitoring permission is revoked at runtime (user toggles it off in System Settings while TurboTalk is running) and surface a UI error toast so the user knows why the hotkey stopped working.

## Context
Scroll Reverser KVO-observes `hasAllRequiredPermissions` and auto-disables itself when permissions are revoked mid-session. They call `refreshPermissions` on every event tap callback event. TurboTalk currently has **no runtime detection** — if the user revokes Input Monitoring while TurboTalk is running, the IOHIDManager stops delivering events silently. The user presses the hotkey and nothing happens, with no explanation.

The challenge with IOHIDManager (vs CGEventTap): when TCC revokes Input Monitoring, the IOHIDManager **stops delivering callbacks entirely**. The `CFRunLoopRun()` keeps running but no events arrive. So we can't detect the problem in the callback — we need a watchdog outside the callback.

### Design

**Watchdog thread** that runs alongside the IOHID listener thread:
- Sleeps 30 seconds (configurable)
- Wakes and calls `IOHIDCheckAccess` (the same function used by `input_monitoring_status()` in `permissions.rs:150`)
- Compares current state to `IOHID_LISTENER_RUNNING` atomic:
  - If **denied now but was running**: permission was revoked → emit `ui-error` toast, set `IOHID_LISTENER_RUNNING` to false
  - If **granted now but was not running**: permission re-granted → log info, set `IOHID_LISTENER_RUNNING` to true (the listener thread still exists and will resume receiving events)
  - If state unchanged: no action
- The watchdog terminates when a shutdown signal is set (or just loops forever — same lifetime as the IOHID listener)

### Why this follows the Scroll Reverser pattern
Scroll Reverser uses event-driven checks (on scroll events). For CGEventTap, the callback keeps firing even when accessibility is revoked (the tap just stops modifying events). So they can check in-callback. For IOHIDManager, revocation kills callbacks entirely — we need an out-of-band check. A 30-second interval watchdog is the minimal sufficient approach.

## In scope
- `src-tauri/src/hotkey.rs` — add the watchdog thread in `spawn_hid_mouse_listener` (or as a sibling function), emit `ui-error` on loss
- `src-tauri/src/lib.rs` — verify the `ui-error` event listener is set up to handle `user-permission-lost` kind
- `src/App.svelte` or `src/Toast.svelte` — verify the `ui-error` toast rendering supports the new event kind
- `SESSION-STATUS.md`

## Out of scope
- Auto-re-registering IOHIDManager when permission is re-granted (the existing listener thread is still alive — `CFRunLoopRun` doesn't exit when TCC revokes, it just stops delivering events. When TCC re-grants, events resume automatically.)
- Detection for Accessibility permission loss (the CGEventTap path already has `TapDisabledByTimeout` handling)
- Detection for Microphone permission loss (cpal handles this at stream-open time)
- A "re-check permissions" button in the UI (the user would need to re-open System Settings)

## Steps

### Backend (Rust)
1. In `hotkey.rs`, find the `spawn_hid_mouse_listener` function (~line 1946). Observe that it already has access to `AppHandle` through `HidMouseCtx`.
2. Add a helper function `fn spawn_im_watchdog(app: AppHandle)` that:
   - Links `IOKit` framework (same `extern "C" { fn IOHIDCheckAccess(...) -> u32; }` block as `permissions.rs`)
   - Spawns `std::thread::spawn(move || { loop { ... } })`
   - Inside the loop:
     - `std::thread::sleep(Duration::from_secs(30))`
     - Call `IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT)` via unsafe FFI
     - If status == Denied and `IOHID_LISTENER_RUNNING.load(Acquire)` is true:
       - `IOHID_LISTENER_RUNNING.store(false, Release)`
       - Use `app.emit("ui-error", ...)` to send a toast — match the existing pattern from `lib.rs` or `hotkey.rs` (search for `app.emit("ui-error"` in the codebase)
       - Log a warning
     - If status == Granted and `IOHID_LISTENER_RUNNING.load(Acquire)` is false:
       - `IOHID_LISTENER_RUNNING.store(true, Release)`
       - Log info that permission was re-granted
3. Call `spawn_im_watchdog(app.clone())` from the `spawn` function or from `lib.rs` where the IOHID listener is started.
4. The `ui-error` event payload should follow the existing format used elsewhere in the app. Search for existing `ui-error` emit patterns and use the same JSON shape. Suggested kind: `"user-permission-lost"` with `recoverable: true` and a message like `"Input Monitoring permission was revoked. Turbo Talk hotkeys are disabled. Re-enable in System Settings → Privacy & Security → Input Monitoring, then restart Turbo Talk."`

### Frontend (Svelte)
5. Check `src/App.svelte` and/or `src/Toast.svelte` for how `ui-error` events are rendered. Verify the `"user-permission-lost"` kind will display properly. The existing toast system uses `kind` to decide rendering — if a new kind isn't recognized, it may need a case added.
6. If needed, add a handler for `user-permission-lost` that shows a persistent toast (since the hotkey is now non-functional, the user needs to see this).
7. Verify that `check_readiness` still reports accurate state (the watchdog sets the atomic, and `permissions.rs` reads it via `iohid_listener_running()`).

### Verification
8. Run `cargo check --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml -- -W clippy::all`.
9. Run `npm run typecheck`.
10. Test manually: start TurboTalk, open System Settings → Privacy & Security → Input Monitoring, uncheck TurboTalk, wait up to 30 seconds, observe the error toast.
11. Test manually: re-check TurboTalk in Input Monitoring, wait up to 30 seconds, observe the toast disappear (or app returns to ready state).
12. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- `npm run typecheck` passes.
- A watchdog thread starts alongside the IOHID listener.
- Revoking Input Monitoring at runtime emits a `ui-error` event within 30 seconds.
- The `IOHID_LISTENER_RUNNING` atomic reflects the true permission state.
- Re-granting permission restores the ready state.
- The watchdog thread terminates cleanly on app exit (no cleanup needed — process exit handles it).

## Notes
- The `IOHIDCheckAccess` constants (`K_IOHID_REQUEST_TYPE_LISTEN_EVENT`, access type values) are defined in `permissions.rs:140-143`. You can either duplicate them in `hotkey.rs` (they're simple `const` values) or export them from `permissions.rs`.
- `app.emit("ui-error", payload)` returns `Result<(), tauri::Error>`. Log the error if emitting fails (unlikely but possible if the webview isn't ready).
- The 30-second interval is a tradeoff: too fast wastes CPU and `IOHIDCheckAccess` is an IOKit call (not free but cheap), too slow means the user waits too long for feedback. 30s matches typical macOS TCC polling intervals.
- Do NOT call `IOHIDCheckAccess` inside the HID callbacks themselves — the callback fires at HID rate (can be 1000+ Hz for gaming mice). Even a lightweight IOKit call at that rate is wasteful.
- The watchdog should NOT try to re-enable the IOHIDManager — if TCC revoked access, only the user can re-grant it. The watchdog detects and reports, nothing more.
- If `hid_keyboard_value_callback` is separate from `hid_mouse_value_callback`, the watchdog should be the same single thread — both callbacks are on the same IOHIDManager, so the permission state affects both.
