# TurboTalk — Session Status

**Last updated:** 2026-06-16  
**Current state:** Hotkey.rs dictation-completion path simplified — extracted `bail_out()` and `paste_and_teardown()` helpers, refactored all three completion paths (normal, salvaged, segment-recovery). Net -89 lines, behavior-preserving.

## Open backlog

| Item | Status |
|------|--------|
| **Onboarding welcome-screen cleanup** | **Fixed** — `recheckReadiness()` now dismisses onboarding immediately when all gates green (no longer depends on launch-at-login), and `Onboarding.svelte` auto-close no longer requires `launchAtLogin`. Two changes: `src/App.svelte` (initial-mount gate) and `src/Onboarding.svelte` (removed hard requirement from auto-close condition). |
| **Status window (new)** | **Built** — `src/Status.svelte` handles `ptt-armed`, `ptt-arm-failed`, `transcription-rejected` (flaky/blocked), and `recording-discarded` (empty-final-text). Yellow pulsing border for arming, red pulsing border for rejections, dismiss button on rejections. Window is clickable (no `set_ignore_cursor_events`). |
| **Arming removed from overlay** | **Done** — `Overlay.svelte` no longer listens for `ptt-armed` / `ptt-arm-failed`, no arming CSS classes or template blocks. |
| **Filtered-dictation overlay feedback** | **Superseded** by status window — `transcription-rejected` feedback now lives in `Status.svelte`. |
| **RejectReason::label()** | Added — short 1-3 word label for overlay use (e.g. "Repetition detected", "Junk detected"), separate from the full `description()` used in toasts. |
| **Cancel key paste-through (post-transcription)** | Fixed — Added `SeqCst` ordering on `CANCEL_EPOCH` and state-machine guards before all 3 paste call sites. |
| **Manual device-lost repro** | **TODO** — still needs a fresh runtime capture with an actual `device-lost` line so we can confirm the mid-recording unplug/switch path end-to-end. |
| Release CI run | Complete — v0.9.8 builds, codesign, updater artifacts all green ([#27322438132](https://github.com/eldo9000/TurboTalk/actions/runs/27322438132)) |
| TASK-25/26 — Windows hotkey + paste | Complete |
| TASK-57 — Segment recovery pollutes history | Fixed |
| TASK-48 — CoreML / Neural Engine | Phase 1 built; phase 2 blocked |
| Developer ID / Authenticode signing | Deferred |
| Parakeet v3 multilingual | In catalog; end-to-end not user-confirmed |

## Backend tradeoffs

- **Parakeet** — fastest English; raw output lowercase/unpunctuated (Chaperone normalizes)
- **Whisper** — multilingual, best accuracy; Silero VAD pre-filter when model bundled
- **Moonshine** — lowest silence hallucination; English-only

## Recent commits

- `e5aae45` — docs: log v0.9.8 release CI signing env failure in CI-FAIL-LADDER
- `b40fda9` — fix(ci): only set Apple notarization env vars when Developer ID credentials present
- `3521733` — fix(ci): fall back to ad-hoc identity instead of empty string in release workflow
- `9c6b3ca` — chore(release): bump to 0.9.8

## This session

**Event:** User asked for a subjective technical-design/code simplification pass: identify parts that look overcomplicated or over-designed after the app's first full release.

**Review notes:** `recorder.rs` is a good small core state machine. Complexity clusters around hotkey lifecycle orchestration, duplicated paste/finalization branches, repeated monitor geometry math, monolithic Tauri bootstrap, and frontend event listeners doing direct state mutation. Recommended first simplification is a behavior-preserving extraction/refactor of the dictation completion path in `src-tauri/src/hotkey.rs`.

**Event:** Simplified `src-tauri/src/hotkey.rs` by extracting shared dictation-completion helpers — `bail_out()` (the repeated finish_guarded + tray + emit_ready pattern) and `paste_and_teardown()` (the full emit_transcript → begin_pasting → cancel-check → focus-check → paste → teardown sequence). The normal and salvaged transcript paths now call `paste_and_teardown()` instead of duplicating ~120 lines each. Segment-recovery bail-out blocks use `bail_out(false)`. Net: -89 lines from hotkey.rs. Behavior-preserving refactor — cargo check + 43 relevant tests pass (2 pre-existing transcribe::detect_garbage test failures unchanged).

**Event:** User reported a "strange crash" in TurboTalk. Investigation of logs and macOS crash reports revealed two distinct issues.

### Issue 1: Silent tracing pipeline death (today's crash)

The session log (`turbotalk.2026-06-16.log`) ends cleanly at 18:37 with no errors, but the transcript log continued recording for ~30 seconds afterward. No `.ips` crash report was generated. Root cause: the `tracing-appender` `NonBlocking` writer thread died silently (likely a panic), causing all `tracing::info!()`/`warn!()` calls to become no-ops. The frontend lived but was unresponsive.

### Issue 2: June 13 SIGABRT crash cluster (v0.9.8, 14 crashes in ~1 hour)

All crashes were identical: `SIGABRT` (abort trap) on the main thread, preceded by `[hotkey] CGEventTap failed (accessibility_trusted=false)`. The only `.expect()`/`.unwrap()` in the hotkey code was `create_runloop_source` on the CGEventTap success path. While this `.expect()` is unlikely to be the crash site (it's on the Ok path), the crash cluster suggests a panic in the accessibility error recovery flow that existed in v0.9.8.

### Changes made (3 files)

1. **`src-tauri/src/main.rs`** — Installed a process-wide panic hook that writes the panic location, message, and backtrace to stderr before the default abort. Ensures panics in background threads (including tracing-appender) leave forensic evidence.

2. **`src-tauri/src/lib.rs`** — Added a tracing health watchdog. Every 60s it stats the newest main session log file. If the mtime is > 120s stale, it writes a warning to stderr and emits a one-shot `ui-error` toast (`kind: "tracing-watchdog-dead"`) so the user sees the logging pipeline died and knows to restart.

3. **`src-tauri/src/hotkey.rs`** — Hardened the single remaining `.expect()` call on `create_runloop_source`. It now handles `Err(())` gracefully by logging an error, incrementing the trusted-failure retry budget, sleeping 5s, and retrying CGEventTap creation. After `MAX_TRUSTED_FAILURE_RETRIES` (6), it returns cleanly instead of panicking.

## Next action

Completed: extracted `bail_out()` and `paste_and_teardown()` helpers into `hotkey.rs::common` and refactored all three completion paths to use them. Normal and salvaged paths now share the same paste machinery instead of duplicating ~120 lines each. Segment-recovery bail-out blocks centralized via `bail_out(false)`.

If continuing simplification, next targets: repeated monitor-geometry math in window-placement helpers, monolithic Tauri bootstrap in `lib.rs`, and frontend event listeners doing direct state mutation.
