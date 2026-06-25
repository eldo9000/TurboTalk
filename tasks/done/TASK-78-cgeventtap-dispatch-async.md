# TASK-78: CGEventTap cleanup — dispatch_async + tap-disable event handling

## Goal
Move real work out of the CGEventTap callback into a `dispatch_async` serial queue, and handle tap-disable events (`TapDisabledByTimeout` / `TapDisabledByUserInput`) directly in the callback instead of polling every 8 seconds.

## Context
The macOS CGEventTap callback in `src-tauri/src/hotkey.rs:1949-2100` does real work inline: RwLock read on `hotkey_state` (`:1973`), `key_for_name` (`:1974`), `fkey_code_for_name` (`:1975`), `hid_mouse_usage_for_name` (`:1983`), and the F-key/modifier dispatch ladder (`:2020-2096`). The documented pattern for CGEventTap callbacks is to do **zero work** beyond posting to a `dispatch_async` serial queue. macOS kills event taps that take too long — the watchdog at `:2142-2172` exists precisely because the tap callback is too slow.

Additionally, the tap-disable watchdog polls `CGEventTapIsEnabled` every 8 seconds (`:2153-2172`). macOS **sends** `kCGEventTapDisabledByTimeout` and `kCGEventTapDisabledByUserInput` as actual event types to the tap's run loop source. The documented way to observe a tap disable is to handle those event types in the tap callback — instant re-enable, no 8-second blind spot.

The per-PTT-event thread spawning (`ptt_down`, `ptt_up` at `:477, :884`) is also a concern but is partially addressed by the hotkey controller refactor (TASK-68). This task focuses on the event tap callback itself.

## In scope
- `src-tauri/src/hotkey.rs:1949-2100` — the CGEventTap callback
- `src-tauri/src/hotkey.rs:2142-2172` — the 8-second polling watchdog
- `src-tauri/src/hotkey.rs:1953-1957` — the event types registered in the tap (may need to add `TapDisabledByTimeout` and `TapDisabledByUserInput`)
- `SESSION-STATUS.md`

## Out of scope
- The hotkey state-machine refactor (TASK-68 handles the shared orchestration; this task only touches the event tap callback shape)
- The IOHIDManager path (that's a separate input source and doesn't use CGEventTap)
- The Windows hotkey (TASK-71)
- The per-press thread spawning (if TASK-68 refactors `ptt_down`/`ptt_up` to use dispatch_async, that naturally addresses it; if TASK-68 hasn't landed yet, this task should still move the tap callback's inline work to dispatch_async)

## Steps
1. Read `src-tauri/src/hotkey.rs:1949-2100` (the CGEventTap callback) to understand what work it does inline: config reads, key matching, F-key dispatch, modifier dispatch.
2. Read `src-tauri/src/hotkey.rs:2142-2172` (the polling watchdog) to understand the re-enable logic.
3. Read `src-tauri/src/hotkey.rs:1953-1957` (the tap event mask registration) to see which event types are currently registered.
4. In the tap event mask, add `CGEventType::TapDisabledByTimeout` and `CGEventType::TapDisabledByUserInput` to the `vec![]` of event types. These arrive as callback events when macOS disables the tap.
5. In the tap callback, add a branch for these two event types: when received, immediately call `CGEventTapEnable(tap, true)` to re-enable the tap, then return `None` (don't modify the event). This replaces the 8-second polling watchdog.
6. For the main callback body (the key-matching + dispatch work): move it into a `dispatch_async` block on a serial queue. Create a serial `dispatch_queue_t` at initialization (e.g. `dispatch_queue_create("turbotalk.ptt", DISPATCH_QUEUE_SERIAL)` with `QOS_CLASS_USER_INITIATED`). The callback should capture the event data (keycode, flags, event type) into a small struct, post it to the dispatch queue, and return immediately.
7. The dispatch queue block does the config reads, key matching, and calls `ptt_down()` / `ptt_up()` / `trigger_cancel()` as appropriate. This is where the real work happens, off the tap callback's timeout budget.
8. Remove the 8-second polling watchdog thread (`:2142-2172`). It's no longer needed — the tap-disable events are handled inline.
9. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy`.
10. Run `npm run typecheck`.
11. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- The CGEventTap callback body is minimal: capture event data, `dispatch_async`, return. No RwLock reads, no string matching, no dispatch ladders in the callback.
- `TapDisabledByTimeout` and `TapDisabledByUserInput` are handled in the callback (re-enable + return).
- The 8-second polling watchdog is removed.
- PTT events still fire correctly (hold Right Option → recording starts, release → stops).
- The tap no longer gets disabled by timeout (because the callback is now near-zero work).

## Notes
- `dispatch_queue_create` and `dispatch_async` are available via `core-foundation` or via raw `libc` FFI. The codebase already uses `libc` for `pthread_set_qos_class_self_np` (`hotkey.rs:423-432`). The same FFI approach works for `dispatch_queue_create` / `dispatch_async`. Alternatively, `objc2` exposes some dispatch APIs.
- The serial queue ensures that PTT-down and PTT-up for the same press are processed in order (serial = no concurrent execution). This naturally serializes what the current per-press `thread::spawn` does not.
- `QOS_CLASS_USER_INITIATED` (0x19) is the correct QoS for this work — it's below user-interactive but above utility. Matches the existing `boost_thread()` QoS at `:423-432`.
- The event data captured in the callback should be small: keycode (u16), flags (u64), event type (enum). Copy these into a stack struct, move into the dispatch block. No heap allocation in the callback.
- If the dispatch queue approach conflicts with TASK-68's controller refactor, coordinate: TASK-68 may want the dispatch queue to be the controller's input channel. Either approach is fine — the key change is getting work out of the tap callback.
- The `TapDisabledByUserInput` event fires when the user presses Ctrl+Esc (the system's "kill event tap" shortcut). Re-enabling on this event is correct — it's a transient disable, not a permanent one.
