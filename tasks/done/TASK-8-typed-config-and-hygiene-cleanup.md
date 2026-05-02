# TASK-8: Typed Rust↔TS config contract via specta + small hygiene cleanups

## Goal
- The Rust `Config` struct (and all its sub-structs) in `src-tauri/src/settings.rs` exports a TypeScript type definition that the frontend imports and uses, so that adding/removing/renaming a field in Rust produces a TypeScript compile error in `src/App.svelte` if the frontend isn't updated.
- The two remaining `unsafe` and undocumented blocks have SAFETY comments. The overlay window positions itself on the monitor of the focused window, not the primary monitor. The Tauri `macOSPrivateApi` flag rationale is documented in `TRUTH.md`.

## Context
TurboTalk is a personal-use macOS voice dictation app (Tauri 2 + Rust + Svelte 5). After the higher-priority security and architecture fixes (TASKS 1–7), the multi-agent review identified a final cluster of hygiene items that, together, close the "implicit contract" and "undocumented unsafe" patterns:

1. **Untyped config contract (ARCH-014, ARCH-016)** — the `get_config` and `save_config` Tauri commands pass `settings::Config` (a Rust struct) over IPC. The frontend in `src/App.svelte` constructs the same shape by hand. When Rust adds a field with `#[serde(default)]`, the frontend silently omits it on save and the field reverts to default — a hard-to-spot data-loss bug. Generating TypeScript types from the Rust struct closes this class of bug at compile time.

2. **Unsafe `kCFRunLoopCommonModes` access (SEC-012)** — `src-tauri/src/hotkey.rs:123` has a bare `unsafe { kCFRunLoopCommonModes }` with no SAFETY comment. The constant is a `static` from core-foundation, accessing it is safe, but a reader has no way to know that.

3. **Overlay multi-monitor (ARCH-013)** — `src/Overlay.svelte:59` uses `primaryMonitor()` to position the overlay at the bottom-center of the primary display. On a multi-monitor setup, the overlay appears on the wrong screen.

4. **`macOSPrivateApi` rationale (SEC-014)** — `src-tauri/tauri.conf.json:12` has `macOSPrivateApi: true` with no documented reason. The reason is CGEventTap-based hotkey support; this should be recorded so future maintainers don't blindly remove it.

The recommended tool for the typed contract is `tauri-specta` v2 (specta + the Tauri integration), which:
- Reads `#[derive(Type)]` on Rust structs.
- Generates a TypeScript bindings file at build time (e.g., `src/bindings.ts`).
- Wraps Tauri commands with typed invoke helpers.

The frontend then imports `Config` (and the typed `commands`) from `bindings.ts` instead of using free-form `invoke()` calls.

## In scope
- `src-tauri/Cargo.toml` — add `specta`, `specta-typescript`, `tauri-specta` deps
- `src-tauri/src/settings.rs` — derive `Type` on `Config` and all nested structs (`AudioConfig`, `HotkeyConfig`, `WhisperConfig`, `CleanupConfig`, `Mode`, `HistoryEntry`)
- `src-tauri/src/lib.rs` — `tauri-specta` builder setup, generate the bindings file at build/dev time
- `src-tauri/build.rs` (create if not present) — wire specta export to run during `cargo build`
- `src/bindings.ts` — generated, gitignored or committed (decide; recommendation: commit so frontend dev doesn't need a Rust build)
- `src/App.svelte` — replace untyped `invoke()` calls and ad-hoc Config shape with the generated types
- `src-tauri/src/hotkey.rs:123` — add SAFETY comment
- `src/Overlay.svelte` — switch to `getMonitorFromPoint()` or follow-the-focused-window
- `TRUTH.md` — add a one-line note explaining `macOSPrivateApi`

## Out of scope
- Migrating every `invoke()` call to typed bindings — only migrate the config-related commands (`get_config`, `save_config`, `scan_models_dir`, `list_audio_devices`, `get_launch_at_login`, `set_launch_at_login`, `load_history`, `save_history`, `paste_history_item`). Other invoke calls can stay free-form.
- Rewriting the build pipeline — keep `npm run tauri dev` and `npm run tauri build` working unchanged from the user's perspective.
- Performance tuning
- Adding new features

## Steps
1. Read `src-tauri/Cargo.toml` and check Tauri version and existing dependencies. The current Tauri major version determines the compatible `tauri-specta` version. Pin to the matching one.
2. Add to `[dependencies]` in `src-tauri/Cargo.toml`:
   ```toml
   specta = { version = "2", features = ["derive"] }
   specta-typescript = "0"
   tauri-specta = { version = "2", features = ["derive", "typescript"] }
   ```
   (Use the actual versions compatible with the project's Tauri.)
3. In `src-tauri/src/settings.rs`, add `#[derive(specta::Type)]` to: `Config`, `AudioConfig`, `HotkeyConfig`, `WhisperConfig`, `CleanupConfig`, `Mode` (the enum from TASK-4), `HistoryEntry`. Run `cargo build` and resolve any derive errors (some types may need `#[specta(rename = ...)]` if their TS name should differ).
4. In `src-tauri/src/lib.rs`, build a `tauri_specta::Builder` containing all relevant commands. In a `#[cfg(debug_assertions)]` block (or in a small `build.rs`), call `.export(...)` to write `src/bindings.ts`. This produces a TS file with named types and typed `commands.<name>(...)` wrappers.
5. Decide whether to commit `src/bindings.ts`. Recommendation: commit it so frontend-only contributors don't need a Rust toolchain. Add a comment at the top: "AUTO-GENERATED by tauri-specta. Do not edit by hand. Run `cargo build` from src-tauri/ to regenerate."
6. In `src/App.svelte`, replace ad-hoc `invoke('get_config')` calls with the typed `commands.getConfig()` (or whatever name specta generates). Replace the local `Config` shape literal with the imported type. Run `npm run check` (or `svelte-check`) and fix any type errors that surface — these are exactly the bugs this task exists to catch. Document any non-trivial fixes in commit message.
7. **SAFETY comment:** in `src-tauri/src/hotkey.rs` around line 123, add immediately above the `unsafe { kCFRunLoopCommonModes }` line:
   ```rust
   // SAFETY: kCFRunLoopCommonModes is a static CFStringRef constant exported by
   // core-foundation. Reading it requires unsafe because the binding is a static
   // extern, but the value is immutable and thread-safe to read.
   ```
8. **Overlay multi-monitor:** in `src/Overlay.svelte`, replace the `primaryMonitor()` call with logic that picks the monitor where the user's focus most likely is. Two approaches:
   - **Easier:** track mouse position via Tauri's cursor APIs and use `monitorFromPoint(cursorPos)` to choose the active screen.
   - **Better:** track the focused window's screen via macOS APIs. Tauri exposes `availableMonitors()` and you can query the currently-focused window's frame from a small Rust command (using `core-graphics` `CGWindowListCopyWindowInfo`). Pick the cheaper option for now (cursor position) — the goal is "overlay appears on the screen the user is actually looking at."
   Adjust the position math to be relative to the chosen monitor's origin + size, not the primary's.
9. **TRUTH.md note:** add a one-line entry under a "Tauri config rationale" section (create section if it doesn't exist):
   - `macOSPrivateApi: true` — required for `CGEventTap` hotkey monitoring (`hotkey.rs`). Removing it disables global push-to-talk.
10. Run `cargo build --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, and `npm run build` (which should run `svelte-check` if it's wired up; otherwise run `npx svelte-check` separately).
11. Manually test:
    - **Type drift check:** add a temporary new field to `Config` in Rust, rebuild — verify `src/bindings.ts` regenerates with the new field and `svelte-check` flags any places in App.svelte that don't account for it. Then revert the temporary field.
    - **Normal flow:** launch the app, exercise every settings UI control (theme, hotkey, cleanup mode, model selection, vocabulary edit, prompt edit), save, restart — verify all settings persist correctly.
    - **Multi-monitor overlay:** if you have two displays available, focus an app on the secondary, hold the hotkey, verify the overlay appears on the secondary display. If only one display is available, just verify single-monitor behavior is unchanged.

## Success signal
- `cargo build`, `cargo clippy -D warnings`, and `npm run build` all exit 0.
- `src/bindings.ts` exists, contains exported types matching every Rust struct named in step 3, and is imported by `src/App.svelte`.
- Adding a temporary field to `Config` and rebuilding produces a Svelte type error in App.svelte (verify, then revert).
- `grep -n "SAFETY:" src-tauri/src/hotkey.rs` shows the new comment above `kCFRunLoopCommonModes`.
- The overlay appears on the monitor where the user's cursor is, not always the primary.
- `TRUTH.md` contains the `macOSPrivateApi` rationale.

## Notes
- If `tauri-specta` v2 is not yet stable for the Tauri version in use, fall back to plain `specta` + `specta-typescript` and write a small `bin/export-types.rs` that emits the bindings; call it manually from `npm run gen-types` or via build.rs. The end result is the same: a generated `bindings.ts`.
- Keep the bindings file path stable (`src/bindings.ts`) so the frontend import statement stays consistent.
- Multi-monitor positioning often surfaces edge cases (HiDPI scaling, fractional coordinates). Use `LogicalPosition` and trust Tauri's monitor scale factors — do not hand-compute.
- Multi-agent review reference: findings ARCH-013, ARCH-014, ARCH-016, SEC-012, SEC-014 in `/tmp/code-analysis-concern-based-main-20260501.md`.
