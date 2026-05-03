# TASK-2: Split hotkey.rs into platform-gated facade + macOS impl

## Goal
`src-tauri/src/hotkey.rs` no longer imports macOS-only crates at the top
level. Instead, a small platform facade exposes the public API the rest of
the codebase already calls, the current macOS implementation lives in a
cfg-gated submodule, and non-macOS targets get a clearly-labeled
"unsupported platform" stub. The macOS happy path is unchanged: holding the
push-to-talk hotkey still records, transcribes, and pastes.

## Context
TurboTalk's hotkey module currently does macOS-specific things at the top
of the file:

```
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{CGEventFlags, CGEventTap, CGEventTapLocation,
    CGEventTapOptions, CGEventTapPlacement, CGEventType, EventField};
```

These crates do not compile for `x86_64-pc-windows-*` or
`x86_64-unknown-linux-*`. The roadmap (`BETA-AUDIT-ROADMAP.md`, Block 1)
calls for the file to be split so that:

- macOS keeps its current `CGEventTap`-based push-to-talk path,
- Windows and Linux compile cleanly without dragging in CoreFoundation/
  CoreGraphics,
- the Windows/Linux runtime path returns a clear "unsupported platform"
  error rather than silently doing nothing or panicking.

This is **structural cfg work**, not a rewrite. You are not implementing
Windows or Linux hotkey behavior in this task. You are drawing the cfg
boundary so the codebase is honest about where macOS ends.

The project is **Tier 1**. Keep the abstraction minimal — a module split
plus cfg attributes. Do not introduce a trait object, a registry, or a
plugin system. If the current call sites in `src-tauri/src/lib.rs` (and
anywhere else that touches `crate::hotkey::...`) keep working unchanged on
macOS, the abstraction is the right size.

## In scope
- `src-tauri/src/hotkey.rs` (will likely become `src-tauri/src/hotkey/mod.rs`
  or stay as a single file with cfg blocks — pick whichever yields the
  smaller diff).
- New file(s) under `src-tauri/src/hotkey/` if you choose the directory
  layout (e.g. `macos.rs`, `unsupported.rs`).
- `src-tauri/src/lib.rs` only to the extent that the `mod hotkey;`
  declaration must keep working. Do not change call sites unless a
  signature genuinely needs to move.

## Out of scope
- Implementing Windows or Linux hotkey behavior. The non-macOS branch
  returns a clear unsupported error and that is enough for this task.
- Any change to `src-tauri/src/paste.rs` (TASK-3 handles that).
- Any change to `src-tauri/src/recorder.rs`, `audio.rs`, `tray.rs`, or
  other modules that call into hotkey. If a signature would have to change
  to make this task work, stop and flag it — that suggests the proposed
  facade is wrong.
- Adding new dependencies to `Cargo.toml`. The macOS branch keeps its
  existing `core-foundation` / `core-graphics` deps, ideally already
  cfg-gated as `[target.'cfg(target_os = "macos")'.dependencies]` (move
  them under that section if they aren't already).
- Touching `tauri.conf.json` or sidecar binaries.

## Steps
1. Read the current `src-tauri/src/hotkey.rs` end to end. Note every public
   item exported from the module — these are the surface the facade must
   preserve.
2. Decide layout. Two acceptable shapes:
   - Single file with `#[cfg(target_os = "macos")] mod imp { ... }` and a
     parallel `#[cfg(not(target_os = "macos"))] mod imp { ... }` block,
     plus thin re-exports.
   - Directory: `hotkey/mod.rs` (re-exports), `hotkey/macos.rs`,
     `hotkey/unsupported.rs`. Pick this if the macOS code is large enough
     that the single-file version becomes hard to read.
3. Move the entire current macOS implementation into the macOS branch
   without semantic changes. The diff for the macOS body should be pure
   relocation — line-for-line identical apart from indentation and the
   cfg wrapper.
4. Create the non-macOS branch. It must export the same public names the
   rest of the crate calls into, but every entry point returns a clear
   error containing the string `unsupported platform` (case-insensitive
   match acceptable downstream). If the public surface is "register a
   hotkey listener", returning a `Result::Err` with that message is
   sufficient. If a function returns `()`, log an error at startup and
   no-op — but make sure the user-visible behavior is "the app starts and
   tells you it can't bind a hotkey on this OS", not "the app silently
   does nothing".
5. In `src-tauri/Cargo.toml`, ensure `core-foundation` and `core-graphics`
   are under `[target.'cfg(target_os = "macos")'.dependencies]`. If they
   are already in the unconditional `[dependencies]` block, move them.
6. Run `cargo check` from `src-tauri/` (default target = macOS). Confirm
   it passes with no new warnings introduced by this task.
7. Run `cargo check --target x86_64-unknown-linux-gnu` from `src-tauri/`.
   The hotkey module specifically should now compile clean. Other modules
   may still fail (paste.rs in particular) — that is expected and is
   TASK-3's problem, not this task's.
8. Run `npm run tauri dev` from the repo root. Hold the push-to-talk
   hotkey, dictate "hello world", confirm it pastes into a focused
   TextEdit (or any text field). The macOS behavior must be unchanged.

## Success signal
- `cargo check` passes on the default macOS target.
- `cargo check --target x86_64-unknown-linux-gnu` no longer reports errors
  originating from `src-tauri/src/hotkey*`. Errors originating from other
  files are acceptable at this stage.
- The macOS dev build starts and successfully transcribes one push-to-talk
  utterance end-to-end. Document the exact phrase used and the target app
  in `SESSION-STATUS.md` after the task is reviewed.
- The non-macOS branch contains the literal string `unsupported platform`
  in its error path — this gives downstream UI work an easy hook.

## Notes
- If `core-foundation` / `core-graphics` are already cfg-gated in
  `Cargo.toml`, do not "fix" the dependency table — record that they were
  already correct in your task-completion summary.
- Keep the diff focused. If you find yourself renaming functions,
  reordering fields, or "while I'm here" cleanups, stop. Those are
  separate tasks.
