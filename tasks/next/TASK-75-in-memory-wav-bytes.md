# TASK-75: In-memory WAV bytes for segment transcription (eliminate temp-file round-trip)

## Goal
Stop writing segment audio to a temp WAV file and re-reading it for HTTP multipart upload. Build the WAV bytes in memory and send them directly as multipart body bytes.

## Context
The streaming transcription path writes each segment's `Vec<f32>` samples to a temp WAV file on disk, then the whisper-server HTTP client re-opens and re-reads that file for the multipart upload. Two call sites:

1. `src-tauri/src/transcribe.rs:1332-1336` — `write_segment_wav()` writes `seg.samples` to `turbotalk-seg-{i}.wav` on disk.
2. `src-tauri/src/transcribe.rs:909-910` — `WhisperBackend::transcribe()` calls `multipart::Form::file("file", wav_path)` which re-opens and re-reads the same file.

For a dictation with N silence-boundary segments, this is N extra disk write + disk read round-trips. The samples are already in memory (`SegmentEmit.samples: Vec<f32>`). The disk I/O is pure overhead.

The same pattern exists for the tail WAV and the batch-fallback path. The batch path writes the full recording to a WAV file (`audio.rs` write_wav), but that's a different concern — the segment path is the one where the data is already in memory and the disk round-trip is unnecessary.

`hound` is the WAV library (already a dependency). `hound::WavWriter::new()` accepts any `io::Write` sink, including `std::io::Cursor::new(Vec::new())`. The resulting bytes can be sent via `reqwest::blocking::multipart::Part::bytes_with_fname(bytes, "file.wav")`.

## In scope
- `src-tauri/src/transcribe.rs` — `write_segment_wav()`, `WhisperBackend::transcribe()`, and any other call site that writes samples to disk then re-reads for upload
- `SESSION-STATUS.md`

## Out of scope
- The batch-fallback path in `audio.rs` that writes the full recording to a WAV file (that path receives a file path from the recorder, not in-memory samples — it's a different data flow)
- Changing the whisper-server API contract (it still expects a multipart file upload)
- The Parakeet backend (it reads WAV samples via `transcribe_rs::audio::read_wav_samples` from a file path — changing that to in-memory would require changes to the `transcribe-rs` crate API, which is out of scope)
- The `detect_garbage` function (TASK-72 / separate concern)

## Steps
1. Read `src-tauri/src/transcribe.rs` around lines 1320-1340 (`write_segment_wav`) and 890-920 (`WhisperBackend::transcribe`) to understand the current flow: samples → WAV file on disk → `multipart::Form::file(path)` → HTTP POST.
2. Read the `SegmentEmit` struct to confirm `samples: Vec<f32>` is the in-memory source. Check the sample rate and channel count (should be 16kHz mono for the segment path — verify).
3. Create a helper `fn wav_bytes_from_samples(samples: &[f32], sample_rate: u32) -> Vec<u8>` that uses `hound::WavWriter::new(Cursor::new(Vec::new()), spec)` where `spec` has 1 channel, 32-bit float, the correct sample rate. Write all samples, flush, extract the inner Vec from the Cursor.
4. Replace `write_segment_wav()` with a call to `wav_bytes_from_samples()`. Return `Vec<u8>` instead of a file path.
5. In `WhisperBackend::transcribe()`, replace `multipart::Form::file("file", &wav_path)` with `multipart::Part::bytes_with_fname(wav_bytes, "file.wav").mime_str("audio/wav")?` and build the form from that part.
6. Remove the temp-file creation and cleanup code for the segment path (the `turbotalk-seg-{i}.wav` files and their cleanup). Be careful not to remove temp-file cleanup for paths that still use files.
7. Check if the tail WAV path (the final segment after silence-boundary splitting) also goes through `write_segment_wav` — if so, apply the same in-memory treatment.
8. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy`.
9. Run `npm run typecheck`.
10. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- `grep -n "write_segment_wav\|turbotalk-seg" src-tauri/src/transcribe.rs` returns zero results (no temp file writing for segments).
- `grep -n "Part::bytes_with_fname\|Part::bytes" src-tauri/src/transcribe.rs` shows the multipart upload using in-memory bytes.
- No temp WAV files are created on disk during a multi-segment dictation.
- The whisper-server still receives a valid WAV multipart upload (it doesn't know or care whether the bytes came from disk or memory).

## Notes
- The WAV spec for the segment path: 1 channel, 32-bit float (`hound::SampleFormat::Float`), 16000 Hz sample rate (verify by reading the resampler output config — the streaming path resamples to 16kHz for the VAD, and segments are cut from the resampled buffer).
- `reqwest::blocking::multipart::Part::bytes_with_fname` takes `impl Into<Bytes>` — `Vec<u8>` works directly.
- The `Cursor<Vec<u8>>` approach is standard for in-memory serialization. `WavWriter::new()` returns a `WavWriter<W>` where `W: Write + Seek`. `Cursor<Vec<u8>>` implements both.
- Don't forget to call `.flush()` or drop the `WavWriter` before extracting the bytes from the Cursor — the WAV writer may buffer the data chunk size and only write it on flush/drop.
- If the tail WAV is currently written by `audio.rs` (not `transcribe.rs`) as a file path handed to `transcribe()`, that path is out of scope — it's the batch fallback. Only touch the in-memory segment path.
