# TurboTalk — Session Status

**Last updated:** 2026-06-15  
**Current state:** Removed auto-punctuation from the cleanup pipeline. Whisper naturally adds trailing periods to every utterance/segment, and `strip_whisper_artifacts` wasn't stripping bare `.` — only ` .` (space+dots) and `...`. Combined with the segment transcription pipeline, this produced periods at every silence-boundary pause, breaking sentences apart. Fix: `strip_whisper_artifacts` now also strips bare trailing `.`, and `join_segments` strips trailing periods from each segment before assembly.

## Open backlog

| Item | Status |
|------|--------|
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

Restart the TurboTalk dev session (`npm run tauri dev`) to pick up the changes, then run through a few dictation rounds to confirm the panic hook and watchdog are working correctly. To test the watchdog: manually kill the tracing writer thread or use `SIGSTOP` on it to simulate a stall, then wait 120s for the toast.
