# TASK-2: Build doc + macOS artifact naming convention

## Goal
A new file `BUILD.md` at the repo root documents the exact commands a
maintainer runs to produce the first-beta macOS arm64 DMG, names the
target artifact path with a documented naming convention
(`TurboTalk-<version>-macos-arm64.dmg`), and includes a short
"cross-platform packaging — deferred" section that records exactly what
would need to land before Windows or Linux bundles can be produced. The
build command produces a renamed copy of the DMG matching that
convention so the final artifact has the right filename without manual
post-processing.

## Context
TurboTalk's first beta is **macOS arm64 only** — Block 1 of
`BETA-AUDIT-ROADMAP.md` confirmed Windows/Linux are blocked on missing
Whisper sidecars and platform-specific paste/hotkey work. Block 2 of the
roadmap calls for a documented build flow and clear artifact naming so a
beta DMG can be produced repeatably and labeled honestly.

Repo facts you can rely on:
- `src-tauri/tauri.conf.json` has `"version": "0.0.1"` and
  `"productName": "Turbo Talk"`. Tauri's default DMG output path on
  macOS is something like
  `src-tauri/target/release/bundle/dmg/Turbo Talk_0.0.1_aarch64.dmg`
  (verify before documenting — Tauri's filename convention has shifted
  across versions).
- `package.json` has `"version": "0.0.1"` and `"name": "turbotalk"`.
- The macOS bundle is currently signed ad-hoc (`signingIdentity: "-"`).
  Notarization is not configured. Both are explicitly Block 5 work, not
  this task.
- Project is **Tier 1**. The build doc is one short markdown file with
  copy-pasteable commands, not a multi-page ops binder.

The proof gate is: a maintainer who has never built TurboTalk before can
follow `BUILD.md` from a clean checkout to a `TurboTalk-0.0.1-macos-arm64.dmg`
in their downloads-friendly path. They do *not* need to know Tauri.

## In scope
- New file: `BUILD.md` at the repo root.
- `package.json` — add or extend an npm script that runs `tauri build`
  and copies the resulting DMG to a predictably-named output path
  matching the convention. Keep the diff minimal.
- Reading `src-tauri/tauri.conf.json` to confirm the version and
  productName the script needs to substitute. Do **not** modify it.

## Out of scope
- Code signing, notarization, signing identity changes — Block 5.
- Version bumping. The script reads version from existing config; it
  does not change `0.0.1` here.
- Producing the artifact during this task. This task documents and
  scripts the build; running it is the human's smoke-test step.
- Cross-platform builds. Document what they'd need but don't add Win/Linux
  npm scripts. A future task will package those once sidecars exist.
- Updating `README.md`. TASK-3 handles the user-facing release matrix.
- Anything inside `src-tauri/src/`.

## Steps
1. Read `src-tauri/tauri.conf.json` and `package.json` to confirm the
   current `version` (`0.0.1`) and any productName/identifier strings
   the build script will need.
2. Determine the actual DMG output path Tauri will produce. The
   reliable way is: from the repo root, list
   `src-tauri/target/release/bundle/dmg/` *if it already exists from a
   prior build*. If it does not exist, document the expected pattern
   based on Tauri 2 docs (typically `<productName>_<version>_aarch64.dmg`)
   and validate the path naming as part of the build script (see step 3).
3. Add a `package` npm script in `package.json` that:
   - Runs `tauri build` (which already chains `npm run build` and
     bundling via `beforeBuildCommand`).
   - On success, copies the produced DMG to a `dist-artifacts/` (or
     similar — pick one and document) directory at the repo root with
     the canonical name `TurboTalk-0.0.1-macos-arm64.dmg`. Use a small
     inline node-one-liner or a separate `scripts/rename-artifact.mjs`
     if cleaner. Either way, the version and arch are derived from
     `package.json` / `process.arch` rather than hardcoded.
   - If the expected source DMG path is missing, fail loudly with a
     message naming the path it looked for. Do not silently produce
     nothing.
4. Add `dist-artifacts/` to `.gitignore` (just the new directory; do not
   touch unrelated lines).
5. Write `BUILD.md` at the repo root with the following sections, in
   this order:
   - **Build a macOS arm64 beta DMG** — bullet-list of exact prereqs
     (Rust toolchain, Node 22+, etc. — verify versions from existing
     config), then a single `npm run package` line, then the expected
     output path `dist-artifacts/TurboTalk-0.0.1-macos-arm64.dmg`.
   - **Artifact naming convention** — table or paragraph stating
     `TurboTalk-<version>-<os>-<arch>.dmg|exe|AppImage`. Note that
     `<version>` comes from `package.json` and must match
     `src-tauri/tauri.conf.json`.
   - **Smoke test the artifact** — short checklist (install DMG to
     `/Applications`, launch, grant Microphone + Accessibility, hold
     hotkey, dictate one phrase, see it pasted, quit). Reference
     Block 2 proof gate from the roadmap.
   - **Cross-platform packaging — deferred** — bullet list of exactly
     what would need to land for Windows / Linux beta bundles:
     target-triple-suffixed Whisper sidecar binaries, real
     hotkey/paste implementations (currently stubs returning
     "unsupported platform"), platform-specific build prereqs
     (WebView2 on Windows, WebKitGTK on Linux), and an extension to
     the preflight script. Do not include build commands for these
     platforms — they would be wrong today.
   - **Signing & notarization — deferred** — one short paragraph
     stating that the current build is ad-hoc signed
     (`signingIdentity: "-"`) and that signing/notarization is Block 5
     of the roadmap. Beta-1 distribution is "you have to right-click
     and Open the first time."
6. Verify the build doc is internally consistent by *not* running the
   full Tauri build (it takes minutes) but by:
   - Confirming `npm run package` is wired in `package.json`.
   - Confirming the rename helper would produce the exact filename
     from `BUILD.md`'s "expected output path" line — read both and
     compare strings. If they don't match, fix the doc, not the
     script.
   - Running `npm run typecheck` to ensure no TS surface broke.

## Success signal
- `BUILD.md` exists at the repo root with all five sections listed
  above.
- `package.json` has a `package` script that performs the build +
  rename flow.
- `dist-artifacts/` is ignored by git.
- The expected artifact filename written in `BUILD.md` matches what
  the rename helper would produce — same string, same path, same case.
- `npm run typecheck` still passes.

## Notes
- The actual `npm run package` invocation that produces a DMG is
  outside this task — it takes minutes and the user runs it as the
  Block 2 proof. Your job is to make sure that invocation will work
  on first try.
- If the chosen rename approach is a one-liner inside `package.json`,
  keep it portable (no bashisms — `package.json` runs through the
  shell on each platform). A small `scripts/rename-artifact.mjs` is
  often cleaner; prefer that if the one-liner gets ugly.
- Do not duplicate content between `BUILD.md` and what `BETA-AUDIT-ROADMAP.md`
  already says. `BUILD.md` is for build mechanics; the roadmap is for
  the proof gate. A single sentence linking the smoke test back to the
  roadmap is enough.
