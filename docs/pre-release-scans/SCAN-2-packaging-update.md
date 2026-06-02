# Scan 2 — Packaging / Update Audit

**Goal:** Confirm the macOS bundle is signing/notarization-ready, launch-at-login
behaves, updater metadata is correct, the version bump is consistent, and artifact
naming is right for the updater + release.

**Scope:** read-only audit (you may run build scripts in `--dry-run`/inspect modes
if they support it, but do not produce or publish a release). Produce a findings
report with file:line citations and severity (blocker / should-fix / nit).

**Hard rule:** do **not** create git tags or trigger any CI release build. Release
tagging is user-initiated only.

## Primary files

- `src-tauri/tauri.conf.json` — version `0.9.0`, identifier `io.librewin.turbotalk`,
  updater block (pubkey + endpoint), bundle/macOS signing block.
- `src-tauri/tauri.macos.conf.json` — extra bundled resources (dylibs, vad model,
  onnxruntime).
- `src-tauri/entitlements.plist`, `src-tauri/Info.plist` — referenced by the
  macOS bundle config.
- `package.json` — version `0.9.0`, scripts: `package`, `bump-version`,
  `preflight`, `fetch-sidecars`, `fetch-onnxruntime`, `fetch-vad-model`,
  `refresh-whisper-server`.
- `scripts/bump-version.mjs`, `scripts/verify-macos-bundle.mjs`,
  `scripts/rename-artifact.mjs`, `scripts/preflight.mjs`, `scripts/lib/`.

## Checks to run

1. **Signing / notarization readiness.**
   - `tauri.conf.json` sets `"signingIdentity": "-"` (ad-hoc), `hardenedRuntime: true`,
     and points at `entitlements.plist` + `Info.plist`. Read both files. Confirm the
     entitlements are coherent with hardened runtime (mic access via Info.plist
     `NSMicrophoneUsageDescription`, any JIT/unsigned-memory entitlements the whisper
     sidecars actually need, accessibility usage string for paste/hotkey).
   - Ad-hoc signing (`-`) is **not** notarizable. Determine the intended release
     path: is notarization expected for v0.9, or is this a personal-use unsigned
     build the user gatekeeper-bypasses? State which, and if notarization is
     intended, list exactly what's missing (Developer ID identity, `notarytool`
     step, stapling).
   - The `externalBin` whisper sidecars and the bundled dylibs in
     `tauri.macos.conf.json` must each be signed for hardened runtime to launch.
     Confirm `verify-macos-bundle.mjs` actually checks codesign/load status of every
     bundled binary and dylib — read it and list what it verifies vs. assumes.

2. **Launch-at-login.**
   - Grep the tree for launch-at-login / login-item / autostart wiring
     (`autostart`, `LaunchAgent`, `SMAppService`, `set_activation_policy`,
     tray "Start at login"). Confirm whether the feature exists, how it's toggled,
     and that it's persisted in settings. If it registers a login item, confirm it
     uses the correct bundle identifier and survives an app update (path stability).
   - If launch-at-login is **not** implemented, say so — the audit item may be
     aspirational. Report present/absent plainly.

3. **Updater metadata.**
   - `tauri.conf.json` updater endpoint is
     `https://github.com/eldo9000/TurboTalk-App/releases/latest/download/latest.json`.
     Note repo identifier mismatch risk: CLAUDE.md/updater point at `eldo9000` —
     confirm that's the real release repo and the pubkey corresponds to the private
     key the user signs with.
   - `createUpdaterArtifacts: false` in the bundle block. The updater needs a signed
     `latest.json` + `.app.tar.gz` + `.sig`. With this false, confirm where updater
     artifacts get produced (is `package` script generating them another way, or is
     the updater effectively non-functional for v0.9?). This is the highest-value
     finding in this scan — resolve it definitively.
   - Confirm the `latest.json` `version` will match `tauri.conf.json` version and
     the artifact URL pattern matches what `rename-artifact.mjs` actually emits.

4. **Version-bump consistency.**
   - `package.json` and `tauri.conf.json` both say `0.9.0`. Read `bump-version.mjs`
     and confirm it updates **all** version sources atomically (package.json,
     tauri.conf.json, Cargo.toml in `src-tauri/`, and any updater/Info.plist
     `CFBundleShortVersionString` / `CFBundleVersion`). List any source it misses.
   - Confirm `src-tauri/Cargo.toml` version matches 0.9.0 (the diagnostic report
     prints `CARGO_PKG_VERSION`, so a drift here mislabels every bug report).

5. **Artifact naming.**
   - Read `rename-artifact.mjs`. Confirm the output matches the documented pattern
     `TurboTalk-<version>-macos-arm64.dmg` (CLAUDE.md) and that the name the updater
     endpoint expects lines up with what's uploaded. Flag any productName-vs-filename
     mismatch (`"Turbo Talk"` with a space vs. `TurboTalk` in the artifact name).

## Deliverable

A markdown report. Lead with two verdicts: (a) **is this build notarization-ready,
and if not, what's the gap?** (b) **is the updater functional end-to-end for v0.9,
or is it a no-op?** Then the per-item findings with file:line and severity.
