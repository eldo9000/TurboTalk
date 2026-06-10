# TASK-61: Close Windows platform gaps — cross-platform defaults, process mgmt, UX

## Goal
Every macOS-specific assumption that would break or confuse on a Windows build is fixed. An agent can work through all items sequentially in one session — each is small, mechanical, and independently testable.

## Context
The v0.9 beta is macOS-only. The TRUTH.md roadmap states "v0.9 → v1.0 arc is Windows/Linux porting only." A comprehensive audit (see agent chat earlier today) found ~10 concrete gaps. None require architectural changes — the hotkey, paste, and audio modules already have working non-macOS branches. These are the remaining oversights:

| Area | Issue | Fix |
|------|-------|-----|
| Config paths | `~/.config/librewin/` works on Windows but isn't idiomatic (`%APPDATA%`) | Use `dirs::config_dir()` |
| Chimes | macOS-only `afplay`; Windows silently no-ops | Add Windows sound via `PlaySoundW` or bundled `.wav` |
| `open_data_folder()` | Uses `open` command (macOS only) — will fail on Windows | Add `explorer` branch |
| `kill_orphans()` | Uses `pkill -f` (macOS/Linux only) — will fail on Windows | Add `taskkill` branch |
| Whisper stderr log | Hardcoded `/tmp/whisper-server-stderr.log` | Use `std::env::temp_dir()` |
| Child process env vars | Restores Unix-only `TMPDIR`, `USER`, `LOGNAME` | Also restore `TEMP`, `USERNAME` |
| Permission error msgs | References "System Settings → Privacy → Microphone" | Platform-aware strings |
| Window positioning | Non-macOS branch may have coordinate bug | Fix logical vs physical pixel math |
| Bundle config | No `bundle.windows` section in `tauri.conf.json` | Add NSIS config |
| Microphone prompt | macOS has explicit `requestAccess`; Windows relies on cpal | Confirm/instrument Windows path |

## In scope
All 10 items above. Each is a single-file change with no cross-file dependencies — the agent can work through the list sequentially, commit after each logical group, and verify with `cargo check`.

## Out of scope
- Windows CI build or packaging (covered by TASK-50)
- Windows smoke testing (covered by TASK-52, TASK-53)
- Linux/Wayland improvements
- Any macOS feature development
- Architectural changes or new dependencies beyond those listed

## Steps

The agent should process items in this order (grouped by subsystem for minimum context switching):

### Group 1 — Platform path / process hygiene (transcribe.rs)
1. **Whisper stderr log path** — In `transcribe.rs`, replace `"/tmp/whisper-server-stderr.log"` with `std::env::temp_dir().join("whisper-server-stderr.log")` using `temp_dir()` from `std::env`. Keep the `.to_string_lossy()` for the `Stdio::from` argument.
2. **Child process env vars** — In `transcribe.rs`, add `"TEMP"` and `"USERNAME"` to the env-var restoration list alongside `"TMPDIR"`, `"USER"`, `"LOGNAME"`. Use conditional compilation or unconditional existence — `std::env::var()` returns `Err` on missing vars, which is already handled by the `if let Ok(val) = ...` pattern.
3. **`kill_orphans()`** — In `transcribe.rs`, add a `#[cfg(target_os = "windows")]` arm that runs `taskkill /F /IM whisper-server.exe` instead of `pkill -f whisper-server`. Use `std::process::Command::new("taskkill")` with args `["/F", "/IM", "whisper-server.exe"]`. The `cfg` on the existing `pkill` call is implicit (it just fails on Windows) — make it explicit: `#[cfg(not(target_os = "windows"))]` for the pkill arm, `#[cfg(target_os = "windows")]` for the new taskkill arm. Log identically on success/failure.

### Group 2 — Data paths and config (settings.rs)
4. **Config/history/model dirs** — In `settings.rs`, replace `dirs::home_dir().unwrap_or_default().join(".config/librewin/turbotalk")` with a helper function that delegates to `dirs::config_dir()` which returns `~/.config/` on macOS/Linux and `%APPDATA%` on Windows:
   ```rust
   pub fn data_dir() -> PathBuf {
       dirs::config_dir()
           .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
           .join("librewin/turbotalk")
   }
   ```
   Update all four call sites: `data_dir()`, `config_path()`, `history_path()`, `canonical_models_dir()`. Remove the hardcoded `.config/` prefix.
   - **Migration consideration:** Existing Windows users (none today) would lose config on upgrade. Add a one-time migration in `load_detailed()`: if the old `~/.config/librewin/turbotalk/` path exists and the new path doesn't, copy the directory. This is future-proofing only — macOS users continue to get `~/.config/librewin/turbotalk/` since `dirs::config_dir()` returns that on macOS.
5. **Permission error messages** — In `audio.rs`, replace the hardcoded macOS permission messages with platform-aware strings. The `open_stream()` error messages say "grant permission in System Settings → Privacy → Microphone" — this must say "Windows Settings → Privacy & Security → Microphone" on Windows. Use `#[cfg]` or a helper to pick the right string.

### Group 3 — UI and sound (paste.rs, audio.rs, lib.rs)
6. **`open_data_folder()`** — In `lib.rs`, add a `#[cfg(target_os = "windows")]` branch using `explorer` (mirroring the existing `open_releases_page` pattern). The macOS `open` arm gets `#[cfg(target_os = "macos")]`, and add a `#[cfg(not(any(target_os = "macos", target_os = "windows")))]` arm for Linux with `xdg-open`.
7. **Sound chimes** — In `hotkey.rs` `play_chime()` (in the `common` module), the `#[cfg(not(target_os = "macos"))]` arm currently does nothing. Add a `#[cfg(target_os = "windows")]` implementation:
   - Use `winapi`'s `PlaySoundW` (already linked via `winapi` dep) to play system sounds. Map the three chime events to appropriate Windows system sounds:
     - `Start` → `MailBeep` or custom bundled `.wav`
     - `Finish` → `SystemAsterisk` or custom bundled `.wav`
     - `Cancel` → `SystemExclamation` or custom bundled `.wav`
   - Or, bundle small `.wav` files in the app and play them via `rodio` or Tauri's `app.path().resource_dir()`. Simpler: use bundled `.wav` files via `include_bytes!()` and write to a temp file, then use `std::process::Command::new("powershell")` with `[System.Media.SystemSounds]::Asterisk.Play()`.
   - **Recommendation:** The simplest working approach: PowerShell one-liner `[System.Media.SystemSounds]::Asterisk.Play()` — no new deps, no bundled files. But that only gives a few system sounds. For distinct chimes, bundle three small `.wav` files in `src-tauri/sounds/` and use a helper that writes to temp and plays via `powershell -c (New-Object Media.SoundPlayer 'path').PlaySync()`. Or add `rodio` as a dependency. Decide based on how distinct the chimes need to be.
   - At minimum, ensure it compiles and logs what it attempted, even if the sound selection is basic.

### Group 4 — Window math and bundle (lib.rs, tauri.conf.json)
8. **Window positioning bug** — In `lib.rs`, find the non-macOS `reposition_overlay_to_cursor_monitor()` and `position_main_window_on_cursor_monitor()` implementations. The subagent report noted a coordinate bug: `mp.x as f64 / scale` where `mp.x` is already logical. Review and fix the math. This is harder to verify without a Windows machine — aim for "correct by construction" and add a comment explaining the coordinate convention.
9. **Bundle config** — In `tauri.conf.json`, add a `"windows"` section under `"bundle"`:
   ```json
   "windows": {
     "wix": null,
     "nsis": {
       "installMode": "currentUser",
       "installerIcon": "icons/icon.ico"
     }
   }
   ```
   And remove the macOS-only key `"macOSPrivateApi": true` from `app` (it's a no-op on Windows but confuses config readers). Or wrap it in... actually Tauri config is JSON, not conditional. Leave it but add a comment. Add `"icon.ico"` to the `"icon"` array if not already present (it is — line 112).
   - Also add the Windows sidecar suffix to `"externalBin"` — Tauri handles the platform suffix at build time, so `"binaries/whisper-server"` automagically becomes `whisper-server.exe` on Windows. Verify by reading Tauri docs on externalBin, but likely already correct.
10. **Microphone permission flow** — In `permissions.rs`, the non-macOS `microphone_status()` returns `Unsupported`. On Windows, we should return `NotDetermined` (the first cpal open will prompt). Actually the current behavior is fine: `Unsupported` → `ok()` returns true → `ready` is not blocked. The microphone prompt will fire naturally when cpal opens the stream. Add a comment to this effect. No code change needed if the current flow works — just verify in the audit log comment.

### Final step
11. **Build and commit** — After all changes: `cargo check` (or build if possible), verify no warnings introduced, then commit with a message like:
    `chore(windows): close 10 platform gaps — paths, processes, UX, bundle (TASK-61)`
    Update `TRUTH.md` with a note that Windows platform plumbing is now consistent (not tested, but no longer macOS-only assumptions).

## Success signal
`cargo check` passes. Each of the 10 items is addressed in source — either a code change, a documented decision not to change, or an existing correct implementation confirmed. `git diff --stat` shows changes across ~6–8 files.

## Notes
- All changes are independent — the agent can roll back individual items without affecting others.
- No Windows machine is needed to verify correctness; `cargo check` on macOS is sufficient for compilation. The coordinate fix (item 8) is the only exception — it's "best-effort correct by construction."
- If any item turns out to be non-trivial (e.g. sound chimes need a new crate), the agent should note the complexity and move to the next item. Don't block the whole arc on one tricky dependency.
