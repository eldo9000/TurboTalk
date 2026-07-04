# TASK-83: Track whether Input Monitoring TCC prompt has been requested

## Goal
Persist a flag recording whether TurboTalk has already triggered the Input Monitoring TCC prompt via `CGRequestListenEventAccess` / `IOHIDRequestAccess`. After the first request, skip redundant TCC calls and go straight to the System Settings deep-link — and show the user appropriate button text ("Request Input Monitoring permission" vs "Open System Settings").

## Context
Scroll Reverser tracks `HasRequestedInputMonitoringPermission` in `NSUserDefaults`. On macOS, `IOHIDRequestAccess` (and `CGRequestListenEventAccess` on macOS 12+) only shows the TCC permission prompt **once per bundle**. After that first call, subsequent calls are silent no-ops — the OS remembers it was already prompted.

TurboTalk currently calls both TCC APIs on EVERY click of "Open System Settings" in the onboarding wizard (`Onboarding.svelte:143` calls `commands.requestInputMonitoringPermission()`). After the first click, the TCC APIs do nothing, but the user has no way of knowing. The button always says "Open System Settings" and always fires the TCC calls — wasted work.

The fix:
1. Persist a flag (`im_prompted`) in a simple file (same pattern as `onboarding_complete` flag at `permissions.rs:34-54`)
2. In `request_input_monitoring_permission`, check the flag. If already set, skip the TCC calls and return early.
3. Expose the flag via `Readiness` or a new command, so the frontend can show "Request Input Monitoring permission" (first time) vs "Open System Settings" (subsequent requests).

## In scope
- `src-tauri/src/permissions.rs` — add flag file read/write, gate in `request_input_monitoring_permission`, expose in `check_readiness` response
- `src/Onboarding.svelte` — change button label based on whether IM was already requested
- `SESSION-STATUS.md`

## Out of scope
- Adding this flag to the main `Config` struct (serialized config.toml) — a separate flag file is simpler and avoids serialization compatibility concerns
- Accessibility "already prompted" tracking (could be a follow-up — same pattern)
- Any changes to the microphone step (the AVFoundation prompt behavior is different)

## Steps

### Backend (Rust)
1. Add an `im_prompted_flag_path()` helper alongside `onboarding_flag_path()` at ~line 34 in `permissions.rs`. Use `data_dir().join("im_prompted")`.
2. Add `fn has_requested_im() -> bool` and `fn mark_im_requested()` following the same pattern as `has_completed_onboarding()` / `mark_onboarding_complete()` (lines 41-54).
3. In `request_input_monitoring_permission` (TASK-82 will make it async — if TASK-82 is not yet merged, apply this change to the current sync version and TASK-82 will carry it forward):
   - Call `mark_im_requested()` BEFORE the TCC calls (so the flag is set even if the TCC calls fail)
   - Check `has_requested_im()` at the top. If true, skip the TCC calls entirely and return `input_monitoring_status()`.
4. Add `pub input_monitoring_requested: bool` to the `Readiness` struct (line 83).
5. In `check_readiness`, set `input_monitoring_requested: has_requested_im()`.
6. Use `#[cfg(target_os = "macos")]` guards for the flag file since Input Monitoring is macOS-only.

### Frontend (Svelte)
7. In `Onboarding.svelte`, read `readiness.input_monitoring_requested` (it's in the `Readiness` struct from `checkReadiness()`).
8. Change the "Open System Settings" button text in Step 1:
   - If `input_monitoring_requested` is false: show "Request Input Monitoring permission" (this is the first-time TCC prompt)
   - If `input_monitoring_requested` is true: show "Open System Settings" (TCC was already prompted, just need user to toggle it on)
9. The button action (`openInputMonitoring()`) stays the same — the Rust backend gates the TCC calls internally.

### Verification
10. Run `cargo check --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml -- -W clippy::all`.
11. Run `npm run typecheck` — the `Readiness` struct changed, so bindings will regenerate. Verify the new field appears in `CheckReadinessResponse`.
12. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- `npm run typecheck` passes — `readiness.input_monitoring_requested` is accessible in Svelte.
- The flag file `~/.config/turbotalk/im_prompted` is created on first IM prompt request.
- On subsequent runs, `has_requested_im()` returns true and the TCC calls are skipped.
- The onboarding button text changes from "Request Input Monitoring permission" to "Open System Settings" after the first click.
- The TCC dialog only fires once — closing and reopening the onboarding wizard does not re-trigger it.

## Notes
- The `onboarding_flag_path()` uses `crate::settings::data_dir()`. Use the same path for consistency.
- The flag file is never deleted (unlike `onboarding_complete` which is deleted by `reset_onboarding`). This is intentional — even after reset, re-prompting IM TCC is pointless since macOS ignores it.
- If you want `reset_onboarding` to also clear the IM flag (for testing), add `let _ = std::fs::remove_file(im_prompted_flag_path());` to the `reset_onboarding` function. This is optional but convenient for development.
- The `Readiness` struct is serialized with serde and specta. Adding a bool field is fine — it serializes as a JSON boolean, no compatibility issues.
