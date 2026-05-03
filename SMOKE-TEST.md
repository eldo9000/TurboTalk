# TurboTalk Beta Smoke Test

**Purpose:** Manual verification that the 7 core dictation flows work before sharing with beta users.

**Target platform:** macOS (Apple Silicon or Intel). All steps assume the app is installed from a DMG.

**Required setup before starting:**

- TurboTalk is installed and appears in /Applications.
- A microphone is connected or the built-in microphone is available.
- A Whisper model file (`.bin` or `.gguf`) has been downloaded and its path is entered in TurboTalk Settings → Model path.
- Ollama is installed on the machine (used in Test 5 only — it will be stopped deliberately during that test).
- TextEdit is available (used as the target app for paste tests).

---

## Test 1 — Clean launch with no prior configuration

**Setup:**

Completely remove any previous TurboTalk configuration so the app starts fresh. In Terminal, run:

```
rm -rf ~/.config/librewin/turbotalk
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

## Test 2 — Microphone permission denied

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

## Test 3 — Push-to-talk produces a transcript and pastes it

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

## Test 4 — Model file missing shows a clear error

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

## Test 5 — Chaperone enabled without Ollama running falls back gracefully

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

## Test 6 — App switch during transcription pastes into the correct window

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

## Test 7 — Quit and relaunch preserves settings and history

**Setup:**

- Settings are configured (model path set, any preferences toggled).
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
- The app is in idle/ready state and is ready to dictate again without any re-setup.

**If it fails:**

In Terminal, run:

```
ls ~/.config/librewin/turbotalk/
```

If the directory is empty or missing, settings were not written to disk on quit. If the directory has files but settings appear blank after relaunch, there may be a read error on startup. Note what you see and include it in your report.

---

## Reporting a failure

When you hit a step that does not match the Expected behavior:

1. Note the test number and the specific Expected line that was not met.
2. In TurboTalk, open **Settings → Copy diagnostics** (or equivalent) and paste the copied text into your bug report. This includes version info, model path, and recent log lines.
3. If the app crashed, open Console.app, filter by "TurboTalk", and copy the crash entry.
4. Send the test number, what you actually saw, and the diagnostics text to the developer.
