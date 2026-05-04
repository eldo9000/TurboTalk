# TASK-25: Hotkey implementation for Windows + Linux (X11)

## Goal
Push-to-talk hotkey works on Windows and Linux/X11 with the same public surface as the macOS implementation (`pub fn spawn(recorder, tray_icon, app, hotkey_state)`). Default key: Right Alt (matches macOS default). Linux/Wayland is explicitly documented as unsupported and falls back to a clear log + UI error, not a crash.

## Context
`src-tauri/src/hotkey.rs` has two `mod imp` branches today:
- `#[cfg(target_os = "macos")]` — full CGEventTap implementation (~470 LOC).
- `#[cfg(not(target_os = "macos"))]` — stub that logs `"unsupported platform"` and returns. See lines 481–507.

The mac branch has machinery beyond just keycode capture: job-id allocation (`next_job_id`), focus tracking (`FOCUS_AT_START`), cancel-pending race handling (`CANCEL_PENDING`), tray state transitions (`TrayState::Recording` → `Transcribing` → `Idle`), and a 7-stage `dictation-stage` event lifecycle. **All of that lifecycle logic must run on Windows/Linux too** — it's not mac-specific. Only the **key event source** is platform-specific.

Recommended approach: extract the lifecycle (`ptt_down`, `ptt_up`, `next_job_id`, the static mutexes, `emit_critical`, etc.) into a shared `mod common` that all platforms call into. Each platform `mod imp` then only owns the OS-specific key listener that calls `common::ptt_down(...)` / `common::ptt_up(...)`.

Crate choice for Win + Linux key listening:
- **`rdev` 0.5+** — global key listener, cross-platform, used by sibling project Handy. Works on Win + X11. Wayland: limited.
- **`tauri-plugin-global-shortcut`** — Tauri-native, but registers shortcuts (named combos) rather than raw key down/up events. Push-to-talk needs press *and* release events, which the plugin does not surface cleanly.
- **`device_query`** — polling-based, lower fidelity. Avoid.

Pick `rdev` unless a dealbreaker shows up (e.g., it conflicts with macOS CGEventTap if both are linked). It is acceptable for `rdev` to be a non-mac-only dependency: keep the mac branch on CGEventTap and use `rdev` only under `#[cfg(not(target_os = "macos"))]`.

Wayland: `rdev` on Linux uses X11 APIs. Under a Wayland session the listener will either fail at startup or silently miss events. Detect at runtime via `XDG_SESSION_TYPE=wayland` env var. If detected, do NOT call `rdev`; instead emit the same `ui-error` event the mac branch emits when CGEventTap fails — kind = `"hotkey-unsupported"`, message explaining Wayland is not supported in this beta.

The hotkey config (`HotkeyConfig` from `settings.rs`) maps a key name (`"right_option"`, `"right_control"`, etc.) and a mode (`"hold"` or `"toggle"`). The mac `key_for_name` returns macOS keycodes (e.g., `0x3D` for Right Option). The Win/Linux impl needs an equivalent table that maps the same config name strings to `rdev::Key` variants. `"right_option"` on Win/Linux maps to `rdev::Key::AltGr` or `rdev::Key::Alt` — pick the one that's actually emitted by the right Alt key on a typical keyboard.

## In scope
- `src-tauri/src/hotkey.rs` — refactor into shared lifecycle + per-OS listener
- `src-tauri/Cargo.toml` — add `rdev` under `[target.'cfg(not(target_os = "macos"))'.dependencies]`

## Out of scope
- macOS CGEventTap code — must work identically after the refactor
- Paste impl (TASK-26)
- Whisper sidecar binaries (TASK-27)
- Wayland support beyond a clear "unsupported" message
- Custom Linux input methods (uinput, libinput) — X11 only

## Steps
1. Read the current `src-tauri/src/hotkey.rs` end to end. Identify which symbols are platform-neutral (job id, focus mutex, ptt_down body, ptt_up body, emit helpers, UiError struct) and which are mac-specific (CGEventTap, CFRunLoop, AXIsProcessTrusted, key_for_name's CGEventFlags).
2. Add a `mod common` (or `mod lifecycle`) at the top of the file, no `cfg` gates. Move the platform-neutral code into it. Each platform `mod imp` calls into `common`.
3. Verify mac build is unchanged: `cargo check --manifest-path src-tauri/Cargo.toml`. Then `cargo test`.
4. Add `rdev` to `src-tauri/Cargo.toml` under the non-mac target stanza. Pick the latest 0.5.x.
5. In the `#[cfg(not(target_os = "macos"))] mod imp` block, replace the stub with a real `spawn` that:
   - At startup, on Linux, checks `XDG_SESSION_TYPE`. If `wayland`, emit `ui-error` with `kind: "hotkey-unsupported"` and return without registering a listener.
   - Otherwise, spawns a thread that calls `rdev::listen` with a callback. The callback inspects `EventType::KeyPress(key)` / `KeyRelease(key)`. On match against the configured key, calls `common::ptt_down(...)` / `common::ptt_up(...)`. Toggle vs hold mode logic mirrors the mac branch.
   - Provides a non-mac `key_for_name(&str) -> rdev::Key` (or equivalent) covering the same four config values: `right_option`, `right_control`, `right_command`, `right_shift`. On Windows there is no Command — map it to `Win` / `Meta`. On Linux map to `Super`.
6. Provide a non-mac `accessibility_trusted()` that returns `true` (or, on Linux, returns `true` if not Wayland). This is exported via `pub use imp::accessibility_trusted` in the existing module surface.
7. Run `cargo check --manifest-path src-tauri/Cargo.toml` on host (macOS).
8. Run `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu`. Should reach further than before — past `core-foundation`, past hotkey.rs, possibly hitting paste.rs or transcribe.rs next (those are separate tasks).
9. Run full test suite: `cargo test --manifest-path src-tauri/Cargo.toml`. Confirm no regressions.

## Success signal
- macOS happy path unchanged: `cargo test` green, manual hold-to-talk on the dev build still records and pastes.
- `cargo check --target x86_64-pc-windows-gnu` either succeeds or fails on a different module (paste.rs, transcribe.rs binary lookup) — NOT on hotkey.rs or `core-foundation`.
- `grep -n 'unsupported platform' src-tauri/src/hotkey.rs` shows the message now lives only in the Wayland branch, not as a blanket non-mac stub.
- The shared `common` module is referenced by both `#[cfg(target_os = "macos")] mod imp` and `#[cfg(not(target_os = "macos"))] mod imp`.

## Notes
- `rdev::listen` blocks the thread it runs on, same as `CFRunLoop::run_current()`. Spawn it on a dedicated thread.
- `rdev` callbacks must not panic — wrap any user code in `catch_unwind` if there's any doubt.
- Right Alt on most Win keyboards emits `AltGr` as a synthetic Ctrl+Alt sequence. Test against a real Win box; if `AltGr` doesn't fire, `Alt` may be the correct mapping.
- Keep the `dictation-stage` event names identical to the mac branch — the frontend listeners are platform-agnostic and depend on the same strings.

→ verify: on a real Windows host, build dev mode, hold Right Alt, speak, release — the existing transcribe path runs and emits `transcript` event. End-to-end paste depends on TASK-26; this task verifies hotkey events flow.
