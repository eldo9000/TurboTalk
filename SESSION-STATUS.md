# TurboTalk — Session Status

**Last updated:** 2026-06-02  
**Current state:** Pre-release scan sweep complete (diagnostics/privacy, packaging/update, regression). Reports in `docs/pre-release-scans/SCAN-{1,2,3}-FINDINGS.md`. Scan 1 (privacy) clean — dictated text cannot reach an uploaded report. Four fixes applied and staged: (1) updater artifacts now enabled in the CI release build only (via `--config`; the committed `createUpdaterArtifacts` stays `false` so local `npm run package` remains DMG-only) — was blocking the release workflow's Locate step; (2) macOS bundle codesign now gated in `verify-macos-bundle.mjs` + wired into the mac release job (was a `|| true` no-op); (3) device-lost mid-hold no longer defers a fake `recording-cancelled` onto the next press (`lib.rs:2286` arms ptt_up suppression in hold mode); (4) launch-agent plist id corrected in docs (`com.*` → `io.librewin.turbotalk`) + privacy guard comment at `record_client_event`.

**Next action:** Two verifications remain, both off the local box — (a) trigger a release CI run to confirm updater artifacts emit + codesign gate passes on the signed bundle; (b) manually verify the device-lost fix: hold key → unplug/switch mic mid-recording → release → next press must start a normal recording (no instant "recording-cancelled").

## Open backlog

| Item | Status |
|------|--------|
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
