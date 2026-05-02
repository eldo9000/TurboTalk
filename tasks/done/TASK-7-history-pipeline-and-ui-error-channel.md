# TASK-7: Backend-owned history truncation, awaited save with errors surfaced, and a unified `ui-error` event channel

## Goal
- The 50-entry history limit is enforced inside the Rust `save_history()` function, not in the frontend. The frontend stops slicing the array before saving.
- The frontend `await`s `invoke('save_history', ...)` and surfaces failures (disk full, permission denied) as a user-visible toast.
- Loaded history entries are validated against a strict shape (`{ text: String, ts: u64 }`) at deserialize time; malformed or unexpected fields cause that single entry to be dropped (not the whole file) with a warn log.
- A single convention `ui-error` event channel exists with payload shape `{ kind: String, message: String, recoverable: bool }`. Pre-existing silently-swallowed error paths (config parse fallback, history-save failure, emit failures) route through it. The frontend has one toast/banner component that listens to `ui-error` and renders all of them uniformly.

## Goal (one sentence)
History persistence is reliable and bounded by the backend, and silent error-swallowing across the app is replaced by a single `ui-error` event the frontend surfaces.

## Context
TurboTalk is a personal-use macOS voice dictation app. The history pipeline currently works like this:

1. After a successful transcript, the frontend (`src/App.svelte:~313`) prepends the new entry to a Svelte `history` array, slices it to the first 50 items, and calls `invoke('save_history', { entries: history })` *without `await`*. If the call fails, the error is silently lost.
2. The Rust `save_history` function in `src-tauri/src/settings.rs` writes the array straight to `~/.config/librewin/turbotalk/history.json` as JSON.
3. On startup, `load_history` reads the file and `serde_json::from_str` deserializes it into `Vec<HistoryEntry>`. If a single entry is malformed, the whole file is treated as parse-failed and an empty vec is returned silently.

A multi-agent code review identified four converging issues:
- **MAC-4 — silent corruption + XSS + plaintext at rest** (SEC-007, SEC-008, ARCH-011, ARCH-012)
- **Systemic Pattern 1 — silent failure as a default** across hotkey errors, config parse fallback, and history saves

CSP enforcement (TASK-1) handles the XSS path at the rendering layer. Plaintext-at-rest is not in scope for this task — it's a personal-use single-user app and encryption-at-rest is a deliberate non-goal for now. This task focuses on:
- Single source of truth for the 50-entry limit
- Awaiting the save and surfacing failures
- Per-entry validation at load time
- A reusable `ui-error` channel that other modules can adopt

The `ui-error` channel is the load-bearing piece for systemic pattern #1 — once it exists, future tasks (and existing silently-swallowed errors) have a place to route.

## In scope
- `src-tauri/src/settings.rs` — `save_history`, `load_history`, the `HistoryEntry` struct
- `src-tauri/src/lib.rs` — only as needed: `save_history` / `load_history` Tauri commands and any centralized error-emission helper
- `src/App.svelte` — frontend history list management, the `ui-error` listener and its toast UI
- A small backend helper `emit_ui_error(app, kind, message, recoverable)` in `lib.rs` (or a new `errors.rs`) that wraps `app.emit("ui-error", ...)` with the standard payload

## Out of scope
- Encrypting history at rest (deliberate non-goal — single-user app)
- Migrating every existing silent error to the new channel — only migrate the obvious 2-3 (history save, config parse fallback, history load malformed entry). Other migrations can happen in follow-up work.
- Changing the visual design of the toast — pick the simplest design that matches existing TurboTalk UI tokens
- Adding pagination to the history UI
- Adding history search / filtering

## Steps
1. Read `src-tauri/src/settings.rs` (`HistoryEntry`, `save_history`, `load_history`) and `src/App.svelte` (the transcript listener that calls save_history, the history rendering).
2. **Backend truncation:** in `save_history`, after deserializing the input, truncate the vec to 50 entries (keep the most recent — depending on insertion order, this is `entries.truncate(50)` if the vec is most-recent-first, or `entries.split_off(entries.len().saturating_sub(50))` if oldest-first; verify the existing convention). Persist the truncated form. Document the limit in a `const HISTORY_LIMIT: usize = 50;` at the top of the file.
3. **Per-entry validation in `load_history`:** instead of `serde_json::from_str::<Vec<HistoryEntry>>(...)`, parse to `serde_json::Value`, iterate the array, and for each element try `serde_json::from_value::<HistoryEntry>`. Drop entries that fail validation, log each drop at `tracing::warn!` level with the offending JSON. Return only the valid ones. Validate `text` is non-empty and `ts` is non-zero.
4. **`ui-error` helper:** in `src-tauri/src/lib.rs` (or a new `errors.rs` module), add a small struct or helper:
   ```rust
   pub fn emit_ui_error(app: &tauri::AppHandle, kind: &str, message: impl Into<String>, recoverable: bool)
   ```
   Internally calls `app.emit("ui-error", json!({"kind": kind, "message": message.into(), "recoverable": recoverable}))` and logs a warn if emit fails.
5. **Migrate three call sites to `ui-error`:**
   - In `save_history`'s Tauri command wrapper, on Err: emit `ui-error` with kind `"history-save"` before returning the error to the frontend.
   - In `settings::load`'s TOML parse-failure fallback: emit `ui-error` with kind `"config-parse"` and the parser's error message; recoverable=true.
   - In `load_history`'s per-entry drop: emit a single aggregated `ui-error` with kind `"history-load-malformed"` and `"N entries skipped"` if any drops occurred (avoid spamming once per entry).
   - Note: emitting in `lib.rs` requires the AppHandle. Pass it through to `settings::load` / `settings::load_history` via small wrapper functions if necessary, or do the emit at the call site in lib.rs's setup hook.
6. **Frontend save flow:** in `src/App.svelte`'s transcript listener, change the unawaited `invoke('save_history', ...)` to:
   ```js
   try {
     await invoke('save_history', { entries: history });
   } catch (e) {
     // server-side ui-error already emitted; this catch is belt-and-suspenders
   }
   ```
   Stop slicing `history` to 50 in the frontend — let it grow in memory, the backend bounds the on-disk size. (If the in-memory size matters for UI perf, slice for display only, not for the save call.)
7. **Frontend `ui-error` toast:** add a Svelte 5 state variable `uiErrors = $state([])`. Add `listen('ui-error', (e) => { uiErrors = [...uiErrors, { ...e.payload, id: ++idCounter }]; setTimeout(() => uiErrors = uiErrors.filter(x => x.id !== id), 5000) })` in onMount. Render a small fixed-position toast stack in App.svelte that maps `uiErrors` to small banners using existing TurboTalk UI tokens (the `transcript-error` banner already exists — model after it). Each toast should show the message and dismiss on click. Do not block the UI.
8. **Replace existing transcript-error wiring (optional):** the `transcript-error` event already exists. You may keep it as a separate channel (it's transcript-specific) or migrate it to `ui-error` with kind=`"transcribe"`. Recommendation: keep it separate since it has UI semantics tied to history error state — do not over-fold.
9. Run `cargo build --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
10. Manually test:
    - **Truncation:** transcribe 60 short utterances. Inspect `~/.config/librewin/turbotalk/history.json` and confirm exactly 50 entries are stored. The frontend may still show 60 in-memory until restart.
    - **Save failure:** make `~/.config/librewin/turbotalk/` unwritable (`chmod a-w`). Transcribe an utterance. Verify a toast appears with kind=`history-save` and the transcript still shows in history (in-memory). Restore permissions afterward.
    - **Malformed history file:** edit history.json by hand to inject one bogus entry like `{"text": null, "ts": "not-a-number"}` between valid ones. Restart the app. Verify the bogus entry is dropped (not in the list) and a `ui-error` toast appears with kind=`history-load-malformed`. Verify the rest of the history is intact.
    - **Config parse failure:** corrupt config.toml (add a stray quote). Restart the app. Verify a `ui-error` toast surfaces and the app still launches with defaults (existing behavior).
    - **Normal flow regression:** record + transcribe normally. Verify history persists across restart with no toasts.

## Success signal
- `cargo build` and `cargo clippy -D warnings` exit 0.
- All five test scenarios behave as described.
- `grep -rn "ui-error" src-tauri/src` returns at least three emit sites (history-save, config-parse, history-load-malformed).
- `grep -n "ui-error" src/App.svelte` returns the listener and the toast rendering code.
- `~/.config/librewin/turbotalk/history.json` never exceeds 50 entries on disk.
- `await invoke('save_history'` exists in App.svelte (no orphan unawaited call).

## Notes
- The Tauri `app.emit` API for v2 is `app.emit(event, payload)`. The payload should be JSON-serializable; `serde_json::json!` works.
- Prefer reusing the existing transcript-error banner styling for the new toast, just generalized.
- Do not import a new toast library — Svelte 5 + a small `$state` array + Tailwind classes is enough.
- Keep `HistoryEntry` deserialization strict: use `#[serde(deny_unknown_fields)]` if you want to be aggressive, or just tolerate unknown fields and require text+ts. The trade-off is forward compatibility vs. tampering detection — pick the lenient option (don't deny_unknown_fields) for a personal app.
- Multi-agent review reference: findings SEC-007, SEC-008, ARCH-010, ARCH-011, ARCH-012 / MAC-4 + Systemic Pattern 1 in `/tmp/code-analysis-concern-based-main-20260501.md`.
