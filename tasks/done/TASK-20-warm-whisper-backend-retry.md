# TASK-20: Warm Whisper backend retry — branch through three options, land the cheapest one that works

## Goal
Repeated dictations within one app session no longer pay full Whisper process+model startup cost — OR this task formally re-defers TASK-18 with new evidence (a different upstream blocker, or a measured "warmup not worth it on this hardware"). The choice between options is made by the worker based on what actually builds and runs on this host.

## Context
TurboTalk is a personal-use macOS voice dictation app. The transcribe path in `src-tauri/src/transcribe.rs` currently spawns `whisper-cli` once per recording. Per-recording startup pays a model load + Metal context init cost that compounds across back-to-back dictations.

TASK-18 attempted `whisper-rs` 0.16 with the `metal` feature twice on this host (2026-05-02). Both attempts hung in `whisper-rs-sys`'s build script: `cmake -Wdev --debug-output ... -DCMAKE_VERBOSE_MAKEFILE:BOOL=ON` spawns a `TryCompile` probe (`cmTC_*`) which runs but never terminates — wall time 11+ min, ~6 min CPU, no `cargo check` progress. Confirmed reproducible after disk space was freed (28+ GiB available on retry). The verbose/debug cmake flags are baked into `whisper-rs-sys`'s build.rs; cannot be suppressed without forking. The dependency was reverted; the existing `whisper-cli` per-recording path remains in place. Full deferral evidence is in `tasks/done/TASK-18-persistent-whisper-transcription-worker.md` (lines 89–117).

This task is the retry. It branches through three options in priority order (cheapest first) and lands the first one that succeeds. If none succeed, it formally re-defers TASK-18 with the new evidence appended to that archived file.

Already in place from prior tasks:
- Stage timings — `audio.rs::stop()` emits a `[audio] stage timings (ms): capture_clone=… downmix=… resample=… vad=… normalize=… wav_write=… total=…` line (TASK-13).
- Job lifecycle — `Recorder` enforces one job at a time; states are `Ready / Recording / FinalizingAudio / Transcribing / Cleaning / Pasting` (TASK-14).
- Stage separation — raw transcription is its own stage with a `job_id`; `dictation-stage` events are emitted for each stage (TASK-15).
- Path validation — `is_allowed_whisper_path` and `validate_model_path` already gate any subprocess invocation (TASK-2).
- Bundled sidecar — `src-tauri/binaries/whisper-cli` is in the app bundle and `tauri.conf.json` lists it as `externalBin`.

## In scope
- `src-tauri/src/transcribe.rs` — wherever the actual whisper invocation lives
- `src-tauri/Cargo.toml` — adding/removing dependencies as the chosen branch requires
- `src-tauri/src/lib.rs` — only as needed to manage TranscriptionWorker lifecycle (init at setup, drop on app exit, invalidate on model change)
- `src-tauri/src/settings.rs` — only the model-change hook needed to invalidate the warm worker; no schema changes
- `src-tauri/binaries/` — bundling a second binary if option 2 (whisper-server) is chosen
- `src-tauri/tauri.conf.json` — adding the new binary to `bundle.externalBin` if option 2 is chosen
- `tasks/done/TASK-18-persistent-whisper-transcription-worker.md` — append a "Retry 2026-05-02 (or later)" note documenting which option was tried and the outcome

## Out of scope
- Audio file format changes (WAV stays 16 kHz mono 16-bit)
- Network transcription — local-only, period
- Multi-job queueing or parallelism across recordings (TASK-14's one-in-flight invariant is non-negotiable)
- Cleanup module changes (TASK-4)
- Path-validation loosening — the allowlist stays exactly as-is
- Re-attempting `whisper-rs` if option 1's gate (step 2 below) shows the cmake hang is still reproducible
- Streaming audio finalizer (TASK-19/TASK-21 — separate task, gated on the evidence this task produces)

## Implementation options (try in order)

**Option 1 — `whisper-rs` (re-test the original block)**
- Best long-term: model loaded in-process, no IPC overhead, full warmup benefit.
- Gate: a `cargo check` of a temporary minimal crate adding `whisper-rs = "0.16"` (with `metal` feature) must complete in under 5 minutes. If it hangs at the `cmTC_*` probe like before, abandon and move to option 2.
- Risk: the cmake hang is environmental (macOS 26.4.1 + cmake interaction). Unlikely to have resolved in 24 hours, but cheap to verify.

**Option 2 — `whisper-server` long-lived sidecar**
- Acceptable if `whisper-server` is available alongside the bundled `whisper-cli`. whisper.cpp ships both binaries from the same build; check `src-tauri/binaries/` first, then check the upstream release tarball.
- Architecture: spawn `whisper-server --port <localport> -m <model>` once, keep it alive for the app's lifetime, transcribe by HTTP-POST'ing the WAV to `http://127.0.0.1:<localport>/inference`. Reuse the localhost-allowlist guard from `cleanup.rs::validate_ollama_url` (it's literally the same threat model — pull the helper out into a shared module if needed).
- Tradeoff: HTTP roundtrip per recording (~1ms localhost), but the model stays warm across recordings. No cmake build; bundles a binary, not Rust source.

**Option 3 — Serialized worker around `whisper-cli`**
- Lifecycle ownership only; no model warmup. Original task notes call this "least benefit because it still spawns per recording."
- Useful only if it cleans up some other state (avoiding race conditions, centralizing path validation, etc.). Do not claim it solved warmup.
- Lowest risk: no new dependencies, just a refactor.

## Decision rule

Run options in order. Land the **first** option whose gate passes. If all three are blocked or rejected, this task ends in re-deferral, not failure.

| Option | Gate to pass | If gate passes | If gate fails |
|--------|--------------|----------------|---------------|
| 1 | `cargo check` on a probe crate with `whisper-rs = "0.16"` (metal feature) finishes in < 5 min | Implement TranscriptionWorker around `whisper-rs::WhisperContext` | Move to option 2 |
| 2 | `whisper-server` binary exists in the upstream whisper.cpp release for the version we already bundle, and it accepts `-m model.bin --port N` | Bundle the binary, implement HTTP-based TranscriptionWorker with a managed sidecar process | Move to option 3 |
| 3 | (no external gate — always implementable) | Refactor transcribe.rs into a serialized worker that owns the path-validation + spawn lifecycle, even though it spawns per-recording | (n/a — last option) |

If option 3 is what lands, **also** record a note in `SESSION-STATUS.md` that warmup is still pending; this is acceptable but suboptimal.

## Steps
1. Read `src-tauri/src/transcribe.rs` and `tasks/done/TASK-18-persistent-whisper-transcription-worker.md` end-to-end. Confirm the current invocation shape and the prior deferral evidence.
2. **Option 1 gate.** Create a temp crate at `/tmp/whisper-rs-probe/` with a minimal `Cargo.toml` declaring `whisper-rs = { version = "0.16", features = ["metal"] }` and an empty `lib.rs`. Run `timeout 300 cargo check --manifest-path /tmp/whisper-rs-probe/Cargo.toml 2>&1 | tail -30`. If it completes, proceed with option 1; if it hangs/times out at `cmTC_*` (the symptom from the prior deferral), record one line of evidence and proceed to option 2.
3. **Option 1 implementation (only if gate passed).** Add `whisper-rs = { version = "0.16", features = ["metal"] }` to `src-tauri/Cargo.toml`. Implement `TranscriptionWorker` in a new module or inside `transcribe.rs`:
   - `pub struct TranscriptionWorker { ctx: WhisperContext, model_path: PathBuf, prompt: Option<String> }`
   - Methods: `new(model_path)`, `transcribe(&self, wav_path) -> Result<String>`, `model_path() -> &Path`
   - Wrap in `Arc<Mutex<Option<TranscriptionWorker>>>` managed in `lib.rs::run` setup.
   - On settings change in `save_config`, if `cfg.whisper.model` differs from the worker's model, drop and reinit the worker.
   - On worker init failure, set the worker to `None` and fall back to the existing `whisper-cli` path. Log at `warn!`.
   - Skip to step 6.
4. **Option 2 gate.** Check `ls src-tauri/binaries/` for `whisper-server`. If absent, look at how `whisper-cli` is provisioned (likely a build script or a manual download); the same source should provide `whisper-server`. If neither the bundled tree nor a fetchable upstream release contains a working `whisper-server` for the same whisper.cpp version, proceed to option 3.
5. **Option 2 implementation (only if gate passed).** Bundle `whisper-server` alongside `whisper-cli` (update `tauri.conf.json` `bundle.externalBin` and the path-allowlist in `transcribe.rs::is_allowed_whisper_path`). Implement `TranscriptionWorker`:
   - On init: pick a free localhost port (try 18527 then increment), spawn `whisper-server -m <validated_model_path> --port <port> --host 127.0.0.1`, wait for readiness (poll `/` or `/healthz` for ~3s).
   - `transcribe(wav_path)`: POST the WAV bytes to `http://127.0.0.1:<port>/inference` with the same `--prompt`/`--no-context`/`--temperature 0` semantics encoded as form fields per whisper-server's API. Reuse `cleanup.rs::validate_ollama_url`-style host check on the constructed URL (move that helper to a shared `net.rs` module if needed; do not duplicate logic).
   - Lifecycle: kill the sidecar process on app shutdown via Tauri's `on_window_event` or a `Drop` impl on the worker.
   - On worker init failure, set the worker to `None` and fall back to the existing `whisper-cli` path. Log at `warn!`.
6. **Option 3 implementation (only if options 1 and 2 are blocked).** Refactor `transcribe.rs` so the spawn logic lives behind a `TranscriptionWorker` type — same shape as options 1/2 but the implementation still runs `whisper-cli` per call. The worker owns path validation and config; the call site in `hotkey.rs` (or wherever TASK-15 left it) goes through the worker rather than calling `transcribe::run` directly. Document explicitly in code comments that this option does NOT keep the model warm.
7. **Common to all options.** Preserve:
   - The exact path-validation semantics from TASK-2 (`is_allowed_whisper_path`, `validate_model_path`).
   - The vocabulary prompt + tuned flags from TASK-12 (`--no-context`, `beam=5`, `temp=0`, `--suppress-blank`, `--prompt <vocab>`).
   - The one-in-flight invariant from TASK-14 — a Mutex on the worker is sufficient.
   - The `dictation-stage` events from TASK-15.
   - The `transcript-error` event shape on failure.
8. **Tests.** Run `cd src-tauri && cargo test` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`. Pre-existing tray.rs warnings are acceptable; new code must be clippy-clean. Add at minimum:
   - A unit test that constructing the worker with an invalid model path returns Err (without spawning anything).
   - A unit test that the worker's URL/path construction respects the loopback allowlist (option 2 only).
9. **Manual verification — defer to user.** Note in your return that the manual proofs (first-recording latency, repeated-recording latency, model-switch invalidation) require a real microphone and cannot be done by an agent. List the exact terminal-log lines the user should look for to confirm warmup happened.
10. **Append outcome** to `tasks/done/TASK-18-persistent-whisper-transcription-worker.md` under a new heading `## Retry $(date +%Y-%m-%d) — outcome: <option N | re-deferred>` with one paragraph of evidence (which gate passed/failed, what was implemented, what residual risk remains).

## Success signal
- `cargo build`, `cargo test`, and `cargo clippy -D warnings` all green for `src-tauri`.
- The chosen option is identifiable by either (a) a new dependency in `Cargo.toml`, (b) a new entry in `tauri.conf.json` `bundle.externalBin`, or (c) a `TranscriptionWorker` type in `transcribe.rs` whose method bodies still call `whisper-cli` but go through the same struct.
- A `TranscriptionWorker` (or equivalent) struct exists; the call site no longer directly calls `transcribe::run` from outside the module.
- TASK-18's archive file has a `## Retry …` section appended.
- If options 1 and 2 are both blocked: `SESSION-STATUS.md` notes that warmup is still pending and option 3 was the chosen lifecycle fix.

## Notes
- **Do not fork `whisper-rs-sys`** to suppress the cmake flags. That's a yak-shave the prior deferral note explicitly warns against.
- **Loopback allowlist applies to option 2.** Treat `whisper-server` exactly like Ollama: only `127.0.0.1` / `::1` / `localhost`. Reuse `cleanup.rs`'s `validate_ollama_url` helper (or extract a shared `validate_loopback_url`).
- **Drop semantics matter for option 2.** If the sidecar process leaks across app restarts, that's worse than no warm worker. Use `Child::kill()` in a Drop impl plus a setup-time check for stale processes on the chosen port.
- **Do not change cleanup behavior.** The cleanup module already runs after raw transcription per TASK-15. Whatever this task does, it returns the same raw text shape to the same caller.
- **Option 3 is acceptable.** The original task notes were skeptical of it; the deferral note explicitly says option 3 is acceptable as a lifecycle fix without warmup. If options 1 and 2 are blocked, do not refuse to ship option 3 — it still cleans up the spawn path even without warmup.
- Multi-agent review reference: original task at `tasks/done/TASK-18-persistent-whisper-transcription-worker.md`; deferral evidence at lines 89–117.
