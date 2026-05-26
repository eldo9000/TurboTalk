# TASK-56: Silero VAD pre-filter for whisper-server

## Goal
whisper-server is launched with Silero VAD enabled, so silent regions of the recorded WAV are skipped before the decoder runs. Holding PTT through 3s of silence + 2s of speech produces a transcript covering only the speech portion. A settings toggle exposes the VAD on/off so a false-negative regression can be diagnosed without rebuilding.

## Context
TurboTalk is a personal-use macOS push-to-talk dictation utility. Tier 1 product. Read `CLAUDE.md` at repo root and `~/Downloads/Github/Business-OS/standards/ENGINEERING.md` before starting.

This task assumes Phase 1 (TASK-55, the post-hoc hallucination filter) has already landed. VAD is the second line of defense: rather than detecting garbage after Whisper invents it, VAD prevents Whisper from being asked to transcribe silence in the first place. whisper.cpp's server binary supports `--vad` + `--vad-model <path>` with a Silero ONNX model (~2 MB). Reference: <https://github.com/ggml-org/whisper.cpp> — search the server README for "VAD".

Current whisper-server spawn is at `src-tauri/src/transcribe.rs:338-347`. Sidecar binaries live in `src-tauri/binaries/` in dev and next to the executable in release bundles (see `find_whisper_server` at `src-tauri/src/transcribe.rs:147`). The Silero model should be bundled the same way and pathed in via the same allow-list logic.

Settings live in `src-tauri/src/settings.rs` with persistence under `~/.config/librewin/turbotalk/config.toml`. Add a new `whisper.vad_enabled: bool` field defaulting to `true`. The settings UI lives in `src/` — look for the Settings tab component.

## In scope
- `src-tauri/binaries/` — add the Silero VAD model file (e.g. `ggml-silero-v5.1.2.bin` or the current canonical name from whisper.cpp's model download script); also bundle into the release in `tauri.conf.json` if the bundling list needs updating
- `src-tauri/src/transcribe.rs` — modify the `whisper-server` spawn to conditionally pass `--vad` and `--vad-model <path>` based on the new setting; reuse `find_whisper_server`-style allow-list logic to locate the VAD model file
- `src-tauri/src/settings.rs` — add `vad_enabled` field to the whisper config struct with `default = true`
- `src/` — Settings tab: add a toggle for "Skip silent regions (VAD)"; bind to the new field
- `SESSION-STATUS.md` and `TRUTH.md` — one-line update each

## Out of scope
- Custom VAD thresholds (start with whisper-server defaults; expose tuning only if it misbehaves)
- Replacing the post-hoc detection from TASK-55 — both layers stay
- Any new backend (Phase 3)
- Bundling VAD for non-macOS platforms — wire the path resolution so it could work, but only ship the macOS arm64 file for now

## Steps
1. Read `CLAUDE.md`, `~/Downloads/Github/Business-OS/standards/ENGINEERING.md`, `SESSION-STATUS.md`, `TRUTH.md`, `src-tauri/src/transcribe.rs`, `src-tauri/src/settings.rs`.
2. Confirm whisper.cpp/server in your current `binaries/` build supports `--vad` — run `./src-tauri/binaries/whisper-server --help | grep -i vad`. If the bundled build is too old, plan a binary refresh as part of this task and note it.
3. Obtain the Silero VAD model file from the whisper.cpp `models/download-vad-model.sh` script or upstream release. Place it in `src-tauri/binaries/` with a stable filename.
4. Add a `vad_model_candidates()` sibling to `server_sidecar_candidates()` and a `find_vad_model()` sibling to `find_whisper_server()` in `transcribe.rs`, reusing the existing allow-list logic.
5. Add `vad_enabled: bool` (default true) to the whisper section of `Config` in `settings.rs`. Update any existing tests that construct a default `Config`.
6. In `TranscriptionWorker::from_config`, read `cfg.whisper.vad_enabled`. If true, resolve the VAD model path and append `--vad`, `--vad-model <path>` to the spawn args.
7. In the Settings tab in `src/`, add a labeled toggle for "Skip silent regions (VAD)". On change, persist via the existing settings save path. Confirm the worker is rebuilt on settings change — `invalidate_worker()` should already fire on save (see `src-tauri/src/transcribe.rs:573`).
8. Update `tauri.conf.json` `bundle.resources` (or sidecar list) if needed so the VAD model file ships in the release bundle.
9. Run `npm run tauri dev`. Hold PTT, stay silent for 3s, then say "hello world" for 2s, release. Confirm the transcript is approximately "hello world" with no silence-hallucination text in front of it. Confirm VAD-enabled log line appears in `/tmp/whisper-server-stderr.log` or wherever whisper-server logs.
10. Toggle the setting off, repeat the same dictation. Confirm whisper-server is rebuilt (worker invalidated → new spawn on next press) and that the silence-prefix returns (compared to the VAD-on case) — proves the toggle actually does something.
11. Update `SESSION-STATUS.md` and `TRUTH.md`.
12. Commit with `feat(transcribe): enable Silero VAD pre-filter, toggleable in settings`.

## Success signal
- Dictation: 3s silence held + 2s "hello world" → transcript is approximately "hello world" with no leading hallucinated text.
- whisper-server log contains evidence of VAD initialization (a log line referencing "vad" or "silero" at server start).
- Settings toggle round-trips: turning VAD off, then on, then off again, dictating each time, shows visibly different behavior between on and off.
- `cargo test` exits 0 (settings default test updated if needed).
- The VAD model file is present in the built release bundle (verify by running `npm run package` and inspecting the resulting `.app`).

## Notes
- If the bundled whisper-server binary is older than VAD support, decide whether to refresh it as part of this task or open a separate task. Refreshing the binary also re-runs the libggml-instance-collision risk noted in `src-tauri/src/transcribe.rs:141-146` — read that block before swapping the binary.
- VAD can be too aggressive on quiet voices. If your normal speaking voice gets clipped, the toggle gives you an escape hatch. Tuning is out of scope here but note misbehavior in `SESSION-STATUS.md` if you see it during dogfooding.
- Phase 1's post-hoc detection (TASK-55) still runs after VAD — they compose, not replace.
