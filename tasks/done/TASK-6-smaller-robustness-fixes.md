# TASK-6: Smaller robustness fixes — toggle tile, garbage, orphans, seg-recovery

## Goal
Fix four smaller robustness gaps: toggle-mode yellow arming tile dismissal, garbage detection refinement, orphan kill scoping, and segment-recovery tray race.

## Context

### Fix 1: Toggle mode can't dismiss the yellow arming tile (hotkey.rs:395)
While the first press's worker polls readiness (30s loop), `START_IN_FLIGHT` blocks every new `ptt_down`, so the "press again during warmup = cancel" branch is unreachable in toggle mode. Hold mode escapes via key-up → `CANCEL_PENDING`; toggle has no path. The user is stuck watching the yellow tile for up to 30 seconds with no way to cancel.

Fix: in the readiness poll loop, also check a flag that a suppressed second press sets. If the flag is set, bail out of the poll and cancel.

### Fix 2: detect_garbage bigram filter blocks entire paste (transcribe.rs or audio_finalizer.rs)
The `detect_garbage` function uses a bigram repeat filter (>3 repeats) that blocks the *entire* paste. Natural speech like "no no no no no" or "ha ha ha ha ha" trips it, and a minutes-long dictation is withheld (text only visible in the rejected toast).

Fix: run garbage detection per-segment or only block the paste when garbage *dominates* the transcript (e.g., >50% of segments flagged). The exact approach: if garbage is detected, still emit non-garbage segments instead of blocking everything.

### Fix 3: kill_orphans kills any whisper-server on the machine (cleanup.rs)
The `kill_orphans` function runs `pkill -f whisper-server`, which kills *any* whisper-server process on the machine, not just TurboTalk's. This could kill another user's or another instance's server.

Fix: record the PID of the spawned whisper-server and only kill that specific PID, or match the specific binary path that TurboTalk launched.

### Fix 4: Seg-recovery path flips tray Idle after new job starts (hotkey.rs:795-890)
The segment-recovery path checks `CURRENT_JOB_ID.lock().is_some()` once before cleanup, but cleanup can block ~2s on Ollama. If a new job starts during that window, the old cleanup's tray write flips the tray to Idle, corrupting the display.

Fix: re-check `CURRENT_JOB_ID` (or a generation counter) before writing to the tray. Only write Idle if the job ID hasn't changed.

## In scope
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/hotkey.rs`
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/transcribe.rs`
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/cleanup.rs`

## Out of scope
- Any other files
- The four main robustness gaps already covered by TASK-1 through TASK-5
- Audio processing, recorder state machine

## Steps

### Fix 1 — Toggle arming tile
1. In `hotkey.rs`, find the readiness poll loop in `ptt_down` or wherever `START_IN_FLIGHT` is checked (~line 395).
2. Add a new `AtomicBool` flag (e.g., `CANCEL_ARMING`) that gets set when a suppressed `ptt_down` fires during toggle-mode arming.
3. In the poll loop, check this flag. If set, clear it, break out of the poll, and cancel the operation.
4. Clear the flag at appropriate reset points (same places `CANCEL_PENDING` is cleared).

### Fix 2 — Garbage detection
1. In `transcribe.rs` or `audio_finalizer.rs`, find the `detect_garbage` function.
2. Modify the rejection logic so that when garbage is detected:
   - It still allows non-garbage segments to be emitted (or pasted).
   - Or only blocks the entire paste when garbage segments constitute >50% of the total transcript.
3. The simplest approach: instead of returning a boolean "is garbage", return which segments are garbage vs clean, and let the caller decide.

### Fix 3 — Orphan kill scoping
1. In `cleanup.rs`, find `kill_orphans` and the `pkill -f whisper-server` call.
2. Replace with either:
   - Store the whisper-server PID at spawn time and kill by PID (`kill <pid>`).
   - Or match the exact binary path (`pkill -f "/path/to/TurboTalk/.../whisper-server"`).
3. If the PID is the approach, the PID must be stored in a static at server spawn time (likely in `transcribe.rs`).

### Fix 4 — Seg-recovery tray race
1. In `hotkey.rs`, find the segment-recovery cleanup path (~line 795-890).
2. Before writing the tray to Idle, re-check that `CURRENT_JOB_ID` still matches the job being cleaned up (or increment a generation counter at each new job start and store it at cleanup start).
3. If the job ID or generation changed, skip the tray write.

5. Run `cargo check` in `src-tauri/` to verify compilation after all fixes.

## Success signal
`cargo check` passes. All four fixes are implemented. Specific evidence:
- A `CANCEL_ARMING` flag (or equivalent) exists and is checked in the readiness poll loop.
- `detect_garbage` is per-segment or uses a dominance threshold rather than blocking the entire paste.
- `kill_orphans` only kills the specific whisper-server instance (by PID or exact path).
- Seg-recovery re-checks job identity before writing the tray to Idle.
