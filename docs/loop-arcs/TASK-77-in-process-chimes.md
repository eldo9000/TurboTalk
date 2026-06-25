# Arc Log — TASK-77: In-process chime playback (NSSound / PlaySoundW)

## Gate
Replace per-chime subprocess spawning (`afplay` on macOS, `powershell` on Windows)
with in-process platform-native audio APIs.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** `play_chime` spawns `Command::new("afplay")` (macOS, ~10-30ms fork per
  chime) and `Command::new("powershell")` (Windows, ~150-400ms fork per chime) for
  short sound clips. Each chime launches a process for a <500ms sound.
- **PREMISE:** Replacing `afplay` with `NSSound::soundNamed_()` + `play()` (macOS) and
  `powershell` with `PlaySoundW SND_ASYNC` (Windows) eliminates all process spawning
  for short sound playback while keeping the same system sounds and async behavior.
- **DERIVATION:** `objc2-app-kit` is already in Cargo.toml with `NSSound`. `PlaySoundW`
  with `SND_ASYNC` returns immediately and plays in background. Both APIs play system
  sounds by name without needing file paths.
- **FALSIFICATION:** If `cargo check` fails (NSSound selector not found, PlaySoundW
  symbol not available), or if sounds don't play, the premise is false.
- **FALSIF-RESULT:** `cargo check` + `cargo clippy` clean. macOS: NSSound::soundNamed_ + play() zero-subprocess. Windows: PlaySoundW SND_ALIAS | SND_ASYNC | SND_NODEFAULT zero-subprocess.
- **DISPOSITION:** CONFIRMED — dispatch 1 green. Commit d4ff787.
