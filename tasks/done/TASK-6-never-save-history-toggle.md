# TASK-6: Add "Never save history" toggle and "Open data folder" button

## Goal
A "Never save history" toggle exists in settings and is respected by the backend (no history entries written when enabled). An "Open data folder" button opens `~/.config/librewin/turbotalk/` in Finder. Both are wired end-to-end.

## Context
TurboTalk is a Tauri 2 + Svelte 5 dictation app on macOS. Settings are defined in `src-tauri/src/settings.rs` and persisted to `~/.config/librewin/turbotalk/settings.json`. History is stored as JSON under `~/.config/librewin/turbotalk/history/`. The frontend is `src/App.svelte`.

Currently there is a history retention setting (e.g. number of days or entries), but no way to disable history recording entirely. Beta users who dictate sensitive text need a guaranteed "never write anything to disk" mode separate from "clear on restart."

**"Never save history" toggle:**
- Add a `save_history: bool` field to the `Settings` struct in `settings.rs`. Default: `true` (preserve existing behavior).
- In the code path that writes a history entry (find by searching for history file writes in the backend), gate the write on `settings.save_history`. If false, skip silently.
- Add the toggle to the settings section in `src/App.svelte`. Label: "Save history". When off, the existing retention controls should be visually disabled (greyed out) but not removed.

**"Open data folder" button:**
- Use Tauri's `shell::open` (or the equivalent Tauri 2 API) to open `~/.config/librewin/turbotalk/` in the default file manager.
- Add a Tauri command `open_data_folder` in `lib.rs` that resolves the path and calls `tauri::api::shell::open` (or `tauri_plugin_shell` if that's what's wired).
- Add the button to the settings section in `src/App.svelte`. Label: "Open data folder".

Both changes go in the same commit. Neither requires UI redesign.

## In scope
- `src-tauri/src/settings.rs` — add `save_history` field
- `src-tauri/src/lib.rs` — add `open_data_folder` command; wire `save_history` into history write gating
- The history write site (wherever history entries are persisted — find by grepping for history file path)
- `src/App.svelte` — "Save history" toggle + "Open data folder" button
- `src/bindings.ts` — add `open_data_folder` binding

## Out of scope
- Changing the history retention logic (keep existing behavior when `save_history = true`)
- Deleting existing history files (user does that manually via the data folder)
- PRIVACY.md (TASK-5)
- Chaperone UI label (TASK-7)
- Windows or Linux path handling

## Steps
1. Add `save_history: bool` to `Settings` in `settings.rs` with `#[serde(default = "default_true")]` so existing settings files without the field default to `true`.
2. Grep for the history write path (`~/.config/librewin/turbotalk/history` or similar). In the write function, check `settings.save_history` before writing. If false, return early.
3. Add `open_data_folder` Tauri command in `lib.rs`. Resolve `~/.config/librewin/turbotalk/` using `tauri::path::home_dir()` or equivalent, then open it. Check how other shell operations are done in the codebase and use the same API.
4. Register `open_data_folder` in the invoke handler.
5. Add the binding to `src/bindings.ts`.
6. In `src/App.svelte`: add "Save history" toggle bound to the `save_history` setting field. Add "Open data folder" button that calls `open_data_folder`. Disable retention controls when `save_history` is false.
7. Run `cargo clippy -D warnings` — must exit 0.

## Success signal
- `cargo clippy -D warnings` exits 0.
- In `npm run tauri dev`: toggle "Save history" off, dictate a phrase, check `~/.config/librewin/turbotalk/history/` — no new file written.
- Toggle back on, dictate again, check history directory — entry written.
- Click "Open data folder" — Finder opens to `~/.config/librewin/turbotalk/`.

## Notes
- `default_true` helper in serde: `fn default_true() -> bool { true }`.
- Tauri 2 shell open API may be `tauri_plugin_shell::open` — check `Cargo.toml` for which shell plugin is already imported.
