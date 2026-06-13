# TurboTalk — Session Status

**Last updated:** 2026-06-13  
**Current state:** Beta bug reporting is prepared in tree. Settings → Developer → Report a bug now calls the shipping `submit_bug_report` path, saves a local report path, opens the report folder through the cross-platform opener for Windows reliability, and includes runtime context, readiness, diagnostics, sanitized config, UI events, session log, and error log while keeping dictated transcript text out. Verified by `npm run typecheck` and `cargo check --manifest-path src-tauri/Cargo.toml`.

## Open backlog

| Item | Status |
|------|--------|
| **TASK-61 — Windows platform gaps** | **Complete** — all 10 items already implemented, verified in source. |
| **Manual device-lost repro** | **TODO** — verify `lib.rs:2286` fix: hold key → unplug/switch mic mid-recording → release → next press must start a normal recording (no instant "recording-cancelled"). Fix is verified-by-construction only; runtime not yet observed. |
| Release CI run | **Complete** — v0.9.8 builds, codesign, updater artifacts all green ([#27322438132](https://github.com/eldo9000/TurboTalk-App/actions/runs/27322438132)) |
| TASK-25/26 — Windows hotkey + paste | Hotkey fix ready for retest; paste still unproven E2E |
| TASK-57 — Segment recovery pollutes history | Fixed — partial chunks no longer added to history |
| TASK-48 — CoreML / Neural Engine | Phase 1 built; phase 2 blocked on dyld-init hang — mitigated via Metal-only default + preflight guard |
| Developer ID signing + notarization | Deferred until credentials available |
| Parakeet v3 multilingual | In catalog; end-to-end not user-confirmed |
| **June 12 Parakeet hang/cancel** | **Patched** — logs showed job 40 (1.2s audio) entered Transcribing at 13:23:06 local, next press was ignored as busy at 13:23:09, cancel invalidated worker at 13:23:10, repeated presses during warmup were cancelled, model finished reload at 13:23:22, app restarted at 13:23:28. Fix keeps in-process ONNX workers reusable after cancel; compile checks pass. |
| **June 12 repeat-filter paste-through** | **Patched in tree / installed app stale** — packaged log at 13:38:30 local shows detector caught `feel feel feel...` as `TrigramRepetition`, but old build logged "continuing to paste". Current `hotkey.rs` blocks rejected transcripts; added `detect_garbage_feel_loop_flagged` regression test. |
| **Overlay size indicators** | **Patched in tree** — Settings → Indicators now exposes Visual Overlay `Small` / `Medium` / `Large`; legacy length counter settings/config/rendering removed; mini closes right-side empty space and pulses its red dot with smoothed normalized mic level; waveform + mini visuals now soft-gate below the existing speech threshold so silence noise floor is not amplified; waveform canvas background is transparent; runtime visual QA in Tauri still pending. |
| **Beta bug reporting** | **Patched in tree** — one-click tester report now uses `submit_bug_report`; local save failures are surfaced; report folder opening no longer shells out to raw `explorer`; report includes enough diagnostics/log context to avoid first-round follow-up questions. Runtime Tauri button proof still pending. |

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

Runtime bug-report proof: run the Tauri app, open Settings → Developer, click Create Bug Report with a short note; success signal is a saved report file containing Runtime context, Readiness, Diagnostics, Config, UI events, Session log, and Error log sections, with no dictated transcript text.
