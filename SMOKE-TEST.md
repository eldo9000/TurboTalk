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

---

## Installed-artifact smoke test

Run this section after every release build (signed + notarized DMG) and **before publishing** the release. The 7 dev-build tests above catch code regressions; this section catches packaging-layer regressions that only appear once the app is installed from a real DMG. It covers Gatekeeper acceptance, the macOS permission prompt flow when launched from `/Applications`, one end-to-end dictation, and the documented uninstall + data cleanup path. Skipping this section is how broken DMGs reach users.

### Prerequisites

- A signed and notarized DMG sitting in `dist-artifacts/` (per `BUILD.md` and `RELEASING.md`), along with its matching `.sha256` file. The DMG must be the actual artifact you intend to publish — not an unsigned local build.
- **A clean macOS user account with no prior TurboTalk install.** Either:
  - (a) A fresh macOS VM, or
  - (b) A new local user account on the maintainer's Mac (System Settings → Users & Groups → Add Account), then log into that account before starting.

> **Do not run this on the maintainer's daily-driver account.** That account already has Microphone and Accessibility grants for TurboTalk cached, and may have stale data under `~/.config/librewin/turbotalk/` or `~/Library/Application Support/`. Running the test there silently passes the failures it is designed to catch.

### Steps

1. **Verify checksum before installing.**

   **Action:** In Terminal, `cd` into `dist-artifacts/` (or wherever the DMG and its `.sha256` sit together) and run:

   ```
   shasum -a 256 -c TurboTalk-<version>-macos-arm64.dmg.sha256
   ```

   **Expected:** `TurboTalk-<version>-macos-arm64.dmg: OK`. Anything else (`FAILED`, missing file, wrong digest) means the DMG is corrupt or mismatched — stop here and do not install.

2. **Verify Gatekeeper acceptance.**

   **Action:** From Terminal, run:

   ```
   spctl -a -t open --context context:primary-signature -v TurboTalk-<version>-macos-arm64.dmg
   ```

   **Expected:** Output ends with `accepted` and the developer name on the `source=` line (for a notarized release, `source=Notarized Developer ID`). Any `rejected` result means Gatekeeper will block the install on a real user's machine — stop and re-check signing/notarization in `BUILD.md`.

3. **Mount and install.**

   **Action:** Double-click the DMG to mount it. Drag `Turbo Talk.app` into `/Applications`. Eject the DMG from Finder.

   **Expected:** `Turbo Talk.app` appears in `/Applications` and is launchable from Finder or Spotlight.

4. **First launch.**

   **Action:** Open `/Applications/Turbo Talk.app` (double-click in Finder, or via Spotlight).

   **Expected:** The app window appears. **No** Gatekeeper warning dialog. Specifically, a dialog reading "Turbo Talk cannot be opened because the developer cannot be verified" or "macOS cannot verify that this app is free from malware" is a **FAIL** — stop, and treat the notarization as broken.

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
   ls ~/.config/librewin/turbotalk/
   ```

   **Expected:** The directory exists and contains the expected config file (`settings.json`) and, if history is enabled, a `history/` directory — matching the paths documented in `PRIVACY.md` → "How to delete everything." If the directory is missing entirely after a successful dictation, settings persistence is broken when launched from `/Applications`.

9. **Quit and relaunch.**

   **Action:** Quit TurboTalk completely (menu bar icon → Quit). Wait a few seconds, then relaunch from `/Applications/Turbo Talk.app`.

   **Expected:** App launches clean. Settings are still present (model path, hotkey, etc.). Microphone and Accessibility permissions are **not** re-prompted. The app reaches idle/ready state without any first-run prompts.

10. **Uninstall.**

    **Action:** Drag `/Applications/Turbo Talk.app` to the Trash. Empty the Trash if you want to fully complete the removal.

    **Expected:** `Turbo Talk.app` is gone from `/Applications`.

11. **Verify data cleanup path.**

    **Action:** Follow `PRIVACY.md` → "How to delete everything." Specifically:

    - If you enabled autostart at any point, run `launchctl unload ~/Library/LaunchAgents/com.librewin.turbotalk.plist` and then delete that plist file.
    - Delete `~/.config/librewin/turbotalk/settings.json`.
    - Delete `~/.config/librewin/turbotalk/history/` (entire directory).
    - Delete `~/.config/librewin/turbotalk/models/` (entire directory, or whichever model path you configured).

    Then verify nothing remains:

    ```
    ls ~/.config/librewin/turbotalk/ 2>/dev/null
    ls ~/Library/LaunchAgents/com.librewin.turbotalk.plist 2>/dev/null
    ```

    **Expected:** Both commands report no such file or directory. Every path PRIVACY.md lists is gone after running its documented commands. If any path remains that PRIVACY.md does not mention, that is a documentation gap — note it for follow-up.

### Pass/fail recording

Record the outcome of this run in `SESSION-STATUS.md` under the release entry. On pass, note the macOS version you tested against (e.g., "Installed-artifact smoke test: PASS on macOS 14.5"). On fail, note **which numbered step failed** and **what you actually observed**, not just "failed" — that observation is what the next debugging session starts from.
