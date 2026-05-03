# Releasing TurboTalk

## Scope

This is the procedure for cutting a TurboTalk beta release. macOS arm64 (Apple
Silicon) only for now — see `BUILD.md` for why other platforms are deferred.
This document is the *release procedure* (versioning, tagging, publishing,
release notes); `BUILD.md` is the *build procedure* (compiling, signing,
notarizing). Follow it linearly from top to bottom.

## Pre-flight

Before doing anything else, confirm:

- `git status` is clean and you are on `main`.
- `cargo test` passes (run from `src-tauri/`).
- `cargo clippy -- -D warnings` is green (run from `src-tauri/`).
- A manual smoke test on a fresh `npm run tauri dev` build passes — at
  minimum, complete Test 3 in `SMOKE-TEST.md` (push-to-talk → transcript
  pastes into TextEdit).

If any of the above fails, stop. Do not cut a release on top of a red tree.

## Step 1 — Bump the version

```bash
npm run bump-version -- <new-version>
```

This script updates `package.json`, `src-tauri/Cargo.toml`, and
`src-tauri/tauri.conf.json` together. Inspect the diff to confirm all three
files moved to the same version, then commit:

```bash
git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json Cargo.lock
git commit -m "chore(release): bump to <new-version>"
```

## Step 2 — Build the signed + notarized DMG

Set the four `APPLE_*` environment variables described in `BUILD.md` →
"Release build (signed + notarized)" → "Required env vars", then run:

```bash
npm run package
```

The notarization upload + scan typically takes 5–15 minutes; the build
will appear to hang during that window. When it finishes, confirm both
files exist:

```
dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg
dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg.sha256
```

If either is missing, do not proceed. See `BUILD.md` for diagnosis.

## Step 3 — Verify the artifact

On the build machine, verify signing and notarization with the `spctl`
commands in `BUILD.md` → "Verify the result". Healthy output for the DMG is:

```
dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg: accepted
source=Notarized Developer ID
```

Then run the full smoke test against the **installed** DMG (not a dev
build) on a clean macOS account — see `SMOKE-TEST.md` →
"Installed-artifact" section. Every test in that section must pass before
you publish.

## Step 4 — Tag and publish

```bash
git tag v<new-version>
git push origin main --tags
```

Write the release notes for this version into `RELEASE_NOTES.md` at the
repo root using the template at the bottom of this file, then publish:

```bash
gh release create v<new-version> \
  --title "TurboTalk <new-version> beta" \
  --notes-file RELEASE_NOTES.md \
  dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg \
  dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg.sha256
```

`RELEASE_NOTES.md` is a scratch file per release — do not commit it.

## Step 5 — Update SESSION-STATUS.md

Add a single line under "Where We Are":

> Released v<new-version> beta on <YYYY-MM-DD>.

Commit as `chore(status): record v<new-version> release`.

## Manual updates policy

**TurboTalk beta uses manual updates only.** When a new release ships,
users download the new DMG and replace `Turbo Talk.app` in `/Applications`.
The Tauri updater plugin is intentionally **not** enabled.

Before enabling auto-update we need: (1) a long-lived updater signing key
with a documented secure-custody plan, (2) a stable artifact-hosting URL
that we control, (3) a written key-rotation/loss procedure. See
`BETA-AUDIT-ROADMAP.md` line 333 — "Do not add auto-update until artifact
naming, signing, and release hosting are stable." Until those three exist,
do not enable the updater plugin.

## Release notes template

Copy this into `RELEASE_NOTES.md` and fill in the `<placeholders>`:

```markdown
## TurboTalk <version> beta

**Platform:** macOS 12+ on Apple Silicon (arm64).

**Install:** Download `TurboTalk-<version>-macos-arm64.dmg`, drag
`Turbo Talk.app` into `/Applications`, launch it, and grant Microphone
and Accessibility permissions when prompted.

**What's in this release**
- <one bullet per user-visible change>

**Known limitations**
- macOS arm64 only. Intel Macs, Windows, and Linux are not supported in
  this beta.
- <other known issues>

**Privacy and data** — see `PRIVACY.md`. To delete all local TurboTalk
data, follow `PRIVACY.md` → "How to delete everything".

**Verify the download**

    shasum -a 256 -c TurboTalk-<version>-macos-arm64.dmg.sha256

**Feedback** — file an issue at
https://github.com/eldo9000/TurboTalk-App/issues
```

## Hotfix path

Beta hotfixes follow the same procedure: bump the patch version, rebuild,
re-verify, re-publish. There is no separate fast-path.

## Rollback

We do not delete published GitHub releases. If a release is broken, mark
it as **pre-release** (`gh release edit v<version> --prerelease`) so the
"Latest" badge moves off it, then publish a corrected release with a
bumped patch version. The broken release stays visible for forensics.
