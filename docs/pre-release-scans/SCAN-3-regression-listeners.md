# Scan 3 — Regression Pass: Listener Duplication + Recovery vs. Fake Cancellation

**Goal:** Re-check two specific regressions flagged from recent session logs:
1. **Event-listener duplication** after a long-running session.
2. **Recovery signaling** — confirm that after a device hiccup / long session, the
   app emits `recording-recovered` and recovers cleanly, instead of surfacing a
   **fake cancellation** to the user.

**Scope:** read-only code audit plus, if feasible, one targeted runtime repro.
Produce a findings report with file:line citations and severity. Do not refactor;
propose fixes, don't apply them.

## Background / contract

The frontend registers Tauri event listeners in `App.svelte`'s `onMount` via a
`listenTracked(eventName, handler)` helper that pushes each unlisten fn into a
cleanup list (`addCleanup`). Relevant events (see `src/App.svelte` ~lines 979–1166):
`ptt-down`, `ptt-up`, `transcript`, `recording-discarded`, `recording-cancelled`
(~1129), `recording-recovered` (~1138), `device-lost` (~1156), and others.

Backend cancel/recover lifecycle lives in:
- `src-tauri/src/audio.rs` — `cancel()` / `cancel_inner(device_lost)` /
  `cancel_after_device_lost()` (~lines 898–946), watchdog + device-lost handling.
- `src-tauri/src/recorder.rs` — the 3-state machine and where recovery vs cancel
  events are emitted.
- `src-tauri/src/audio_finalizer.rs` — segment emission / flush.
- `src-tauri/src/lib.rs` — where these events are `emit`ted to the frontend.

## Part A — Listener duplication after a long session

**Hypothesis to test:** listeners get registered more than once over a long
session, so a single `ptt-up` (or `transcript`) fires its handler N times —
manifesting as duplicate pastes, double state transitions, or duplicated UI logs.

Checks:
1. Confirm `onMount` runs exactly once for the main window and that
   `listenTracked` cleanups are invoked on the matching `onDestroy` / teardown.
   Grep for every `onMount` / `onDestroy` in `src/App.svelte`, `src/Overlay.svelte`,
   `src/CursorDot.svelte`, `src/Onboarding.svelte`. The overlay/cursor-dot windows
   are long-lived (`closable: false`) — confirm they don't re-register listeners on
   tab switches, settings reloads, or HMR-style re-runs.
2. Look for any listener registered **outside** the tracked helper, or inside a
   reactive block / function that can run repeatedly (Svelte `$:` or an event
   handler that calls `listen` again). Those are the classic duplication source.
3. Backend side: confirm the CGEventTap hotkey listener (`hotkey.rs`) and the cpal
   stream feeder (`audio.rs`, kept warm with a 45s idle-close watchdog) are not
   re-registered on each recording. Check that re-arming after the idle-close
   watchdog fires doesn't stack a second tap/stream. A long session that idles and
   re-warms repeatedly is exactly the scenario the log flagged.
4. If a runtime repro is feasible: run `npm run tauri dev`, leave it idle past the
   45s watchdog several cycles, then dictate and watch the UI-event log
   (`logUi`/`record_client_event`) for any doubled event lines.

## Part B — `recording-recovered` vs. fake cancellation

**Hypothesis to test:** a transient device-lost / re-warm during or after a long
session is being reported to the user as a cancellation (toast / state reset) when
it should be a silent recovery emitting `recording-recovered`.

Checks:
1. Trace the two cancel paths in `audio.rs`: `cancel_inner(device_lost=false)` vs
   `cancel_after_device_lost()` (`device_lost=true`). Confirm the `device_lost`
   branch leads to a `recording-recovered` emission (or no user-facing cancel
   toast), while only a genuine user/ESC cancel emits `recording-cancelled`.
2. In `App.svelte`, compare the `recording-cancelled` handler (~1129, which clears
   recording/transcribing flags) against the `recording-recovered` handler (~1138).
   Confirm a device-lost recovery does **not** trip the cancelled path. Look for any
   place where `device-lost` (~1156) and `recording-cancelled` can both fire for the
   same event, producing a visible "cancelled" flash before recovery.
3. Confirm the emission site in `lib.rs` / `recorder.rs` picks `recording-recovered`
   for the recovery case. Grep the whole tree for `recording-recovered` and
   `recording-cancelled` emit sites and map each to the state that triggers it.
4. Check the recovery banner lifecycle: `recording-recovered` should clear the
   "recovering" UI state set by `device-lost`. Confirm there's no path where the
   banner is set but never cleared (stuck recovering) after a long session.
5. If a runtime repro is feasible: start a recording, force a device change
   (unplug/switch the default input mid-record, or toggle the audio device in
   settings), and confirm the UI shows recovery — not a cancellation — and that
   `recording-recovered` appears in the UI-event log.

## Deliverable

A markdown report with two verdicts:
- **Listener duplication:** present or not, with the exact registration path that
  proves it (or proves single-registration).
- **Recovery signaling:** does a device-lost/long-session re-warm correctly emit
  `recording-recovered` instead of a fake cancellation? Cite the branch in
  `audio.rs` and the matching frontend handler.

Each finding: file:line, severity, proposed fix (not applied). If you ran a
runtime repro, include the observed UI-event log lines as proof.
