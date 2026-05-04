# TASK-1: Add run_diagnostics Tauri command

## Goal
A `run_diagnostics` Tauri command exists in the Rust backend, returns a structured JSON object describing the app's runtime health, and is typed in `src/bindings.ts`.

## Context
TurboTalk is a Tauri 2 + Svelte 5 dictation app on macOS. The backend is in `src-tauri/src/`. Beta users will need a way to report their environment when things break. Right now there is no diagnostic surface — errors are logged internally but never surfaced in a copyable form.

The diagnostic command should check and return:
- Platform string (`std::env::consts::OS`)
- Whether at least one audio input device is available (use `cpal::default_host().input_devices()`, non-empty = true)
- Whether the configured model file exists on disk (read model path from `Settings`)
- Whether the whisper-cli sidecar binary exists and is executable
- Current cleanup mode from `Settings` (`raw`, `regex`, `advanced`)
- If cleanup mode is `advanced`: whether Ollama is reachable (HTTP GET to the configured Ollama URL, e.g. `http://localhost:11434`, 2-second timeout)
- Paste capability (hardcode `"supported"` on macOS; return `"unsupported"` on other platforms)

`Settings` is defined in `src-tauri/src/settings.rs`. The sidecar lives under `src-tauri/binaries/` at a path derivable from `tauri::utils::platform::current_exe()` or similar — use whatever mechanism `transcribe.rs` already uses to locate the sidecar binary.

The command should never panic. Wrap each check in a `match`/`unwrap_or` so a failing check returns a descriptive string, not a crash.

Tauri specta is already used for typed bindings. Add `DiagnosticsResult` to the specta export if specta is wired; otherwise add the return type manually to `src/bindings.ts`.

## In scope
- `src-tauri/src/lib.rs` — register `run_diagnostics` command
- `src-tauri/src/diagnostics.rs` — new file with the command implementation and `DiagnosticsResult` struct
- `src/bindings.ts` — add the typed binding for `run_diagnostics`

## Out of scope
- Any UI that calls this command (TASK-2)
- Changing how errors are surfaced in the main dictation flow (TASK-3)
- History, privacy settings, or PRIVACY.md (TASK-5–7)

## Steps
1. Create `src-tauri/src/diagnostics.rs` with a `DiagnosticsResult` struct (all fields `String` or `bool` for easy serialization) and a `run_diagnostics` async fn.
2. Implement each check described in Context above. For Ollama reachability use `reqwest` (already a dependency) with a 2-second timeout.
3. Register `run_diagnostics` in `lib.rs` `tauri::Builder` `.invoke_handler()` and add it to the specta builder if specta is present.
4. Export `DiagnosticsResult` type and `run_diagnostics` binding in `src/bindings.ts`.
5. Run `cargo build` — must compile clean with no warnings under `cargo clippy -D warnings`.

## Success signal
`cargo build` exits 0 and `cargo clippy -D warnings` exits 0. The binding appears in `src/bindings.ts`. Calling `invoke("run_diagnostics")` from the browser console of `npm run tauri dev` returns a JSON object with the expected fields and no runtime error.

## Notes
- If `reqwest` is not already a dependency, add it with `features = ["json"]` to `src-tauri/Cargo.toml`.
- Do not add a `tokio` runtime directly; Tauri commands already run on the async runtime.
- Check how `transcribe.rs` resolves the sidecar path — replicate that logic rather than hardcoding a path.
