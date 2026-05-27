# TurboTalk — Session Status

**Last updated:** 2026-05-26  
**Current state:** macOS dictation is feature-complete for personal use. All three backends proven end-to-end: Whisper (multilingual), Moonshine (English, FP32 ONNX), Parakeet (English/multilingual, int8 ONNX). Parakeet is the default for new installs. Whisper Silero VAD pre-filter user-confirmed 2026-05-26 (bundled `ggml-silero-v5.1.2.bin`). Latest commit `35d2157` on main (Windows onboarding platform fix).

**Next action:** User-retest onboarding drag-during-download fix on macOS and Windows. Windows v1.0 path: TASK-25/26 (real hotkey + paste on hardware).

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
