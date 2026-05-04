# TASK-1: Wire macOS code signing + notarization for beta DMG

## Goal
`npm run package` produces a signed and notarized DMG suitable for distribution
outside the Mac App Store. The DMG passes `spctl -a -t open --context context:primary-signature -v <dmg>`
on a machine that has not run TurboTalk before.

## Context

TurboTalk is preparing its first external beta. The app is currently signed
ad-hoc with `signingIdentity: "-"` in `src-tauri/tauri.conf.json` (line 59) and
is **not notarized**. This is fine for the developer's own machine but will
trip Gatekeeper on any other Mac:

> "Turbo Talk" cannot be opened because the developer cannot be verified.

For external beta we need:

1. A real Developer ID Application certificate baked into the build (signing).
2. A successful submission to Apple's notary service with a stapled ticket
   (notarization).

Tauri 2 supports both via `bundle.macOS` config and environment variables.
Reference: <https://v2.tauri.app/distribute/sign/macos/>

The user has (or will obtain) an Apple Developer account. This task wires the
config and documents the env vars; it does **not** require the agent to perform
an actual signed build (the signing identity is sensitive and the user runs the
real release build).

The whisper-cli sidecar binary and the three bundled `.dylib` files under
`src-tauri/binaries/` must also be signed (Tauri handles sidecar signing
automatically when `signingIdentity` is set, but the dylibs are bundled as
`resources` and may need explicit handling — verify in the Tauri docs).

`BUILD.md` already documents the local-dev build path. This task adds a
"Release build" section describing the signed/notarized flow.

## In scope
- `src-tauri/tauri.conf.json` — replace ad-hoc signing identity with a
  configurable one; add notarization-relevant config if any.
- `BUILD.md` — add a "Release build (signed + notarized)" section listing the
  required environment variables, certificate setup, and the actual build
  invocation.
- `.gitignore` — confirm any cert/keychain artifacts are ignored if the docs
  reference local files.
- Optionally a `scripts/release-build.mjs` that validates required env vars
  exist before invoking `tauri build`. Only add if it's a clean win — skip if
  it would just be a thin wrapper.

## Out of scope
- Performing the actual signed/notarized build (requires Apple Developer
  credentials the agent does not have).
- Windows or Linux signing — beta is macOS-first per
  `BETA-AUDIT-ROADMAP.md` recommended sequencing.
- Tauri updater plugin signing — covered by a separate task in this arc.
- Acquiring the Developer ID certificate — user task.

## Steps
1. Read the current `src-tauri/tauri.conf.json` `bundle.macOS` block and the
   Tauri 2 macOS signing docs at
   <https://v2.tauri.app/distribute/sign/macos/>.
2. Decide the config approach:
   - Keep `signingIdentity: "-"` as the default (so dev builds still work) and
     have release builds override via the `TAURI_SIGNING_IDENTITY` env var, OR
   - Switch the config to read the identity from env at build time.
   Pick whichever Tauri 2 supports cleanly. Document the choice in `BUILD.md`.
3. Add notarization config if Tauri exposes it via `tauri.conf.json` (it
   typically uses env vars: `APPLE_ID`, `APPLE_PASSWORD` or
   `APPLE_API_KEY`/`APPLE_API_ISSUER`, `APPLE_TEAM_ID`).
4. Verify the bundled `.dylib` resources will be signed by the build pipeline.
   If they won't, document the manual `codesign --deep` step or add it to the
   release script.
5. Add a "Release build (signed + notarized)" section to `BUILD.md` with:
   - Required env vars and where to get each value.
   - Where to install the Developer ID cert (Keychain Access, login keychain).
   - The exact command to run a release build.
   - The exact command to verify the result locally:
     `spctl -a -t open --context context:primary-signature -v
     dist-artifacts/TurboTalk-<version>-macos-arm64.dmg`
   - A note that the notary upload can take 5–15 minutes.
6. Confirm `.gitignore` does not need updates (no local cert files should be
   committed).

## Success signal
- `git diff` shows updates to `src-tauri/tauri.conf.json` and `BUILD.md`.
- `npm run tauri dev` still works (dev path unbroken).
- `BUILD.md` "Release build (signed + notarized)" section, when read by
  someone unfamiliar with macOS code signing, gives them every command and
  env var they need with no external lookup required.
- The success signal for the actual signed build is recorded in `BUILD.md`
  as the `spctl` verification command — the user will run it and report back.

## Notes
- Tauri 2 docs may have changed since the URL above was captured. If the page
  has moved, search for "tauri 2 macos sign" on `v2.tauri.app`.
- App-specific passwords for `APPLE_PASSWORD` are generated at
  `appleid.apple.com` → Sign-In and Security → App-Specific Passwords. The
  account password itself will not work.
- The notarization stapling step (`xcrun stapler staple`) may need to be added
  to the release script if Tauri does not do it automatically.
