# TurboTalk — Session Status

**Last updated:** 2026-06-13  
**Current state:** Two fixes landed: Parakeet `vocab.txt` SHA-256 hash corrected (was stale, causing download failure during onboarding), and PTT hotkey now silently suppressed while the welcome/onboarding screen is visible.

## Open backlog

| Item | Status |
|------|--------|
| **Parakeet vocab.txt SHA-256 stale** | **Fixed** — hashes updated for tdt-0.6b-v2 and tdt-0.6b-v3; verified against live HuggingFace content. |
| **Hotkey fires during onboarding** | **Fixed** — `ONBOARDING_ACTIVE` atomic gate in `ptt_down`; cleared on ready startup or `clear_force_onboarding`. |
| **TASK-61 — Windows platform gaps** | **Complete** — all 10 items already implemented, verified in source. |
| **Manual device-lost repro** | **TODO** — verify `lib.rs:2286` fix: hold key → unplug/switch mic mid-recording → release → next press must start a normal recording (no instant "recording-cancelled"). Fix is verified-by-construction only; runtime not yet observed. |
| Release CI run | **Complete** — v0.9.8 builds, codesign, updater artifacts all green ([#27322438132](https://github.com/eldo9000/TurboTalk-App/actions/runs/27322438132)) |
| TASK-25/26 — Windows hotkey + paste | Complete — Windows full dictation loop marked complete per user confirmation. |
| TASK-57 — Segment recovery pollutes history | Fixed — partial chunks no longer added to history |
| TASK-48 — CoreML / Neural Engine | Phase 1 built; phase 2 blocked on dyld-init hang — mitigated via Metal-only default + preflight guard |
| Developer ID / Authenticode signing | Deferred — 1.0 will ship unsigned/ad-hoc. |
| Parakeet v3 multilingual | In catalog; end-to-end not user-confirmed |
| **June 12 Parakeet hang/cancel** | **Patched** — logs showed job 40 (1.2s audio) entered Transcribing at 13:23:06 local, next press was ignored as busy at 13:23:09, cancel invalidated worker at 13:23:10, repeated presses during warmup were cancelled, model finished reload at 13:23:22, app restarted at 13:23:28. Fix keeps in-process ONNX workers reusable after cancel; compile checks pass. |
| **June 12 repeat-filter paste-through** | **Patched in tree / installed app stale** — packaged log at 13:38:30 local shows detector caught `feel feel feel...` as `TrigramRepetition`, but old build logged "continuing to paste". Current `hotkey.rs` blocks rejected transcripts; added `detect_garbage_feel_loop_flagged` regression test. |
| **Cancel paste-through regression** | **Complete** — cancel-after-release suppression marked complete per user confirmation. |
| **Overlay size indicators** | **Complete** — Settings → Indicators exposes Visual Overlay `Small` / `Medium` / `Large`; overlay size modes marked complete per user confirmation. |
| **Beta bug reporting** | **Complete** — Settings → Developer bug-report button flow marked complete per user confirmation. |
| **Main window placement safeguards** | **Complete** — compact min size + monitor work-area clamp added; placement safeguards marked complete per user confirmation. |
| **Installed-artifact smoke** | **Complete** — clean installed-artifact smoke for the 1.0 macOS/Windows path marked complete per user confirmation. |
| **1.0 docs alignment** | **Complete** — README, build, releasing, and smoke-test docs now describe unsigned macOS/Windows 1.0 and Linux as 2.0. |
| **Roadmap refresh** | **Updated** — user-confirmed completed items are checked off; remaining 1.0 proof list is now shorter. |

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
