# TASK-30: Cross-platform documentation — SMOKE-TEST, README, RELEASING

## Goal
`SMOKE-TEST.md`, `README.md`, and `RELEASING.md` honestly describe the Win/Linux beta: how to install, what's supported, what isn't (Wayland, code signing), how to build artifacts on each host, and how to run the smoke test on each OS.

## Context
Three docs in the repo today are macOS-only or mac-shaped:

**`SMOKE-TEST.md`** (TASK-4 of Block 3, commit `c76fa9d`) — 7-step manual beta test script. Steps assume macOS Accessibility, Cmd+V, the DMG install path, and a TextEdit verification target.

**`README.md`** — the recent commits (`38a0eb9`, `5112d0e`, `0eff544`) revised the structure but the supported-platform copy still reads as a mac-only beta. There is **no current "release matrix"** section — it was previously removed in commit `0eff544` (`Remove release matrix and installation details`). Restore it now in a multi-platform shape.

**`RELEASING.md`** (Block 5 TASK-4, commit `02be37e`) — release procedure with mac signing/notarization steps. For this beta we are explicitly **not signing** anything. The doc should make that clear, retain mac signing steps for the future, and add Win/Linux unsigned-build procedures.

User constraint from the conversation: **no code signing of any kind for this release**. Document the consequences:
- Win: SmartScreen will flag the unsigned `.exe`. Provide users instructions to "More info → Run anyway".
- Linux: AppImage runs without signing; just `chmod +x` and execute.
- Mac: ad-hoc signing already in place (`signingIdentity: "-"`); first launch needs the existing right-click → Open trick. Do not enable Developer ID for this beta even though TASK-1 of Block 5 wired it up.

Tier 1 product per `CLAUDE.md` — keep docs terse and copy-pasteable. No long prose.

## In scope
- `SMOKE-TEST.md` — add Win + Linux runs alongside the existing mac run
- `README.md` — restore release-matrix table; add per-platform install steps; document SmartScreen + Wayland caveats
- `RELEASING.md` — add Win + Linux build procedures; add explicit "this beta is unsigned" section; preserve existing mac signing reference for the future

## Out of scope
- `BUILD.md` (only touch if a build instruction lives there that contradicts what's added; otherwise leave)
- `PRIVACY.md` — already accurate, no per-platform delta needed for this beta
- `ARCHITECTURE.md` — no doc-only changes here
- `CLAUDE.md` — Tier-1 instructions stand
- Any code change

## Steps
1. Read `SMOKE-TEST.md` end to end. For each step, identify the mac-specific verb (Cmd+V, DMG install, Accessibility settings) and write a Win and Linux equivalent. Result format: keep the existing 7 + 11 step blocks for mac, add parallel sections "Windows beta smoke test" and "Linux beta smoke test (X11)" with per-step substitutions:
   - Install: `.exe` (NSIS) double-click → "More info → Run anyway" past SmartScreen / `.AppImage` `chmod +x` → run.
   - Permissions: Win has none at first launch; Linux/X11 has none.
   - Hotkey: Right Alt (default) — same on all platforms.
   - Paste verification target: Notepad on Win, gedit (or the user's text editor) on Linux.
   - Quit/relaunch path differs only in start-menu / Activities lookup.
2. Edit `README.md`:
   - Add a `## Supported platforms` section with a table: OS, arch, install method, signed?, known limits.
     - macOS arm64 — DMG, ad-hoc signed, install caveat from SESSION-STATUS.
     - Windows x64 — NSIS .exe, **unsigned (SmartScreen will warn)**, no Wayland concept.
     - Linux x64 — AppImage, unsigned, **X11 only — Wayland not supported**.
   - Per-platform install instructions, three short blocks. Each block ends with a one-line "first dictation" sanity sentence.
   - One-paragraph Wayland note explaining why Wayland is unsupported (compositor blocks global keystroke injection by design). Don't editorialize beyond that.
3. Edit `RELEASING.md`:
   - Add a top-of-file note: "**This beta release is unsigned on all platforms.** Mac DMG uses ad-hoc signing only; Win .exe and Linux AppImage have no signature."
   - Move the existing Developer ID / notarization section under a "Future signed releases (deferred)" heading. Keep the env-var commands for reference but make clear they are not executed for this beta.
   - Add `## Build procedure — Windows` with the actual host commands: `npm install`, `npm run package`, expected `dist-artifacts/TurboTalk-<v>-windows-x64-setup.exe` + `.sha256`.
   - Add `## Build procedure — Linux (X11)` mirroring the Win section: `npm run package` → `dist-artifacts/TurboTalk-<v>-linux-x64.AppImage` + `.sha256`. Note system deps required (`libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, etc., per Tauri 2 prereqs).
   - Update the release-notes template to include three artifact lines (mac/win/linux) and three sha256 lines.
4. Re-read all three docs once more end-to-end. Confirm no step instructs the user to sign anything.
5. Confirm `README.md` does not promise features that don't exist yet (don't claim Wayland support; don't claim Linux .deb).

## Success signal
- `grep -ni "wayland" README.md` returns at least one line under a clear "not supported" / "X11 only" caveat.
- `grep -ni "smartscreen" README.md` returns the SmartScreen workaround line.
- `RELEASING.md` contains the literal phrase "unsigned" near the top.
- `SMOKE-TEST.md` has three labeled smoke-test sections — macOS, Windows, Linux — each with the same 7-step shape.
- No doc instructs the user to acquire a code-signing certificate for this beta.

## Notes
- Keep tables narrow — most users will read the README on GitHub on a phone.
- AppImage requires the FUSE library on most distros. Note that as a known install hurdle.
- WebView2 on Win: most Win11 machines already have it; Win10 users may need the Edge Evergreen installer link. Add the URL.
- Don't write a separate Linux install doc per distro. One AppImage, one set of instructions.

→ verify: a fresh reader on a Win or Linux box can follow README + RELEASING + SMOKE-TEST and reach a working dictation event without asking questions.
