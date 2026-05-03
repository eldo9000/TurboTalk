# TASK-3: Split paste.rs into platform-gated facade + macOS impl

## Goal
`src-tauri/src/paste.rs` no longer shells out to `osascript` on non-macOS
targets. The macOS path keeps its current clipboard + Cmd+V behavior. The
non-macOS branch returns a clear "unsupported platform" error from both
`paste()` and `frontmost_app()`. The macOS happy path is unchanged: after
hotkey-driven dictation, the transcript still pastes into the focused app.

## Context
The paste module is partially cfg-aware already: `frontmost_app()` is gated
behind `#[cfg(target_os = "macos")]`, but `paste()` itself shells out to
`osascript` unconditionally and uses `arboard::Clipboard` for clipboard
write/restore. On Windows, `osascript` does not exist — running the
current code would either fail at the process spawn or silently no-op,
neither of which is acceptable.

The roadmap (`BETA-AUDIT-ROADMAP.md`, Block 1) calls for a paste facade
with a clear macOS implementation and a clear non-macOS unsupported stub.
This task implements that facade structurally — it does **not** add a
real Windows or Linux paste path. Windows would eventually need clipboard
+ Ctrl+V via `enigo` or native input APIs; Linux would need separate X11
and Wayland handling. None of that is in this task.

The project is **Tier 1**. Keep the abstraction minimal: a cfg split, no
trait object, no plugin registry. Preserve every public function name the
rest of the codebase already calls.

## In scope
- `src-tauri/src/paste.rs` (will likely become `src-tauri/src/paste/mod.rs`
  with submodules, or stay as a single file with cfg blocks — pick the
  layout that yields the smaller diff).
- New file(s) under `src-tauri/src/paste/` if you go directory-style.
- `src-tauri/src/lib.rs` only for the `mod paste;` declaration if the
  layout changes.

## Out of scope
- Implementing real Windows or Linux paste behavior. The non-macOS branch
  returns a clear "unsupported platform" error and that is enough.
- Any change to `src-tauri/src/hotkey*` (TASK-2 handles that).
- Any change to `src-tauri/src/recorder.rs`, `audio.rs`, or any caller
  of `crate::paste::...`. If a signature would have to change to make
  this task work, stop and flag it — that signals the proposed facade is
  wrong.
- Adding new dependencies. `arboard` may stay in the unconditional
  `[dependencies]` block since it is itself cross-platform; do not move
  it under a target-specific section unless `cargo check` on Linux
  reveals a problem with it.
- Touching `tauri.conf.json`, sidecar binaries, or anything outside
  `src-tauri/src/paste*` and `src-tauri/src/lib.rs`.

## Steps
1. Read `src-tauri/src/paste.rs` end to end. List the public items
   (functions, types) that the rest of the crate calls. Common ones:
   `paste(text)`, `frontmost_app()`. Capture exact signatures so the
   facade preserves them.
2. Decide layout. Same two options as TASK-2: single file with
   `#[cfg(target_os = "macos")]` blocks, or a `paste/` directory with
   `mod.rs`, `macos.rs`, `unsupported.rs`. Choose the smaller-diff option.
3. Move the existing macOS body (osascript Cmd+V dispatch, clipboard
   save/restore, `frontmost_app()`) into the macOS branch verbatim.
   Pure relocation.
4. Add the non-macOS branch. It must export the same public names. Every
   entry point returns a `Result::Err` (or equivalent) carrying the
   literal string `unsupported platform`. If a function currently returns
   `Option<String>` (like `frontmost_app()`), the non-macOS version
   should return `None` *and* log a one-line warning at first call. The
   downstream UI banner can rely on the literal `unsupported platform`
   string showing up wherever a `Result` is involved.
5. Run `cargo check` from `src-tauri/` (default target = macOS). Confirm
   it passes with no new warnings.
6. Run `cargo check --target x86_64-unknown-linux-gnu` from `src-tauri/`.
   The paste module specifically must now compile clean. Combined with
   TASK-2, the audit-table classification of mac-only Rust modules
   should be down to whatever modules remain (audio, transcribe, etc.) —
   note any new findings in your task-completion summary but do **not**
   fix them here.
7. Run `npm run tauri dev` from the repo root. Hold the push-to-talk
   hotkey, dictate one short phrase into a focused text field, confirm
   the transcript pastes correctly. Note the exact target app and phrase.

## Success signal
- `cargo check` passes on the default macOS target.
- `cargo check --target x86_64-unknown-linux-gnu` no longer reports
  errors originating from `src-tauri/src/paste*`.
- The macOS dev build still completes one push-to-talk dictation
  end-to-end, with the transcript visibly pasted into a normal text
  field. Record the proof phrase + target app in `SESSION-STATUS.md`
  after review.
- The non-macOS `paste()` branch contains the literal string
  `unsupported platform` in its error path.

## Notes
- After this task, combined with TASK-2, the project should have honest
  cfg boundaries on the two big mac-only surfaces. The audit-table cells
  for "Build app" and "Paste into focused app" / "Global hotkey" on
  Windows and Linux move from "compile fails" to "compiles, returns
  unsupported-platform error". That is still not a working
  cross-platform app — it is just an honest one. Update
  `PLATFORM-AUDIT.md` after both task-2 and task-3 land if the user
  asks.
- Keep the diff focused. Do not refactor clipboard handling, do not add
  retries, do not rewrite the osascript invocation. Move and gate.
