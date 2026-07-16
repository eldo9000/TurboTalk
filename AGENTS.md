## Context

TurboTalk is a personal-use voice dictation utility for macOS (not a public product). Push-to-talk → record mic → local Whisper transcription → optional LLM cleanup → paste. Standalone, no product-ecosystem dependencies. Tier 1 operating model — `SESSION-STATUS.md` and `TRUTH.md` only. Don't add heavy ceremony.

## Build & Run

```bash
npm install
npm run tauri dev    # dev port 1428
npm run package      # produces build/TurboTalk-<version>-macos-arm64.dmg
```

## Platform API Discipline

TurboTalk touches platform boundaries on every hotkey, audio callback, clipboard op, paste injection, and subprocess spawn. The single largest source of "works but wrong" bugs is agents reaching for shell commands instead of native APIs.

**Three triggers — when you see yourself doing any of these, stop and read the platform overview doc first:**

- **`Command::new("...")` for a platform capability** — spawning `afplay`, `osascript`, `powershell` instead of using the native API.
- **`thread::sleep` in a polling loop** — the correct idiom is an event source, condvar, run-loop callback, or dispatch queue.
- **Touching a platform subsystem** — CoreAudio, CGEventTap, IOHIDManager, NSPasteboard, NSRunningApplication, NSSound, WH_KEYBOARD_LL, Win32 message loops.

**Gate:** before writing code that hits any of these, read the platform's overview doc (Apple Event Handling Guide, MS Keyboard Input Overview, etc.) and name the documented-correct idiom first. If you can't name it, you haven't read enough.

**Known idiom map** (add as you learn):

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

## Key Modules

See `docs/ARCHITECTURE.md`. Core: `audio.rs` (cpal mic, 45s idle-close), `recorder.rs` (6-state dictation lifecycle), `transcribe.rs` (whisper.cpp sidecar, 300ms pre-roll), `hotkey.rs` (CGEventTap), `cleanup.rs` (Chaperone LLM router), `paste.rs` (arboard + osascript).

## Habits

- **Name the proof before calling work done.** "It compiles" isn't proof. "I held F1, said 'hello world', and it appeared in the focused window" is.
- **Update `SESSION-STATUS.md`** after meaningful work. **Update `TRUTH.md`** when "what works end-to-end" changes.
- **When a failure isn't obvious, classify it before fixing.**
