# Scan 1 — Diagnostics / Privacy: Findings

**Date:** 2026-06-02 · **Scope:** read-only audit, no behavior changed.

## Verdict

**Can dictated text reach an uploaded report? — NO.**

There is no path from transcript text to `submit_bug_report` / `build_report_text`.
The isolation holds by three independent mechanisms, any one of which is sufficient:

1. **Separate sink.** Transcript text is written only through `TRANSCRIPT_WRITER`
   (`diagnostic_log.rs:52–62`), a dedicated `NonBlocking` writer that is never part
   of the `tracing` pipeline.
2. **Disjoint filename prefix + correct prefix matching.** `build_report_text`
   embeds only `read_recent_logs(MAIN_LOG_PREFIX, …)` (`diagnostic_log.rs:216`).
   `MAIN_LOG_PREFIX = "turbotalk"`, transcript files use `"transcripts"`
   (`diagnostic_log.rs:25,36`). `log_files_for` matches on `format!("{prefix}.")`
   = `"turbotalk."` (`diagnostic_log.rs:76,85`), which cannot match
   `transcripts.YYYY-MM-DD.log`. No code reads `TRANSCRIPT_LOG_PREFIX` into any report.
3. **Release builds don't even write the transcript log.** `init_transcript_writer`
   is called only inside a `#[cfg(debug_assertions)]` block (`lib.rs:1906–1922`),
   so in a shipped release bundle `record_transcript` is a no-op and the
   `transcripts.*.log` file is never created.

The transcript also travels to the frontend over the live `transcript` /
`transcription-rejected` IPC events, but that text stays in frontend memory and
local `history.json`; it is never fed back into `log_client_event`. Only a length
or a rejection reason is logged (see Check 2 below). `build_report_text` never reads
`history.json`.

## Findings

| # | Severity | Finding |
|---|----------|---------|
| 1 | — (pass) | Transcript isolation is sound — see verdict. No blocker found. |
| 2 | nit | UI-event channel relies on convention, not an enforced guard. |
| 3 | nit | `submit_bug_report` always writes a local plaintext report copy. |

### Check 1 — Transcript isolation ✅
All three `record_transcript` call sites pass `text` only to the dedicated writer:
- `transcribe.rs:835` (whisper) — adjacent logs print `text.chars().count()` only
  (`:829`, `:838–841`); explicit comment at `:827–828` states the contract.
- `transcribe_backends/moonshine.rs:323` — log at `:318–322` prints char count only.
- `transcribe_backends/parakeet.rs:320` — log at `:315–319` prints char count only.

No `tracing::{info,warn,error,debug}` call anywhere interpolates a transcript
variable. `cleanup.rs` has only two tracing calls: `:102` logs the Ollama error
string `{e}` (not transcript), `:409–412` logs `prompt.len()` bytes (not content,
and `debug`-level).

### Check 2 — UI-event leakage ✅ (with nit #2)
Every `logUi(...)` call site in `App.svelte` was inspected:
- `:1038` `transcript` → `${text.length} chars` (length only). ✅
- `:1072` `transcription-rejected` → `p.reason` only. The rejected text `p.text`
  is held in `filteredEntry` state (`:1073`) for display but never logged. ✅
- `:1054` `ui-error` → `payload.message`; `:1092` `paste-miss`, `:1119`
  `recording-discarded` → `e.payload`; `:953` `settings-saved` → hotkey/mode/
  backend/overlay only.

All backend emitters of those events use hardcoded strings or non-text values
(`hotkey.rs:724,728,656,788`, `cleanup.rs:104–108`, `lib.rs:46–50,2294`); none
interpolate transcript text. **Nit #2:** `record_client_event` mirrors every event
into both `tracing::info` and the report's UI-events section (`diagnostic_log.rs:106–114`),
so this is the one channel where a *future* `emit("ui-error", {message: <transcript>})`
would leak into an uploaded report. It's clean today purely by convention.
*Fix suggestion:* leave a comment on `record_client_event` / the `ui-error` emit
helper stating "never interpolate transcript/cleanup text into `message`."

### Check 3 — Log noise / level discipline ✅
No per-frame or per-segment `info` logging found in the transcript path; transcript
logs are one line per completed transcription. The 2 MB `MAX_LOG_TAIL_BYTES`
(`diagnostic_log.rs:18,216`) is comfortable for a normal session at this cadence.
No expected-control-flow path logs at `warn`/`error` spuriously (hallucination
rejection at `transcribe.rs:837` is `warn`, which is reasonable).

### Check 4 — Error-path surfacing ✅
- `submit_bug_report` credential-missing branch returns a clean user string and
  saves locally (`diagnostic_log.rs:402–407`); upload-failure branch likewise
  (`:417–423`). Telegram error body is truncated to 300 chars (`:375`).
- `chaperone-fallback` emits a clean toast and falls back to raw output
  (`cleanup.rs:103–110`). ✅
- `ui-error` / `transcript-error` / `paste-error` / `paste-miss` listeners all
  render toasts in `App.svelte` (`:1051,1085,1098,1091`). ✅

### Check 5 — Debug-only guard ✅
`init_transcript_writer` is invoked only within `#[cfg(debug_assertions)]`
(`lib.rs:1906–1922`). Release bundles cannot activate the transcript log.

### Nit #3 — local plaintext report
`submit_bug_report` always writes `turbotalk-bugreport-<id>.txt` to the logs folder
(`diagnostic_log.rs:394–396`), and `export_diagnostic_report` writes
`turbotalk-report-<id>.txt`. These contain the (transcript-free) report incl. the
reporter note and session log in plaintext on disk. Not a leak per the privacy
contract (nothing dictated, stays local), but worth knowing the file persists.

## Bottom line
No release blocker. Privacy contract holds end-to-end. The two nits are
hygiene/forward-safety, not active leaks.
