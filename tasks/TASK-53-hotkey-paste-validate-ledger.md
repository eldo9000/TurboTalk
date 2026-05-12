# TASK-53: Windows hotkey + paste validation and truth ledger update

## Goal
Right Alt push-to-talk and Ctrl+V paste injection are proven working (or definitively broken) on the UTM Windows VM, and `TRUTH.md` + `SESSION-STATUS.md` are updated to reflect actual Windows status — replacing the stale "stubs only" claim.

## Context
`TRUTH.md` currently states: "Windows hotkey + paste — stubs only (`Err("unsupported platform")`); TASK-25/26." This is stale. The code landed:
- `hotkey.rs` non-mac branch (`#[cfg(not(target_os = "macos"))]`): uses `rdev 0.5` with `SetWindowsHookEx(WH_KEYBOARD_LL)` for global key capture. Right Alt maps to `rdev::Key::AltGr`.
- `paste.rs` non-mac branch: uses `arboard 3` (clipboard write) + `enigo 0.3` (Ctrl+V via `SendInput`). Prior clipboard is saved and restored.

Neither has been proven on a real Windows box. This task runs after TASK-52 (app launches, settings work) and collects the runtime evidence needed to update the truth ledger.

The transcription loop (whisper sidecar + recorder state machine) is not part of this task. We are only validating: does key-down register as PTT? Does key-up trigger paste injection?

## In scope
- Human-executed hotkey + paste tests in the UTM VM (steps below)
- Agent updates `TRUTH.md` and `SESSION-STATUS.md` based on reported results
- If tests pass: close out TASK-25 / TASK-26 references in the ledgers
- If tests fail: document the failure mode and create targeted fix notes

## Out of scope
- Transcription (whisper sidecar) — explicitly deferred until hotkey + paste are proven
- Any code changes — this task is evidence collection only; fixes belong in a new task
- Linux testing

## Steps

### Human steps (must be executed inside the UTM VM)

**Pre-conditions**
- TurboTalk is installed and launches (TASK-52 passed)
- A text editor is open in the VM (Notepad is fine)

**Test A — Hotkey registration**
- With TurboTalk running (window open or hidden to tray), press and hold Right Alt
- Expected: TurboTalk recording indicator appears (red dot in tray or recording overlay)
- Expected: microphone access prompt may appear on first press — allow it
- Release Right Alt
- Expected: indicator disappears (transcribing state, then back to idle — no audio output because no model is configured, but the state machine should still transition)
- PASS if: holding Right Alt triggers the recording indicator; releasing it ends the recording state
- FAIL signal: no visible response to Right Alt, OR Windows language/input method switcher activates instead of PTT

**Test B — rdev hook conflict check**
- With TurboTalk running, confirm that Right Alt still works for its normal Windows function (AltGr for special characters in text fields) when TurboTalk is NOT in recording mode
- Press Right Alt + E in Notepad: on a standard US keyboard this is a no-op; on European layouts it may produce a character
- PASS if: no key events are swallowed when TurboTalk is idle (hotkey should only fire PTT on down/up, not suppress other AltGr combos)
- Note: if AltGr character input is blocked, rdev is consuming the event — this is a known rdev limitation and worth recording

**Test C — Paste injection**
- With Notepad open and cursor in the text area, trigger a dictation cycle:
  - Hold Right Alt (recording starts) → release Right Alt (recording stops → transcription attempted)
  - Since whisper is not configured, transcription will fail with an error
- Alternative simpler test: if there is a "Test paste" debug surface or a way to inject directly, use it
- If no direct paste test is available: accept Test A as the proxy for paste (paste is only called after successful transcription; without a working sidecar the paste path is not reachable in this sprint)
- Record whether a paste error event appears in the TurboTalk UI (expected: "Transcription failed" or similar — not a paste error)

**Test D — Accessibility / UAC prompt**
- Check if Windows prompts for elevated permissions when the app first tries to install the keyboard hook
- PASS if: no elevation prompt; rdev hook installs silently (expected — `WH_KEYBOARD_LL` is a low-level hook that does not require elevation)
- FAIL signal: UAC prompt appears, or hook silently fails with no recording indicator

Report results for A, B, C, D to the agent.

### Agent steps
1. After receiving the human's test results, update `TRUTH.md`:
   - Replace the stale "Windows hotkey + paste — stubs only" line with accurate status
   - If Test A passed: "Windows hotkey — rdev `WH_KEYBOARD_LL` hook confirmed working in UTM Windows 11 ARM (x64 emulation). Right Alt triggers PTT."
   - If Test A failed: "Windows hotkey — rdev `WH_KEYBOARD_LL` hook NOT confirmed; [specific failure description from human]"
   - For paste: if no direct evidence (whisper not configured), note "paste path not reachable without whisper sidecar; arboard + enigo impl in place, untested"
2. Update `SESSION-STATUS.md` "Next action" section to reflect the new known state
3. If Test A + B passed, note that TASK-25/TASK-26 can be marked verified-in-emulation, with real-hardware proof still pending
4. If any test failed, add a new entry to the open backlog with the specific failure and reproduction steps

## Success signal
`TRUTH.md` no longer says "stubs only" for Windows hotkey — it reflects the actual runtime result. `SESSION-STATUS.md` "Next action" is updated. The agent can confirm: `grep -n "stubs only" TRUTH.md` returns no matches.

## Notes
- Right Alt = AltGr on Windows = `rdev::Key::AltGr`. This is the correct mapping for the TurboTalk default hotkey on Windows.
- Windows Defender may block `WH_KEYBOARD_LL` hooks from unsigned binaries. If the hook silently fails, check Windows Security → Virus & Threat Protection → Protection History for any blocked events.
- If rdev hook fails in UTM but the app otherwise works, the fix path is TASK-25: replace rdev with a direct Win32 `SetWindowsHookEx` + `GetMessage` loop (same approach as the macOS CGEventTap replacement). That would be a separate sprint task, not this one.
- enigo `SendInput` for paste: if this fails under x64 emulation, the fallback is `PowerShell Set-Clipboard + SendKeys("{v}")` (same pattern as the macOS osascript branch). Record the failure mode precisely — "enigo init failed" vs. "Ctrl press failed" vs. "paste lands but in wrong window" are different root causes.
