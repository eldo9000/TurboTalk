# TASK-5: Extend SMOKE-TEST.md with an "Installed-artifact" section

## Goal
`SMOKE-TEST.md` gains a clearly-labeled section titled "Installed-artifact
smoke test" that walks through verifying a packaged DMG (not a dev build)
on a clean macOS user account. Covers Gatekeeper acceptance, permissions
flow, one end-to-end dictation, and uninstall + data cleanup verification.

## Context

`SMOKE-TEST.md` already exists from Block 3 (TASK-4). It covers a 7-step
manual test against the **dev build** (`npm run tauri dev`). That smoke test
catches code regressions but cannot catch packaging-layer regressions:

- DMG fails to mount or fails Gatekeeper.
- `.app` bundle is signed but a sidecar binary or dylib isn't (notarization
  passes but Gatekeeper rejects on first run).
- Required `.dylib` resources are missing from the bundle.
- Permission prompts behave differently when the app is in `/Applications`
  vs running from the dev cwd.
- LaunchAgent / autostart wires up against the wrong path.
- Local data paths are correct when the app is launched from `/Applications`.

Block 5 of `BETA-AUDIT-ROADMAP.md` "Proof Gate" (lines 356–363) requires:
> Artifact installs on a clean user account.
> App launches without developer tooling.
> Permissions flow is understandable.
> Dictation works once end-to-end.
> Upgrade or reinstall path is known.
> Uninstall path and local data cleanup path are documented.

This is the "smoke test from the user's perspective, not the developer's"
that releases must pass before being published.

The cleanest delivery is a new section appended to the existing
`SMOKE-TEST.md`, not a separate file — both smoke flows belong together so
the maintainer running a release does not have to remember which file is
which.

## In scope
- `SMOKE-TEST.md` — append a new section.
- A short reference from `RELEASING.md` Step 3 to this new section (only if
  RELEASING.md already exists; if TASK-4 hasn't landed yet, skip the
  cross-reference and leave RELEASING.md to add it).

## Out of scope
- Automating any part of the smoke test — it is intentionally manual; the
  whole point is that a human exercises the install path.
- Testing on Intel Macs, Windows, Linux — beta is macOS arm64.
- Testing the upgrade path from a previous version — there is no previous
  version yet. Add an upgrade test in a future arc once v0.2.0 ships.
- Setting up a "clean user account" provisioning script — the steps assume
  the maintainer either creates a temporary macOS user account manually or
  uses a fresh VM.

## Steps
1. Read `SMOKE-TEST.md` to understand the existing format and tone. Match it.
2. Append a new top-level section: `## Installed-artifact smoke test`.
3. Add a one-paragraph preamble explaining when to run this (after every
   release build, before publishing) and what makes it different from the
   dev-build smoke test (covers Gatekeeper, packaging, permission prompts
   from `/Applications`, uninstall).
4. Add a "Prerequisites" subsection:
   - A signed + notarized DMG in `dist-artifacts/` (per `BUILD.md` and
     `RELEASING.md`).
   - A clean macOS user account with no prior TurboTalk install. Either:
     (a) a fresh macOS VM, or
     (b) a new local user account on the maintainer's Mac (System Settings
     → Users & Groups → Add Account).
5. Add the numbered steps. Each step is "Action: ... Expected: ..." matching
   the dev smoke test format. Cover:
   1. **Verify checksum** before installing:
      `shasum -a 256 -c TurboTalk-<v>-macos-arm64.dmg.sha256`. Expected:
      `<dmg>: OK`.
   2. **Verify Gatekeeper acceptance**:
      `spctl -a -t open --context context:primary-signature -v <dmg>`.
      Expected: `accepted` and the developer name.
   3. **Mount and install**: double-click DMG, drag `Turbo Talk.app` into
      `/Applications`, eject the DMG. Expected: app appears in Applications.
   4. **First launch**: open `/Applications/Turbo Talk.app`. Expected: no
      Gatekeeper warning ("cannot be opened because the developer cannot be
      verified" = FAIL). App window appears.
   5. **Microphone permission prompt**: hold the push-to-talk hotkey.
      Expected: macOS prompts for Microphone permission. Grant it.
   6. **Accessibility permission prompt**: TurboTalk should prompt (or fail
      with a clear in-app message) for Accessibility/Input Monitoring.
      Grant it via System Settings → Privacy & Security → Accessibility.
      Re-launch if necessary.
   7. **End-to-end dictation**: open TextEdit (or any text field), hold the
      push-to-talk hotkey, say "hello world", release. Expected: "hello world"
      appears in TextEdit within ~2 seconds.
   8. **Verify local data path**: confirm
      `~/.config/librewin/turbotalk/` exists and contains the expected
      config / history files (per `PRIVACY.md`).
   9. **Quit and relaunch**: quit TurboTalk, relaunch from
      `/Applications/Turbo Talk.app`. Expected: app launches clean, settings
      persist, no permission re-prompts.
   10. **Uninstall**: drag `Turbo Talk.app` from `/Applications` to Trash.
       Expected: app removed.
   11. **Verify data cleanup path**: follow `PRIVACY.md` "How to delete all
       local data". Expected: `~/.config/librewin/turbotalk/` (and any other
       paths PRIVACY.md lists) are gone after the documented commands.
6. Add a final "Pass/fail recording" subsection: a one-paragraph note that
   the maintainer should record the outcome (pass + macOS version, or
   fail + which step + observed behavior) in `SESSION-STATUS.md` under the
   release entry.
7. If `RELEASING.md` exists at the time this task runs, add a one-line link
   to this new section in RELEASING.md Step 3. If it doesn't exist yet, do
   not create it — TASK-4 owns RELEASING.md.

## Success signal
- `SMOKE-TEST.md` has a new "Installed-artifact smoke test" section.
- A maintainer who has never released TurboTalk before can follow the
  numbered steps and produce a pass/fail outcome with no ambiguity.
- The section explicitly checks Gatekeeper, permission prompts, end-to-end
  dictation, and uninstall + data cleanup — every item in the
  `BETA-AUDIT-ROADMAP.md` Block 5 Proof Gate.
- If `RELEASING.md` exists, it links to this section from Step 3.

## Notes
- The "clean user account" requirement is the single biggest reason this
  test gets skipped. Make the prerequisites section emphasize that running
  it on the maintainer's daily-driver account does not catch the failures
  it's designed to catch (cached permission grants, prior installs in
  `~/Library/Application Support`, etc.).
- The step numbering should restart at 1 within the new section; do not
  continue from the dev smoke test.
