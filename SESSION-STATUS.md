# TurboTalk — Session Status

**Last updated:** 2026-05-29  
**Current state:** Segment recovery path no longer leaks partial chunk text into history. When tail audio is too short after streaming trim, the recovered segment text is still pasted to the active app but no longer creates a persistent history entry (replaced `transcript` event with `recording-cancelled` in the recovery path).

**Next action:** Rebuild / retest — verify that partial-segment dictations no longer appear as separate history entries when the tail is too short.

## Open backlog

| Item | Status |
|------|--------|
| TASK-25/26 — Windows hotkey + paste | Hotkey fix ready for retest; paste still unproven E2E |
| TASK-57 — Segment recovery pollutes history | Fixed — partial chunks no longer added to history |
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
