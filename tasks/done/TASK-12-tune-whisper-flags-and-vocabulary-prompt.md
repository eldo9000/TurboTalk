# TASK-12: Tune whisper-cli flags and wire user vocabulary into `--prompt`

## Goal
The `whisper-cli` invocation in `src-tauri/src/transcribe.rs::run` uses flags appropriate for short-form **dictation** (not long-form transcription): `--no-context`, `--beam-size 5`, `--temperature 0`, `--suppress-blank`. The existing `cleanup.vocabulary` config field — already user-editable in the Modes tab — is joined with commas and passed as `--prompt` so user-specific names, jargon, and code identifiers are spelled correctly.

## Context
TurboTalk is a personal-use macOS dictation app. Today, `transcribe.rs::run` invokes `whisper-cli` with minimal flags:
```rust
.args(["-m", model_str, "-f", wav.to_str().unwrap(), "-otxt", "-np", "-nt", "-l", "en"])
```
Whisper's defaults are tuned for long-form transcription where cross-segment context helps. For short PTT dictation utterances, the defaults under-search the beam and condition on previous-segment context that often doesn't exist (or that contains stale user errors). Tuning the flags is pure config — no new dependencies, no audio pipeline changes.

The `cleanup.vocabulary: Vec<String>` field already exists in `src-tauri/src/settings.rs::CleanupConfig`. It's currently used only by the Chaperone classifier. The same words also belong in whisper's `--prompt` flag — this is the standard mechanism for biasing the language model toward correct spellings of named entities. Reuse the existing field; do not add a duplicate setting.

Reference: cjpais/Handy exposes a "custom words" setting that does exactly this — see `/tmp/handy-ref/src-tauri/src/managers/transcription.rs:546`.

## In scope
- `src-tauri/src/transcribe.rs::run` — build args dynamically, add the new flags, append `--prompt <vocabulary>` when the vocabulary is non-empty.

## Out of scope
- Adding a separate "whisper vocabulary" UI field — reuse `cleanup.vocabulary`.
- Changing the `-l en` (language) flag — the .en models are English-locked anyway.
- Streaming or partial transcription — still write WAV → run whisper → read .txt.
- Any change to `audio.rs`, `recorder.rs`, or the frontend.
- Making the new flags user-configurable. Pick sensible defaults, hard-code them. We can expose them later if there's a real need.

## Dependencies
- Independent of TASK-9, TASK-10, TASK-11. Touches a different file (`transcribe.rs`) and can run in parallel.

## Steps
1. Read `src-tauri/src/transcribe.rs::run` end-to-end (lines ~109–151). Note the current `Command::new(&bin).args([...]).output()?` call and how `output.stderr` is surfaced on failure.
2. Read `src-tauri/src/settings.rs::CleanupConfig` to confirm `vocabulary: Vec<String>` exists. The config is already loaded inside `run` (`let cfg = crate::settings::load();` near the top of the function).
3. Replace the static `.args([...])` array with a `Vec<String>` built dynamically:
   ```rust
   let mut args: Vec<String> = vec![
       "-m".into(), model_str.to_string(),
       "-f".into(), wav.to_str().unwrap().to_string(),
       "-otxt".into(),
       "-np".into(),
       "-nt".into(),
       "-l".into(), "en".into(),
       "--no-context".into(),
       "--beam-size".into(), "5".into(),
       "--temperature".into(), "0".into(),
       "--suppress-blank".into(),
   ];
   if !cfg.cleanup.vocabulary.is_empty() {
       args.push("--prompt".into());
       args.push(cfg.cleanup.vocabulary.join(", "));
   }
   ```
4. Update the `Command::new(&bin)` call: `.args(&args)` works directly with `Vec<String>` (Command accepts `IntoIterator<Item=AsRef<OsStr>>`).
5. Run `cargo build --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`. Both must exit 0.
6. Run `cargo test --manifest-path src-tauri/Cargo.toml`. Existing tests in `transcribe.rs` should still pass; if any test asserts on the exact arg list, update it to match the new flags.
7. Manually test:
    - **Smoke test:** open the app, PTT a normal sentence, verify a transcript appears and is reasonably accurate. (No regression from default-flag behavior.)
    - **Vocabulary test:**
      - Open the Modes tab, set Post-processing to anything (Off is fine — the vocabulary field is still editable).
      - In "Custom vocabulary," add `TurboTalk`, `Tauri`, `Svelte` (one per line), Save.
      - PTT and say "I'm working on TurboTalk in Tauri with Svelte today."
      - Confirm the brand names are spelled correctly. Without the prompt they often render as "turbo talk", "tory", or "svelt". With the prompt they should preserve casing.
    - **Empty-vocab test:** Clear the vocabulary, save, and re-test the smoke transcript. Confirm the `--prompt` flag is NOT added (you can verify via `tracing::debug!("[transcribe] args: {:?}", args)` if you want to log it). The transcript should match the smoke test from step 7.1.

## Success signal
- The smoke test produces a transcript at least as accurate as the previous version.
- With `TurboTalk` in the vocabulary, "turbotalk" or "turbo talk" is no longer produced; the casing is preserved as `TurboTalk` (allowing for whisper's own punctuation/capitalization quirks).
- With an empty vocabulary, no `--prompt` flag is passed (verifiable by adding a temporary debug log of the final `args` vector).
- `cargo build`, `cargo clippy -- -D warnings`, `cargo test` exit 0.

## Notes
- `--prompt` and `--initial-prompt` are aliases in whisper.cpp; either works. Use `--prompt`.
- Whisper's prompt token budget is ~224 tokens (~150 words). At small vocabulary sizes this is a non-issue. If a future user pushes past it, whisper truncates silently — no error to handle.
- Beam-size 5 is a moderate bump from default 1. Going higher (8, 10) buys diminishing returns for proportional CPU cost. Don't overshoot.
- `--temperature 0` makes decoding deterministic. whisper.cpp falls back to higher temperatures internally on no-speech / decoding-failure conditions, so setting 0 explicitly does not lock us out of fallback recovery.
- `--suppress-blank` reduces silent-frame hallucinations. Pairs well with the VAD work in TASK-11 but is helpful even without it.
- `--no-context` is critical for dictation. Without it, whisper conditions on the previous utterance's text — which is fine for one continuous recording but actively harmful for separate dictation events spaced minutes apart.
- Don't add `--print-progress` or anything that changes stdout/stderr formatting. The `.txt` output file is what we read.
- If a future task wants to expose the prompt as a separate field (independent of `cleanup.vocabulary`), that's a config-schema change — not in scope here.
