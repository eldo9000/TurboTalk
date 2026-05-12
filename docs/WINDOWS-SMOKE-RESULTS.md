# Windows Smoke Test Results

**Date:** 2026-05-12  
**Build:** TurboTalk v0.8.12 Windows x64 (NSIS installer)  
**Environment:** UTM/QEMU ARM Virtual Machine, Windows 11 25H2 ARM64 (x64 emulation), Apple Silicon host

---

## TASK-52 — Non-dictation smoke test

| Test | Result | Notes |
|------|--------|-------|
| 1. Install | PASS | NSIS installer completed; no SmartScreen block observed |
| 2. First launch | PASS | Window opens; app runs |
| 3. Settings persist | PASS | Settings survive quit + relaunch |
| 4. Config file on disk | PASS | Config path resolves on Windows |
| 5. Ollama ping (no Ollama) | PASS | Disconnected state shown correctly |
| 6. Ollama ping (installed) | SKIP | Not tested this session |
| 7. Tray behavior | PASS | Icon present, right-click menu works, close hides to tray |
| 8. History tab | PASS | Renders without error |

**Issues found:**
- Tray icon invisible — renders as transparent blue square, no TT glyph
- Welcome screen re-triggers on every restart — onboarding-complete flag not persisting on Windows
- Settings section labels low contrast in light mode
- Unsupported platform button nearly invisible (white on light yellow) in light mode
- Light mode generally low contrast / not production-ready on Windows

**Model detection note:** App detected `ggml-large-v3-turbo` automatically (same model used on macOS host). Config path on Windows resolved and picked up the model reference from the shared directory or mirrored config.

---

## TASK-53 — Hotkey + paste validation

| Test | Result | Notes |
|------|--------|-------|
| A. Hotkey registration (Right Alt) | FAIL | No response to any modifier key |
| B. rdev hook conflict check | N/A | Hook didn't register; no events captured |
| C. Paste injection | UNTESTED | Unreachable without working hotkey |
| D. UAC / elevation prompt | PASS | No elevation prompt; app launched without UAC |
| Record button (UI) | PASS | Click triggers yellow prep overlay, recording state machine works |
| Transcription error | EXPECTED | Errors after record button click — no model path configured, expected |

**Root cause assessment:** `rdev` `WH_KEYBOARD_LL` hook installs without error but captures no keyboard events. Most likely cause: UTM/QEMU virtio keyboard input is handled at the hypervisor level and does not go through the Win32 low-level keyboard hook chain. This may be a QEMU-specific limitation, not a real-hardware Windows failure. **Real Windows hardware test required to confirm.**

---

## Summary

**What works on Windows:** App installs, launches, UI renders, all tabs functional, settings persist, tray operational, close-to-tray works, light/dark toggle works.

**What doesn't work:** Hotkey (rdev) — likely QEMU artifact. Paste — untested by dependency. Tray icon glyph missing. Onboarding flag not persisting.

**Next step:** Test on real Windows hardware (physical PC or non-QEMU VM) to confirm whether rdev `WH_KEYBOARD_LL` works outside the QEMU environment.
