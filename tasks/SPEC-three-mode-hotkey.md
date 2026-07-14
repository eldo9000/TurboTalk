# SPEC: Three-mode hotkey (hold / auto / toggle)

## Overview

Replace the current two-mode (hold/toggle) hotkey system with a three-mode system
adding an **Automatic** mode that blends hold and toggle behavior using a configurable
tap threshold (~0.4s).

## Mode definitions

### Hold (push-to-talk)
- Key-down → start recording
- Key-up → stop recording
- TurboTalk already has this via `HoldController` in `hotkey.rs`

### Toggle
- Press → start recording (if Ready) or stop recording (if Recording)
- Press during warmup → cancel (already implemented via `START_IN_FLIGHT` poll)
- TurboTalk already has this via `ToggleController` in `hotkey.rs`

### Automatic (new)
A hybrid: the mode the user wants 90% of the time for a hands-free workflow.

- **Key-down**: start recording immediately (same as hold/toggle on press)
- **Key-release before threshold**: if held < 0.4s, this is a "tap" — stop recording
- **Key-release after threshold**: if held >= 0.4s, this was a deliberate hold — stop recording (same as hold mode)
- **Second tap while recording**: if already recording and user taps again (< 0.4s), stop
- **Long hold**: if already recording, key-up always stops (ignores threshold)

The 0.4s threshold is a constant (`automaticTapThresholdSeconds`). It should be
configurable in settings (range 0.2–1.0s) so users can tune it to their tapping speed.

## Key edge cases (from FluidVoice)

**1. UUID-token key-up debounce**
When a hold-mode key is released, schedule the stop on a short delay.
Each new key-down generates a UUID token. If a pending stop exists with a
different (older) token, it is cancelled. This prevents rapid press/release jitter
from ghost-stopping.

TurboTalk's current approach (`CANCEL_PENDING` AtomicBool + `LAST_RECORDING_START_MS`)
handles the same problem but is harder to reason about. A token-based approach
is cleaner: each press creates a new token, the release only stops if its token
matches the current one.

**2. Modifier-only shortcuts**
A single modifier key (Fn, right-Command, etc.) as the entire hotkey:
- Track `modifierOnlyKeyDown` flag
- Track `otherKeyPressedDuringModifier` — if user hits another key while modifier is down, the release is NOT a dictation trigger (it was a real chord like Cmd+C)
- Release of the modifier-only key with no companion key pressed → trigger dictation

**3. Mouse-as-trigger**
Support mouse buttons as dictation triggers (useful for foot pedals, extra mouse
buttons). The CGEventTap already captures mouse events — just extend the press/release
dispatch to include mouse button numbers.

**4. Tap-disabled recovery (inline, not polling)**
Already implemented in TurboTalk's CGEventTap callback. FluidVoice does the same
— re-enable the tap inline when `TapDisabledByTimeout` fires, with a fallback to
recreate the tap if re-enable fails. The 30s health check is backup only.

## TurboTalk integration points

### `src-tauri/src/hotkey.rs` (2706 lines)
The existing architecture has a clean separation:
- `mod imp` — platform key-event source (CGEventTap / IOHIDManager)
- `Controller / HoldController / ToggleController` — mode-specific lifecycle
- `mod common` — shared dictation engine (`ptt_down`, `ptt_up`, cancel, paste, chimes)

The three-mode system should add an `AutomaticController` alongside the existing
two. The controller trait/pattern already exists — just need a third variant.

Key changes:
1. Add `AutomaticController` struct with a UUID generator and the 0.4s timer
2. Add `HotkeyMode::Automatic` to the enum (currently has `Hold` / `Toggle`)
3. Add `automaticTapThresholdMs: u64` to settings
4. UUID-token logic: replace or supplement `CANCEL_PENDING` / `START_IN_FLIGHT` /
   `LAST_RECORDING_START_MS` atomics with a token-based cancellation system in
   the common module
5. Modifier-only tracking: new atomics or state struct for `MODIFIER_KEY_DOWN` /
   `OTHER_KEY_PRESSED_DURING_MODIFIER`
6. Mouse-button trigger: extend `spawn` to accept an optional mouse button number,
   dispatch into the same press/release flow

### `src-tauri/src/settings.rs`
- Add `hotkey_mode: HotkeyMode` field (enum: Hold / Toggle / Automatic)
- Add `automatic_tap_threshold_ms: u64` (default 400)
- Add `modifier_only_hotkey: Option<ModifierKey>` for modifier-only shortcut
  configuration

### Frontend (`src/` settings UI)
- Add dropdown/radio selector for hotkey mode
- Add slider for tap threshold (only visible when mode = Automatic)
- Add modifier-only shortcut picker

## Out of scope

- Changing the IOHID/CGEventTap platform layer (it already dispatches press/release)
- Any audio pipeline changes
- Any transcription or cleanup changes
- Removing the existing Hold/Toggle modes (Automatic is additive)

## Implementation order

1. Add `HotkeyMode::Automatic` to the mode enum in `settings.rs`, wire through to
   `hotkey.rs` config loading
2. Implement `AutomaticController` with the 0.4s threshold + UUID-token debounce
3. Add modifier-only tracking atomics
4. Add mouse-button dispatch
5. Frontend settings UI
6. Configurable threshold in settings
7. Update SESSION-STATUS.md

## Success signal

- A user can set their hotkey to Automatic mode
- Quick tap (< 0.4s) starts recording; another quick tap stops
- Press and hold starts recording; release after > 0.4s stops
- Rapid press/release jitter does not ghost-stop
- Modifier-only shortcut works (e.g., Fn key alone = trigger)
- Mouse button trigger works
- `cargo check` and `cargo clippy` pass
- `npm run typecheck` passes
