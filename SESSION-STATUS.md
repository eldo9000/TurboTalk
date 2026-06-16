# TurboTalk — Session Status

**Last updated:** 2026-06-16  
**Current state:** `TASK-70` is in progress. First extraction landed: History, Models, and Modes now live in standalone Svelte components, while Settings and the modals stay in `App.svelte` for this pass. The remaining cleanup is already split into task files under `tasks/next/` so the refactor can continue in smaller, reviewable chunks.

## Open backlog

| Item | Status |
|------|--------|
| **Onboarding welcome-screen cleanup** | **Fixed** — `recheckReadiness()` now dismisses onboarding immediately when all gates green (no longer depends on launch-at-login), and `Onboarding.svelte` auto-close no longer requires `launchAtLogin`. Two changes: `src/App.svelte` (initial-mount gate) and `src/Onboarding.svelte` (removed hard requirement from auto-close condition). |
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
- **Moonshine** — retired; legacy configs normalize to Parakeet

## Recent commits

- `e5aae45` — docs: log v0.9.8 release CI signing env failure in CI-FAIL-LADDER
- `b40fda9` — fix(ci): only set Apple notarization env vars when Developer ID credentials present
- `3521733` — fix(ci): fall back to ad-hoc identity instead of empty string in release workflow
- `9c6b3ca` — chore(release): bump to 0.9.8

## This session

**Event:** Re-checked the terminal launch path and then closed the loop on the refactor work. The dev server bind issue was fixed by moving the local dev server to `127.0.0.1:1431` and updating the Tauri dev URL to match, which let `npm run tauri dev` reach the Rust binary and keep running instead of dying during startup.

**Event:** Finished the UI extraction pass far enough to restore launchability cleanly: `App.svelte` now delegates History, Models, and Modes to standalone components, with the remaining shell/settings work left for the next slice.

**Checks:** `npm run tauri dev` started Vite, launched `target/debug/turbotalk`, and the process stayed alive. I also exercised both toggle and hold hotkey modes successfully. `npm run build` and `npm run typecheck` both passed. The remaining startup log line is a recoverable macOS hotkey warning: `CGEventTap failed (accessibility_trusted=false, retry=0)`.

## Next action
Keep TASK-70 moving with the remaining shell/settings split, and separately verify the hotkey permission flow on a machine with Accessibility enabled. After that, the work left is mostly polish rather than correctness.
