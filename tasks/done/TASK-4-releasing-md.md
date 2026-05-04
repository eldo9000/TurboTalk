# TASK-4: Write RELEASING.md — checklist, manual update policy, release notes template

## Goal
A single `RELEASING.md` file at the repo root that a maintainer can follow
linearly to cut a beta release: bump version, build signed/notarized DMG,
checksum, write release notes, publish, and document what users do for
manual updates. Includes a copy-paste release-notes template.

## Context

Block 5 of `BETA-AUDIT-ROADMAP.md` lists a "Suggested Release Checklist" and a
"Updates" subsection. For first beta the decision (per roadmap line 328) is
**manual updates only** — no Tauri updater. This decision needs to be written
down so future maintainers don't accidentally enable auto-update without doing
the signing-key custody work.

This task ties together the deliverables of TASK-1 through TASK-3:
- `npm run bump-version` (TASK-2)
- Signed/notarized release build per `BUILD.md` "Release build" section (TASK-1)
- DMG + `.sha256` in `dist-artifacts/` (TASK-3)

It does **not** re-document the build pipeline — `BUILD.md` owns that. This
file is the *release procedure*, not the build procedure.

The release notes template should match what's reasonable for a private beta:
supported platform, install steps, required permissions, known limitations,
how to delete local data, and where to send feedback. Permissions and data
paths already exist in `README.md` and `PRIVACY.md` — link out, don't restate.

GitHub Releases is the implied publishing target (the repo is on GitHub) but
RELEASING.md should not assume any CI/release automation exists; the first
few releases will be manual `gh release create` invocations or web-UI uploads.

## In scope
- `RELEASING.md` (new file at repo root).
- A short reference from `BUILD.md` to `RELEASING.md` so the cross-reference is
  bidirectional ("for the full release procedure see RELEASING.md").
- A short reference from `README.md` if there's an obvious place (e.g. a
  contributing/development section) — only if it's a clean fit, do not invent
  a section just for the link.

## Out of scope
- Setting up GitHub Actions release workflows.
- Tauri updater plugin setup (explicitly deferred per roadmap line 333).
- Writing the actual release notes for v0.1.0 (template only).
- Changing versioning policy (e.g. semver vs calver) — use whatever the
  current `0.0.1` implies (semver).
- Re-documenting signing/notarization (lives in `BUILD.md`, just link).

## Steps
1. Read `BETA-AUDIT-ROADMAP.md` lines 296–363 for the release checklist and
   updates discussion.
2. Read `BUILD.md`, `README.md`, `PRIVACY.md`, `SMOKE-TEST.md` to understand
   what already exists and what to link to.
3. Create `RELEASING.md` with these sections (in order):
   - **Scope** — one paragraph: this is the procedure for cutting a TurboTalk
     beta release. macOS arm64 only for now.
   - **Pre-flight** — checklist:
     - `git status` clean on `main`.
     - `cargo test` and `cargo clippy -D warnings` green.
     - Manual smoke test on dev build (see `SMOKE-TEST.md`).
   - **Step 1: Bump version** — `npm run bump-version -- <new-version>`.
     Verify the diff. Commit: `chore(release): bump to <version>`.
   - **Step 2: Build signed + notarized DMG** — link to `BUILD.md` "Release
     build" section. Confirm `dist-artifacts/TurboTalk-<v>-macos-arm64.dmg`
     and `.sha256` exist.
   - **Step 3: Verify the artifact** — run `spctl` check from BUILD.md, then
     run `SMOKE-TEST.md` against the **installed DMG** on a clean macOS
     account (see TASK-5 in this arc once landed; reference SMOKE-TEST.md
     "Installed-artifact" section).
   - **Step 4: Tag + publish** — `git tag v<version> && git push --tags`.
     Then `gh release create v<version> --title "TurboTalk <version> beta" --notes-file RELEASE_NOTES.md dist-artifacts/TurboTalk-<v>-macos-arm64.dmg dist-artifacts/TurboTalk-<v>-macos-arm64.dmg.sha256`.
   - **Step 5: Update SESSION-STATUS.md** — single line under "Where We Are":
     "Released v<version> beta on <date>".
   - **Manual updates policy** — a clearly-marked paragraph stating that
     beta uses manual updates: users download the new DMG and replace
     `Turbo Talk.app` in `/Applications`. Tauri updater is intentionally
     **not** enabled; before enabling it we need (1) a long-lived updater
     signing key with secure custody, (2) a stable artifact-hosting URL,
     (3) a key-rotation/loss plan. Cite roadmap line 333.
   - **Release notes template** — a fenced block users can copy:
     ```markdown
     ## TurboTalk <version> beta

     **Platform:** macOS 12+ on Apple Silicon (arm64).
     **Install:** Download `TurboTalk-<version>-macos-arm64.dmg`, drag
     `Turbo Talk.app` into `/Applications`, launch it, grant microphone
     and Accessibility permissions when prompted.

     **What's in this release**
     - <one bullet per user-visible change>

     **Known limitations**
     - macOS arm64 only. Intel Macs, Windows, and Linux are not supported
       in this beta.
     - <other known issues>

     **Privacy & data** — see `PRIVACY.md`. To delete all local TurboTalk
     data: see PRIVACY.md "How to delete all local data".

     **Verify the download**
     ```
     shasum -a 256 -c TurboTalk-<version>-macos-arm64.dmg.sha256
     ```

     **Feedback** — file an issue at <github repo URL>/issues.
     ```
   - **Hotfix path** — one paragraph: bump patch version, rebuild, repeat
     above. No special procedure for hotfixes in beta.
   - **Rollback** — one paragraph: GitHub Releases supports marking a release
     as draft / pre-release; if a release is broken, mark it pre-release and
     publish a corrected one with a bumped patch version. We do not delete
     published releases.
4. Add a one-line cross-reference at the top of `BUILD.md`:
   > For the full release procedure (versioning, tagging, publishing,
   > release notes), see `RELEASING.md`.
5. If `README.md` has a "Development" or "Contributing" section, add a
   one-line link there too. If it doesn't, skip — don't invent a section.

## Success signal
- `RELEASING.md` exists at repo root.
- A maintainer with no prior context can read `RELEASING.md` linearly and know
  exactly what to type at each step (no "see also XYZ" rabbit holes for the
  core path; cross-references are reference-only, not required reading to
  proceed).
- The release-notes template is copy-pasteable as-is, with placeholder
  brackets `<like-this>` for the version and bullet list.
- The manual-updates-only policy is explicit and cites why auto-update is not
  enabled.
- `BUILD.md` has a one-line link to `RELEASING.md` near the top.

## Notes
- The GitHub repo URL: get it from `git config --get remote.origin.url` (the
  repo is private per `CLAUDE.md` "personal-use scope, private repo, MIT
  license"). If the URL points to a private repo, that's fine — beta testers
  invited to the repo can file issues; if the repo is shared by direct DMG
  link only, switch the feedback channel line to email or another channel
  the user specifies.
- Keep RELEASING.md under ~150 lines. It's a checklist, not an essay.
