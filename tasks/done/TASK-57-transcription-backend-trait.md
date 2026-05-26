# TASK-57: TranscriptionBackend trait abstraction (no behavior change)

## Goal
The transcription pipeline runs through a `TranscriptionBackend` trait. The existing whisper.cpp implementation is one concrete impl (`WhisperBackend`) behind the trait. The end-to-end dictation loop behaves identically to before — same anti-hallucination flags, same model swap behavior, same prewarm. This refactor must land green before adding any new backend.

## Context
TurboTalk is a personal-use macOS push-to-talk dictation utility. Tier 1 product. Read `CLAUDE.md` at repo root and `~/Downloads/Github/Business-OS/standards/ENGINEERING.md` before starting.

Today, all transcription logic lives in `src-tauri/src/transcribe.rs` as a single concrete type `TranscriptionWorker`. Phase 3 of the hallucination plan adds two more backends (Moonshine, Parakeet via the `transcribe-rs` crate). To keep that diff small and reviewable, this task introduces a trait first and refactors the existing whisper.cpp code behind it, with **no functional change**.

The trait must accommodate the existing behavior shape:
- Lifecycle: built from a settings snapshot, owns a long-lived resource (subprocess/loaded model), can be invalidated and rebuilt on model swap
- Transcribe: takes a path to a WAV file, returns the raw transcript text
- Abort: kill in-flight work (for cancel-during-transcribe)
- Streaming segments: the `SegmentTranscriber` queue at `src-tauri/src/transcribe.rs:824` calls `run_raw` — it must still work, ideally through the trait too

The anti-hallucination flags at `src-tauri/src/transcribe.rs:472-487` are non-negotiable and must continue to apply on the Whisper code path. The post-hoc detection added in TASK-55 and the VAD pre-filter added in TASK-56 (assuming both have landed first) must continue to apply.

The process-wide `WORKER` static at `src-tauri/src/transcribe.rs:551` needs to become a `Mutex<Option<Arc<dyn TranscriptionBackend>>>` or equivalent (`Box<dyn …>` won't work with the existing `Arc`-clone pattern; `Arc<dyn …>` is correct).

## In scope
- `src-tauri/src/transcribe.rs` — extract trait, refactor `TranscriptionWorker` into `WhisperBackend: TranscriptionBackend`
- `src-tauri/src/lib.rs` and any other call sites that touch `TranscriptionWorker` or `run_raw` directly — update to the trait surface
- Tests in `src-tauri/src/transcribe.rs` — keep all existing tests passing; rename if the concrete type they reference is renamed
- `SESSION-STATUS.md` — one-line refactor note
- (no `TRUTH.md` update needed — behavior unchanged)

## Out of scope
- Any new backend implementation — that is TASK-58 (Moonshine) and TASK-59 (Parakeet)
- Settings UI for backend selection — TASK-60
- Removing or modifying any anti-hallucination flag
- Modifying the post-hoc detection from TASK-55
- Modifying the VAD logic from TASK-56
- Performance changes — the trait may add one `dyn` indirection but must not measurably affect latency

## Steps
1. Read `CLAUDE.md`, `~/Downloads/Github/Business-OS/standards/ENGINEERING.md`, `SESSION-STATUS.md`, `TRUTH.md`, `src-tauri/src/transcribe.rs` end-to-end.
2. Grep for every direct reference to `TranscriptionWorker`, `run_raw`, `WORKER`, `worker_for`, `prewarm`, `invalidate_worker`, `abort_active`, `is_ready`, `prewarm_failed`, `kill_orphans` outside of `transcribe.rs`. Build a list of caller sites.
3. Draft the trait. Suggested shape:
   ```
   pub trait TranscriptionBackend: Send + Sync {
       fn transcribe(&self, wav: &Path) -> anyhow::Result<String>;
       fn abort(&self);
       fn model_identity(&self) -> String;  // for cache-validity comparison
   }
   ```
   Plus a free function `build_backend(cfg) -> anyhow::Result<Arc<dyn TranscriptionBackend>>` that today only returns the Whisper impl.
4. Refactor `TranscriptionWorker` → `WhisperBackend`. Keep ALL existing fields. Implement `TranscriptionBackend` for it. `model_identity()` returns the canonicalized model path as a string.
5. Update `WORKER` static type to `Mutex<Option<Arc<dyn TranscriptionBackend>>>`.
6. Update `worker_for`, `run_raw`, `prewarm`, `invalidate_worker`, `abort_active` to operate on the trait object. The cache-validity check now compares `model_identity()` strings rather than `&Path`s.
7. The `SegmentTranscriber` path in `transcribe.rs` continues to call `run_raw` — confirm it still compiles and behaves.
8. Run `cargo test`. All existing tests must pass without behavioral edits. If a test references `TranscriptionWorker` by name, rename in-place — that's the only acceptable change.
9. Run `cargo clippy -- -D warnings` if the project uses clippy gates. Resolve any new lints from the indirection.
10. Run `npm run tauri dev`. Hold PTT, say "hello world", release. Confirm the transcript pastes into the focused app exactly as it did before this refactor. Hold PTT, dictate 5s of silence — confirm the TASK-55 filter still fires (transcript shown in window with "⚠ filtered" tag, no paste). Toggle VAD off in Settings, dictate the silence+speech test from TASK-56 — confirm VAD toggle still has the expected effect.
11. Update `SESSION-STATUS.md`: one line noting the trait refactor with no behavior change.
12. Commit with `refactor(transcribe): extract TranscriptionBackend trait, Whisper as one impl`.

## Success signal
- `cargo test` exits 0. Every existing test still passes.
- End-to-end dictation works: "hello world" PTT → "hello world" pasted into focused app.
- TASK-55 hallucination filter still fires on silent recording (proves the rejection path still runs).
- TASK-56 VAD toggle still has visible effect when flipped (proves the spawn args still wire through).
- `grep -r "TranscriptionWorker" src-tauri/src/` returns either no hits or only the renamed `WhisperBackend` and a deprecated alias if you chose to keep one.

## Notes
- `Arc<dyn Trait>` is the right cache shape because the spawn lock + abort path want to drop the outer mutex before the long HTTP POST.
- Keep the trait surface minimal. Anything Whisper-specific (audio_ctx tuning, vocabulary, the `READY`/`PREWARM_FAILED`/`PREWARM_IN_FLIGHT` atomics) stays inside `WhisperBackend`. The atomics describe global readiness state; they can either stay as module-level state in `transcribe.rs` for now (simplest) or move behind a trait method later. Don't generalize the atomics yet — wait until TASK-58 reveals what Moonshine actually needs.
- If the refactor introduces any subtle change to model-swap cache invalidation, you've broken the contract. Compare `model_identity()` strings rather than paths to avoid this — same string in same string out = same backend.
- This is a foundation task. The proof for "done" is that nothing changed except the type layout.
