# TASK-77: In-process chime playback (NSSound / PlaySoundW)

## Goal
Replace per-chime subprocess spawning (`afplay` on macOS, `powershell.exe` on Windows) with in-process audio playback via the platform-native API.

## Context
The `play_chime` function in `src-tauri/src/hotkey.rs:354-414` plays start/stop/cancel sound effects. On macOS it spawns `afplay` as a subprocess (`Command::new("afplay").spawn()` at `:375`). On Windows it launches `powershell.exe` with a `[SystemSounds]` invocation (`Command::new("powershell")` at `:398`).

Each chime forks a process:
- macOS `afplay`: ~10-30ms process startup for a <500ms sound clip
- Windows PowerShell: ~150-400ms process startup for a system sound — this is the worst offender. Launching PowerShell just to play a beep is heavy.

The documented-correct idioms:
- **macOS**: `NSSound` (via `objc2-app-kit`, already a dependency in `Cargo.toml:126`). `NSSound::soundNamed:` + `.play()` is a single in-process call with no fork. For bundled sound files, `NSSound::initWithContentsOfFile:` works. For the existing chime WAV files, either path works.
- **Windows**: `PlaySoundW` from `winmm.dll` is the documented one-liner for playing system sounds or WAV files. It's a single function call, no process spawn. Requires `winapi` feature `winmm` (NOT currently in `Cargo.toml` — needs to be added) OR use the `windows-sys` crate. Alternatively, `winapi` has `playsound` via the `winmm` feature if available; verify.

The chime sounds are bundled as WAV files in the app resources. Check `src-tauri/sounds/` or `src-tauri/icons/` or the `tauri.conf.json` bundle config for where they live.

## In scope
- `src-tauri/src/hotkey.rs:354-414` — the `play_chime` function
- `src-tauri/Cargo.toml` — add `winmm` feature for Windows `winapi` (if using winapi; alternatively add `windows-sys` with `Win32_Media_Audio` feature)
- `SESSION-STATUS.md`

## Out of scope
- Changing which sounds play or when (the chime policy stays the same)
- Adding new sounds
- The paste-activation `osascript` subprocess (separate concern, could be a follow-up)
- Cross-platform audio library (like `rodio`) — the platform-native APIs are lighter and sufficient for playing short WAV clips

## Steps
1. Read `src-tauri/src/hotkey.rs:354-414` to understand the current `play_chime` flow: it takes a chime type (Start/Stop/Cancel), resolves the sound file path, and spawns the platform subprocess.
2. Find where chime sound files are bundled. Check `tauri.conf.json` for resource paths, and look for WAV files in `src-tauri/sounds/` or similar.
3. For macOS: replace the `Command::new("afplay")` path with `NSSound`. Use `objc2-app-kit` (already a dependency). For file-based sounds: `NSSound::initWithContentsOfFile_byReference_` or `NSSound::alloc().initWithContentsOfFile_byReference_`. For named system sounds: `NSSound::soundNamed_`. Call `.play()` — it's async and returns immediately. No fork.
4. For Windows: replace the `Command::new("powershell")` path with `PlaySoundW`. The function signature is `PlaySoundW(pszSound: *const u16, hmod: HMODULE, fdwSound: u32)`. For file-based sounds: pass the file path as a wide string with `SND_FILENAME | SND_ASYNC`. For system sounds: pass the sound name with `SND_ALIAS`. Add `winmm` to the `winapi` features in `Cargo.toml`, or use `windows-sys` if `winapi` doesn't expose `PlaySoundW`.
5. Handle errors gracefully: if the native API call fails (e.g. file not found), log a warning and continue. The current `afplay`/`powershell` path already swallows errors (the `.spawn()` result is not checked for success). Match that behavior.
6. The chime should be non-blocking (fire and forget). macOS `NSSound.play()` is async by default. Windows `PlaySoundW` with `SND_ASYNC` is also async. Neither should block the PTT event thread.
7. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy`.
8. Run `npm run typecheck`.
9. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- `grep -n "afplay\|powershell" src-tauri/src/hotkey.rs` returns zero results in the `play_chime` function.
- macOS chime plays via `NSSound.play()` with no process spawn.
- Windows chime plays via `PlaySoundW` with no process spawn.
- Chimes are non-blocking (the PTT event thread is not delayed by chime playback).
- The chime sounds are the same as before (same WAV files or equivalent system sounds).

## Notes
- `objc2-app-kit` is already in `Cargo.toml:126` (`objc2-app-kit = "0.3"`). The `NSSound` class is in `objc2-app-kit`.
- For Windows, `winapi` is already a dependency (`Cargo.toml:136`) with features `["winuser", "winbase", "synchapi", "errhandlingapi"]`. Check if adding `"winmm"` to the features list gives access to `PlaySoundW`. If `winapi` doesn't expose it, consider `windows-sys = { version = "0.59", features = ["Win32_Media_Audio"] }` as a lightweight alternative.
- `PlaySoundW` with `SND_ASYNC` returns immediately and plays in the background. `SND_FILENAME` flag for file paths, `SND_ALIAS` for system sound names.
- `SND_NODEFAULT` flag prevents the default beep if the file/sound is not found — useful for graceful degradation.
- The chime path may resolve differently for dev builds (relative path) vs packaged builds (resource path). Verify the path resolution still works with the native APIs.
- If the chime WAV files are in the Tauri resources, use `app.path().resource_dir()` to resolve them at runtime, matching how the whisper sidecar path is resolved.
