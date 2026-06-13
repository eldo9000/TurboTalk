# TurboTalk Smoke Test

**Purpose:** Manual verification that the core dictation flows work before publishing a release.

This document has three labeled sections — one per supported platform:

- [macOS smoke test](#macos-smoke-test) — supported for 1.0
- [Windows smoke test](#windows-smoke-test) — supported for 1.0
- [Linux smoke test (X11)](#linux-smoke-test-x11) — 2.0 Linux track

Each section has the same 7-step shape (clean launch → mic permission → push-to-talk → missing model → chaperone fallback → app switch → quit/relaunch). The macOS and Windows sections also include installed-artifact checks that gate publishing a 1.0 release.

---

## macOS smoke test

**Target platform:** macOS (Apple Silicon). All steps assume the app is installed from a DMG.

**Required setup before starting:**

- TurboTalk is installed and appears in /Applications.
- A microphone is connected or the built-in microphone is available.
- A Whisper model file (`.bin` or `.gguf`) has been downloaded and its path is entered in TurboTalk Settings → Model path.
- Ollama is installed on the machine (used in Test 5 only — it will be stopped deliberately during that test).
- TextEdit is available (used as the target app for paste tests).

---

### Test 1 — Clean launch with no prior configuration

**Setup:**

Completely remove any previous TurboTalk configuration so the app starts fresh. In Terminal, run:

```
rm -rf ~/.config/turbotalk
```

Then launch TurboTalk from /Applications.

**Action:**

Open the app. Observe the main window and the menu bar icon.

**Expected:**

- The app launches without crashing.
- The status indicator shows an idle/ready state (not an error).
- A TurboTalk icon appears in the menu bar.
- If the app shows a first-run setup prompt or a settings panel asking for a model path, that is expected and correct.

**If it fails:**

Check Console.app for a crash report from TurboTalk immediately after the launch attempt.

---

### Test 2 — Microphone permission denied

**Setup:**

Revoke TurboTalk's microphone access in System Settings:

1. Open System Settings → Privacy & Security → Microphone.
2. Find TurboTalk in the list and toggle it off.
3. Quit TurboTalk completely (menu bar icon → Quit).
4. Relaunch TurboTalk from /Applications.

**Action:**

Hold the Right Option key (⌥ on the right side of the keyboard) and attempt to speak.

**Expected:**

- The app does not crash.
- A visible message, alert, or status indicator tells you microphone access is required.
- The message gives clear direction — for example, pointing you to System Settings → Privacy & Security → Microphone to re-enable access.
- No transcript is produced.

**If it fails:**

If the app crashes or silently does nothing, check Console.app for an audio permission error. If you do not see TurboTalk in the Microphone list in System Settings, try granting permission first and then revoking it — macOS only adds an app to that list after it has requested access at least once.

---

### Test 3 — Push-to-talk produces a transcript and pastes it

**Setup:**

- Microphone access is granted (System Settings → Privacy & Security → Microphone → TurboTalk is on).
- The Whisper model path is configured in TurboTalk Settings → Model path and the file exists at that path.
- Open TextEdit and create a new plain-text document. Click inside the document so it has keyboard focus.

**Action:**

1. Hold the Right Option key (⌥ right side).
2. While holding it, say "hello world" clearly into the microphone.
3. Release the Right Option key.
4. Wait up to 10 seconds for transcription to complete.

**Expected:**

- While you hold the key, the status indicator shows a recording state (e.g., a colored dot, waveform, or "Recording" label).
- After you release, the status indicator changes to show transcription is in progress.
- Within a few seconds, the text "hello world" (or very close to it) appears at the cursor position in TextEdit.
- The status indicator returns to idle.

**If it fails:**

If nothing appears in TextEdit, check: (1) TextEdit had focus before you pressed the hotkey — click inside the document and try again. (2) The model file exists at the path shown in Settings. (3) The status indicator reached "transcribing" state after key release — if it stayed in "recording" indefinitely, the audio capture may be stalling.

---

### Test 4 — Model file missing shows a clear error

**Setup:**

Open TurboTalk Settings and change the Model path to a file that does not exist (for example, type a path like `/tmp/no-model-here.bin` and save). Keep the Microphone permission granted.

**Action:**

Hold the Right Option key, say a short phrase, and release.

**Expected:**

- The app does not crash.
- A visible error message appears explaining that the model file was not found or could not be loaded.
- The message is specific enough that a tester knows to check the model path in Settings, not to assume something else is wrong.
- No garbled or empty transcript is pasted.

**If it fails:**

If the app silently produces no output and shows no error, the error path is not surfaced to the UI. Note the step and report it.

---

### Test 5 — Chaperone enabled without Ollama running falls back gracefully

**Setup:**

- Restore a valid model path in TurboTalk Settings.
- In TurboTalk Settings, enable the Chaperone / LLM cleanup option.
- Stop Ollama completely. In Terminal, run:

```
killall ollama
```

Confirm Ollama is not running by checking Activity Monitor or running `pgrep ollama` in Terminal (no output means it is stopped).

- Open TextEdit with an empty plain-text document and give it focus.

**Action:**

Hold the Right Option key, say "hello world", and release.

**Expected:**

- The transcription still completes and the raw text is pasted into TextEdit.
- The app does not hang or crash.
- Either: (a) a brief status message indicates that LLM cleanup was skipped and the raw transcript is used, or (b) the text is pasted without the LLM step, with no error shown (silent fallback is acceptable as long as something is pasted).
- The app returns to idle state.

**If it fails:**

If the app hangs indefinitely after key release with Ollama stopped, the Chaperone step is blocking instead of falling back. Note how long it hung before you force-quit, and report it.

---

### Test 6 — App switch during transcription pastes into the correct window

**Setup:**

- Open two apps side by side: TextEdit (with a plain-text document) and another text-accepting app such as Notes or a browser's address bar.
- Give TextEdit focus and place the cursor inside the document.

**Action:**

1. Hold the Right Option key and say "switch test phrase".
2. While still holding the key (still recording), click into the second app (Notes or browser) to switch focus.
3. Release the Right Option key and wait for transcription.

**Expected:**

- The transcript is pasted into whichever window had focus at the moment the key was released (the second app you clicked into), or the window that had focus when recording began — observe which one actually receives the text.
- The app does not crash.
- The text that appears is legible (not garbled).

Note which window received the paste and confirm it matches the intended behavior described in the app's release notes or any in-app tooltip.

**If it fails:**

If nothing is pasted anywhere, check that the destination window is a text-editable field. If the app crashes during the switch, check Console.app for a signal or panic from TurboTalk.

---

### Test 7 — Quit and relaunch preserves settings and history

**Setup:**

- Settings are configured (model path set, any preferences toggled).
- The Save history setting is enabled. This is the default.
- Complete at least one successful dictation (Test 3) so there is at least one history entry.

**Action:**

1. Click the TurboTalk menu bar icon → Quit (or use Cmd+Q on the main window if present).
2. Wait 3 seconds.
3. Relaunch TurboTalk from /Applications.
4. Open TurboTalk Settings.
5. Open the history view (if one exists in the UI).

**Expected:**

- The model path and any other settings you configured are still present after relaunch — nothing is reset to defaults.
- The history view shows the dictation(s) performed before quitting.
- `~/.config/turbotalk/history.json` exists and contains a plain JSON array capped at the newest 50 entries.
- The app is in idle/ready state and is ready to dictate again without any re-setup.

**If it fails:**

In Terminal, run:

```
ls ~/.config/turbotalk/
```

If the directory is empty or missing, settings were not written to disk on quit. The expected settings file is `config.toml`; the expected history file is `history.json` when Save history is enabled. If the directory has files but settings appear blank after relaunch, there may be a read error on startup. Note what you see and include it in your report.

---

### Reporting a failure (macOS)

When you hit a step that does not match the Expected behavior:

1. Note the test number and the specific Expected line that was not met.
2. In TurboTalk, open **Settings → Copy diagnostics** (or equivalent) and paste the copied text into your bug report. This includes version info, model path, and recent log lines.
3. If the app crashed, open Console.app, filter by "TurboTalk", and copy the crash entry.
4. Send the test number, what you actually saw, and the diagnostics text to the developer.

---

### Installed-artifact smoke test (macOS)

Run this section after every release build (unsigned/ad-hoc DMG for 1.0) and **before publishing** the release. The 7 dev-build tests above catch code regressions; this section catches packaging-layer regressions that only appear once the app is installed from a real DMG. It covers the macOS permission prompt flow when launched from `/Applications`, one end-to-end dictation, and the documented uninstall + data cleanup path. Skipping this section is how broken DMGs reach users.

> **Signing status determines first-run behavior.** The 1.0 steps below describe the **unsigned/ad-hoc** case (the default when `APPLE_SIGNING_IDENTITY` is not configured). If signing secrets were configured in CI, the artifact is Developer-ID-signed and notarized: skip the right-click → Open trick in step 4 (double-click works), and step 2 shows `accepted` with `source=Notarized Developer ID` instead of `rejected`. See `RELEASING.md` → [Signing secrets reference](../RELEASING.md#signing-secrets-reference) for how to configure CI signing.

#### Prerequisites

- An unsigned/ad-hoc DMG sitting in `dist-artifacts/` or downloaded from the GitHub release workflow, along with its matching `.sha256` file. The DMG must be the actual artifact you intend to publish, not a stale local build.
- **A clean macOS user account with no prior TurboTalk install.** Either:
  - (a) A fresh macOS VM, or
  - (b) A new local user account on the maintainer's Mac (System Settings → Users & Groups → Add Account), then log into that account before starting.

> **Do not run this on the maintainer's daily-driver account.** That account already has Microphone and Accessibility grants for TurboTalk cached, and may have stale data under `~/.config/turbotalk/` or `~/Library/Application Support/`. Running the test there silently passes the failures it is designed to catch.

#### Steps

1. **Verify checksum before installing.**

   **Action:** In Terminal, `cd` into `dist-artifacts/` (or wherever the DMG and its `.sha256` sit together) and run:

   ```
   shasum -a 256 -c TurboTalk-<version>-macos-arm64.dmg.sha256
   ```

   **Expected:** `TurboTalk-<version>-macos-arm64.dmg: OK`. Anything else (`FAILED`, missing file, wrong digest) means the DMG is corrupt or mismatched — stop here and do not install.

2. **Verify Gatekeeper status (ad-hoc expected).**

   **Action:** From Terminal, run:

   ```
   spctl -a -t open --context context:primary-signature -v TurboTalk-<version>-macos-arm64.dmg
   ```

   **Expected (unsigned 1.0):** `rejected` with `source=no usable signature` or similar. The right-click → Open trick in step 4 is how users get past it.

   **Expected (signed release):** `accepted` with `source=Notarized Developer ID`. The DMG is Developer-ID-signed and Apple-notarized; double-click launch in step 4 works without the Gatekeeper dialog.

3. **Mount and install.**

   **Action:** Double-click the DMG to mount it. Drag `Turbo Talk.app` into `/Applications`. Eject the DMG from Finder.

   **Expected:** `Turbo Talk.app` appears in `/Applications` and is launchable from Finder or Spotlight.

4. **First launch (right-click → Open).**

   **Action:** Open `/Applications`, **right-click** (or Control-click) `Turbo Talk.app`, and choose **Open**. macOS will show a Gatekeeper warning ("Apple cannot verify…"). Click **Open** in that dialog. If macOS shows only a refusal dialog, open System Settings → Privacy & Security and click **Open Anyway** for Turbo Talk, then try again.

   **Expected:** The app window appears after you click Open in the Gatekeeper dialog. A normal double-click on first launch will refuse; that is expected for the ad-hoc 1.0 release. After the right-click → Open trick has been used once, future double-clicks work normally.

5. **Microphone permission prompt.**

   **Action:** Hold the push-to-talk hotkey (Right Option by default) and speak briefly.

   **Expected:** macOS shows a system permission prompt requesting Microphone access for TurboTalk. Click "Allow." If no prompt appears and recording also does not work, this is a FAIL — the entitlement or `Info.plist` `NSMicrophoneUsageDescription` is missing from the bundle.

6. **Accessibility permission prompt.**

   **Action:** TurboTalk should either prompt for Accessibility / Input Monitoring on its own or display a clear in-app message explaining that it is required and how to grant it. Open System Settings → Privacy & Security → Accessibility, find TurboTalk in the list, and toggle it on. Quit and relaunch TurboTalk if the in-app guidance asks you to.

   **Expected:** TurboTalk appears in the Accessibility list and can be toggled on. After granting and (if needed) relaunching, TurboTalk no longer surfaces the "Accessibility required" message.

7. **End-to-end dictation.**

   **Action:** Open TextEdit (or any text-editable field), click into the document so it has focus, hold the push-to-talk hotkey, say "hello world" clearly, and release.

   **Expected:** The text "hello world" (or very close) appears at the cursor position in TextEdit within roughly 2 seconds of releasing the key. The app returns to idle.

8. **Verify local data path.**

   **Action:** In Terminal, run:

   ```
   ls ~/.config/turbotalk/
   ```

   **Expected:** The directory exists and contains the expected config file (`config.toml`) and, with the default Save history setting enabled, `history.json` — matching the paths documented in `PRIVACY.md` → "How to delete everything." If Save history was disabled during testing, `history.json` may be absent or unchanged. If the directory is missing entirely after a successful dictation, settings persistence is broken when launched from `/Applications`.

9. **Quit and relaunch.**

   **Action:** Quit TurboTalk completely (menu bar icon → Quit). Wait a few seconds, then relaunch from `/Applications/Turbo Talk.app`.

   **Expected:** App launches clean. Settings are still present (model path, hotkey, etc.). Microphone and Accessibility permissions are **not** re-prompted. The app reaches idle/ready state without any first-run prompts.

10. **Uninstall.**

    **Action:** Drag `/Applications/Turbo Talk.app` to the Trash. Empty the Trash if you want to fully complete the removal.

    **Expected:** `Turbo Talk.app` is gone from `/Applications`.

11. **Verify data cleanup path.**

    **Action:** Follow `PRIVACY.md` → "How to delete everything." Specifically:

    - If you enabled autostart at any point, run `launchctl unload ~/Library/LaunchAgents/com.turbotalk.dictation.plist` and then delete that plist file.
    - Delete `~/.config/turbotalk/config.toml`.
    - Delete `~/.config/turbotalk/history.json`.
    - Delete `~/.config/turbotalk/models/` (entire directory, or whichever model path you configured).

    Then verify nothing remains:

    ```
    ls ~/.config/turbotalk/ 2>/dev/null
    ls ~/Library/LaunchAgents/com.turbotalk.dictation.plist 2>/dev/null
    ```

    **Expected:** Both commands report no such file or directory. Every path PRIVACY.md lists is gone after running its documented commands. If any path remains that PRIVACY.md does not mention, that is a documentation gap — note it for follow-up.

#### Pass/fail recording

Record the outcome of this run in `SESSION-STATUS.md` under the release entry. On pass, note the macOS version you tested against (e.g., "Installed-artifact smoke test: PASS on macOS 14.5"). On fail, note **which numbered step failed** and **what you actually observed**, not just "failed" — that observation is what the next debugging session starts from.

---

## Windows smoke test

> **Status:** Windows is a 1.0 supported target. The installer is unsigned, so SmartScreen is expected on first run.

**Target platform:** Windows 10 (1809+) or Windows 11, x64. All steps assume the app is installed from the NSIS `.exe` installer.

**Required setup before starting:**

- TurboTalk is installed (see Test 1 below).
- A microphone is connected or the built-in microphone is available.
- WebView2 runtime is present (default on Windows 11; Windows 10 users may need the Edge Evergreen installer — see `README.md`).
- A Whisper model file (`.bin` or `.gguf`) has been downloaded and its path is entered in TurboTalk Settings → Model path.
- Ollama is installed on the machine (used in Test 5 only — it will be stopped deliberately during that test).
- Notepad is available (used as the target app for paste tests).

---

### Test W1 — Clean launch with no prior configuration

**Setup:**

Remove any previous TurboTalk configuration. In PowerShell, run:

```
Remove-Item -Recurse -Force "$env:APPDATA\turbotalk" -ErrorAction SilentlyContinue
```

Then run the installer: double-click `TurboTalk-<version>-windows-x64-setup.exe`. Windows SmartScreen will show **"Windows protected your PC"**. Click **More info → Run anyway**. Complete the installer. Launch TurboTalk from the Start menu.

**Action:**

Open the app. Observe the main window and the system tray icon.

**Expected:**

- The app launches without crashing.
- The status indicator shows an idle/ready state.
- A TurboTalk icon appears in the system tray (notification area).
- If a first-run setup prompt or a settings panel asks for a model path, that is expected.

**If it fails:**

Check Event Viewer → Windows Logs → Application for a TurboTalk crash entry around the launch attempt time.

---

### Test W2 — Microphone permission denied

**Setup:**

Revoke TurboTalk's microphone access:

1. Open Settings → Privacy & security → Microphone.
2. Find TurboTalk in the per-app list and toggle it off (or toggle "Let apps access your microphone" off if TurboTalk is not listed yet).
3. Quit TurboTalk completely (right-click tray icon → Quit).
4. Relaunch TurboTalk from the Start menu.

**Action:**

Hold the Right Alt key (default trigger) and attempt to speak.

**Expected:**

- The app does not crash.
- A visible message tells you microphone access is required, with direction to Settings → Privacy & security → Microphone.
- No transcript is produced.

**If it fails:**

If the app crashes or silently does nothing, check Event Viewer for an audio permission error. If TurboTalk does not appear in the Microphone list, grant access first, then revoke and retry — Windows only lists apps after they have requested access.

---

### Test W3 — Push-to-talk produces a transcript and pastes it

**Setup:**

- Microphone access is granted.
- The Whisper model path is configured in Settings → Model path and the file exists.
- Open Notepad and create a new document. Click inside it so it has focus.

**Action:**

1. Hold the Right Alt key.
2. While holding it, say "hello world" clearly.
3. Release Right Alt.
4. Wait up to 10 seconds for transcription.

**Expected:**

- While holding, status indicator shows recording state.
- After release, status changes to "transcribing".
- Within a few seconds, "hello world" appears at the cursor position in Notepad.
- Status returns to idle.

**If it fails:**

Check (1) Notepad had focus before the hotkey was pressed, (2) the model file exists at the configured path, (3) the bundled `whisper-cli.exe` sidecar is present in the install directory.

---

### Test W4 — Model file missing shows a clear error

**Setup:**

In Settings → Model path, enter a path that does not exist (e.g., `C:\no-model-here.bin`) and save. Keep mic permission granted.

**Action:**

Hold Right Alt, say a short phrase, release.

**Expected:**

- App does not crash.
- A visible error message says the model file was not found.
- No garbled or empty transcript is pasted.

**If it fails:**

If the app produces no output and no error, the error path is not surfaced.

---

### Test W5 — Chaperone enabled without Ollama running falls back gracefully

**Setup:**

- Restore a valid model path.
- Enable Chaperone / LLM cleanup in Settings.
- Stop Ollama. In PowerShell:

```
Stop-Process -Name ollama -Force -ErrorAction SilentlyContinue
```

Confirm with `Get-Process ollama -ErrorAction SilentlyContinue` (no output = stopped).

- Open Notepad with an empty document and give it focus.

**Action:**

Hold Right Alt, say "hello world", release.

**Expected:**

- Transcription completes and raw text pastes into Notepad.
- App does not hang or crash.
- Either a status message indicates cleanup was skipped, or text is pasted silently with no error.
- App returns to idle.

**If it fails:**

If the app hangs indefinitely after key release, the Chaperone step is blocking. Note the duration before force-kill and report it.

---

### Test W6 — App switch during transcription pastes into the correct window

**Setup:**

- Open Notepad and a second text-accepting app (a browser address bar works).
- Give Notepad focus.

**Action:**

1. Hold Right Alt and say "switch test phrase".
2. While still holding, click into the second app to switch focus.
3. Release Right Alt and wait for transcription.

**Expected:**

- Transcript pastes into whichever window had focus at the moment the key was released, or the one that had focus when recording began — observe which one.
- App does not crash.
- Text is legible.

**If it fails:**

If nothing is pasted, confirm the destination is a text-editable field. If the app crashes, check Event Viewer.

---

### Test W7 — Quit and relaunch preserves settings and history

**Setup:**

- Settings are configured.
- At least one successful dictation has been completed.

**Action:**

1. Right-click tray icon → Quit.
2. Wait 3 seconds.
3. Relaunch from Start menu.
4. Open Settings.
5. Open history view.

**Expected:**

- All settings persist.
- History shows the prior dictation(s).
- App is idle/ready, no re-setup needed.

**If it fails:**

In PowerShell:

```
ls "$env:APPDATA\turbotalk\"
```

If empty or missing, settings were not written on quit.

---

### Reporting a failure (Windows)

1. Note the test number and the specific Expected line not met.
2. In TurboTalk → Settings → Copy diagnostics, paste into your report.
3. If the app crashed, copy the relevant Event Viewer entry (Windows Logs → Application, source TurboTalk).
4. Send test number, observed behavior, and diagnostics to the developer.

---

### Installed-artifact smoke test (Windows)

Run this section after every Windows release build and **before publishing** the release. It covers installer launch behavior, one end-to-end dictation from the installed app, quit/relaunch persistence, and uninstall.

#### Prerequisites

- An unsigned Windows installer in `dist-artifacts/` or downloaded from the GitHub release workflow, along with its matching `.sha256` file.
- A clean Windows 10/11 x64 user profile or VM with no prior TurboTalk install.

#### Steps

1. **Verify checksum before installing.**

   **Action:** In PowerShell, run:

   ```powershell
   Get-FileHash .\TurboTalk-<version>-windows-x64-setup.exe -Algorithm SHA256
   ```

   Compare the hash to `TurboTalk-<version>-windows-x64-setup.exe.sha256`.

   **Expected:** The hashes match. Anything else means the installer is corrupt or mismatched; stop here.

2. **Install from the real artifact.**

   **Action:** Double-click the installer. If SmartScreen appears, choose **More info → Run anyway**. Complete the installer and launch TurboTalk from the Start menu.

   **Expected:** The app launches without crashing and reaches the first-run/onboarding flow.

3. **Complete onboarding.**

   **Action:** Grant microphone access if prompted, choose/download a model, and finish onboarding.

   **Expected:** The main app reaches idle/ready state and does not re-open onboarding after completion.

4. **End-to-end dictation.**

   **Action:** Open Notepad, click into a new document, hold Right Alt, say "hello world", and release.

   **Expected:** "hello world" or close appears at the cursor in Notepad, and TurboTalk returns to idle.

5. **Quit and relaunch.**

   **Action:** Quit from the tray menu, wait a few seconds, then relaunch from the Start menu.

   **Expected:** Settings and onboarding completion persist. History shows the prior dictation when history is enabled.

6. **Verify local data path.**

   **Action:** In PowerShell, run:

   ```powershell
   ls "$env:APPDATA\turbotalk\"
   ```

   **Expected:** The directory exists and contains the expected config file and, with history enabled, `history.json`.

7. **Uninstall.**

   **Action:** Uninstall TurboTalk from Windows Settings → Apps, then confirm the Start menu entry is gone.

   **Expected:** The app is removed cleanly. User data may remain under `%APPDATA%\turbotalk` until the documented cleanup path is followed.

#### Pass/fail recording

Record the outcome in `SESSION-STATUS.md` under the release entry. On pass, note Windows version and whether the artifact came from local packaging or CI. On fail, note which numbered step failed and what you observed.

---

## Linux smoke test (X11)

> **Status:** Smoke test will be runnable once Linux sidecar, hotkey, and paste paths are bundled and validated on real X11 hardware. Until then, Test 3 onward cannot prove the dictation loop.

> **Wayland is not supported.** This test must be run on an X11 session. On GNOME/KDE/most modern distros that default to Wayland, log out and choose "GNOME on Xorg" / "Plasma (X11)" at the login screen before running this test. See `README.md` → Wayland note.

**Target platform:** Linux x64 on an X11 session. All steps assume the app is run from the `.AppImage` artifact.

**Required setup before starting:**

- The AppImage is downloaded and `chmod +x` has been applied.
- FUSE is installed (`libfuse2` on Debian/Ubuntu; AppImage requires it).
- A microphone is connected or available.
- A Whisper model file (`.bin` or `.gguf`) has been downloaded and its path is entered in Settings → Model path.
- Ollama is installed (used in Test 5 only).
- A text editor that accepts pasted text is open — `gedit` is fine; any GTK or Qt editor works.

---

### Test L1 — Clean launch with no prior configuration

**Setup:**

Remove any previous TurboTalk configuration:

```
rm -rf ~/.config/turbotalk
```

Make the AppImage executable and run it:

```
chmod +x TurboTalk-<version>-linux-x64.AppImage
./TurboTalk-<version>-linux-x64.AppImage
```

**Action:**

Observe the main window and the tray icon (if your DE supports tray icons — GNOME may require the AppIndicator extension).

**Expected:**

- App launches without crashing.
- Status indicator shows idle/ready.
- If your DE supports tray icons, a TurboTalk icon appears.
- A first-run prompt asking for a model path is expected.

**If it fails:**

Run the AppImage from a terminal so any panic or missing-library error is visible on stderr. Common cause: missing `libwebkit2gtk-4.1-0` — install it with your distro's package manager.

---

### Test L2 — Microphone permission denied

> Linux/X11 has no per-app microphone permission system the way macOS or Windows do. To exercise the failure path, simulate it by muting the input device or by routing the input to a non-existent device.

**Setup:**

In `pavucontrol` (PulseAudio Volume Control) or your distro's sound settings, mute the input device or set TurboTalk's input source to "no input."

**Action:**

Hold Right Alt, attempt to speak, release.

**Expected:**

- App does not crash.
- A visible message tells you no usable audio was captured (low-volume / silent input warning).
- No transcript is produced.

**If it fails:**

Check the terminal stderr from the AppImage. If the app crashes with an audio backend error, the cpal Linux backend is failing — note distro and Pulse/Pipewire version.

---

### Test L3 — Push-to-talk produces a transcript and pastes it

**Setup:**

- Audio input is unmuted.
- Whisper model path is set and the file exists.
- Open `gedit` (or your editor) with a new document. Click inside so it has focus.

**Action:**

1. Hold Right Alt.
2. Say "hello world" clearly.
3. Release Right Alt.
4. Wait up to 10 seconds.

**Expected:**

- Status indicator shows recording while holding.
- After release, status shows transcribing.
- "hello world" appears at the cursor in gedit.
- Status returns to idle.

**If it fails:**

Check (1) gedit had focus, (2) you are on an X11 session — `echo $XDG_SESSION_TYPE` must say `x11`, (3) the model file exists, (4) the bundled `whisper-cli` Linux sidecar is present inside the AppImage's `usr/bin/`.

---

### Test L4 — Model file missing shows a clear error

**Setup:**

In Settings → Model path, enter `/tmp/no-model-here.bin` and save.

**Action:**

Hold Right Alt, say a short phrase, release.

**Expected:**

- App does not crash.
- A clear "model not found" error is shown.
- No garbled or empty transcript is pasted.

**If it fails:**

Silent failure means the error path is not surfaced.

---

### Test L5 — Chaperone enabled without Ollama running falls back gracefully

**Setup:**

- Restore a valid model path.
- Enable Chaperone in Settings.
- Stop Ollama:

```
killall ollama
```

Confirm with `pgrep ollama` (no output = stopped).

- Open gedit with an empty doc, give it focus.

**Action:**

Hold Right Alt, say "hello world", release.

**Expected:**

- Transcription completes and raw text pastes into gedit.
- App does not hang or crash.
- Either a status message indicates cleanup was skipped, or paste happens silently.
- App returns to idle.

**If it fails:**

If the app hangs indefinitely, the Chaperone step is blocking. Note duration before force-kill.

---

### Test L6 — App switch during transcription pastes into the correct window

**Setup:**

- Open gedit and a second text-accepting app (a terminal accepting text, a browser address bar, etc.).
- Give gedit focus.

**Action:**

1. Hold Right Alt and say "switch test phrase".
2. While still holding, click into the second app.
3. Release Right Alt.

**Expected:**

- Transcript pastes into whichever window had focus at the moment the key was released, or the one that had focus when recording began — observe which.
- App does not crash.
- Text is legible.

**If it fails:**

If nothing is pasted, the X11 paste injection (xdotool/xtest path) is failing — note the WM/DE.

---

### Test L7 — Quit and relaunch preserves settings and history

**Setup:**

- Settings configured.
- At least one successful dictation completed.

**Action:**

1. Quit TurboTalk (tray icon → Quit, or close all windows).
2. Wait 3 seconds.
3. Relaunch the AppImage.
4. Open Settings.
5. Open history view.

**Expected:**

- Settings persist.
- History shows prior dictation(s).
- App reaches idle/ready, no re-setup needed.

**If it fails:**

```
ls ~/.config/turbotalk/
```

Empty or missing means settings were not written on quit.

---

### Reporting a failure (Linux)

1. Note the test number and the specific Expected line not met.
2. In TurboTalk → Settings → Copy diagnostics, paste into your report.
3. Include distro name + version, desktop environment, and `echo $XDG_SESSION_TYPE` output.
4. If the app crashed, attach the stderr output from running the AppImage in a terminal.
5. Send test number, observed behavior, and diagnostics to the developer.
