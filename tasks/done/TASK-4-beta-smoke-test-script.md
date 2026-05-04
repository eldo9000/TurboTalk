# TASK-4: Write beta smoke test script

## Goal
`SMOKE-TEST.md` exists at the repo root with a numbered manual test procedure covering the 7 core beta scenarios. Each step states exact actions and exact expected observations so any human tester can execute it cold.

## Context
TurboTalk is a macOS dictation app. Before sharing with beta users, a documented smoke test needs to exist so testers can report failures consistently. This is a documentation-only task — no source code changes.

The smoke test covers:
1. Clean launch (no prior config)
2. Mic permission denied → app explains what to grant
3. Mic granted → push-to-talk → speak "hello world" → transcript appears
4. Model missing → error points to model setup
5. Chaperone enabled without Ollama running → raw/regex fallback is clear
6. Focus switches during transcription → UI names destination or pastes correctly
7. Quit and relaunch → settings and history persist as configured

Each step must include:
- **Setup:** what state the machine/app must be in before this step
- **Action:** exactly what the tester does (button click, keyboard shortcut, menu item, system setting)
- **Expected:** exactly what should appear/happen
- **If it fails:** one-line triage hint (not a fix, just a hint to narrow the cause)

The hotkey is Right Option (⌥ on the right side of the keyboard). Push-to-talk = hold Right Option while speaking, release to stop.

Settings are under `~/.config/librewin/turbotalk/`. The model is a `.bin` or `.gguf` file at the path configured in settings. Whisper sidecar is bundled in the app at `TurboTalk.app/Contents/MacOS/whisper-cli-aarch64-apple-darwin`. History is stored as JSON under `~/.config/librewin/turbotalk/history/`.

## In scope
- `SMOKE-TEST.md` — new file at repo root

## Out of scope
- Any source code changes
- Automated tests
- Windows or Linux scenarios
- Diagnostics command or panel (TASK-1, TASK-2)
- Privacy documentation (TASK-5)

## Steps
1. Create `SMOKE-TEST.md` at the repo root.
2. Add a header block: purpose, target platform, required setup (app installed from DMG, microphone available, model downloaded, Ollama installed but can be stopped for step 5).
3. Write 7 numbered test cases, each with Setup / Action / Expected / If it fails subsections.
4. Add a "Reporting a failure" section at the end: tell the tester to copy diagnostics from Settings → Copy diagnostics and include it in the bug report alongside the step number that failed.

## Success signal
`SMOKE-TEST.md` exists at the repo root. It contains 7 numbered test cases. Each test case has Setup, Action, Expected, and If it fails subsections. A human who has never seen the codebase can follow it without asking clarifying questions.

## Notes
- Keep language plain — assume the tester is a non-developer who is technically literate.
- Do not reference internal module names (transcribe.rs, cleanup.rs) in the tester-facing text.
- Step 2 (mic permission denied) may require the tester to revoke permission in System Settings → Privacy & Security → Microphone, then relaunch the app. Include that instruction explicitly.
