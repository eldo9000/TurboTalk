# TASK-26: Paste implementation for Windows + Linux (X11) using enigo

## Goal
`crate::paste::paste(text)` writes to the system clipboard and synthesizes the OS-native paste shortcut on Windows (`Ctrl+V`) and Linux/X11 (`Ctrl+V`). After return, the clipboard's prior contents are restored on a best-effort basis. macOS path is unchanged. Linux/Wayland returns the same `unsupported platform` error the current stub returns, with a one-line clarifying log.

## Context
`src-tauri/src/paste.rs` today has:
- `#[cfg(target_os = "macos")] pub fn paste(text)` — clipboard write via `arboard`, then `osascript` Cmd+V, then prior-clipboard restore. ~30 LOC.
- `#[cfg(not(target_os = "macos"))] pub fn paste(_text)` — `anyhow::bail!("unsupported platform: ...")`.
- `frontmost_app()` is mac-only and returns `None` on other platforms — leave that alone, hotkey.rs already tolerates `None`.

`enigo = "0.3"` is **already in `src-tauri/Cargo.toml` line 51** as an unconditional dep. It compiles on all three platforms. Use it for keystroke injection.

`arboard` is also already a dep and is cross-platform — use it for clipboard read/write on all three platforms (the macOS branch already does).

Wayland behavior: `enigo` on Linux uses the X11 backend by default. Under Wayland (`XDG_SESSION_TYPE=wayland`) keystroke injection is blocked by the compositor for security reasons. Detect at runtime and return the `unsupported platform` error so the existing UI banner still works. The error message string MUST contain the literal substring `unsupported platform` — the test in `paste.rs:131` and the UI banner both grep for it.

The 50ms / 150ms sleep timings around clipboard write and shortcut send in the mac branch exist to give the target app time to observe the clipboard write and to debounce the prior-clipboard restore. Keep equivalent timings on Win/Linux. They may need tuning but start with the same numbers.

The non-mac branch already has a unit test (`paste_returns_unsupported_platform_off_mac` at line 131) that verifies the error message. After this task, that test will need to be either deleted or scoped to Wayland-only (since real X11/Win paste should now succeed). Reframe the test as "Wayland returns unsupported" if you can fake the env var, or just delete it and replace with a smoke test that calls `paste("hi")` and asserts no panic.

## In scope
- `src-tauri/src/paste.rs` — implement non-mac `paste()`
- `src-tauri/src/paste.rs` tests — adjust to reflect new behavior

## Out of scope
- `frontmost_app()` — leave the `None`-returning non-mac stub in place
- macOS branch — must be byte-identical after this task
- Hotkey impl (TASK-25)
- Whisper sidecar binaries (TASK-27)
- Wayland support beyond detection + clear error message
- Adding `enigo` to Cargo.toml (already present)
- Diagnostics integration (TASK-29)

## Steps
1. Read `src-tauri/src/paste.rs` end to end. Note the prior-clipboard save/restore pattern in the macOS branch — replicate it on Win/Linux.
2. Replace the `#[cfg(not(target_os = "macos"))] pub fn paste()` stub with a real implementation:
   - On Linux, check `std::env::var("XDG_SESSION_TYPE")`. If it equals `"wayland"`, log a one-line `tracing::warn!` and `anyhow::bail!("unsupported platform: paste under Wayland is not supported in this beta")`. The string must contain `unsupported platform`.
   - Open `arboard::Clipboard`. Save prior text content (best-effort, ignore error).
   - Set new text.
   - Sleep 50 ms.
   - Construct `enigo::Enigo` and send `Ctrl+V` via the keystroke API. On Windows and Linux/X11 the modifier is `Control`. Use whatever current `enigo` 0.3 API style applies — `enigo.key_sequence_parse("{+CTRL}v{-CTRL}")` or the `Direction` builder, depending on version.
   - Sleep 150 ms.
   - Restore prior clipboard text if it was captured.
   - Return `Ok(())`.
3. Verify the macOS branch is untouched. `git diff src-tauri/src/paste.rs` should show changes only in the non-mac block and its test.
4. Update or replace the `paste_returns_unsupported_platform_off_mac` test. New test goal: under Wayland-emulating env, paste returns an error containing `unsupported platform`. Under non-Wayland non-mac targets, paste either succeeds (in a graphical session) or returns a non-`unsupported platform` error. In CI without a display, it will likely fail to open the keyboard — that's acceptable as long as it doesn't panic.
5. `cargo test --manifest-path src-tauri/Cargo.toml`. Confirm green on macOS.
6. `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu`. Past hotkey + paste, should now reach further.

## Success signal
- macOS: `cargo test` green; manual dev build still pastes correctly into TextEdit.
- `git diff src-tauri/src/paste.rs` shows zero changes inside any `#[cfg(target_os = "macos")]` block.
- `cargo check --target x86_64-pc-windows-gnu` no longer fails inside `paste.rs`.
- The string `unsupported platform` exists in the source only on the Wayland-specific branch.

## Notes
- `enigo` 0.3 API has changed across point releases — verify against the version Cargo.lock resolves to before writing keystroke code.
- On Linux, `arboard` requires a running X server or Wayland clipboard portal. Under Wayland with no portal it may block — keep clipboard ops inside the `XDG_SESSION_TYPE != "wayland"` branch.
- On Windows, `arboard` may briefly hold the clipboard lock. If `enigo`'s Ctrl+V races the clipboard write, increase the 50 ms delay.
- Do not add a separate Wayland code path that uses `wtype` or `ydotool`. Beta is X11 + Win only.

→ verify: on a real Windows host, `paste("hello world")` from a quick test harness writes `hello world` into Notepad. On a real Linux/X11 host, same into gedit/kate/xed.
