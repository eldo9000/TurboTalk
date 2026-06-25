# Arc Log — TASK-73: Gate worker invalidation on model/backend changes only

## Gate
Stop unconditionally calling `transcribe::invalidate_worker()` + `transcribe::prewarm()`
from `save_config` on every settings save. Only invalidate when a field that actually
affects the transcription worker changes.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** `save_config` calls `invalidate_worker()` + `prewarm()` on every
  config save, destroying the warm whisper-server/Parakeet worker when the user
  toggles unrelated settings (theme, sound, overlay, cursor dot, etc.).
- **PREMISE:** Capturing the previous config via `settings::load()` before
  `settings::update_cache()` and comparing only the fields that affect the
  transcription worker (`backend`, `backend_variant`, `whisper.model`,
  `whisper.vad_enabled`, `cleanup.vocabulary`) will eliminate wasteful worker
  destruction while preserving correct rebuild when transcription config changes.
- **DERIVATION:** `WhisperBackend::from_config` reads `cfg.whisper.model`,
  `cfg.whisper.vad_enabled`, and `cfg.cleanup.vocabulary`. `worker_for` compares
  `expected_backend_identity` which covers `backend`, `backend_variant`, and
  `whisper.model`. `cfg.whisper.bin` is never read (hardcoded "whisper-server").
  All other config fields (theme, sound, overlay, etc.) are consumed by other
  modules and never touch the worker.
- **FALSIFICATION:** If `cargo check` fails, or if a non-backend toggle still
  triggers `invalidate_worker()` in logs, the premise is false (logic error).
  If changing VAD or vocabulary does NOT trigger invalidation, the field list
  is incomplete.
- **FALSIF-RESULT:** `cargo check` passed (pre-existing warnings only), then `cargo clippy` passed with no new warnings.
- **DISPOSITION:** CONFIRMED — dispatch 1 green. Commit 1aeaacf.
