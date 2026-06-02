# Scan 3 — Listener Duplication + Recovery vs. Fake Cancellation: Findings

**Date:** 2026-06-02 · **Scope:** read-only static audit (no runtime repro run; static
proof was conclusive). No code changed — fixes proposed, not applied.

## Verdicts

**Listener duplication — NOT PRESENT.** Frontend listeners register exactly once
(single `onMount` → `listenTracked`, no reactive/`$:` or handler-nested `listen`).
The backend CGEventTap is spawned once (`lib.rs:2423`) and the cpal stream lives in a
single `Option<ActiveStream>` slot that is *replaced*, never stacked. Idle re-warm and
device-change both swap the one slot. See Part A.

**Recovery signaling — PARTLY OK, one real deferred fake-cancellation.** At the moment a
device is lost, the app does **not** surface a fake cancel — it emits a dedicated
`device-lost` (+ `recording-discarded`) and shows an honest "Microphone disconnected"
banner (`lib.rs:2287–2294`, `App.svelte:1156`). It does **not** emit `recording-recovered`,
because that event is a *different* feature (partial-segment recovery on a normal release,
`hotkey.rs:796`) — the in-flight recording is intentionally discarded and the stream
recovers transparently on the next press (`audio.rs:653–661`). **However**, in the default
**hold** mode, a device-lost *while the key is still held* arms `CANCEL_PENDING` via the
trailing key-up, so the user's **next** press is cancelled with a spurious
`recording-cancelled`. That is a genuine fake-cancellation regression — Finding 3.

---

## Findings

| # | Severity | Part | Finding |
|---|----------|------|---------|
| 1 | pass | A | No frontend or backend listener/tap/stream duplication. |
| 2 | nit | A | `Overlay.svelte` / `CursorDot.svelte` register listeners without tracking unlisten. |
| 3 | **should-fix** | B | Device-lost mid-hold arms `CANCEL_PENDING` → next press fake-cancels. |
| 4 | pass | B | Device-lost itself is an honest discard, not a fake cancel. |
| 5 | nit | B | `recording-recovered` is unrelated to device recovery (naming/expectation gap). |

### Part A — Listener duplication

**Finding 1 (pass).**
- *Frontend, App.svelte:* one `onMount` (`:969`). All Tauri listeners go through
  `listenTracked` (`:979–981`) inside a single `init()`; each unlisten is pushed to
  `cleanups` via `addCleanup`, and the `onMount` return tears them down with a `disposed`
  guard (`:1206–1209`). No `listen()` appears in a `$:` reactive block, a tab-switch, a
  settings reload, or any repeatable handler — grep of `listen(`/`listenTracked` shows every
  registration is in that one mount path. So a single `ptt-up`/`transcript` fires each
  handler once.
- *Backend tap:* `hotkey::spawn` is called exactly once (`lib.rs:2423`) and spawns one
  OS thread (`hotkey.rs:927`). The CGEventTap is rebuilt **only** on Accessibility-permission
  transitions (the watchdog retry loop, `hotkey.rs:935–1100`); `CGEventTap::new` replaces the
  prior tap (old one dropped), so taps never accumulate. Rebuild is not tied to recording or
  re-warm.
- *cpal stream:* `start()` holds `warm_stream` as a single `Option<ActiveStream>` and either
  reuses it (same device) or replaces it (`*warm = Some(stream)`, `audio.rs:700–704`) — it
  cannot append a second stream. Crucially, `idle_timeout_secs` defaults to **0**
  (`settings.rs:209–216`), so the stream is closed immediately after every recording and
  reopened on the next press — a clean 1:1 open/close, never a stack. The "45 s warm + idle
  watchdog re-warm" path only exists if a power-user sets a non-zero timeout in `config.toml`,
  and even then the single-slot invariant holds. The flagged "re-warm stacks a second
  tap/stream" scenario does not occur.

**Finding 2 (nit).** `Overlay.svelte:196–309` and `CursorDot.svelte:15–19` call `listen(...)`
without retaining the unlisten handle, and `Overlay.svelte` has no `onDestroy`. This is safe
today only because both windows are created once and are `closable:false` (never destroyed),
so their `onMount` runs once. *Fix (defensive):* collect unlisten fns and return them from
`onMount` so a future window-recreation or HMR can't leak duplicate listeners. Low priority.

### Part B — Recovery vs. fake cancellation

**Finding 4 (pass) — device-lost is an honest discard.** The level/watchdog thread, on
observing the `device_lost` flag, calls `cancel_after_device_lost()` then emits `device-lost`
**and** `recording-discarded` — never `recording-cancelled` (`lib.rs:2286–2294`). The frontend
`device-lost` handler clears state and shows a transient (5 s auto-clear) "Microphone
disconnected" banner (`App.svelte:1156–1164`); the `recording-discarded` handler with an empty
payload adds nothing (`App.svelte:1118–1128`). No fake cancel at loss time, and no stuck
"recovering" state (the banner self-clears; there is no persistent recovering flag).

**Finding 3 (should-fix) — deferred fake cancellation after device-lost in hold mode.**
The device-lost cancel runs on the level thread (`lib.rs:2288`) and goes straight to
`recorder.cancel_after_device_lost()`, **bypassing** `hotkey::trigger_cancel` and therefore
**not** calling `arm_ptt_up_suppression()`. In **hold** mode (the default —
`App.svelte:992`/`'hold'`), the user is still physically holding the key when the mic drops.
On key release, `ptt_up` runs (`hotkey.rs:466`), finds no suppression slot
(`try_consume_ptt_up_suppression()` → false, `:477`), calls `rec.stop()` on an
already-`Ready` recorder, gets `Err(IllegalTransition { from: Ready })`, and sets
`CANCEL_PENDING = true` (`hotkey.rs:865–872`). Unlike the quick-tap race this guard was built
for, there is **no pending `ptt_down` to consume it** — so `CANCEL_PENDING` persists. The
user's **next** press then hits `if CANCEL_PENDING.swap(false) { … emit "recording-cancelled" }`
(`hotkey.rs:422–426`) and the fresh recording is cancelled instantly with a spurious
"cancelled".

*Repro (not run):* hold mode → hold PTT and start dictating → unplug / disconnect the active
mic mid-hold → release the key (banner shows correctly) → press PTT again to dictate → the new
recording immediately cancels with `recording-cancelled` instead of recording.

*Proposed fix:* on the device-lost cancel path, when the hotkey is in hold mode, arm one
suppression slot before/with the cancel — mirror the existing callers that pair
`arm_ptt_up_suppression()` with `trigger_cancel` (`lib.rs:1774–1775`, `2100–2103`). Concretely,
in the `lib.rs:2286` block, if `hotkey_state.read().mode == "hold"`, call
`hotkey::arm_ptt_up_suppression()` so the trailing key-up no-ops instead of arming
`CANCEL_PENDING`. (Toggle mode is unaffected: key-release is already a no-op, so no `ptt_up`
stop fires.)

**Finding 5 (nit) — `recording-recovered` naming/expectation gap.** The scan brief expected
device recovery to emit `recording-recovered`, but in this codebase `recording-recovered` is
emitted only on a normal release when *segment* recovery produced partial text that is pasted
without creating a history entry (`hotkey.rs:791–796`). It has nothing to do with device loss.
No code bug — but the event name invites exactly the misread in the brief. *Optional:* a
one-line comment at the emit site (or renaming to `segment-recovered`) would prevent future
confusion. Both `recording-recovered` and `recording-cancelled` have matching frontend
handlers that only clear `recording`/`transcribing` (`App.svelte:1129–1142`).

## Bottom line
No listener/tap/stream duplication — the long-session multiplication hypothesis does not hold.
Device-lost is handled honestly at the moment of loss. The one real regression is Finding 3: a
device-lost mid-hold (hold mode, default) defers a fake `recording-cancelled` onto the user's
next press. Fix is small and localized to the `lib.rs:2286` device-lost block.
