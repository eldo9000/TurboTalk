# TurboTalk — Session Status

**Last updated:** 2026-06-10  
**Current state:** Pre-release security audit arc complete — 6 artifacts integrity checks, signing pipeline, file permissions, CSP/icon hardening, URL/token cleanup (TASKS 61–66).

## Open backlog

| Item | Status |
|------|--------|
| **TASK-61 — Windows platform gaps** | **Complete** — all 10 items already implemented, verified in source. |
| **Manual device-lost repro** | **TODO** — verify `lib.rs:2286` fix: hold key → unplug/switch mic mid-recording → release → next press must start a normal recording (no instant "recording-cancelled"). Fix is verified-by-construction only; runtime not yet observed. |
| Release CI run | Pending — confirms updater artifacts emit + codesign gate passes in CI (user-triggered) |
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
