# TASK-1: CGEventTap re-enable after macOS timeout

## Goal
When macOS disables the event tap (timeout or user input), the hotkey system automatically re-enables it instead of silently dying.

## Context
TurboTalk uses a CGEventTap (`hotkey.rs` lines ~1263–1417) to listen for global hotkey events. macOS will disable an event tap that's slow to respond — this happens under system load, when the app is paused by a debugger, or during heavy swap. When disabled, macOS delivers `kCGEventTapDisabledByTimeout` (0x1) or `kCGEventTapDisabledByUserInput` (0x2) event types to the callback.

Currently the callback only handles `FlagsChanged`, `KeyDown`, and `KeyUp` event types. The disabled-event types are silently ignored. Once the tap is disabled, dictation stops working permanently — only an app restart fixes it. This is a silent total failure.

The fix must allow the closure to re-enable the tap. Two approaches:
- Store the `CFMachPort` handle in a static/Arc that the callback can access and call `CGEventTapEnable` on it.
- Add a periodic watchdog thread that polls `CGEventTapIsEnabled` and re-enables if needed (simpler, less invasive).

Either approach is acceptable. The watchdog approach is preferred since it doesn't require restructuring the callback closure.

## In scope
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/hotkey.rs`

## Out of scope
- Any other files
- The HID listener path (separate from the CGEventTap)
- Win32 hotkey code (`hotkey_win32.rs`)
- Threading model changes beyond adding a watchdog

## Steps
1. Read the full `hotkey.rs` file to understand the event tap setup and callback structure.
2. Locate the CGEventTap creation site (the `CGEventTapCreate` call or similar) and the callback closure.
3. Identify where the `CFMachPort` or tap reference is currently stored.
4. Implement the fix using one of these approaches:
   a. **Preferred — watchdog thread**: In `start()` or wherever the tap is created, spawn a thread that periodically (every 5–10 seconds) calls `CGEventTapIsEnabled` on the tap and calls `CGEventTapEnable` if it's disabled. The thread should sleep when idle and terminate cleanly on app shutdown.
   b. **Alternative — callback re-enable**: Make the tap handle accessible to the callback closure (e.g., Arc<CFMachPort>) and in the callback, match on the disabled event types and call `CGEventTapEnable(tap, true)`.
5. Ensure the watchdog thread is properly joined/stopped when the tap is torn down (on app exit or hotkey stop).
6. Run `cargo check` in the `src-tauri` directory to verify compilation.

## Success signal
`cargo check` passes with no warnings. The code includes a mechanism (watchdog or callback) that detects a disabled CGEventTap and re-enables it.

## Notes
- The disabled event types are `kCGEventTapDisabledByTimeout = 0x1` and `kCGEventTapDisabledByUserInput = 0x2` (the raw `CGEventType` values).
- If using the watchdog approach, the thread needs access to the tap handle. Store it in an `Arc<Mutex<Option<CFMachPort>>>` or similar.
- This is a classic macOS gap — every event-tap app (Karabiner, Rectangle) handles this. The fix is cheap and prevents silent total failure.
