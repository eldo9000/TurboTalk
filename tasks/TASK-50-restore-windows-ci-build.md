# TASK-50: Restore Windows x64 CI build and download installer artifact

## Goal
A TurboTalk Windows x64 NSIS installer (.exe) exists in `dist-artifacts/` and is verified intact — ready to be copied into the UTM VM.

## Context
The current `release.yml` only builds `darwin-aarch64`. The Windows matrix leg was dropped after the signed-updater workflow was introduced (commit `febd9af`). The last known-green Windows CI build was commit `0e9ad71` using a matrix workflow (reference: `.github/workflows/release.yml` at that commit). The Windows runtime impl exists in the codebase — `paste.rs` uses arboard + enigo, `hotkey.rs` uses rdev — but has never been tested on a real Windows box. This task produces the installer needed for UTM testing.

The old workflow used `npm run fetch-sidecars` to download the whisper.cpp v1.8.4 x64 binary (sha256-pinned in `scripts/fetch-sidecars.mjs`) and then `npm run package` to build the NSIS installer.

## In scope
- `.github/workflows/release.yml` — add a temporary `windows-x64` matrix leg for `workflow_dispatch` only
- `scripts/fetch-sidecars.mjs` — verify it still references the correct sha256-pinned whisper.cpp v1.8.4 zip
- `dist-artifacts/` — destination for downloaded artifact

## Out of scope
- Any changes to Rust source, frontend, or Tauri config
- Restoring the Windows leg to tag-push builds (keep it `workflow_dispatch`-only to avoid interfering with the signed macOS release pipeline)
- Codesigning — unsigned beta is fine for UTM testing

## Steps
1. Read `.github/workflows/release.yml` in full to understand the current structure.
2. Read `git show 0e9ad71:.github/workflows/release.yml` to get the old Windows matrix leg definition.
3. Add a `windows-x64` matrix entry to the existing `workflow_dispatch` trigger path only. Pattern from the old workflow:
   - `os: windows-latest`
   - `target_label: windows-x64`
   - Steps: `npm install` → `npm run fetch-sidecars` → `npm run package` → upload artifact named `turbotalk-windows-x64`
   - The Windows matrix leg must NOT run on `push: tags` — add an `if:` condition or move it to a separate job gated on `github.event_name == 'workflow_dispatch'`.
4. Read `scripts/fetch-sidecars.mjs` — confirm the sha256 pin and zip URL are still valid (whisper.cpp v1.8.4 upstream). If the URL is dead, note it prominently in the task but do not change the pin without confirming the new hash.
5. Commit the workflow change: `chore(ci): add windows-x64 workflow_dispatch leg for UTM pre-testing`
6. Push to `main` (or a branch — confirm with user before pushing to main).
7. Trigger the workflow: `gh workflow run release.yml` (this triggers `workflow_dispatch`).
8. Wait for the run to complete: `gh run list --workflow=release.yml --limit 3` then `gh run watch <run-id>`.
9. Download the artifact: `gh run download <run-id> --name turbotalk-windows-x64 --dir dist-artifacts/windows-x64-tmp/`
10. Verify the .exe is present: `ls dist-artifacts/windows-x64-tmp/`
11. Note the exact filename and path in a comment at the bottom of this task file before archiving.

## Success signal
`ls dist-artifacts/windows-x64-tmp/` shows a `.exe` file (NSIS installer). `gh run view <run-id>` shows conclusion `success` for the `windows-x64` job.

## Notes
- If `npm run fetch-sidecars` fails because the whisper.cpp v1.8.4 zip URL is stale, the Windows CI job will fail. Check `scripts/fetch-sidecars.mjs` for the URL before triggering. The fix would be to update to the latest whisper.cpp release zip (re-pin sha256).
- The current workflow has a `LIBRE_APPS_TOKEN` secret dependency. The Windows job must also include the `librewin-common` checkout step or it will fail at `npm install` if the package.json references the private `@libre/ui` package. Check `package.json` for `@libre/*` deps before triggering.
- Do NOT push the workflow change to a tag (that would trigger the signed macOS release pipeline).
