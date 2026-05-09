# TASK-43: Verify bundled Metal backend in packaged app

## Goal

Confirm that the packaged TurboTalk DMG contains every GGML backend dylib whisper-cli needs at runtime, and that a packaged-app dictation on a Homebrew-free Mac shows Metal active in the logs — not a Homebrew-path fallback or a silent CPU-only path.

## Context

TurboTalk bundles `whisper-cli` as a sidecar, with the active candidate-search logic in `src-tauri/src/transcribe.rs:71-130` (priority: bundled-next-to-exe → dev binaries dir → configured path). The macOS bundle config lives in `src-tauri/tauri.macos.conf.json`, which currently bundles only a small set of dylibs. In dev, `whisper-cli` has been observed loading GGML backend libraries from Homebrew paths (`/opt/homebrew/...`). That's fine for development but the packaged app must not depend on Homebrew, or it will silently fall back (or fail) on a fresh Mac.

A packaged DMG can be produced with `npm run package` → `dist-artifacts/TurboTalk-<version>-macos-arm64.dmg`.

Tier 1: name the proof. The proof is "packaged app dictates successfully on a Homebrew-free Mac (or with Homebrew temporarily masked) and the whisper backend init log shows Metal."

## In scope

- `src-tauri/tauri.macos.conf.json` — bundle resources / frameworks
- `src-tauri/binaries/` — audit which whisper/ggml dylibs ship
- packaging output: `dist-artifacts/TurboTalk-<version>-macos-arm64.dmg`

## Out of scope

- whisper.cpp version upgrade (CoreML task handles binary replacement)
- adding new accelerators
- aggressive bundle-size optimization beyond fixing missing dylibs

## Steps

1. Build a release DMG: `npm run package`. Mount the produced DMG. Inspect `TurboTalk.app/Contents/MacOS/`, `Contents/Frameworks/`, and `Contents/Resources/` for the bundled `whisper-cli` and any GGML/whisper dylibs.
2. Run `otool -L <bundled-whisper-cli>` and `otool -L <each bundled dylib>`. Note every referenced dylib path. Flag any path that resolves to `/opt/homebrew/...`, `/usr/local/...`, or any developer-machine-specific location.
3. Find runtime-loaded backend modules: `strings <bundled-whisper-cli> | grep -iE 'ggml-(metal|cpu|blas|coreml)'` and `strings <bundled-whisper-cli> | grep -i 'libggml'`. Note every backend module name the binary may try to dlopen at runtime.
4. Confirm each runtime-loaded backend dylib is present inside the bundle alongside the whisper-cli binary (whisper.cpp typically searches the executable's directory). If `ggml-metal.dylib` or any other expected backend is missing, add it to `src-tauri/tauri.macos.conf.json` resources or to `src-tauri/binaries/` so it ships next to the sidecar.
5. Install the DMG to `/Applications`. Mask Homebrew dylibs for the test: easiest is `sudo mv /opt/homebrew /opt/homebrew.disabled` for the duration of the test, then restore. (Alternatively run from a Mac account that has no Homebrew installed.)
6. Launch the app from `/Applications`. Dictate a normal utterance. Confirm transcription succeeds.
7. Capture the whisper-cli stderr/stdout from the run. Confirm the backend init log line shows Metal (e.g. `whisper_backend_init: using Metal backend` or whisper.cpp's equivalent for the bundled version). Confirm no log line indicates Homebrew-path resolution or CPU-only fallback.
8. Restore Homebrew (`sudo mv /opt/homebrew.disabled /opt/homebrew`). Verify the dev build still works.
9. If any dylibs were added to the bundle, re-run steps 1–7 to confirm the fix.

## Success signal

- Packaged app dictates successfully with Homebrew masked / on a clean Mac.
- `otool -L` on every bundled binary shows zero Homebrew-path or developer-machine-path dependencies.
- whisper-cli backend init log line clearly shows Metal selected (not CPU fallback).
- The relevant log lines are captured as proof in this task file or SESSION-STATUS.

## Notes

- Don't blindly copy every Homebrew dylib into the bundle. Stick to what `otool -L` and `strings | grep` say is needed. Over-bundling bloats the DMG and risks dylib-version mismatches.
- whisper.cpp may emit a multi-line backend selection log; capture the whole block, not just one line.
- If the bundled `whisper-cli` cannot find Metal dylibs even when they're present, the issue is likely an `@rpath` or install-name problem. Use `install_name_tool -change` or rebuild whisper-cli with the right rpath rather than working around it with environment variables.
- Restoring Homebrew after the test is critical — don't leave the dev environment broken.
