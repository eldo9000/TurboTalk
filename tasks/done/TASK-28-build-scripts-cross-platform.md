# TASK-28: Cross-platform preflight + artifact-rename build scripts

## Goal
`scripts/preflight.mjs` checks that the right sidecar + companion libs exist for the **current build host** before `tauri build` runs, regardless of OS. `scripts/rename-artifact.mjs` produces correctly-named final artifacts for macOS, Windows, and Linux with matching `.sha256` files. `npm run package` works on all three host OSes.

## Goal (what done looks like)
- Building from a macOS host → produces `dist-artifacts/TurboTalk-<v>-macos-arm64.dmg` + `.sha256`. (unchanged)
- Building from a Windows host → produces `dist-artifacts/TurboTalk-<v>-windows-x64-setup.exe` + `.sha256`.
- Building from a Linux host → produces `dist-artifacts/TurboTalk-<v>-linux-x64.AppImage` + `.sha256`.
- Preflight fails with a clear error if any required sidecar / lib is missing for the host platform.

## Context
Two scripts already exist and are mac-shaped:

**`scripts/preflight.mjs`** — checks that `src-tauri/binaries/whisper-cli-aarch64-apple-darwin`, `libwhisper.1.dylib`, etc. exist. Wired via `npm run package` (preflight && build). Today it errors on a non-mac host because it expects the dylibs.

**`scripts/rename-artifact.mjs`** — looks at `target/release/bundle/dmg/*.dmg`, copies it to `dist-artifacts/TurboTalk-<v>-macos-arm64.dmg`, and emits a `.sha256` companion. Hardcoded to dmg.

Tauri builds output different artifact paths per host:
- macOS: `target/release/bundle/dmg/<productName>_<v>_aarch64.dmg`
- Windows: `target/release/bundle/nsis/<productName>_<v>_x64-setup.exe` (NSIS) or `bundle/msi/...` (MSI). Beta target = NSIS.
- Linux: `target/release/bundle/appimage/<productName>_<v>_amd64.AppImage` and/or `bundle/deb/...`. Beta target = AppImage.

Both scripts need to detect the host OS via `process.platform` (`darwin` / `win32` / `linux`) and branch.

This is **infrastructure only** — no new sidecars, no new bundle config, no new release procedure. Just makes the existing scripts honest about the three host shapes.

`package.json` already wires `npm run package` → `npm run preflight && npm run tauri build`. Leave that wiring; just make both scripts platform-aware.

## In scope
- `scripts/preflight.mjs` — host-OS-aware sidecar checks
- `scripts/rename-artifact.mjs` — host-OS-aware artifact discovery + naming
- `package.json` — add per-platform npm scripts only if needed for clarity (e.g. `package:win`, `package:linux`); keep the existing `package` script working as the default

## Out of scope
- Cross-compilation from one host OS to another — out of scope for v1 beta. Each host builds for its own OS.
- Sidecar binaries themselves (TASK-27)
- `tauri.conf.json` resources (TASK-24)
- CI workflow (TASK-31)
- Code signing (explicitly skipped for this beta)
- Upload/release publishing — only artifact naming + checksums

## Steps
1. Read both existing scripts and identify every mac-specific assumption (dylib names, artifact path, version-bump logic if any).
2. Refactor `scripts/preflight.mjs`:
   - Detect host via `process.platform`.
   - On `darwin`: check existing mac arm64 sidecar + dylibs.
   - On `win32`: check `whisper-cli-x86_64-pc-windows-msvc.exe` + DLLs (or just the exe if static build chosen in TASK-27).
   - On `linux`: check `whisper-cli-x86_64-unknown-linux-gnu` + .so files (or just the binary).
   - Each missing file logs a specific message including the path and the TASK-27 reference.
   - Exit non-zero with `process.exit(1)` on any missing file.
3. Refactor `scripts/rename-artifact.mjs`:
   - Read version from `package.json`.
   - Branch on `process.platform`:
     - `darwin`: glob `target/release/bundle/dmg/*.dmg` → `dist-artifacts/TurboTalk-<v>-macos-arm64.dmg`.
     - `win32`: glob `target/release/bundle/nsis/*-setup.exe` → `dist-artifacts/TurboTalk-<v>-windows-x64-setup.exe`.
     - `linux`: glob `target/release/bundle/appimage/*.AppImage` → `dist-artifacts/TurboTalk-<v>-linux-x64.AppImage`.
   - For each, emit a `.sha256` in `shasum -a 256 -c` format (already done for mac — reuse the same writer).
   - If multiple matches (rare), pick the most recently modified.
   - If zero matches, exit with a clear error pointing at the expected directory.
4. Test locally on macOS: `npm run package` produces the same DMG + .sha256 as before. Confirm `dist-artifacts/` shows no spurious files.
5. Smoke-test the script on the other hosts via dry-run if real builds aren't available yet:
   - On Win/Linux, run only `node scripts/preflight.mjs` with the sidecars in place (after TASK-27). Confirm green.

## Success signal
- macOS: `npm run package` still produces `dist-artifacts/TurboTalk-0.8.0-macos-arm64.dmg` + valid `.sha256`. `shasum -a 256 -c TurboTalk-0.8.0-macos-arm64.dmg.sha256` returns OK.
- Both scripts contain a `switch (process.platform)` (or equivalent) — `grep` confirms.
- `node scripts/preflight.mjs` on a host where the local sidecars are intentionally renamed prints a clear "missing sidecar" error and exits 1.
- Running `npm run package` on a Linux dev host (after TASK-27 binaries are in place) produces `TurboTalk-<v>-linux-x64.AppImage` + `.sha256`.

## Notes
- Tauri 2's exact artifact filename inside `bundle/<format>/` may include the productName with spaces (`Turbo Talk_0.8.0_aarch64.dmg`). Glob with care.
- AppImage may emit either `.AppImage` or `.AppImage.tar.gz` depending on bundler config. Match the actual output, not the doc.
- NSIS installer naming may include `-setup` or `_setup` — confirm against the actual file Tauri produces, then match exactly.
- Don't conflate "host" and "target". This task assumes host == target. Cross-compile is explicitly out of scope.

→ verify: from a Windows host, `npm run package` exits clean with a single `.exe` + `.sha256` in `dist-artifacts/`. Same shape on Linux with `.AppImage` + `.sha256`. macOS unchanged.
