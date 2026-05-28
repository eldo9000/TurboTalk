# TurboTalk — Session Status

**Last updated:** 2026-05-28  
**Current state:** macOS dictation is feature-complete for personal use. Latest commit `88a26b8`: overlay dismisses on transcription-rejected (fixes stuck Transcribing pill). Prior `1124d4a`: onboarding drag-during-download fix.

**Next action:** User-retest Windows onboarding Parakeet download buttons. Optional: tail-rejection discarding valid streaming segments.

## Open backlog

| Item | Status |
|------|--------|
| TASK-25/26 — Windows hotkey + paste | Stub on Windows; real hardware test pending |
| TASK-48 — CoreML / Neural Engine | Phase 1 built; phase 2 blocked on dyld-init hang — mitigated via Metal-only default + preflight guard |
| Developer ID signing + notarization | Deferred until credentials available |
| Parakeet v3 multilingual | In catalog; end-to-end not user-confirmed |

## Backend tradeoffs

- **Parakeet** — fastest English; raw output lowercase/unpunctuated (Chaperone normalizes)
- **Whisper** — multilingual, best accuracy; Silero VAD pre-filter when model bundled
- **Moonshine** — lowest silence hallucination; English-only

## Recent commits

- `1a41878` — Parakeet default, v3, chunk WAV fix, model naming
- `b77641e` — Moonshine FP32 end-to-end, alt-backend wiring
- `7bdb005` — ort conflict resolved; Moonshine + Parakeet activated
