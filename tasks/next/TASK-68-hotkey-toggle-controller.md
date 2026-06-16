# TASK-68: Separate hold and toggle hotkey controllers

## Goal
Redesign hotkey orchestration so hold mode and toggle mode each have their own logic path, while sharing the lower-level dictation engine work. Toggle mode must remain supported and become more robust, not be treated as an afterthought.

## Context
The current hotkey implementation in `src-tauri/src/hotkey.rs` has accumulated a lot of shared lifecycle state: `CANCEL_PENDING`, `START_IN_FLIGHT`, `CANCEL_ARMING`, `SUPPRESS_PTT_UP_COUNT`, `CANCEL_EPOCH`, `CURRENT_JOB_ID`, `FOCUS_AT_START`, segment recovery, hold-cancel, toggle-specific suppression, and device-loss handling. The current code works, but the orchestration is harder to reason about than the rest of TurboTalk.

The desired direction is Option B from the discussion:
- hold mode stays simple and press/release-based,
- toggle mode gets its own state machine,
- both reuse the same dictation engine pieces below the controller layer.

## In scope
- `src-tauri/src/hotkey.rs`
- any small helper module extracted specifically to support the controller split
- `SESSION-STATUS.md`

## Out of scope
- Changing the transcription backends
- Splitting `App.svelte`
- Removing toggle mode
- Broad behavioral changes to paste, cleanup, or history

## Design target
Aim for a structure like this:

```text
Hotkey input layer
  -> normalized events
  -> HoldController or ToggleController
  -> shared dictation engine
```

The controllers should own the mode-specific lifecycle rules. The shared engine can still own the actual record/transcribe/cleanup/paste steps.

## Steps
1. Map the current hotkey responsibilities and identify which transitions are specific to hold mode, toggle mode, or shared.
2. Extract a controller boundary that makes hold and toggle separate enough to be read independently.
3. Keep the shared dictation work where it belongs, but stop using the same state maze for both interaction models.
4. Preserve the current edge cases that matter:
   - quick tap
   - warmup / arming
   - cancel
   - device lost
   - segment recovery
   - focus change tracking
5. Add or update tests where the split creates new seams.
6. Update `SESSION-STATUS.md` with the new hotkey architecture once it is landed.

## Success signal
- Hold mode still behaves as press-to-start / release-to-stop.
- Toggle mode still works for long dictation sessions while walking around.
- The hotkey code is easier to reason about because hold and toggle no longer share one overloaded logic path.
