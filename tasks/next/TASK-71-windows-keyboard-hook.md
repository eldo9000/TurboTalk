# TASK-71: Replace Windows hotkey polling with WH_KEYBOARD_LL hook

## Goal
The Windows push-to-talk hotkey uses a documented-wrong pattern (125 Hz `GetAsyncKeyState` polling). Replace it with the correct `WH_KEYBOARD_LL` low-level keyboard hook running on a dedicated thread with a Windows message loop.

## Context
The file `src-tauri/src/hotkey_win32.rs` (224 lines) implements the entire Windows PTT path via `GetAsyncKeyState` polling at 8ms intervals. The header comment says `WH_KEYBOARD_LL` "often installs successfully but receives zero events in packaged Tauri builds" — but the documented cause of that symptom is **not installing a message loop on the hook's owning thread**. A `WH_KEYBOARD_LL` callback only fires while its owning thread runs `GetMessage`/`PeekMessage` in a loop. If the hook is installed on a thread that doesn't pump messages, it silently never receives events.

The current polling approach:
- Burns CPU forever (wakes 125 times/second even when idle)
- Can miss events when two presses land within the 8ms tick
- Cannot reliably detect modifier-key `up` events for certain combinations
- Requires per-tick `String` clone of the hotkey config (`hotkey_state` RwLock read at `:114`)
- Needs the `down`/`esc_down` `AtomicBool` dedup logic (`:142-148`) that a hook callback makes unnecessary

The macOS hotkey path (`hotkey.rs`) already uses the correct platform idiom (CGEventTap + IOHIDManager with proper run loops). The Windows path is the only module where the platform idiom was abandoned.

The current hotkey config reads from a `HotkeyState` struct (RwLock-protected) that holds the configured key name. The macOS path reads this in its event tap callback. The Windows path should read it once at hook registration time and re-read only on config-change events.

The app also has a separate `play_chime` function in `hotkey.rs:354-414` that spawns `powershell.exe` on Windows for sound effects — that is handled by a separate task (TASK-77), not this one.

## In scope
- `src-tauri/src/hotkey_win32.rs` — complete rewrite of the polling loop into a hook + message pump
- `src-tauri/Cargo.toml` — may need `winapi` features for `winuser` (already present: `winuser`, `winbase`, `synchapi`, `errhandlingapi`) — verify `SetWindowsHookExW`, `UnhookWindowsHookEx`, `GetMessageW`, `PeekMessageW`, `PostThreadMessageW` are available
- `SESSION-STATUS.md`

## Out of scope
- macOS hotkey changes (that path is already correct)
- Chime/sound implementation (TASK-77 handles that separately)
- The hotkey state-machine refactor (TASK-68 handles the shared `hotkey.rs` orchestration; this task only touches the Windows-specific `hotkey_win32.rs` input layer)
- Linux hotkey (rdev, deferred)
- Any changes to how PTT events are dispatched to the recorder/engine layer — the hook should call the same `ptt_down()` / `ptt_up()` entry points that the polling loop currently calls

## Steps
1. Read `src-tauri/src/hotkey_win32.rs` completely to understand the current polling contract: what functions the polling `tick()` calls on key-down and key-up, what state it reads, and how it signals the shared hotkey layer.
2. Read `src-tauri/src/hotkey.rs` to find the `ptt_down()` / `ptt_up()` / `trigger_cancel()` entry points that the Windows path must call — these are the shared contract.
3. Create a dedicated worker thread that:
   - Reads the configured hotkey key from `HotkeyState` once at startup
   - Calls `SetWindowsHookExW(WH_KEYBOARD_LL, callback, null, 0)` on that thread
   - Enters a `GetMessageW` loop that keeps the thread alive and pumping messages
   - The callback receives `WPARAM` (event type: `WM_KEYDOWN`/`WM_KEYUP`/`WM_SYSKEYDOWN`/`WM_SYSKEYUP`) and `LPARAM` (pointer to `KBDLLHOOKSTRUCT` containing `vkCode` and `flags`)
4. In the callback, match the `vkCode` against the configured hotkey virtual key code. On key-down, call the shared `ptt_down()` entry point. On key-up, call `ptt_up()`. Handle the Esc-cancel key the same way the current `tick()` does.
5. Remove the `GetAsyncKeyState` polling loop, the `POLL_CTX` global, the `down`/`esc_down` `AtomicBool` dedup, and the 8ms `sleep` loop entirely.
6. Add a clean shutdown path: `PostThreadMessageW(thread_id, WM_QUIT, 0, 0)` to break the `GetMessage` loop, then `UnhookWindowsHookEx` and join the thread. This should be called from the same cleanup path that currently stops the polling thread.
7. Handle config changes: if the user rebinds the hotkey key (currently the key is hardcoded Right Alt but the config struct exists), re-read the config. Either re-register the hook, or read the config inside the callback (cheaper than re-registering). The macOS path reads config in its callback, so matching that pattern is fine.
8. Verify the `winapi` crate features in `Cargo.toml` include everything needed. The existing features (`winuser`, `winbase`) should cover `SetWindowsHookExW`, `GetMessageW`, `UnhookWindowsHookEx`, `PostThreadMessageW`. Add `libloaderapi` if `GetModuleHandleW` is needed (it usually isn't for `WH_KEYBOARD_LL` — passing `null` for the module handle is valid).
9. Run `cargo check --manifest-path src-tauri/Cargo.toml` to verify it compiles. If on macOS, cross-compile check with `--target x86_64-pc-windows-gnu` if the toolchain is available; otherwise ensure the `#[cfg(target_os = "windows")]` gates are correct so macOS build still passes.
10. Update `SESSION-STATUS.md` noting the Windows hotkey path is now using the correct hook idiom.

## Success signal
- `cargo check --manifest-path src-tauri/Cargo.toml` passes (on macOS, the Windows code is behind `#[cfg(target_os = "windows")]` so it won't compile locally — verify the `cfg` gates are correct and the macOS build still passes).
- `hotkey_win32.rs` no longer contains `GetAsyncKeyState`, `thread::sleep` in a polling loop, or the `POLL_CTX` / `down` / `esc_down` bookkeeping.
- The hook thread installs `SetWindowsHookExW(WH_KEYBOARD_LL, ...)`, runs `GetMessageW` in a loop, and calls `ptt_down()` / `ptt_up()` on the correct key events.
- A clean shutdown path exists (`PostThreadMessageW` + `UnhookWindowsHookEx` + thread join).
- The shared `ptt_down()` / `ptt_up()` / `trigger_cancel()` contract is unchanged — the macOS path and the recorder layer are not modified.

## Notes
- The `KBDLLHOOKSTRUCT` has a `flags` field; bit 0 (`LLKHF_INJECTED`) and bit 4 (`LLKHF_UP`) are the ones to check. `WM_KEYDOWN`/`WM_KEYUP` WPARAM values already distinguish down/up, so the `flags` field is mainly for detecting injected (synthetic) events if you want to ignore them.
- `WH_KEYBOARD_LL` is a global hook (not a thread-specific hook like `WH_KEYBOARD`). It does not require a DLL — the callback lives in the process. This is the documented-correct approach for global hotkeys on modern Windows.
- The hook callback runs on the thread that installed the hook, but only while that thread is pumping messages. This is why the message loop is mandatory.
- The hook is lower-level than `GetAsyncKeyState` polling and will not miss events. It also fires for modifier keys reliably.
- Do not use `WH_KEYBOARD` (the older hook) — it requires a DLL and is process-specific. `WH_KEYBOARD_LL` is the modern, documented choice.
