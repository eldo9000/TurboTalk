# TurboTalk — Session Status

**Last updated:** 2026-06-14  
**Current state:** Fixed intermittent cancel-paste-through bug in `hotkey.rs` — `cancel_epoch_at_stop` is now captured before `rec.stop()` instead of after three `Mutex::take()` calls, eliminating the race where `CANCEL_EPOCH` could be incremented by the Escape handler before the snapshot was taken.

## Open backlog

| Item | Status |
|------|--------|
| **Cancel key paste-through regression** | **Fixed** — `cancel_epoch_at_stop` moved before `rec.stop()` in both Wav and Discard arms of `ptt_up`. Previously, 3 `Mutex::lock().take()` calls between `stop()` return and the epoch capture created a window where the Escape handler could increment `CANCEL_EPOCH` first, making the snapshot match the post-cancel value and causing all `job_cancelled_since` checks to never fire. Verified by build. |
| **Manual device-lost repro** | **TODO** — verify `lib.rs:2286` fix: hold key → unplug/switch mic mid-recording → release → next press must start a normal recording (no instant "recording-cancelled"). Fix is verified-by-construction only; runtime not yet observed. |
| Release CI run | **Complete** — v0.9.8 builds, codesign, updater artifacts all green ([#27322438132](https://github.com/eldo9000/TurboTalk-App/actions/runs/27322438132)) |
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

## Next action

Runtime device-lost proof: hold key → unplug/switch mic mid-recording → release → next press must start a normal recording with no instant `recording-cancelled`.
