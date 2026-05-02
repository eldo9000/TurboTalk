# TASK-16: Define paste ordering and focused-app policy for dictation jobs

## Goal
TurboTalk has clear rules for where and when a completed dictation is pasted. The app should not paste stale text unpredictably if focus changes or if future work adds queueing.

## Context
Today paste uses the clipboard plus Cmd+V in `src-tauri/src/paste.rs`. It pastes into whichever app is focused when paste runs, not necessarily the app that was focused when recording began.

With one in-flight job, this is usually acceptable for a personal push-to-talk app. But the rule should be explicit, and the backend should be structured so a future queue does not accidentally paste old text into a surprising target.

## In scope
- Document the current policy in `ARCHITECTURE.md`.
- Add lightweight focused-app observation on macOS if practical:
  - capture the frontmost app name or bundle id at recording start.
  - capture the frontmost app again before paste.
  - log both values with the job id.
- If focus changed, use a conservative default:
  - still paste into the current focus, but log the change and emit a recoverable UI warning; or
  - skip paste and leave transcript in history.
- Choose the default that best matches existing TurboTalk UX. For personal dictation, "paste into current focus, but make focus changes observable" is probably the right first step.

## Out of scope
- Queueing multiple dictations.
- App-specific paste adapters.
- Cross-platform focus detection.
- Replacing clipboard-based paste.

## Steps
1. Read `src-tauri/src/paste.rs` and the post-transcript path in `src-tauri/src/hotkey.rs`.
2. Add a small macOS helper, either in `paste.rs` or a new focused module, that returns best-effort frontmost app identity.
   - Keep it best-effort. If detection fails, return `None` and continue.
   - Use a simple `osascript` query if that matches the existing macOS glue style.
3. Capture focus identity at recording start and carry it with the job metadata.
4. Capture focus identity immediately before paste.
5. Log `{ job_id, focus_at_start, focus_at_paste }`.
6. If focus changed, emit `focus-changed-before-paste` with a user-readable payload.
7. Update frontend to show a gentle recoverable banner only if the event is emitted.
8. Update `ARCHITECTURE.md`:
   - Current policy: paste into current focused app at paste time.
   - Focus changes are logged and surfaced.
   - Future queueing must revisit this policy.
9. Run:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
10. Manual test:
   - Start recording in TextEdit/Notes, switch focus before release or before paste, confirm the behavior matches the documented policy.
   - Normal same-focus dictation still pastes without warning.

## Success signal
- Paste target policy is documented.
- Focus changes before paste are observable.
- Normal dictation remains unchanged.
- Tests and clippy pass.

## Notes
- Do not block paste on focus-change unless you deliberately choose and document that policy.
- The important win is making the policy explicit before any future queue work.

