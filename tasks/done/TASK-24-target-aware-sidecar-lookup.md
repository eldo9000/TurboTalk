# TASK-24: Target-aware Whisper sidecar lookup + per-platform bundle resources

## Goal
`src-tauri/src/transcribe.rs` resolves the Whisper sidecar by target triple (not the hardcoded `whisper-cli-aarch64-apple-darwin` string), and `src-tauri/tauri.conf.json` declares mac/windows/linux bundle resources separately so a non-mac `tauri build` does not try to bundle `.dylib` files. macOS happy path is unchanged.

## Context
TurboTalk is preparing a Win/Linux beta (no code signing, unsigned artifacts). This is a **Tier 1** product per `CLAUDE.md` — keep ceremony light. macOS arm64 v0.8 is the working baseline; do not regress it.

Two scoped problems:

1. **`src-tauri/src/transcribe.rs:65`** has `let sidecars = ["whisper-cli", "whisper-cli-aarch64-apple-darwin"];` — the second name is mac-arm64-only. On Windows the actual file will be `whisper-cli-x86_64-pc-windows-msvc.exe`; on Linux `whisper-cli-x86_64-unknown-linux-gnu`. The lookup needs to be aware of the build's target triple.
2. **`src-tauri/tauri.conf.json` lines 53–57** lists three `.dylib` files unconditionally under `bundle.resources`. On Windows/Linux those files do not exist and `tauri build` will fail before our code runs. Tauri 2 supports per-platform resource maps under `bundle.macOS.resources`, `bundle.windows.resources`, and `bundle.linux.resources` (verify against current Tauri 2 docs — schema may use `bundle.resources` keyed by platform).

The audit at `PLATFORM-AUDIT.md` lines 122–157 documents what each target triple needs.

`src-tauri/binaries/` currently holds only the mac-arm64 sidecar + dylibs. The Windows/Linux sidecar binaries do not exist yet — that is **TASK-26's** problem, not this one. This task only changes the **lookup logic** and **config**, not the binaries.

The `externalBin` declaration `"binaries/whisper-cli"` (line 52) is target-triple-agnostic by Tauri convention — Tauri auto-suffixes. Leave that line alone.

`tauri::process::Command::new_sidecar` is one option for the runtime lookup; reading the target triple at compile time via `env!("TARGET")` or a build-script-injected constant is another. Either is acceptable — pick the one that needs the smallest change to `transcribe.rs`.

## In scope
- `src-tauri/src/transcribe.rs` — `find_whisper()` and the `sidecars` array
- `src-tauri/tauri.conf.json` — `bundle.resources` block (split per platform)
- `src-tauri/build.rs` if a build-time target triple constant is the chosen approach

## Out of scope
- Adding the actual Windows/Linux sidecar binaries (TASK-26)
- Changing `externalBin` or any other field in `tauri.conf.json`
- Touching `src-tauri/Cargo.toml` deps
- Hotkey, paste, diagnostics, onboarding, scripts — all separate tasks
- Refactoring `find_whisper()` beyond the sidecar-name list change

## Steps
1. Decide approach: either (a) read the target triple from `env!("TARGET")` exposed via `build.rs`, or (b) use `tauri::process::Command::new_sidecar` which auto-suffixes the base name. Pick (a) if it's a smaller diff.
2. In `find_whisper()`, replace the hardcoded `["whisper-cli", "whisper-cli-aarch64-apple-darwin"]` array with a lookup that tries `whisper-cli` (unsuffixed, for dev), then `whisper-cli-<target-triple>` (with `.exe` on Windows). Keep the existing dev-fallback path against `src-tauri/binaries/`.
3. Make sure on macOS arm64 the lookup still resolves the existing `whisper-cli-aarch64-apple-darwin` file.
4. In `tauri.conf.json`, move the three `libwhisper.1.dylib` / `libggml.0.dylib` / `libggml-base.0.dylib` entries under a macOS-only resources map. Add empty (or absent) windows/linux resources maps with a comment placeholder pointing to TASK-26.
5. Run `cargo check --manifest-path src-tauri/Cargo.toml` on the host (mac arm64). Confirm green.
6. Run `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu`. Expect either green or a *different* error than `core-foundation` / `dylib`-related.
7. Run `cargo test --manifest-path src-tauri/Cargo.toml`. Confirm the existing test suite still passes.

## Success signal
- `cargo check` green on host (mac arm64).
- `cargo test --manifest-path src-tauri/Cargo.toml` green (66+ tests).
- `tauri.conf.json` parses cleanly (`npm run build` does not error on schema).
- `grep -n "whisper-cli-aarch64-apple-darwin" src-tauri/src/transcribe.rs` returns no matches (the hardcoded triple is gone).
- `grep -n "libwhisper.1.dylib" src-tauri/tauri.conf.json` returns matches **only** under a macOS-keyed section.
- A subsequent `npm run package` on macOS produces the same DMG as before (no regression).

## Notes
Tauri 2 resource schema: confirm against `https://v2.tauri.app/reference/config/#bundleconfig` whether per-platform resources are nested under `bundle.macOS.resources` or `bundle.resources` with platform keys. Current config uses `bundle.macOS.signingIdentity` style nesting, suggesting the former.

→ verify: macOS DMG built post-change still launches and dictates one phrase end-to-end.
