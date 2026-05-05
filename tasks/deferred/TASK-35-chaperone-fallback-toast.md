# TASK-35: Surface Chaperone silent fallback as a recoverable ui-error toast

## Goal
When the Chaperone classifier fails (Ollama unreachable, model missing, classifier returned an unrecognized token, etc.) and the cleanup pipeline silently falls back to the raw transcript, the user now sees exactly one transient toast — kind `chaperone-fallback`, message "Chaperone unreachable — used raw output. Set up Ollama in Modes → Advanced." — through the existing `ui-error` event channel and the existing `uiErrors` toast stack in `src/App.svelte`. Clicking the toast switches the app to the Modes tab. The fallback behavior itself (returning the raw transcript) does not change.

## Context
Today, when `classify_blocking()` in `src-tauri/src/cleanup.rs` (at lines 73-79 of the file as it currently stands — the `CleanupMode::Chaperone => match ...` arm) returns `Err`, the cleanup module logs a `tracing::warn!` and silently calls `handle_raw(trimmed)`. The user sees no UI signal — they just notice their text is unprocessed (and that there was a 2-second delay before paste). This is fine as a fallback policy but bad as a discoverability problem: someone enabling Chaperone for the first time without Ollama installed has no clue what happened.

We already have a typed error channel built for exactly this case. Read `src/App.svelte` and search for `ui-error`. The pattern:

- Backend emits an event with `app.emit("ui-error", payload)` where the payload is `{ kind: String, message: String, recoverable: bool }`.
- Frontend `listen('ui-error', ...)` pushes into the `uiErrors` reactive array, which renders a stack of dismissible toasts at the top of the window. Each toast auto-dismisses after 5 seconds. The existing handler also has click-to-action logic for `kind: 'hotkey-permission'` and `kind: 'mic-permission'` — both deep-link to System Settings panes via `commands.openSystemSettings(...)`.

Add one more `kind`: `chaperone-fallback`. On click, the handler should call a new helper that switches the active tab to `'modes'` (the existing `switchTab('modes')` function — already declared in `App.svelte`).

The toast must be **rate-limited** to fire at most once per minute per app session. Without rate-limiting, a user with Chaperone enabled and Ollama down would see a toast on every single dictation. The deduplication key is the `kind` string. Implement a small in-memory cooldown in the backend cleanup module — e.g. a `std::sync::Mutex<Option<Instant>>` static guarded by `std::sync::OnceLock`, or `parking_lot::Mutex` if it's already a dependency. Don't add a new dep just for this.

The cleanup module currently does **not** have access to `tauri::AppHandle`. To emit, you need to plumb the handle to `process()` (or to a small helper in cleanup.rs). Two options:

1. **Pass the handle as a parameter.** Find every caller of `cleanup::process()` (grep for `cleanup::process` / `process(&` in `src-tauri/src/`). The transcribe pipeline is the main caller. Change the signature to `pub fn process(raw: &str, app: &AppHandle) -> String` and propagate. Most idiomatic; no global state.
2. **Store a global handle.** During Tauri's `setup` callback in `lib.rs`, stash `app.handle().clone()` into a `OnceLock<AppHandle>` exposed by a small accessor. Less plumbing, but introduces a global. Acceptable for a Tier-1 app.

Pick option 1 if the transcribe → cleanup → paste call chain is short (≤2 hops); pick option 2 if it's deeper or if multiple modules would otherwise need the same plumbing. Either is fine.

The toast wording must match the existing voice (concise, lowercase technical terms, em-dash, no exclamation): `"Chaperone unreachable — used raw output. Set up Ollama in Modes → Advanced."`. The arrow is the literal Unicode `→` (U+2192), not `->`.

The existing `ui-error` payload `recoverable: bool` should be `true` for this kind — the user can fix it.

## In scope
- `src-tauri/src/cleanup.rs` — emit a `ui-error` event in the `Chaperone => Err(_)` arm; add a 60-second per-kind cooldown so we emit at most once per minute
- Whatever signature/plumbing change is needed to give cleanup.rs access to `AppHandle` (option 1 from Context, or option 2 if cleaner)
- `src/App.svelte` — extend the existing `listen('ui-error', ...)` click handler to recognize `kind === 'chaperone-fallback'` and call `switchTab('modes')`

## Out of scope
- Changing the fallback behavior (raw transcript) itself
- Removing the `tracing::warn!` log line — keep it; toast and log serve different audiences
- Adding new error kinds for other Chaperone failure modes (allowlist rejection, prompt-injection sniff, etc.) — single `chaperone-fallback` kind for any failure within the Chaperone path
- Persistence of the cooldown across app restarts (in-memory is fine; first dictation after launch always emits)
- Adding new cleanup-mode toasts for `Off` or `Regex` (those don't fail meaningfully)
- A "Don't show again" affordance on the toast (the 60s rate limit is enough for now)
- TASK-32 / TASK-33 / TASK-34 functionality — this task is independent and can ship before, between, or after them

## Steps
1. Read `src-tauri/src/cleanup.rs` end to end. Confirm the current Chaperone arm (around the `CleanupMode::Chaperone => match classify_blocking(...) { ... }` block) and the `tracing::warn!` line.
2. Grep for callers of `cleanup::process` in `src-tauri/src/`. Pick option 1 (param passthrough) if the call chain is shallow; option 2 (global OnceLock) if not.
3. Add a private static cooldown structure to `cleanup.rs`:
   - Map of `kind: &'static str → Instant` of the last emit, gated by 60 seconds
   - A `should_emit_ui_error(kind: &str) -> bool` helper that updates the timestamp and returns whether to fire
4. In the `Chaperone => Err(_)` arm, after the existing `tracing::warn!`, call `should_emit_ui_error("chaperone-fallback")`. If true, build the payload and emit:
   ```
   {
     "kind": "chaperone-fallback",
     "message": "Chaperone unreachable — used raw output. Set up Ollama in Modes → Advanced.",
     "recoverable": true
   }
   ```
   Use `app.emit("ui-error", payload)`. Keep the exact wording — copy is part of the contract.
5. In `src/App.svelte`, find the `listen('ui-error', ...)` block (around the `uiErrors` push). Inside the existing toast click handler, add a branch for `err.kind === 'chaperone-fallback'` that calls `switchTab('modes')` and dismisses the toast (the existing dismiss-on-click already runs). Do not add a separate System Settings deep-link — there is no settings pane to deep-link to; the in-app Modes tab is the destination.
6. Build the backend: `cargo build --manifest-path src-tauri/Cargo.toml`. Must compile.
7. Run `npm run tauri dev` and smoke test:
   - Set Chaperone mode in the app's Modes tab.
   - Stop Ollama (or set the URL to an unreachable loopback port like `http://localhost:11999`).
   - Trigger a dictation (hold the hotkey, say a short phrase).
   - Within ~3 seconds of release: a toast appears at the top of the window with the exact wording. Click it: the app switches to the Modes tab. The toast disappears.
   - Trigger another dictation immediately. **No new toast** (cooldown). Wait 60 seconds, dictate again. Toast appears.
8. Update `SESSION-STATUS.md` with one line.

## Success signal
- With Chaperone enabled and Ollama not reachable, dictating a phrase produces exactly one `ui-error` toast within ~3 seconds, with kind `chaperone-fallback` and the exact wording from Step 4.
- Clicking the toast switches `activeTab` to `'modes'` and dismisses the toast.
- A second dictation within 60 seconds produces no additional toast (cooldown works). After 60 seconds, the next dictation produces a fresh toast.
- The fallback transcript is still pasted into the focused app — the toast does not block the paste.

## Notes
- Don't move the toast wording into a constant in `bindings.ts` or anywhere TS-side — the canonical string lives in the Rust emitter. The frontend just renders whatever arrives in the payload. (The kind string `chaperone-fallback` IS part of the contract, and may eventually be worth extracting into a typed enum, but that's out of scope here.)
- If the call chain to `cleanup::process` is wider than expected (e.g. tests call it without an `AppHandle`), pick option 2 (OnceLock global) and document the access pattern with a one-line comment in `cleanup.rs`.
- The cooldown deliberately persists state inside `cleanup.rs` so the rest of the app doesn't need to know about it. Keep it private.
- A future task could replace the in-Rust cooldown with a frontend-side dedupe (use the `kind` as a key, ignore push if a same-kind toast is already on screen). That would be more general but is more change for the same outcome — skip for now.
