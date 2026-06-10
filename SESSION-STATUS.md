# TurboTalk — Session Status

**Last updated:** 2026-06-10  
**Current state:** Recording race fixed; flat-waveform diagnostic reported.

1. **Recording race fix (critical)** — `frontmost_app()` (osascript, ~1–2 s) was called after `rec.start()` but before `ptt-down` emit. During the block, `ptt_up` could stop+discard the recording while the UI still showed "Recording" with a flat waveform. Fixed by moving `frontmost_app()` before `rec.start()` — now the recorder stays `Ready` during the block, and `CANCEL_PENDING` catches quick taps.

2. **Job_id=6 flat waveform note** — user cancelled ~1 s in with a flat waveform on "Elliott's AirPods" (Bluetooth). This may be a legitimate AirPods HFP SCO link establishment delay, or a secondary symptom of the stale UI state from the race above. Not addressed in this fix — monitor and repro on next build.

3. **TASK-61 data_dir migration is NOT the cause** — the path change is unrelated; the race was introduced with TASK-16 focus-change detection (`frontmost_app()`).

1. **IOHIDManager raw HID mouse listener** (new) — reads raw HID Button usage values via IOHIDManager, bypassing CGEventTap entirely for mouse buttons. This works even when Logi Options+ (or similar driver software) intercepts buttons at IOKit — IOKit delivers HID reports to ALL registered IOHIDManager clients, so Logi cannot block us. Mouse back/forward/middle now work regardless of mouse software. No configuration needed beyond selecting the button in Settings.

2. **F13–F19 function key hotkeys** (new) — alternative PTT path for users whose mouse software lets them map buttons to keystrokes. Works as the fallback if the IOHIDManager path is unavailable.

3. **`finish_guarded()` hardening** — six `rec.finish()` call sites changed to `rec.finish_guarded()` to prevent cancel + rapid re-press races from corrupting the next job's state machine. Segment recovery now carries `final_text` in the `recording-recovered` payload (fixes TASK-57 — partial chunks no longer lost).

4. **UI updates** — mouse buttons (back/forward/middle) and F13–F24 added to hotkey dropdown. Logi Options+ warning banner when a mouse button is selected. Numpad keys removed from UI; existing configs auto-migrate to platform default.

**Next action:** Execute TASK-61 — close 10 Windows platform gaps. Ready for agent dispatch.

## Open backlog

| Item | Status |
|------|--------|
| **TASK-61 — Windows platform gaps** | **Ready** — 10 mechanical fixes across 6–8 files. Single agent session. |
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
