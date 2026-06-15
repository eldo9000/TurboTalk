# TurboTalk — Session Status

**Last updated:** 2026-06-15  
**Current state:** Removed auto-punctuation from the cleanup pipeline. Whisper naturally adds trailing periods to every utterance/segment, and `strip_whisper_artifacts` wasn't stripping bare `.` — only ` .` (space+dots) and `...`. Combined with the segment transcription pipeline, this produced periods at every silence-boundary pause, breaking sentences apart. Fix: `strip_whisper_artifacts` now also strips bare trailing `.`, and `join_segments` strips trailing periods from each segment before assembly.

## Open backlog

| Item | Status |
|------|--------|
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
- **Moonshine** — lowest silence hallucination; English-only

## Recent commits

- `e5aae45` — docs: log v0.9.8 release CI signing env failure in CI-FAIL-LADDER
- `b40fda9` — fix(ci): only set Apple notarization env vars when Developer ID credentials present
- `3521733` — fix(ci): fall back to ad-hoc identity instead of empty string in release workflow
- `9c6b3ca` — chore(release): bump to 0.9.8

## This session

**Event:** User requested a dedicated status window for warm-up/arming and rejection/flaky feedback, separate from the ephemeral recording overlay. The new window should be clickable, 100%-opaque, and use the yellow/red pulsing border aesthetic from the existing warm-up tile.

**Design:**
1. **`src/Status.svelte`** — new Svelte component loaded in a separate Tauri window (`label: "status"`). Listens for `ptt-armed` (yellow pulsing "Starting…"), `ptt-arm-failed` (red pulsing "Model failed to load"), `transcription-rejected` (red pulsing with dismiss button for flaky/blocked pastes), and `recording-discarded` with `empty-final-text` payload (red pulsing "Nothing to paste").
2. **Window config** — added to `tauri.conf.json` as a transparent, always-on-top, non-decorated window (280×80), starting hidden. Capability file provides event listen/unlisten + window hide/show permissions.
3. **No `set_ignore_cursor_events`** — the status window accepts mouse clicks, so the dismiss button works.
4. **Positioning** — macOS version centers the window on the cursor's monitor (same cursor-position logic as overlay/splash). Non-macOS version no-ops (window is centered via tauri.conf.json).
5. **Arming removed from overlay** — `Overlay.svelte` no longer handles `ptt-armed`/`ptt-arm-failed`; the arming class, template block, and spinner CSS are removed. The overlay now only shows recording and transcribing states.
6. **Event routing** — the new window is wired in `main.js`; the Rust `lib.rs` setup positions it at startup.

**Files changed:**
- `src/Status.svelte` — new (yellow/red status overlay)
- `src-tauri/tauri.conf.json` — added status window config
- `src-tauri/capabilities/status.json` — new (window permissions)
- `src/main.js` — added status window routing
- `src-tauri/src/lib.rs` — added `reposition_status_to_cursor` + status window setup
- `src/Overlay.svelte` — removed arming mode/state/CSS

## Next action

Test the status window end-to-end: trigger a warm-up (cold start with ptt-armed), a rejection (via the Test rejection button in History), and verify the window shows correct colors, animations, and dismiss behavior.
