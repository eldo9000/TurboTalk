# TASK-3: README release matrix + beta scope honesty

## Goal
`README.md` gains a "Release matrix" section that names exactly which
operating systems, architectures, and install methods are supported for
the first beta, what permissions the app requests, what limitations
testers should expect, and how to install / uninstall / delete local
data. The section is honest about being macOS arm64 only and does not
imply Windows or Linux are supported when they are not.

## Context
TurboTalk's first beta is **macOS arm64 only** — confirmed by Block 1 of
`BETA-AUDIT-ROADMAP.md`. Hotkey and paste have honest unsupported-platform
stubs on non-mac targets, and Win/Linux Whisper sidecars do not exist in
the repo. Block 2 of the roadmap calls for a release matrix in README so
beta users know what they are getting before they download anything.

Repo facts you can rely on:
- `README.md` exists at the repo root. Read it first to see what's
  already there and match its tone — TurboTalk is a personal-use tool,
  not a polished public product.
- The macOS app currently requests: Microphone (always), Accessibility
  (for hotkey input monitoring via `CGEventTap`), and possibly
  Automation/System Events (for the `osascript`-driven paste). Verify
  the actual prompts by reading `src-tauri/src/hotkey.rs` and
  `src-tauri/src/paste.rs` if needed.
- Local data lives under `~/.config/librewin/turbotalk/` per
  `CLAUDE.md`. History is opt-in/configurable per the privacy section
  of `BETA-AUDIT-ROADMAP.md`. Models are downloaded under that dir;
  audio temp files live wherever `audio.rs` writes them — check before
  documenting.
- Project is **Tier 1**. Add one focused section, not a marketing page.
- The artifact naming convention is
  `TurboTalk-<version>-<os>-<arch>.dmg|exe|AppImage` (introduced by
  the build doc task). Reference it but do not redefine it.

The proof gate is: a beta user can answer, *from the README alone*,
"will this run on my machine?" and "what permissions does it want and
why?" without grepping the source.

## In scope
- `README.md` — add one new top-level section (or replace an existing
  stub release section if one already exists). Do not rewrite unrelated
  parts of the README.

## Out of scope
- `BUILD.md`, `BETA-AUDIT-ROADMAP.md`, `PRIVACY.md`. Linking to them is
  fine; rewriting is not.
- Code changes, dependency changes, configuration changes.
- Adding badges, screenshots, or marketing copy.
- A full privacy section. Block 4 of the roadmap will add `PRIVACY.md`
  separately. This task only mentions the data-deletion path in one
  bullet and links to the future privacy doc as TBD if it doesn't
  exist yet.

## Steps
1. Read the current `README.md` end to end. Identify whether a
   "Release", "Install", "Status", or similar section already exists.
   If yes, edit it; if no, add a new section under the current top
   matter. Keep section ordering coherent with what's already there.
2. Verify the actual permission prompts the macOS build triggers by
   reading `src-tauri/src/hotkey.rs` and `src-tauri/src/paste.rs`
   briefly. Document them factually, not aspirationally.
3. Verify where local data lives by reading `src-tauri/src/settings.rs`
   and (if relevant) `src-tauri/src/audio.rs` for temp-file paths.
   Document the actual paths.
4. Add a `## Release matrix` (or `## Beta status` — pick whichever
   reads better in context) section. Required content:
   - **Supported platforms table** — three columns: Platform |
     Architecture | First-beta status. Three rows: macOS / arm64 /
     supported (beta-1); macOS / x86_64 / not supported (no sidecar);
     Windows / x86_64 / not supported (deferred); Linux / x86_64 /
     not supported (deferred). Be explicit.
   - **Install** — one short paragraph. Download
     `TurboTalk-<version>-macos-arm64.dmg`, drag to `/Applications`,
     right-click + Open the first time (ad-hoc signed in beta-1).
   - **Permissions the app will request** — bulleted list with one
     sentence each explaining *why* (mic = capture audio for
     transcription; accessibility = global push-to-talk hotkey;
     automation/system-events = paste into the focused app — only
     mention this one if `paste.rs` actually triggers it on the
     current build).
   - **Local data** — one bullet naming the data dir (verified path),
     one bullet on Whisper model download location, one bullet on
     audio temp files (location + when deleted). One bullet on how
     to delete everything: quit app, `rm -rf <dir>`.
   - **Known limitations** — short list. At minimum: Apple Silicon
     only, ad-hoc signed (you'll see a Gatekeeper warning the first
     launch), no auto-updates, history saved by default unless
     toggled off (verify whether that's actually the current default
     before claiming it).
   - **Feedback** — one line stating where to file beta feedback.
     If there is no public issue tracker, say so honestly: "personal-
     use beta — feedback by direct message until a public tracker
     opens."
5. Validate that no statement in the new section overstates what works.
   In particular: do *not* say "cross-platform", do *not* say "auto-
   updates", do *not* say "code-signed", do *not* say "notarized" —
   none of those are true for beta-1.
6. Run no scripts. This is a docs-only task. The success signal is
   reading the diff and confirming each bullet maps to a verifiable
   claim about the current code.

## Success signal
- `README.md` contains a `Release matrix` (or equivalent) section with
  the supported-platforms table, permissions, local-data paths,
  known limitations, and feedback channel.
- Every claim in that section is verifiable against the current code:
  - Each listed permission corresponds to actual code that requests it.
  - Each data path matches what `settings.rs` / `audio.rs` / etc.
    actually use.
  - The "first-beta status" column lists `supported` for exactly one
    row (macOS arm64) and explicit non-support reasons for the rest.
- The section does not claim signing, notarization, auto-updates,
  Windows support, Linux support, or Intel-Mac support.
- No other section of the README is rewritten.

## Notes
- If you find that a permission listed in this task's Context is no
  longer triggered by the current code (e.g. `paste.rs` no longer
  needs Automation), document only what the code actually does. The
  README is the source of truth for users; the roadmap text is older
  context.
- Keep the section dense. Bullets, not paragraphs. The whole release
  matrix should fit on one screen.
- If `README.md` is currently very minimal (one or two paragraphs),
  this section may end up being the largest part of the file. That's
  fine — it's the most user-facing content the repo has right now.
