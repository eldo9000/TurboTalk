# Scan 1 — Diagnostics / Privacy Audit

**Goal:** Confirm the diagnostic/bug-report export never leaks dictated transcript
text, that logging is useful but not noisy, and that error paths surface cleanly
to the user.

**Scope:** read-only audit. Do **not** change behavior. Produce a findings report
with file:line citations and a severity per finding (blocker / should-fix / nit).
If you find a real leak, that's a release blocker — flag it loudly.

## Why this matters

TurboTalk is a local-first dictation tool. The privacy contract is: **nothing a
user dictates leaves their machine.** The bug-report path (`submit_bug_report`)
uploads a report to Telegram, so any transcript text reaching that bundle is an
exfiltration bug.

## Primary files

- `src-tauri/src/diagnostic_log.rs` — the whole export/upload pipeline:
  - `TRANSCRIPT_LOG_PREFIX` / `record_transcript()` / `TRANSCRIPT_WRITER` (lines ~28–62):
    the local-only transcript debug log. Verify the claimed contract holds — that
    this sink is fully separate from `tracing` and is **never** read by
    `read_recent_logs` / `build_report_text`.
  - `sanitized_config()` (lines ~138–151): confirm the struct only carries
    settings metadata, no free text, no paths that embed usernames beyond what's
    already expected.
  - `build_report_text()` (lines ~209–276): trace every field that lands in the
    report. Readiness, diagnostics, sanitized config, UI events, session-log tail.
  - `read_recent_logs(MAIN_LOG_PREFIX, …)`: confirm it reads only `turbotalk.*.log`
    and can never pick up `transcripts.*.log` (prefix-matching correctness).
  - `record_client_event()` / `log_client_event()`: UI event ring buffer. The
    2000-char detail cap is the only guard — confirm no transcript text is ever
    passed as an event detail from the frontend.

## Checks to run

1. **Transcript isolation.** Grep every call site of `record_transcript`. Confirm
   transcript text only ever flows to `TRANSCRIPT_WRITER` and never into
   `tracing::{info,warn,error,debug}`, `record_client_event`, or any string that
   reaches `build_report_text`. Search the whole tree for `tracing::.*transcript`,
   and for the actual transcript variable names in `transcribe.rs` /
   `transcribe_backends/*` / `recorder.rs` / `cleanup.rs` — trace whether any
   transcript string is logged at any level.
2. **UI-event leakage.** In `src/`, find every `commands.logClientEvent` /
   `log_client_event` call site. Confirm none pass transcript text (or LLM cleanup
   output) as the `detail`. The `transcript` and `transcription-rejected` event
   handlers in `App.svelte` are the highest-risk spots.
3. **Log noise / level discipline.** Skim `tracing::` calls across `src-tauri/src/`.
   Flag anything logged at `info` that fires per-audio-frame or per-segment (would
   flood `turbotalk.log`), and anything at `warn`/`error` that's actually expected
   control flow (would make the error log useless). The session-log tail is capped
   at 2 MB (`MAX_LOG_TAIL_BYTES`) — confirm a noisy logger can't push useful
   context out of that window in a normal session.
4. **Error-path surfacing.** Confirm failures reach the user as a clean toast, not
   a silent swallow or a raw `Debug` dump:
   - `submit_bug_report` credential-missing and upload-failure branches (the
     user-facing `Err` strings).
   - `cleanup.rs` `chaperone-fallback` toast on LLM failure (per CLAUDE.md).
   - `export_diagnostic_report` / `open_logs_folder` error returns.
   Check the frontend `ui-error` / `transcript-error` / `paste-error` listeners in
   `App.svelte` actually render these.
5. **Debug-only guard.** The transcript log is described as a debug-build feature.
   Confirm it is genuinely gated (e.g. `#[cfg(debug_assertions)]` or equivalent at
   the writer-init site in logging setup) and cannot be active in a release bundle.
   Check where `init_transcript_writer` is called.

## Deliverable

A markdown report: each finding with file:line, severity, and a one-line fix
suggestion. Lead with a yes/no verdict on the core question: **can dictated text
reach an uploaded report?** If no leak exists, say so explicitly and cite the
isolation boundary that guarantees it.
