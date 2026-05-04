# TASK-2: Add a script that bumps version in all three manifests at once

## Goal
`npm run bump-version -- 0.1.0` (or equivalent) updates the version field in
`package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` in a
single command, and verifies all three match before exiting.

## Context

A TurboTalk release requires three separate version fields to stay in lockstep:

- `package.json` → `"version"`
- `src-tauri/Cargo.toml` → `[package] version = "..."`
- `src-tauri/tauri.conf.json` → `"version"`

Today they're all `0.0.1`. Drift between them produces confusing artifacts
(DMG named with one version, app reporting another in About, Cargo metadata
with a third). Block 5 of `BETA-AUDIT-ROADMAP.md` lists "Bump version in
package.json, src-tauri/Cargo.toml, and src-tauri/tauri.conf.json" as a release
checklist item. A script removes the chance of forgetting one.

The repo already follows a `scripts/*.mjs` pattern for build helpers (see
`scripts/preflight.mjs`, `scripts/rename-artifact.mjs`). Match that style.

The script should be strict: reject malformed semver, fail loudly if any of
the three files cannot be parsed, and re-read all three after writing to
confirm they agree.

## In scope
- `scripts/bump-version.mjs` (new file).
- `package.json` — add a `"bump-version"` entry to the `"scripts"` section.
- The three version fields in `package.json`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json` are **not** changed by this task — the script is
  the deliverable, not a version bump.

## Out of scope
- Git tagging, changelog generation, release notes — separate task in this arc.
- Bumping the actual version (the script is added; running it is a user action
  at release time).
- Touching `Cargo.lock` (Cargo updates this automatically on next build).
- Any other manifest files (e.g. there is no `librewin-common` version to bump
  here — it's vendored, not versioned by TurboTalk).

## Steps
1. Read `scripts/preflight.mjs` and `scripts/rename-artifact.mjs` to match the
   existing style: ESM, `node:fs`/`node:path` only, no external deps,
   `process.exit(1)` on error with a `[script-name]` prefix.
2. Create `scripts/bump-version.mjs`:
   - Accept a single positional arg: the new version string.
   - Validate it matches `^\d+\.\d+\.\d+(-[\w.]+)?$` (basic semver, allow
     prerelease tags like `0.1.0-beta.1`).
   - Read `package.json`, parse JSON, update `version`, write back with a
     trailing newline.
   - Read `src-tauri/tauri.conf.json`, parse JSON, update `version`, write
     back with a trailing newline.
   - Read `src-tauri/Cargo.toml` as text. Replace the first line matching
     `^version = "..."` inside the `[package]` section. Be careful not to
     touch `version = "..."` in `[dependencies]`. A simple approach: split on
     `[package]` / `[`-prefixed section headers, only edit within the
     `[package]` block.
   - After writing all three, re-read each, extract the version, and assert
     all three equal the new version. Print a final summary line:
     `[bump-version] all three manifests at <new-version>`.
3. Add to `package.json`:
   ```
   "bump-version": "node scripts/bump-version.mjs"
   ```
   so the invocation is `npm run bump-version -- 0.1.0`.
4. Test locally by running `npm run bump-version -- 0.0.1` (no-op bump to the
   current version) and verifying:
   - Exit code 0.
   - Files unchanged on disk (or only whitespace-equivalent).
   - The summary line prints.
5. Test the failure modes by hand:
   - `npm run bump-version -- not-a-version` → exits non-zero with a clear
     error.
   - `npm run bump-version -- 0.1.0` → updates all three. Then
     `git diff` should show exactly three version-field changes.
   - **Revert with `git checkout`** before considering the task done — the
     task's deliverable is the script, not a version bump.

## Success signal
- `scripts/bump-version.mjs` exists and is executable via `npm run bump-version`.
- Running `npm run bump-version -- 0.1.0` then `git diff --stat` shows
  exactly three changed files (`package.json`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json`).
- After `git checkout` to revert, the repo is clean again.
- Running `npm run bump-version -- garbage` exits non-zero and prints a
  clear error.

## Notes
- Don't pull in a TOML parser dep just for this. The Cargo.toml `version`
  line in the `[package]` section is stable and a regex with section-aware
  splitting is enough. If the regex would be brittle, a 30-line hand parser
  is fine.
- The script is run by humans at release time, not by CI. Optimize for
  clear error messages over clever automation.
