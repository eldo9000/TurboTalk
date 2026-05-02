# TASK-2: Canonicalize all paths passed to subprocesses (close path traversal)

## Goal
The whisper model path and the whisper-cli binary path used by `transcribe.rs` are canonicalized and verified to live inside an allowed directory before being passed to the subprocess. `scan_models_dir()` returns only paths that resolve safely. A user editing the config file to specify a path with `..` or an arbitrary absolute path no longer causes that path to be executed or read.

## Context
TurboTalk is a personal-use macOS voice dictation app (Tauri 2 + Rust). A multi-agent security review found high-severity path traversal vulnerabilities:

- `src-tauri/src/transcribe.rs:30` — `find_whisper()` falls back to `cfg.whisper.bin` (a user-configurable string from settings) without validation.
- `src-tauri/src/transcribe.rs:50` — the model path from `cfg.whisper.model` is passed directly to the whisper-cli subprocess as an argument with no canonicalization.
- `src-tauri/src/settings.rs:170` — `scan_models_dir()` lists `.bin` files in `~/.config/librewin/turbotalk/models/` but does not canonicalize results, so symlinks pointing outside the directory are returned and later passed to whisper-cli.

A user (or anything that can write the config file at `~/.config/librewin/turbotalk/config.toml`) could specify `model = "/etc/passwd"` or `model = "../../../bin/something-malicious"`. Whisper-cli would then read or attempt to load that file. With a malicious binary substituted for `whisper.bin`, this becomes arbitrary code execution.

Allowed locations:
- **Models directory:** `~/.config/librewin/turbotalk/models/` (canonical form)
- **Whisper binary — bundled sidecar:** the location returned by Tauri's `app.path().resource_dir()` join `binaries/whisper-cli` (this is what `find_whisper()` checks first via `current_exe()`)
- **Whisper binary — dev fallback:** `src-tauri/target/debug/whisper-cli` etc — already covered by the existing dev fallback logic

Anything outside these must be rejected.

## In scope
- `src-tauri/src/transcribe.rs` — add path validation to `find_whisper()` and to the model-path argument construction
- `src-tauri/src/settings.rs` — add canonicalization to `scan_models_dir()`

## Out of scope
- The frontend (App.svelte) — it just lists what the backend returns; backend hardening is sufficient
- Other paths in the codebase (audio temp file path is a separate task)
- Refactoring the existing sidecar discovery logic — only add validation, don't restructure
- Adding new config fields

## Steps
1. Read `src-tauri/src/settings.rs` and locate `scan_models_dir()`. Identify the canonical models directory (likely built from `dirs::config_dir()` + `librewin/turbotalk/models`).
2. Compute the canonical form of that directory once at the top of the function using `Path::canonicalize()`. If the directory does not exist, return an empty vec.
3. For each `.bin` file found, canonicalize the entry. If canonicalization fails, skip it. If the canonicalized path does not start with the canonical models directory, skip it. Only return paths that pass both checks.
4. Read `src-tauri/src/transcribe.rs` and locate `find_whisper()`. Identify the existing precedence order (sidecar first, dev fallback, then `cfg.whisper.bin`).
5. Add a helper `is_allowed_whisper_path(p: &Path) -> bool` that canonicalizes and verifies the path resolves to one of: the bundled sidecar location, the dev target directory, or the explicit dev/test paths already supported. Reject everything else.
6. When `find_whisper()` reaches the `cfg.whisper.bin` fallback, run `is_allowed_whisper_path` and log + return an error if it fails. Do not silently fall back to a rejected path.
7. In the model-path argument construction (around `transcribe.rs:50`), canonicalize `cfg.whisper.model`. If canonicalization fails, return an error like `"model path does not exist or could not be resolved: {path}"`. If the canonical path does not start with the canonical models directory, return an error `"model path is outside the allowed models directory"`.
8. Pass the canonicalized path to the whisper-cli subprocess instead of the raw config string.
9. Run `cargo build --manifest-path src-tauri/Cargo.toml` and confirm compilation.
10. Manually test: launch the app with `npm run tauri dev`, choose a model normally — verify transcription still works. Then edit `~/.config/librewin/turbotalk/config.toml` to set `model = "/etc/hosts"`, restart the app, attempt to transcribe — verify the error is surfaced (an error event reaches the frontend or shows in the log) and no subprocess is spawned with the bad path.

## Success signal
- `cargo build` exits 0.
- Normal transcription with a valid model in the models directory works unchanged.
- Setting `model` in config.toml to a path outside the models directory causes transcription to fail with a clear error message and no whisper-cli subprocess is launched (verify via Activity Monitor or by adding a temporary `tracing::info!`).
- Setting `whisper.bin` in config.toml to an arbitrary path (e.g., `/bin/ls`) does not cause that binary to be executed.
- `scan_models_dir()` does not return paths that resolve outside the models directory (test by creating a symlink in the models dir pointing to `/tmp/foo.bin` and verifying it does not appear in the list).

## Notes
- `Path::canonicalize` returns `io::Result<PathBuf>`. Treat the `Err` case as "rejected" — do not unwrap.
- On macOS, `canonicalize` resolves symlinks, which is the desired behavior for blocking symlink traversal.
- Do not add any new public API for path validation — keep helpers private to the module.
- Multi-agent review reference: findings SEC-001, SEC-002, SEC-013 / C-1 in `/tmp/code-analysis-concern-based-main-20260501.md`.
