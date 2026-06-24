# TASK-73: Gate worker invalidation on model/backend changes only

## Goal
Stop destroying and rebuilding the warm whisper/parakeet transcription worker when the user toggles unrelated settings (theme, sound, overlay size, cursor dot, cancel-on-esc, etc.). Only invalidate the worker when a field that actually affects the transcription backend changes.

## Context
The `save_config` command in `src-tauri/src/lib.rs:74-94` calls `transcribe::invalidate_worker()` + `transcribe::prewarm()` on **every** config save, regardless of which fields changed. This means:

- Toggling "sound on cancel" → destroys the warm whisper-server process and respawns it
- Changing the theme → same
- Changing overlay size → same
- Toggling cursor dot → same

The `invalidate_worker()` call kills the persistent whisper-server (or Parakeet ONNX session) and `prewarm()` rebuilds it. For whisper-server this means killing the child process and respawning it (multi-second model reload). For Parakeet it means re-loading the ONNX model into memory. This happens on every single settings toggle.

The frontend (`src/App.svelte`) calls `commands.saveConfig(cfg)` from `saveSettings()` (`:1003`), `saveModes()` (`:862`), and `saveModels()` (`:814`). None of these are debounced (except the volume slider at `:1744`). Every toggle is an immediate full config save + worker teardown.

The fields that actually require worker invalidation are:
- `backend` (Whisper ↔ Parakeet switch)
- `backend_variant` (model variant within a backend)
- `whisper.model` (whisper model path)
- `whisper.bin` (whisper binary path)
- `cleanup.vocabulary` / `cleanup.antivocabulary` (if the Chaperone classifier prompt or vocabulary changes — though vocabulary is used at cleanup time, not worker init time; verify whether it actually needs invalidation)

Everything else (theme, sound, overlay, cursor dot, window geometry, zoom, cancel-on-esc, media control, etc.) should save to disk + update the settings cache but NOT touch the worker.

## In scope
- `src-tauri/src/lib.rs` — the `save_config` command
- `src-tauri/src/settings.rs` — may need a field-comparison helper or expose the previous config
- `src-tauri/src/transcribe.rs` — only if the invalidation contract needs to change (likely not — `invalidate_worker` + `prewarm` are the right calls, just gated)
- `SESSION-STATUS.md`

## Out of scope
- Debouncing frontend saves (that's a frontend concern, handled by TASK-80)
- The App.svelte split (TASK-70)
- Changing what `invalidate_worker` does internally
- Adding new settings fields
- Changing the settings cache structure (TASK-74 handles that)

## Steps
1. Read `src-tauri/src/lib.rs:74-94` (the `save_config` command) to see the current flow: it calls `settings::save()`, `settings::update_cache()`, then unconditionally `transcribe::invalidate_worker()` + `transcribe::prewarm()`.
2. Read `src-tauri/src/settings.rs` to understand the `Config` struct and identify exactly which fields affect the transcription backend. The fields are: `backend`, `backend_variant`, `whisper.model`, `whisper.bin`. Verify whether `cleanup.vocabulary` / `cleanup.antivocabulary` need worker invalidation (they're used at cleanup time by `cleanup.rs`, not at worker init — likely do NOT need invalidation).
3. In `save_config`, capture the previous config (before `update_cache` overwrites it) by reading the current cached config. Compare the backend-affecting fields between old and new. Only call `invalidate_worker()` + `prewarm()` if one of those fields changed.
4. The comparison can be a simple field-by-field check: `if old.backend != new.backend || old.backend_variant != new.backend_variant || old.whisper.model != new.whisper.model || old.whisper.bin != new.whisper.bin { invalidate + prewarm }`.
5. For non-backend fields, still call `settings::save()` + `settings::update_cache()` so the config persists and the cache is fresh. Just skip the invalidation.
6. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy`.
7. Run `npm run typecheck` (no frontend changes expected).
8. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- Toggling "sound on cancel" or changing the theme does NOT call `transcribe::invalidate_worker()` or `transcribe::prewarm()`.
- Changing `whisper.model` or `backend` or `backend_variant` DOES call them.
- Config still saves to disk and updates the cache on every save.
- The whisper-server process is not killed when unrelated settings change.

## Notes
- To verify at runtime: on macOS, toggle "sound on cancel" while a `ps aux | grep whisper-server` is running. The whisper-server PID should NOT change. Then change the whisper model path — the PID SHOULD change.
- Be careful reading the "previous config" — if the settings cache is cold (first save after startup), there may be no previous config to compare. In that case, default to invalidating (safe fallback).
- The `settings::load()` function returns a clone of the cached config. Call it BEFORE `settings::update_cache()` to get the old value, then compare against the new value after `update_cache()`.
