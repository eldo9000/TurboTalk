# TASK-7: Waste removal and documentation cleanup

## Goal
Remove dead code paths, stale documentation, and unnecessary work: dead paste-miss logic, vestigial code, inefficient HID filtering, level thread config waste, and outdated CLAUDE.md.

## Context

### Issue 1: Ok(false) paste-miss path is dead code (hotkey.rs + paste.rs)
Both `paste()` implementations only ever return `Ok(true)` or `Err`. The `paste-miss` arms in `ptt_up` (two sites) and the `paste-miss` event are unreachable. Either delete the dead arms, or actually make `paste()` return `false` when `focused_ax_role()` is None-ish — right now that diagnostic is computed and discarded.

Fix: either delete the dead `paste-miss` branches, or implement the missing case where `paste()` returns `Ok(false)` when the focused accessibility role is None/unavailable.

### Issue 2: find_whisper / sidecar_candidates are dead code (transcribe.rs)
These are `#[allow(dead_code)]` legacy functions from the whisper-cli era. Keep only what the tests pin. Delete anything not used by tests or live code.

### Issue 3: is_our_button is vestigial (hotkey.rs:1120-1128)
`is_our_button` in the HID callback is always true past the early return — the function result is never used meaningfully after an early-return guard. Remove it or collapse it.

### Issue 4: HID listener matches all devices (hotkey.rs:1198)
The HID listener matches *all* HID devices with a null matching dictionary, meaning audio events from mouse x/y/wheel all invoke the callback. Fix by adding a matching dictionary that filters to usage page 0x09 (button) or the specific device.

### Issue 5: Level thread reads full settings every 50ms (lib.rs:2427)
The level thread does `settings::load()` (full `Config` clone including vocabulary `Vec`) every 50ms forever, even when idle. It only needs one boolean from the config. Fix: either read only the needed field, or cache the config and only reload on change.

### Issue 6: CLAUDE.md says "3-state machine" but it's 6 states (CLAUDE.md)
The architecture comment in CLAUDE.md describes `recorder.rs` as a "3-state machine" but it has 6 states now. Update the documentation to reflect the current state count.

## In scope
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/hotkey.rs`
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/paste.rs`
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/transcribe.rs`
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/lib.rs`
- `/Users/eldo/Downloads/Github/TurboTalk/CLAUDE.md`

## Out of scope
- Any other files
- New features or behavior changes beyond cleanup
- The deferred/ and done/ task directories
- CI configuration, build scripts

## Steps

### Issue 1 — Dead paste-miss path
1. Read `paste.rs` to understand the `paste()` return type and what conditions (if any) could produce `Ok(false)`.
2. Read `hotkey.rs` to find the `paste-miss` match arms in `ptt_up`.
3. If `paste()` genuinely cannot return `Ok(false)`: delete the `Ok(false)` match arm and the `paste-miss` event emission.
4. If `paste()` *could* return `Ok(false)` but doesn't implement the `focused_ax_role()` None case: implement it (return `Ok(false)` when no valid focus target exists) and keep the `paste-miss` arm — but only if it adds real diagnostic value.

### Issue 2 — Dead legacy functions
1. In `transcribe.rs`, find `find_whisper` and `sidecar_candidates` (both `#[allow(dead_code)]`).
2. Check if any tests reference them (`rg "find_whisper|sidecar_candidates" src-tauri/`).
3. If no tests use them, delete both functions. If tests use them, keep only what the tests need.

### Issue 3 — Vestigial is_our_button
1. In `hotkey.rs`, find `is_our_button` (~line 1120-1128).
2. If its result is always true past the early-return guard, remove the function and collapse the logic into the caller.

### Issue 4 — HID filter
1. In `hotkey.rs`, find the HID listener setup (~line 1198).
2. Add a matching dictionary to filter to only button-related HID events (usage page 0x09, usage 0x01-0xFF for buttons). Use the IOHIDManager API with `IOHIDManagerSetDeviceMatching`.

### Issue 5 — Level thread config
1. In `lib.rs`, find the level thread loop (~line 2427).
2. Replace `settings::load()` with either:
   - Reading only the specific config field that the level thread needs.
   - Or caching the config in an `Arc<RwLock<>>` that gets updated elsewhere and read here without cloning.
3. The thread reads every 50ms — ensure the replacement is at least as responsive.

### Issue 6 — CLAUDE.md update
1. Read `CLAUDE.md` and find the "3-state machine" comment about `recorder.rs`.
2. Update it to "6-state machine" or "multi-state machine" for forward compatibility.

7. Run `cargo check` in `src-tauri/` to verify compilation after all changes.

## Success signal
`cargo check` passes with no new warnings. Specifically:
- `paste-miss` branches are either deleted or actually reachable.
- `find_whisper` and `sidecar_candidates` are removed or kept only for tests.
- `is_our_button` is removed or collapsed.
- HID listener has a device matching dictionary.
- Level thread no longer clones the full `Config` every 50ms.
- CLAUDE.md says "6-state machine" or equivalent.

## Notes
- These are drive-by cleanups — keep each fix minimal. If any individual fix turns out to be more complex than expected, skip it and note it in the return notes rather than blocking the whole task.
- The HID matching dictionary fix may require adding `IOHIDManagerSetDeviceMatching` to the FFI bindings if not already available.
- The level thread fix should be careful not to change behavior — just eliminate the wasteful full-settings clone.
