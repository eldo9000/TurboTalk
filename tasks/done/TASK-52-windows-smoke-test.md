# TASK-52: Windows app install + non-dictation smoke test

## Goal
TurboTalk installs and launches in the UTM Windows VM, the tray icon appears, settings persist across restarts, and the Ollama ping check passes (or produces the expected "Ollama not running" error) — confirming the non-dictation paths work end-to-end on Windows.

## Context
This task runs after TASK-50 (installer in hand) and TASK-51 (VM running, x64 emulation confirmed). The Windows runtime impls for paste (`paste.rs`: arboard + enigo) and hotkey (`hotkey.rs`: rdev `SetWindowsHookEx`) exist in the codebase and compile — but have never been run on a real Windows machine. The transcription loop (whisper sidecar) is explicitly out of scope for this sprint; we are validating the surrounding infrastructure first.

Settings persist at `%APPDATA%\librewin\turbotalk\config.toml` on Windows (the `dirs` crate maps `~/.config/librewin/turbotalk/` to `%APPDATA%\librewin\turbotalk\` on Windows). The Ollama API endpoint is `http://127.0.0.1:11434`.

This task is human-executed inside the UTM VM. The agent's job is to produce the test script and record results.

## In scope
- Writing the structured test script as `docs/WINDOWS-SMOKE-TEST.md`
- Recording the human's pass/fail results into a results file at `docs/WINDOWS-SMOKE-RESULTS.md`
- No source changes

## Out of scope
- Transcription / whisper sidecar testing
- Hotkey capture and paste injection — those are TASK-53
- Any code fixes — if something fails, record it and move to TASK-53 / new fix tasks
- Codesigning; unsigned installer is expected to show a SmartScreen warning

## Steps

### Agent steps
1. Write `docs/WINDOWS-SMOKE-TEST.md` with the test script below.
2. After the human executes the test and reports results, write `docs/WINDOWS-SMOKE-RESULTS.md` with each item marked PASS / FAIL / SKIP + any error text observed.

### Test script for `docs/WINDOWS-SMOKE-TEST.md`

**Pre-conditions**
- UTM VM running, SPICE guest tools installed, shared folder accessible
- TurboTalk NSIS installer available (from shared folder)

**Test 1 — Install**
- Double-click installer, click through NSIS prompts
- Expected: SmartScreen warning "Windows protected your PC" → click "More info" → "Run anyway"
- Expected: Install completes, shortcut appears on Desktop and/or Start menu
- PASS if: install wizard completes without error

**Test 2 — First launch**
- Launch TurboTalk from Desktop shortcut
- Expected: app window opens, tray icon appears in system tray
- Expected: Chaperone / onboarding pane may appear (this is normal for first run)
- PASS if: window opens; tray icon is visible

**Test 3 — Settings tab**
- Click Settings tab
- Expected: Whisper binary path, model path, hotkey, cleanup mode fields are visible
- Change the "Cleanup mode" dropdown from its current value to a different value and back
- Quit the app (right-click tray → Quit)
- Relaunch the app
- PASS if: the changed cleanup mode value is still set after relaunch (confirms `%APPDATA%\librewin\turbotalk\config.toml` is being read/written)

**Test 4 — Config file on disk**
- Open File Explorer → navigate to `%APPDATA%\librewin\turbotalk\`
- PASS if: `config.toml` exists and contains readable TOML content

**Test 5 — Ollama ping (no Ollama installed)**
- In Settings → Advanced or Modes tab, look for the Ollama connection indicator
- Expected: "Ollama not running" or red/disconnected status
- PASS if: the indicator is present and shows a clear disconnected state (not a blank crash or missing UI)

**Test 6 — Ollama ping (Ollama installed)**
- Download and install Ollama for Windows from https://ollama.com/download/windows
- Launch Ollama (system tray icon appears)
- Return to TurboTalk → Modes tab → Advanced
- Expected: Ollama indicator turns green or shows "Connected"
- PASS if: TurboTalk detects Ollama without restart, OR detects it after a tab switch / window re-focus

**Test 7 — Tray behavior**
- Click the TurboTalk tray icon: window should show/hide
- Right-click tray icon: menu should show "Show" and "Quit" options
- Click the window close button (X): window should hide to tray, NOT quit
- Right-click tray → Quit: app should exit
- PASS if: all four behaviors work as described

**Test 8 — History tab**
- Open History tab
- Expected: empty list (no dictations yet)
- PASS if: History tab renders without error

### Human steps (must be executed by the user inside the UTM VM)
- Execute all 8 tests in order
- For each test, note PASS / FAIL and any error messages, dialog text, or unexpected behavior
- Report results back to the agent

## Success signal
`docs/WINDOWS-SMOKE-RESULTS.md` exists with results for all 8 tests. Tests 1–4 and 7 all pass (these are the minimum viable non-dictation paths). Tests 5–6 and 8 are bonus confirmation.

## Notes
- If Test 2 fails (app crashes on launch), capture the Windows Event Log entry: Event Viewer → Windows Logs → Application → look for TurboTalk errors. This is likely a missing VC++ runtime or a DLL load failure.
- If Test 3 fails (settings don't persist), confirm the `%APPDATA%` path is writable (not a permissions issue).
- Ollama for Windows requires Windows 10 22H2 or later; Windows 11 ARM satisfies this.
