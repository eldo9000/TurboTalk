# Arc Log — TASK-75: In-memory WAV bytes for segment transcription

## Gate
Eliminate the temp-file round-trip for segment WAV writing by building WAV bytes
in memory and sending them directly as multipart body bytes.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** `write_segment_wav` writes `Vec<f32>` samples to a temp file on
  disk, then `WhisperBackend::transcribe` re-opens and re-reads the same file for
  HTTP multipart upload. For N segments, this is N extra disk write + read cycles.
- **PREMISE:** Building WAV bytes in memory via `hound::WavWriter::new(Cursor::new(Vec::new()))`
  and sending them via `reqwest::blocking::multipart::Part::bytes_with_fname` will
  eliminate the disk I/O while producing byte-identical WAV content.
- **DERIVATION:** `hound::WavWriter::new()` accepts any `io::Write + Seek`, including
  `Cursor<Vec<u8>>`. `reqwest::Part::bytes_with_fname` accepts `Vec<u8>`. The whisper-server
  receives the same WAV bytes regardless of origin.
- **FALSIFICATION:** If `cargo check` fails, or if a segment transcription fails
  because the WAV bytes differ from the file-based WAV, the premise is false.
- **FALSIF-RESULT:** (run by worker before fix)
- **DISPOSITION:**
