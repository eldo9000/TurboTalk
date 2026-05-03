# TASK-1: Platform compile audit + compatibility table

## Goal
A new file `PLATFORM-AUDIT.md` exists at the repo root, containing: (a) the
classified output of `cargo check` against Windows and Linux targets, (b) the
capability table from BETA-AUDIT-ROADMAP.md filled in with current ground truth,
and (c) an inventory of the Tauri config and `src-tauri/binaries/` that names
exactly which assets are macOS-only. No source code is modified by this task.

## Context
TurboTalk is currently a working macOS dictation app. The roadmap
(`BETA-AUDIT-ROADMAP.md`, Block 1, lines 32–101) calls for an honest audit of
what is mac-only before any cross-platform refactor begins. This is a
pure-inventory task — fix nothing, classify everything.

Repo facts you can rely on:
- `src-tauri/src/hotkey.rs` imports `core_foundation::runloop` and
  `core_graphics::event::{CGEventTap, CGEventFlags, ...}` unconditionally.
  These crates do not build for non-Apple targets.
- `src-tauri/src/paste.rs` has `frontmost_app()` already cfg-gated for macOS,
  but the `paste()` function shells out to `osascript` unconditionally.
- `src-tauri/tauri.conf.json` sets `"macOSPrivateApi": true` and
  `"externalBin": ["binaries/whisper-cli"]`, with macOS-only resources
  `libwhisper.1.dylib`, `libggml.0.dylib`, `libggml-base.0.dylib`.
- `src-tauri/binaries/` currently contains an Apple Silicon Whisper binary
  (`whisper-cli-aarch64-apple-darwin`) and `.dylib` files only.
- Tauri sidecar docs require target-triple-suffixed binaries for each
  supported architecture (e.g. `whisper-cli-x86_64-pc-windows-msvc.exe`).

The project is **Tier 1** — small app, personal-use scope (see CLAUDE.md and
the operating model). Keep ceremony light. The deliverable is one markdown
file with honest findings, not a multi-document audit binder.

Out of fixing scope for this task, but list in the audit so the next tasks
can consume the inventory: any module under `src-tauri/src/` that uses
platform-specific crates, system commands, or paths.

## In scope
- Read-only inspection of `src-tauri/src/` (every `.rs` file).
- Read-only inspection of `src-tauri/Cargo.toml`.
- Read-only inspection of `src-tauri/tauri.conf.json`.
- Read-only inspection of `src-tauri/binaries/`.
- Running `cargo check` against Windows and Linux targets (via `rustup target
  add` if not already installed). It is acceptable for these to fail — the
  task is to classify the failures, not fix them.
- Writing `PLATFORM-AUDIT.md` at the repo root.

## Out of scope
- Any change to Rust source files.
- Any change to `Cargo.toml`, `tauri.conf.json`, or `package.json`.
- Adding any `#[cfg(target_os = "...")]` boundaries (TASK-2 and TASK-3 do that).
- Building or downloading Whisper sidecars for Windows/Linux.
- Updating `TRUTH.md` or `SESSION-STATUS.md` (do that after the human reviews
  the audit doc).

## Steps
1. From the repo root, run `rustup target add x86_64-pc-windows-gnu` and
   `rustup target add x86_64-unknown-linux-gnu`. Capture whether each target
   added cleanly. If a target refuses to install on this host, record that as
   a finding instead of treating it as a blocker.
2. From `src-tauri/`, run `cargo check --target x86_64-pc-windows-gnu` and
   `cargo check --target x86_64-unknown-linux-gnu`. Capture the full stderr.
   Truncate to the unique error families — do not paste 500-line transcripts
   into the audit doc.
3. Classify each unique error into one of: (a) unsupported platform boundary
   missing (e.g. macOS-only crate imported without cfg guard), (b) missing
   sidecar asset, (c) missing system dependency, (d) real code bug. Be honest
   if a failure does not fit cleanly — name it as "uncategorized" and note it.
4. Grep `src-tauri/src/` for: `core_foundation`, `core_graphics`, `objc`,
   `cocoa`, `osascript`, `target_os = "macos"`, `cfg(target_os`. Build a
   short table of files vs which platform-specific touch points they have.
5. Inspect `src-tauri/tauri.conf.json` and `src-tauri/binaries/` and write a
   short subsection naming exactly which artifacts assume macOS, and which
   target-triple-suffixed binaries would be needed for Windows and Linux per
   Tauri sidecar conventions.
6. Fill in the capability table from BETA-AUDIT-ROADMAP.md (the table around
   line 81) with `proven` / `not proven` / `unknown` per cell, sourced from
   the audit you just ran. Do not guess — if you didn't test it, write
   `unknown`.
7. Save the document as `PLATFORM-AUDIT.md` at the repo root. Structure:
   `## Compile audit`, `## Platform-specific code touch points`,
   `## Tauri config + sidecar assets`, `## Capability table`,
   `## Recommendations for follow-on tasks` (one short paragraph pointing at
   TASK-2 and TASK-3 as the natural next steps).

## Success signal
- `PLATFORM-AUDIT.md` exists at the repo root.
- It contains a populated capability table covering at least: build app,
  global hotkey, mic capture, Whisper sidecar, paste into focused app,
  overlay — for macOS, Windows, and Linux.
- It contains the `cargo check` failure classification for each non-mac
  target, with each unique error labeled (a/b/c/d/uncategorized).
- It names every Rust file in `src-tauri/src/` that has macOS-specific
  imports or shell-outs.
- The macOS happy path on the current machine still works (run
  `npm run tauri dev`, hold the hotkey, dictate one short phrase, see it
  pasted) — i.e. the task did not regress anything.

## Notes
- Cross-compiling a full Tauri build to Windows from macOS will likely fail
  with WebView2 / link errors well before TurboTalk's own modules. That is
  fine — the goal is to capture the *first* layer of compile failures so we
  know what the audit reveals about *our* code. If `cargo check` can't even
  start because of a toolchain gap, record that as the finding.
- If `rustup target add` fails for a target on this host, document it and
  move on. Don't burn the session on toolchain plumbing — that is its own
  task if it ever needs to be one.
