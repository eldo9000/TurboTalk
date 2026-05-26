# TASK-55: Whisper hallucination detection filter

## Goal
Bad Whisper transcriptions (repetition loops, all-zeros, "thanks for watching"-style language-model artifacts on silence) are detected before paste. The user sees the rejected transcript in the TurboTalk window tagged "⚠ filtered" and a toast explaining why, but the clipboard and the focused app are untouched.

## Context
TurboTalk is a personal-use macOS push-to-talk dictation utility. Tier 1 product — minimal ceremony, but `SESSION-STATUS.md` and `TRUTH.md` must reflect behavior changes. Read `CLAUDE.md` at repo root and the engineering standards at `~/Downloads/Github/Business-OS/standards/ENGINEERING.md` before starting.

Whisper has a well-known architectural failure mode: when fed silence or low-SNR audio, the autoregressive decoder's language-model prior overwhelms the thin acoustic signal and emits hallucinated text — usually repetition loops ("the the the the"), garbage characters ("0000000"), or training-set artifacts ("thanks for watching", "subscribe to the channel"). Current anti-hallucination flags in `src-tauri/src/transcribe.rs:472-487` (temperature=0.0, temperature_inc=0.0, suppress_nst=true, no_context=true) reduce but do not eliminate this.

This task adds a post-hoc detection layer on the raw transcript: if the output trips any of three filters, it is flagged as garbage, displayed in the UI with a warning tag, and **not pasted**. The user can confirm whether the filter caught a real false-positive or ate a legit transcription.

The three detection signals:
1. **Compression ratio** — `gzip(text).len() / text.len()`. Highly repetitive text compresses extremely well. Threshold tunable; start ~0.35 (text compressing to <35% of original = suspect).
2. **Trigram repetition** — same three-word sequence appearing more than 3 times.
3. **Non-letter ratio** — fraction of characters that are not letters, digits, spaces, or common punctuation. Above ~0.30 = junk.

Constants live at the top of the module and are clearly labeled as tunable.

Existing toast precedent: `chaperone-fallback` event emitted in `src-tauri/src/cleanup.rs` → consumed in `src/` (search for `chaperone-fallback` to find the handler).

## In scope
- `src-tauri/src/transcribe.rs` — add `detect_garbage()` and `RejectReason` enum, call it after `strip_trailing_filler` in `TranscriptionWorker::transcribe()`
- `src-tauri/src/lib.rs` or wherever the recorder pipeline assembles the transcript — emit `transcription-rejected` event with payload `{ text, reason }`, skip the paste call
- `src/` — frontend handler for `transcription-rejected`: show the toast AND display the transcript in the main window with a "⚠ filtered" tag
- Unit tests for `detect_garbage` covering: "0000000", "thanks for watching " × 5, "the the the the the", a clean sentence (must NOT trigger), an empty string (must NOT trigger)
- `SESSION-STATUS.md` — one-line update
- `TRUTH.md` — one-line update under the existing "What works end-to-end" section

## Out of scope
- Any change to whisper-server spawn args (Phase 2)
- Any new backend (Phase 3)
- Tuning thresholds against a large real-world corpus — pick reasonable starting values and document them; revisit only if Phase 1 misfires
- Renaming or refactoring the existing anti-hallucination flags
- Removing trailing filler stripping (`strip_trailing_filler`) — keep it, run detection on the already-stripped text

## Steps
1. Read `CLAUDE.md`, `~/Downloads/Github/Business-OS/standards/ENGINEERING.md`, `SESSION-STATUS.md`, `TRUTH.md`.
2. Read `src-tauri/src/transcribe.rs` end-to-end, paying attention to `TranscriptionWorker::transcribe()` and `strip_trailing_filler`.
3. Search for `chaperone-fallback` to find the existing toast wiring on both sides.
4. Add a `flate2` (or similar) dependency for gzip if not already present; check `Cargo.toml` first — `reqwest` likely brings it transitively, but the public surface may not expose it.
5. Add `RejectReason` enum + `detect_garbage()` function at module scope in `transcribe.rs`. Constants for the three thresholds at the top of the module.
6. In `TranscriptionWorker::transcribe()`, after `strip_trailing_filler`, call `detect_garbage`. If `Some`, return a new variant or a result type that signals rejection — but keep the raw text accessible to the caller.
7. Decide the cleanest signal-up shape: either return `Result<TranscriptOutcome, Error>` where `TranscriptOutcome { text, rejection: Option<RejectReason> }`, or keep returning `String` and have a sibling method. The first option is cleaner. Whichever you pick, update the recorder pipeline accordingly.
8. In the recorder/lib.rs paste pipeline, branch on rejection: emit `transcription-rejected` with payload `{ text, reason }`, skip `paste::paste_active_app` (or whatever the paste call is).
9. Frontend: add handler for `transcription-rejected`. Display the transcript in the main TurboTalk window with a `⚠ filtered` badge. Toast with the reason.
10. Write unit tests for `detect_garbage`. The clean-sentence case ("Hello world, this is a normal dictation.") must return `None`. Each garbage case must return the expected variant.
11. Run `cargo test` — all tests pass.
12. Run `npm run tauri dev`, hold PTT through ~5s of silence, release. Confirm: (a) main window shows the hallucinated text with "⚠ filtered" badge, (b) toast appears with the reason, (c) clipboard contents are unchanged (paste into TextEdit before and after — text doesn't change).
13. Confirm a normal dictation ("hello world, this is a test") still pastes correctly and shows no filter badge.
14. Update `SESSION-STATUS.md` and `TRUTH.md` with one-line entries.
15. Commit with `feat(transcribe): detect and suppress hallucinated transcripts`.

## Success signal
Concrete observation, not "it compiles":
- Dictate 5s of silence with PTT held. TurboTalk window shows the hallucinated phrase (e.g. "thanks for watching") with a visible "⚠ filtered" tag. A toast appears explaining the reason. The system clipboard, verified by pasting into an open TextEdit window, has NOT changed.
- Dictate "hello world, this is a normal test" with PTT. The text pastes into the focused app. No filter badge. No toast.
- `cargo test` exits 0. All new unit tests pass.

## Notes
- Pick thresholds conservatively at first — false negatives (missed hallucination) are better than false positives (legit dictation eaten) for the first cut. The user can tighten later.
- The transcript-in-window-with-badge is intentionally a debug surface. Don't over-design it; a small inline label is enough.
- If `flate2` introduces a meaningful compile-time cost on the first build, consider a hand-rolled compressibility heuristic (e.g. unique-bigram count / total bigrams). gzip is simpler and the cost is negligible at runtime.
