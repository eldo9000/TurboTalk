# TASK-2: ptt_up statics-take reorder + stale CANCEL_PENDING hardening

## Goal
Fix two race conditions in the hotkey state machine: the toggle-mode double-press race that can drop segments and corrupt the tray, and the stale `CANCEL_PENDING` flag that can fake-cancel the next press.

## Context

### Race 1: Toggle-mode double-press drops segments (hotkey.rs:507-536)
In toggle mode, pressing to stop, then pressing again quickly (within the thread-scheduling window) spawns two `ptt_up` workers. Currently the code takes `CURRENT_JOB_ID`, `FOCUS_AT_START`, and `CURRENT_SEG_TRANSCRIBER` *before* calling `rec.stop()`. If the loser takes these statics first (before the winner's `stop()` call), the winner finds `seg_transcriber = None` and loses all mid-recording segment text. Additionally, the loser sets the tray to Idle mid-transcription.

The fix: reorder — consume suppression, call `rec.stop()`, then take the statics only on `Ok` arms. Err arm takes nothing. This needs no new flags.

### Race 2: Stale CANCEL_PENDING from stray key-up (hotkey.rs:904-927)
A key-up with no matching down sets `CANCEL_PENDING` when `prewarm_in_flight()` is true — including the app-launch prewarm (when no press exists). The flag then sits armed indefinitely; the next legitimate press start succeeds, hits `CANCEL_PENDING.swap(false, Ordering::Relaxed)` (hotkey.rs:463), and instantly cancels itself.

The fix: clear `CANCEL_PENDING` at the top of `ptt_down` before spawning, so a stale flag cannot affect a new press.

## In scope
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/hotkey.rs`

## Out of scope
- Any other files
- Win32 hotkey code (`hotkey_win32.rs`)
- The toggle-mode yellow arming tile issue (separate task)
- Changes to `recorder.rs`

## Steps
1. Read `hotkey.rs` fully. Locate:
   a. The `ptt_up` function (~line 507), specifically where `CURRENT_JOB_ID`, `FOCUS_AT_START`, and `CURRENT_SEG_TRANSCRIBER` are read from statics, and where `rec.stop()` is called.
   b. The `ptt_down` function (~line 395-463), specifically where `CANCEL_PENDING.swap(false)` is checked.
   c. The key-up handler (~line 904-927) where `CANCEL_PENDING` is set.
2. **Fix Race 1**: In `ptt_up`, move the calls to `rec.stop()` (and suppression consumption) *before* reading `CURRENT_JOB_ID`, `FOCUS_AT_START`, and `CURRENT_SEG_TRANSCRIBER`. Only read these statics on the `Ok` arm of `stop()`. On the `Err` arm, take nothing and bail.
3. **Fix Race 2**: In `ptt_down`, at the very top (before any spawn or logic), add `CANCEL_PENDING.store(false, Ordering::Relaxed)` to clear any stale cancellation flag from a previous orphaned key-up.
4. Run `cargo check` in `src-tauri/` to verify compilation.

## Success signal
`cargo check` passes. The logic in `ptt_up` reads the statics after the `rec.stop()` call and only on the `Ok` path. `ptt_down` clears `CANCEL_PENDING` at its entry point.

## Notes
- The toggle-mode statics-take reorder is the direct mash-proofing the design intends. The window is small (spawn latency) but real.
- `rec.stop()` returns a `Result` — examine its return type in `recorder.rs` to confirm the `Ok`/`Err` arms.
- The `CANCEL_PENDING` flag uses `AtomicBool` with `Ordering::Relaxed` — use the same ordering for the new store.
