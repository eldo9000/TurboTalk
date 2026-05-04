# TASK-1: Sidecar preflight script wired into the build

## Goal
A new script at `scripts/preflight.mjs` (or `.sh` — whichever fits the
existing tooling cleanest) verifies that every required macOS bundling
asset exists before `tauri build` runs, fails with a clear message if
any is missing, and is wired into the build flow so a missing sidecar
or dylib aborts before bundling instead of producing a broken DMG.

## Context
TurboTalk's first beta is **macOS arm64 only**. Block 1 of the beta-audit
roadmap (already complete) confirmed Windows/Linux are blocked on missing
Whisper sidecars and remain at the "compile, fail clearly" stage. Block 2
of `BETA-AUDIT-ROADMAP.md` calls for a packaging matrix — this task
implements the small piece of that matrix that's purely about not
shipping a broken artifact: a preflight check that catches missing
bundle assets before they cause runtime failures on a clean install.

Repo facts you can rely on:
- `src-tauri/binaries/` currently contains exactly four files required
  for the macOS arm64 bundle:
  - `whisper-cli-aarch64-apple-darwin`
  - `libwhisper.1.dylib`
  - `libggml.0.dylib`
  - `libggml-base.0.dylib`
- `src-tauri/tauri.conf.json` references these:
  - `"externalBin": ["binaries/whisper-cli"]` — Tauri auto-suffixes the
    triple, so the file on disk must be `whisper-cli-<triple>` (today
    only `whisper-cli-aarch64-apple-darwin` exists).
  - `"resources": { "binaries/libwhisper.1.dylib": ..., "binaries/libggml.0.dylib": ..., "binaries/libggml-base.0.dylib": ... }`
- `package.json` currently has `scripts: { "tauri": "tauri", ... }` and a
  `beforeBuildCommand: "npm run build"` set in `tauri.conf.json`. The
  preflight needs to run before bundling, not before the Vite frontend
  build.
- No `scripts/` directory exists yet at the repo root.
- Project is **Tier 1** — small, personal-use scope. Keep the script
  minimal: a hard list of expected files for the current host triple,
  exit 1 with a clear message on first miss. No multi-platform
  generality, no auto-download, no model handling.

The proof gate is "the build refuses to start when an asset is missing."

## In scope
- New file: `scripts/preflight.mjs` (Node) or `scripts/preflight.sh`
  (bash) — pick one based on what's already idiomatic for this repo
  (`package.json` is type: "module", so `.mjs` is the natural fit and
  avoids adding a new tool).
- `package.json` — add a `"preflight"` npm script and a `"prepackage"`
  or equivalent that runs preflight before `tauri build`. Keep the diff
  minimal.
- Optionally `src-tauri/tauri.conf.json` — only if you decide the
  preflight hook belongs in `beforeBundleCommand` rather than an npm
  script wrapper. State your reasoning in the task summary.

## Out of scope
- Downloading or generating any sidecar/model. The preflight only
  *checks*.
- Cross-platform asset matrices for Windows/Linux. Block 2 followups can
  add those when the project actually has Win/Linux sidecars to ship.
  This script may include a `target_os` switch that no-ops on non-mac
  with an explanatory message, but it must not invent expected files
  for other platforms.
- Whisper model files (`ggml-base.en.bin` etc.) — they are runtime
  downloads, not bundle assets. The preflight does not check for them.
- Code signing, notarization, version bumping. Block 5 handles those.
- Anything in `src-tauri/src/`. This task is purely build-tooling.

## Steps
1. Inventory the actual files in `src-tauri/binaries/` and confirm the
   four assets listed in Context still match the current Tauri config.
   If they don't, stop and flag it — the source of truth has drifted
   and the task needs to be re-scoped.
2. Create `scripts/preflight.mjs`. The script should:
   - Detect platform (`process.platform === 'darwin'`).
   - On macOS, define the expected list of files relative to repo root
     (the four assets above).
   - For each, check existence and that it is non-empty (`fs.statSync`
     and `size > 0`).
   - On first miss, print a single clear line of the form
     `[preflight] missing required bundle asset: <path>` and `process.exit(1)`.
   - On success, print `[preflight] all required bundle assets present`
     and exit 0.
   - On non-macOS, print
     `[preflight] non-macOS host: skipping (Win/Linux beta sidecars not yet defined)`
     and exit 0. Do not silently no-op.
3. Add an npm script in `package.json`:
   - `"preflight": "node scripts/preflight.mjs"`
   - Wire it in front of bundling. Two reasonable options — pick the
     one that yields the smallest diff:
     a. Add `"package": "npm run preflight && tauri build"` and document
        it as the official build command, or
     b. Set `tauri.conf.json` `"beforeBundleCommand": "npm run preflight"`.
     Option (b) integrates with `tauri dev` too if the user later runs
     dev with a fresh checkout; option (a) is a clean separation. Either
     is acceptable — pick one and document the choice in the script's
     header comment in 1–2 lines.
4. Run `npm run preflight` from the repo root. Expect green:
   `[preflight] all required bundle assets present`.
5. Manually rename one of the four bundle assets temporarily (e.g.
   `mv src-tauri/binaries/libggml.0.dylib /tmp/__test_missing.dylib`),
   re-run `npm run preflight`, and confirm it fails with the expected
   error message and exit 1. Restore the file when done. Capture both
   the failing and the recovered output in your return notes.
6. Run `npm run typecheck` to confirm nothing in the existing
   TypeScript surface broke (it shouldn't — but quick sanity check).

## Success signal
- `scripts/preflight.mjs` exists and is executable from `node`.
- `npm run preflight` exits 0 with `[preflight] all required bundle assets present`.
- Temporarily removing any of the four expected files causes
  `npm run preflight` to exit 1 with a single-line error naming the
  missing path.
- The build flow refuses to start bundling without preflight: either
  `package.json` has a `package` script that runs preflight first, or
  `tauri.conf.json`'s `beforeBundleCommand` invokes it.
- `npm run typecheck` still passes.

## Notes
- Keep the file small. ~30 lines is plenty for what this script does.
- Do not add any npm dependencies. `node:fs` and `node:path` cover
  everything needed.
- If you choose option (b) for wiring (touching `tauri.conf.json`),
  remember the existing `beforeBuildCommand: "npm run build"` is
  separate and refers to the *frontend* build, not bundling. Don't
  conflate them.
- The script header comment may briefly explain *why* this exists (one
  line — "fail fast if a sidecar/dylib is missing before tauri bundles
  a broken DMG") and *why* this is mac-only today.
