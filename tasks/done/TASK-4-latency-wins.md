# TASK-4: Latency wins — ptt-down cue ordering + native frontmost/keystroke

## Goal
Reduce perceived dictation-start latency by ~100ms by reordering operations and eliminating unnecessary `osascript` subprocess spawns.

## Context

### Problem 1: Start cue delayed by focus query (hotkey.rs:487-503)
`ptt_down` calls `paste::frontmost_app()` (which spawns `osascript`, ~50-200ms) *before* emitting the `ptt-down` event and starting the recording chime. Audio capture already runs at this point (no data loss), but the user's "recording started" audio/visual cue lands ~100ms late — exactly the perceived-snappiness window.

Fix: reorder — emit `ptt-down` event and start the chime *before* calling `frontmost_app()`. A one-line reorder, free win.

### Problem 2: Three osascript subprocesses per dictation (hotkey.rs + paste.rs)
Each dictation spawns three `osascript` processes:
1. `frontmost_app()` — to know which app to paste into
2. Focus-at-paste query — another `osascript`
3. Cmd+V keystroke — also done via `osascript`

These cost ~50-200ms each. The fix has two parts:
- Replace `frontmost_app()` with `NSWorkspace.shared.frontmostApplication` (sub-millisecond via macOS native API).
- Replace the Cmd+V keystroke with `CGEventPost` (also sub-millisecond and removes the need for a 50ms pre-sleep).

If native APIs are not feasible (e.g., only accessible via objc bridge that isn't already in use), the minimum improvement is the reorder in Problem 1.

## In scope
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/hotkey.rs`
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/paste.rs`

## Out of scope
- Any other files
- The paste content logic itself — only the invocation mechanism
- Recorder, audio, transcribe

## Steps
1. Read `hotkey.rs` and `paste.rs` fully.
2. **Fix Problem 1**: In `ptt_down`, locate the calls to `paste::frontmost_app()` and the `emit ptt-down` / chime calls. Move the `emit`/chime calls to execute *before* `frontmost_app()`. This is a simple line reorder.
3. **Fix Problem 2**: In `paste.rs`, examine the `frontmost_app()` function. If it uses `NSWorkspace` or objc, verify the native API path exists. If it spawns `osascript` via `Command::new("osascript")`, replace with:
   - For `frontmost_app()`: Use `NSWorkspace.sharedWorkspace().frontmostApplication()` via the `objc` or `cocoa` crate (check `Cargo.toml` for existing dependencies).
   - For Cmd+V: Use `CGEventPost` to send a Cmd+V key event instead of `osascript -e 'tell application "System Events" to keystroke "v" using command down'`.
   - If native APIs are not available without adding new crate dependencies, leave Problem 2 as a TODO comment and only apply Problem 1's reorder.
4. Run `cargo check` in `src-tauri/` to verify compilation.

## Success signal
`cargo check` passes. The `ptt-down` event and chime are emitted before the `frontmost_app()` call in `ptt_down`. If native APIs are feasible, `frontmost_app()` and the Cmd+V paste use native APIs instead of `osascript`.

## Notes
- Check `Cargo.toml` for existing `cocoa`, `objc`, `core-graphics`, or `core-foundation` dependencies — these crates provide the native API bindings.
- `NSWorkspace.frontmostApplication` returns a `NSRunningApplication` with a `bundleIdentifier` or `localizedName` property — check the `cocoa` crate docs for the exact API.
- `CGEventPost` requires creating a key down + key up event pair for the 'v' key with the command modifier flag. This is well-documented in the `core-graphics` crate.
- If the existing codebase already uses any macOS-native approach elsewhere (e.g., the CGEventTap), prefer consistency with that approach.
