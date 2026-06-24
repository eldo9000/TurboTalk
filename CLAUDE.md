# TurboTalk — Claude Context

## Shared Standards

- **Engineering standards:** `~/Downloads/Github/Business-OS/standards/ENGINEERING.md` — session protocol, investigation logs, commit conventions. Read before any implementation session.
- **Operating model:** `~/Downloads/Github/Business-OS/bin/SOFTWARE-DEVELOPMENT-OPERATING-MODEL.md` — the portfolio's evidence/ledger discipline. **TurboTalk operates at Tier 1** (see §15): small app, obvious behavior, personal-use scope. Required artifacts are limited to `SESSION-STATUS.md` (status ledger) and `TRUTH.md` (truth ledger). Do **not** add: heavy red-build ladders, observer loops, structured commit notes, milestone gates for every task, or full closure ceremony for every commit. Add weight only if a concrete failure mode appears.
- **Design language & shared patterns:** local conventions described in this repo — Svelte 5 patterns, Tauri 2 patterns, design tokens.

## Tier 1 Habits (enforce these)

- Name the proof before calling work done. ("It compiles" is not proof. "I held F1, said 'hello world', and 'hello world' appeared in the focused TextEdit window" is proof.)
- Keep visible TODOs and stubs explicit — module headers in `src-tauri/src/*.rs` already do this.
- When a failure is not obvious, classify it before fixing.
- Update `SESSION-STATUS.md` after any meaningful work.
- Update `TRUTH.md` whenever the answer to "what works end-to-end" changes.

## Platform API discipline (enforce these)

TurboTalk touches platform boundaries on every hotkey, audio callback, clipboard
operation, paste injection, chime, and subprocess spawn. Idiom drift here is the
single largest source of "works but wrong" code in this repo. These rules exist
because every finding in the 2026-06-24 audit traced back to a platform API
being used the shell-script way instead of the documented-native way.

**The three triggers.** When you are about to do any of these, STOP and run the
gate below before writing the code:

1. **`Command::new("...")` for a platform capability** — spawning `afplay`,
   `osascript`, `powershell`, `pkill`, `explorer`, `xdg-open` to do something
   the OS has a native API for. This is the single biggest tell. The native API
   is always faster, safer, and more correct than forking a shell command.
2. **`thread::sleep` in a polling loop** — any loop that sleeps and re-checks a
   condition. The documented idiom on every platform is an event source, a
   condition variable, a run-loop callback, or a dispatch queue — not a poll.
3. **Touching a platform subsystem** — CoreAudio, CGEventTap, IOHIDManager,
   NSPasteboard, NSRunningApplication, NSSound, Win32 clipboard, WH_KEYBOARD_LL,
   Win32 message loops. If you're calling into a platform framework API, you are
   at a boundary.

**The gate (run before implementing):**

1. **Read the overview doc, not the API reference.** Apple's "Event Handling
   Guide for Mac," Microsoft's "Keyboard Input Overview," CoreAudio's "Audio
   Hardware Guide." These are 5–10 page conceptual docs that name ALL the
   options. API reference pages only describe the API you already found. The
   overview is where you learn IOHIDManager exists alongside CGEventTap, that
   `Stream::pause` exists alongside the warm-stream pattern, that `NSSound`
   exists alongside `afplay`.
2. **Check how native apps do it, not how Rust apps do it.** Search for the
   Objective-C / Swift tutorial (macOS) or the C/C++ Win32 example (Windows).
   A native macOS dev would never spawn `afplay` for a sound; they'd call
   `NSSound.play()`. A native Windows dev would never poll `GetAsyncKeyState`
   at 125 Hz; they'd use `WH_KEYBOARD_LL`. The shell-command approach is the
   tell that you approached a platform problem from the wrong angle.
3. **Name the documented-correct idiom before writing code.** Write it as a
   one-liner in the task file, commit message, or PR: "Using X because the
   overview doc names X as the correct API for this; Y is the shell-command
   shortcut that we're rejecting." If you can't name it, you haven't read the
   overview yet.

**The known idiom map** (add to this as you learn):

| Capability | Wrong (shell/poll) | Correct (native API) |
|---|---|---|
| Play a sound | `Command::new("afplay")` / `powershell` | `NSSound::play()` / `PlaySoundW` |
| Activate an app | `Command::new("osascript")` | `NSRunningApplication::activateWithOptions:` |
| Global hotkey (macOS) | — | `CGEventTap` (Accessibility) or `IOHIDManager` (Input Monitoring) |
| Global hotkey (Windows) | `GetAsyncKeyState` polling | `WH_KEYBOARD_LL` + message loop on dedicated thread |
| Clipboard read/write | `arboard` (no changeCount) | `NSPasteboard` with `changeCount` guard |
| Clipboard (Windows) | unconditional restore | `GetClipboardSequenceNumber` guard |
| Wait for a condition | `thread::sleep` poll loop | `Condvar`, `dispatch_async`, run-loop source |
| Audio capture idle | callback runs forever, gate inside | `Stream::pause()` / `Stream::play()` |
| Audio callback buffer | `Mutex<Vec<f32>>` | lock-free SPSC ring (`rtrb`) |
| Tap disabled (macOS) | poll `CGEventTapIsEnabled` every 8s | handle `TapDisabledByTimeout` event in callback |

## What This Is

TurboTalk is a personal-use voice dictation utility for macOS. Push-to-talk hotkey → record mic → local Whisper transcription → optional LLM cleanup → paste into the focused app.

It is **not a public product.** Personal-use scope. If it earns its place, it gets promoted. Until then: private repo, GPL-3.0 license, no public release.

It is a standalone app with no dependencies on other product ecosystems.

## Repo State

**v0.8 beta — working build, macOS arm64.** Full dictation loop proven end-to-end (2026-05-01). Milestones M0–M5 complete. The scaffold, all core modules, and the Chaperone guided-setup flow are all landed. See `TRUTH.md` for what works and `SESSION-STATUS.md` for current focus.

## Running

```bash
npm install
npm run tauri dev
```

Dev port: **1428**. For a packaged DMG: `npm run package` (produces `build/TurboTalk-<version>-macos-arm64.dmg`).

## Architecture

See `docs/ARCHITECTURE.md` for the full module plan. Key modules in `src-tauri/src/`:

- `audio.rs` — mic capture via `cpal`; keeps stream warm between recordings (45s idle-close watchdog)
- `recorder.rs` — 6-state dictation lifecycle (Ready / Recording / FinalizingAudio / Transcribing / Cleaning / Pasting)
- `transcribe.rs` — whisper.cpp sidecar wrapper; 300ms pre-roll ring buffer for leading-word preservation
- `paste.rs` — active-app text injection (arboard + osascript on macOS)
- `hotkey.rs` — global push-to-talk via CGEventTap (macOS); stub on other platforms
- `cleanup.rs` — LLM postprocessor (Chaperone Layer); emits `chaperone-fallback` ui-error toast on failure
- `ollama.rs` — Ollama HTTP helpers: `ping_ollama`, `check_ollama_model`, `open_url`, `pull_ollama_model`
- `settings.rs` — persistence under `~/.config/turbotalk/`; process-wide RwLock cache
- `diagnostics.rs` — health check command (Settings tab, dev-only surface)
- `whisper_models.rs` — model catalog, download command, progress events

## Portfolio Status

This repo participates in the Business-OS portfolio status system. Update `SESSION-STATUS.md` at the end of every session.

## Workflow

- macOS personal-use tool. No CI gates for now (add when Windows/Linux stubs are unblocked).
- The Chaperone Layer (classifier-router LLM via Ollama) is the differentiator. Reference `Business-OS/memory/project_chaperone_layer.md` for the pattern.
- Promote to public product trigger: "I use this every day for 2 weeks."
