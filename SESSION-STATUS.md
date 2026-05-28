# TurboTalk — Session Status

**Last updated:** 2026-05-26  
**Current state:** macOS dictation feature-complete. Windows hotkey fix landed locally: default Right Control + hold mode, full rdev key mapping, auto-migration from macOS-style `right_option`. Awaiting user retest on real Windows hardware.

**Next action:** Windows test with **Export test log** (Settings → System): flip controls, exercise PTT/hotkey/paste, export report, attach file.

## Open backlog

| Item | Status |
|------|--------|
| TASK-25/26 — Windows hotkey + paste | Hotkey fix ready for retest; paste still unproven E2E |
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
