# Scan 2 — Packaging / Update: Findings

**Date:** 2026-06-02 · **Scope:** read-only audit. No tags created, no CI triggered, no release produced.

## Verdicts

**(a) Notarization-ready? — NO, and that is by design.**
`signingIdentity: "-"` (ad-hoc, `tauri.conf.json:102`) is not notarizable. The release
pipeline ad-hoc signs (`release.yml:215`), ships unsigned, and the release notes tell
users to `xattr -cr` past Gatekeeper (`release.yml:445–452`). This intentional unsigned-beta
posture is documented in `RELEASING.md`, `RELEASE-READINESS.md:64`, and the release body.
**Gap if notarization is ever wanted:** a Developer ID Application identity (replace `"-"`),
a `notarytool submit` step, and `stapler staple` — none exist today. Hardened runtime is
already on and entitlements are coherent (see Finding 1), so the entitlement side is ready.

**(b) Updater functional end-to-end for v0.9? — DESIGNED YES, but currently BLOCKED by a
config/workflow contradiction.** The full chain exists and is coherent: runtime check
(`UpdateManager.svelte`), capability (`capabilities/main.json:15`), CI re-pack + re-sign +
`latest.json` + GitHub release (`release.yml:218–456`), real pubkey populated
(`tauri.conf.json:90`), endpoint matches the manifest base URL. **But** `createUpdaterArtifacts:
false` (`tauri.conf.json:99`) suppresses the `*.app.tar.gz` + `.sig` that the workflow's
"Locate" step hard-requires post-build (`release.yml:192–203`) — so the release job fails
before it reaches re-pack. See Finding 3 (blocker). The local `npm run package` path
deliberately produces a DMG only and **no** updater artifacts — that is expected and correct;
updater artifacts are CI-only.

---

## Findings

| # | Severity | Area | Finding |
|---|----------|------|---------|
| 1 | pass | Signing | Entitlements coherent with hardened runtime; ad-hoc is intentional. |
| 2 | should-fix | Signing | `verify-macos-bundle.mjs` never checks codesign; not run in the macOS release job at all. |
| 3 | **blocker** | Updater | `createUpdaterArtifacts:false` contradicts the workflow's Locate step. |
| 4 | should-fix | Launch-at-login | Uninstall docs reference `com.librewin.turbotalk.plist`; identifier is `io.librewin.turbotalk`. |
| 5 | pass | Version | All three manifests at 0.9.0; bump-version updates them atomically with verify. |
| 6 | nit | Artifact naming | Local rename pattern ≠ CI release names; productName has a space. |

### Finding 1 — Signing / entitlements ✅ (ad-hoc, intentional)
- `entitlements.plist` declares: `device.audio-input` (mic), `automation.apple-events`
  (osascript paste), `cs.allow-jit` (WKWebView), `cs.allow-unsigned-executable-memory` +
  `cs.disable-library-validation` (whisper.cpp dylibs / runtime kernels). All coherent with
  `hardenedRuntime: true`. Each has an explaining comment.
- `Info.plist` carries `NSMicrophoneUsageDescription` + `NSAppleEventsUsageDescription` +
  `LSUIElement` (menu-bar app). Accessibility (CGEventTap hotkey / paste) has **no** Info.plist
  key by design — it is a runtime TCC grant, so nothing is missing there.
- Note: `allow-unsigned-executable-memory` + `disable-library-validation` weaken the hardened
  runtime and would draw scrutiny *if* notarizing; acceptable for ad-hoc. (`tauri.conf.json:101–106`)

### Finding 2 — Bundle verification gaps (should-fix)
`verify-macos-bundle.mjs` checks **file presence** (binaries/dylibs/model exist and are
non-empty), an `otool -L` Homebrew-leak check on `whisper-server` (`:49–62`), and a CoreML
linkage guard (`:64–70`). It does **not** run `codesign --verify` on any binary or dylib —
it *assumes* signing. Two consequences:
- The macOS release job (`release.yml`) builds with plain `tauri build` (`:177–178`) and
  **never invokes `verify-macos-bundle.mjs`** — only local `npm run package` and the Windows
  CI job do. So the Homebrew-leak / CoreML guard is absent from the actual released DMG.
  (CI does pre-patch the rpath at `release.yml:169`, which mitigates the leak, but the guard
  doesn't run.)
- CI's own `codesign --verify --deep --strict` is suffixed `|| true` (`release.yml:216`), so a
  signing failure never fails the build.
*Fix:* run `verify-macos-bundle.mjs` against the built `.app` in the darwin job, and drop the
`|| true` on the codesign verify (or accept both as known beta gaps and note it).

### Finding 3 — Updater artifact generation is disabled (BLOCKER)
`tauri.conf.json:99` sets `createUpdaterArtifacts: false`. In Tauri 2 this suppresses the
`TurboTalk.app.tar.gz` + `.sig` that `tauri build` would otherwise emit. The release workflow
builds with a plain `npm run tauri build` (`release.yml:177–178`, no override), then its
"Locate macOS build output" step requires both files to already exist and hard-errors if they
don't (`release.yml:192–203`, `set -euo pipefail` + the `for f in … TARBALL SIG` existence loop).
With the flag `false`, those files are never produced, so the job fails at Locate — before
reaching the re-pack (`:218`) / re-sign (`:229`) steps that would otherwise rebuild them.
*Resolution (highest-value fix in this scan):* set `createUpdaterArtifacts: true`. The workflow's
clear intent — build → ad-hoc sign the `.app` → re-pack the tarball from the signed app → re-sign
with the minisign key → emit `latest.json` — only works if `tauri build` first emits the tarball/sig
that Locate finds. Confirm with one real CI release run after flipping the flag. *(Hard rule
respected: not triggered here.)*
Everything else in the updater chain is correct: endpoint `eldo9000/TurboTalk-App/.../latest.json`
(`tauri.conf.json:92`) matches the manifest base URL (`release.yml:412`) and the release upload
target; `latest.json.version` = tag version (`release.yml:414`); the tarball URL is derived from
the actually-uploaded filename (`release.yml:407,420`); pubkey is populated (not the placeholder
the workflow header warns about). One thing not verifiable from source: that the `TAURI_SIGNING_
PRIVATE_KEY` secret corresponds to the embedded pubkey — assumed correct.

### Finding 4 — Launch-at-login present; uninstall docs name the wrong plist (should-fix)
Feature is fully implemented via `tauri-plugin-autostart` (`MacosLauncher::LaunchAgent`,
`lib.rs:1975–1978`): commands `get_launch_at_login`/`set_launch_at_login` (`lib.rs:517–527`),
toggled in Settings (`App.svelte:950`), Onboarding (`Onboarding.svelte:185`), and the tray
"launch" item (`lib.rs:2112`); `reset_turbotalk` disables it (`lib.rs:539`). State of truth is
the OS LaunchAgent (`app.autolaunch().is_enabled()`), which survives relaunch — correct mechanism.
**Mismatch:** the bundle identifier is `io.librewin.turbotalk` (`tauri.conf.json:4`), so the
plugin writes `~/Library/LaunchAgents/io.librewin.turbotalk.plist`, but `PRIVACY.md:69` and
`SMOKE-TEST.md:336` tell users to unload/delete `com.librewin.turbotalk.plist`. At least one
prefix is wrong; following the docs would leave a real login item orphaned after "uninstall."
*Fix:* correct the docs to the actual identifier (verify the exact plist name on a real install).
Path-stability across update is the standard autostart caveat (plist embeds the app path; fine
for in-place `/Applications` updates, breaks if the app is moved).

### Finding 5 — Version consistency ✅
`package.json:4`, `tauri.conf.json:3`, and `src-tauri/Cargo.toml:3` are all `0.9.0`.
`bump-version.mjs` updates exactly these three in lockstep (`:68–69,113`) with a re-read
verification pass (`:115–148`). `Info.plist` carries no `CFBundleShortVersionString`/
`CFBundleVersion` keys, so there is no plist version to drift — Tauri injects the version from
`tauri.conf.json` at build time. The diagnostic report's `CARGO_PKG_VERSION` is therefore in
sync. CI additionally gates the tag against `tauri.conf.json` version (`release.yml:102–111`).
*Nit:* that CI gate checks only `tauri.conf.json`, not Cargo.toml/package.json — but bump-version
is the intended atomic path, so drift requires a manual hand-edit.

### Finding 6 — Artifact naming (nit)
- Local `rename-artifact.mjs` emits `TurboTalk-0.9.0-macos-arm64.dmg` + `.sha256`
  (`:90`), matching the CLAUDE.md convention. ✅
- The macOS **release** job does not use `rename-artifact.mjs`; it uploads the Tauri-default
  DMG name and adds a version-less stable copy `TurboTalk-macOS-arm64.dmg` for direct download
  (`release.yml:383–386`). So the published DMG name differs from the local convention — cosmetic,
  not functional.
- productName is `"Turbo Talk"` (space) → bundle is `Turbo Talk.app`, while filenames use
  `TurboTalk` (no space). The workflow probes both (`release.yml:186–191`) and the updater
  re-packs/extracts by the real bundle name, so it resolves — but the space/no-space split is a
  latent footgun worth keeping in mind for the updater's extract-and-replace.

## Bottom line
One blocker for the **release/updater path**: Finding 3 (`createUpdaterArtifacts:false` vs the
workflow's Locate requirement) — flip to `true` and confirm with one CI run. Notarization is
intentionally absent and well-documented (not a blocker). Two should-fixes (bundle-verify not run
on the release artifact; wrong plist name in uninstall docs) and minor naming nits. Version
consistency is solid.
