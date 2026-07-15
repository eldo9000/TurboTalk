# SPEC: Two-instance ASR (streaming no-boost, final with boost)

## Overview

Split TurboTalk's whisper transcription into two passes: a lightweight streaming
pass with no context/vocabulary boosting for live preview, and a final pass with
full vocabulary boosting for accuracy. The two pass system prevents custom 
vocabulary from interfering with the whisper decoder during real-time preview
while still applying corrections in the final output.

## Current TurboTalk pipeline

```
Recording buffer
  → Audio writeback + whisper-cli inference (single pass)
  → Transcription text → preview or paste
```

One shot, one model run. No streaming preview during recording — preview pills
appear only after whisper returns segments. No vocabulary boosting at all.

## Target TurboTalk pipeline

```
Recording buffer
  ├─ Streaming pass (no vocabulary boost)
  │    → live preview pills (words appear as whisper processes chunks)
  │
  └─ Final pass (with vocabulary boost)
       → corrected final text → cleanup → paste
```

Both passes use the same whisper model file (no second model download). The
streaming pass runs on the audio as it accumulates; the final pass runs on the
full buffer after recording ends.

## Why two passes (from FluidVoice)

FluidVoice's `FluidAudioProvider` creates two `AsrManager` instances:

1. **Streaming manager** — no vocabulary boosting, no custom pronunciation
   dictionary. Used during real-time preview. The comment in their code explains:
   "Avoids CTC/ANE contention that causes intermittent SIGTRAP crashes during
   streaming inference." Custom vocabulary injects extra tokens into the CTC
   decoder, which can destabilize real-time decoding on Apple Neural Engine.
   Even without the crash risk, custom words slow down streaming — and streaming
   needs to be fast / low-latency.

2. **Final manager** — with full vocabulary boosting and custom pronunciation
   dictionary. Used once, after recording ends. This pass has no real-time
   pressure — it can take 2-5x longer because the user sees a "Transcribing..."
   state, not live pills.

**Fallback**: If the boosted final pass fails (ANE contention, OOM, decoder
error), the un-boosted streaming result is used. The user always gets text.

**Memory note**: Both managers share the same underlying `MLModel` reference
types, so the only additional memory is decoder state (~100KB in their case,
marginal for whisper.cpp too).

## TurboTalk integration

### Whisper context/grammar API

whisper.cpp supports a "prompt" (initial text) and a "grammar" that constrains
output tokens. The vocabulary boost would be implemented as:

1. Build a custom grammar that gives higher likelihood to custom vocabulary words
   from the user's pronunciation dictionary / custom word list
2. Set the initial prompt: `[CONSIDER USER VOCABULARY: word1, word2, ...]`
3. This whisper.cpp feature is exposed via `--prompt` and `--grammar` flags in
   the `whisper-cli` binary, or directly in the bindings

If TurboTalk uses the CLI sidecar (`whisper-cli`):
- Streaming pass: `whisper-cli --file input.wav --no-prompt`
- Final pass: `whisper-cli --file input.wav --prompt "[CUSTOM: word1, word2]" --grammar custom.gbnf`

If TurboTalk uses the Rust bindings (whisper-rs):
- Streaming pass: call `whisper_full()` with `params: WhisperParams::default()`
- Final pass: call `whisper_full()` with `params.initial_prompt = custom_words_string`

### Vocabulary data source

The custom vocabulary comes from:
1. User's pronunciation dictionary (not yet implemented in TurboTalk — this is a
   prerequisite or can be a simple config file for now)
2. Settings: `custom_words: Vec<String>` in the config file
3. Future: learned corrections from the edit tracker

For v1, a simple static list: `~/.config/turbotalk/custom_vocabulary.txt`, one
word per line. The user edits this file manually. The Chaperone could also
suggest additions.

### Architecture changes

**New concept: `TranscriptionMode` enum**
```rust
pub enum TranscriptionMode {
    /// Fast, no vocabulary boosting. For live preview during recording.
    Streaming,
    /// Full accuracy with vocabulary boosting. For final output.
    Final,
}
```

**Modified `transcribe.rs`**:
```rust
pub fn transcribe(
    audio_path: &Path,
    mode: TranscriptionMode,
    vocabulary: &[String],
) -> Result<TranscriptionResult> {
    match mode {
        TranscriptionMode::Streaming => {
            // whisper-cli: no special flags
            // Returns partial segments as they're decoded
        }
        TranscriptionMode::Final => {
            // Build grammar/prompt from vocabulary
            // whisper-cli: --prompt / --grammar
            // Returns full transcription
        }
    }
}
```

If the final pass fails, fall back to the streaming result:
```rust
fn run_final_with_fallback(
    audio_path: &Path,
    vocabulary: &[String],
    streaming_result: &TranscriptionResult,
) -> TranscriptionResult {
    match transcribe(audio_path, Final, vocabulary) {
        Ok(result) => result,
        Err(e) => {
            log::warn!("Final boosted transcription failed: {e}. Using streaming result.");
            streaming_result.clone()
        }
    }
}
```

### Real-time preview changes

Currently, `recorder.rs` sends transcription segments to the frontend as
whisper produces them. With the two-pass system:

1. During recording: the streaming pass sends live preview pills as today
2. After recording ends: the final pass runs, and its output replaces the
   streaming preview text before cleanup/paste

The frontend already handles "preview updates" — the final pass just sends one
more update with the corrected text. The user sees: live pills → slight pause →
corrected text → paste.

### State machine update (`recorder.rs`)

Current states: `Ready → Recording → FinalizingAudio → Transcribing → Cleaning → Pasting`

The `Transcribing` state needs to reflect two sub-stages:
- `Transcribing(Streaming)` — during recording, preview segments arrive
- `Transcribing(Final)` — after recording, the boosted pass runs

Or simpler: keep the state machine as-is. The streaming pass runs during
`Recording` (not `Transcribing`). `Transcribing` always runs the final pass,
with fallback.

```
Recording: audio capture + streaming ASR → live preview pills
  → key-up → FinalizingAudio
  → Transcribing: final ASR with boost (fallback to streaming result)
  → Cleaning → Pasting
```

## Custom vocabulary management

### File format (`~/.config/turbotalk/custom_vocabulary.txt`)

```
# Custom vocabulary — one word or phrase per line
# These words get boosted during the final ASR pass
Aptible
Hackerman
TurboTalk
Neovim
LSP
GBNF
Despecialize
```

### Settings struct

```rust
pub struct VocabularySettings {
    pub enabled: bool,           // master toggle, default: false
    pub file_path: PathBuf,      // default: ~/.config/turbotalk/custom_vocabulary.txt
    pub auto_learn: bool,        // future: Chaperone suggests additions
}
```

### Whisper grammar file generation

If using `--grammar`: generate a GBNF grammar file that boosts custom words.

```
# custom.gbnf — generated from custom_vocabulary.txt
root ::= word+
word ::= [a-zA-Z] [a-zA-Z'-]*
# Boost rules: custom words get higher weight
# This is whisper.cpp grammar syntax — may need adaptation
```

If using `--prompt` only: simpler, no grammar needed. Just set the initial
prompt context. whisper.cpp's `initial_prompt` mechanism naturally biases
decoding toward the provided tokens.

## Implementation order

1. Add `TranscriptionMode` enum and `custom_vocabulary` to settings
2. Modify `transcribe.rs` to accept mode parameter and vocabulary
3. Add fallback logic: final → streaming result on failure
4. Wire streaming pass into the `Recording` state for live preview
5. Wire final pass into `Transcribing` state
6. Add `custom_vocabulary.txt` file loading in settings
7. Frontend: settings toggle for vocabulary boost + manual word list editor
8. Update SESSION-STATUS.md

## Out of scope

- Actually implementing a pronunciation dictionary (that's a separate feature)
- Auto-learning vocabulary from Chaperone corrections (future)
- Grammar file format research for whisper.cpp (initial prompt may be enough)
- Any changes to the audio capture pipeline
- Any changes to the cleanup/Chaperone pipeline

## Success signal

- Streaming pass produces live preview pills during recording (existing behavior)
- Final pass runs after recording ends with vocabulary boosting
- Words in `custom_vocabulary.txt` appear in the final transcription when the
  model would normally get them wrong
- If the final pass fails, the streaming result is used as fallback
- The user sees: live pills → brief pause → corrected text → paste
- `cargo check && cargo clippy` pass
- `npm run typecheck` pass
