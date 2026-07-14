// Zero-allocation session performance counters for diagnostic reports.
//
// These atomics are updated at each dictation lifecycle stage and read at
// diagnostic export time. They carry zero cost on the hot path — just an
// atomic fetch_add with Relaxed ordering. No Mutex, no allocation, no I/O.
//
// Designed for the slow Windows testing loop: the tester runs through a
// session, exports a diagnostic report, and we can read exact counts of
// what happened at every stage without needing to reproduce the session.
//
// Counters are never reset during a session — each report captures a
// snapshot of the cumulative totals. First report in a session = 0 in
// most counters (startup events may have incremented some).

use std::sync::atomic::{AtomicU64, Ordering};

// ── Session-level counters ─────────────────────────────────────────────────

/// Total `ptt_down` invocations that reached the recorder start path
/// (i.e. hotkey was detected and arm-check passed). Incremented in
/// `common::ptt_down` before `rec.start()`.
static HOTKEY_DOWNS: AtomicU64 = AtomicU64::new(0);

/// Total successful dictation starts — `rec.start()` returned `Ok(())`.
/// If HOTKEY_DOWNS ≠ DICTATION_STARTS, something blocked the start
/// (busy recorder, onboarding gate, etc.).
static DICTATION_STARTS: AtomicU64 = AtomicU64::new(0);

/// Total dictations that reached `finish()` after successful paste.
/// The difference between STARTS and COMPLETED is discarded or failed.
static DICTATIONS_COMPLETED: AtomicU64 = AtomicU64::new(0);

/// Total dictations discarded via `StopOutcome::Discard` (too short,
/// no speech detected, device lost, etc.). Incremented in the discard
/// path of the hotkey common module.
static DICTATIONS_DISCARDED: AtomicU64 = AtomicU64::new(0);

// ── Stage-specific error counters ──────────────────────────────────────────

/// Audio pipeline errors (cpal stream failure, device lost mid-recording,
/// WAV write failure). Incremented in the `Err` branch of `rec.start()`
/// and the audio error path of `common::ptt_up`.
static AUDIO_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Transcription failures (whisper-server POST error, timeout, invalid
/// JSON response). Incremented in `common::run_transcription_stage`.
static TRANSCRIBE_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Cleanup failures (text_formatter error, regex crash). Incremented
/// in `common::run_cleanup_stage`.
static CLEANUP_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Paste returned `Ok(true)` — text was written to clipboard and Ctrl+V
/// was synthesized. Incremented after `paste::paste()` succeeds.
static PASTE_SUCCESSES: AtomicU64 = AtomicU64::new(0);

/// Paste returned `Err` or `Ok(false)` — text was not injected.
/// Incremented in the paste error path.
static PASTE_FAILURES: AtomicU64 = AtomicU64::new(0);

// ── Recording quality counters ─────────────────────────────────────────────

/// Total milliseconds of audio captured across all dictations. Updated
/// in `common::ptt_up` from the WAV duration metadata.
static TOTAL_RECORDED_MS: AtomicU64 = AtomicU64::new(0);

/// Total milliseconds spent in transcription across all dictations.
/// Updated after the Whisper POST round-trip completes.
static TOTAL_TRANSCRIBE_MS: AtomicU64 = AtomicU64::new(0);

// ── Increment helpers (called from hotkey, lib.rs, etc.) ───────────────────

/// Call from `common::ptt_down` when the hotkey fires and arm-check passes.
pub fn record_hotkey_down() {
    HOTKEY_DOWNS.fetch_add(1, Ordering::Relaxed);
}

/// Call after `rec.start()` returns `Ok(())`.
pub fn record_dictation_started() {
    DICTATION_STARTS.fetch_add(1, Ordering::Relaxed);
}

/// Call after `common::finish_dictation` completes successfully
/// (all stages reached Ready and paste succeeded).
pub fn record_dictation_completed() {
    DICTATIONS_COMPLETED.fetch_add(1, Ordering::Relaxed);
}

/// Call when a dictation is discarded (`StopOutcome::Discard`,
/// cancel paths, device-lost mid-recording).
pub fn record_dictation_discarded() {
    DICTATIONS_DISCARDED.fetch_add(1, Ordering::Relaxed);
}

pub fn record_audio_error() {
    AUDIO_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_transcribe_error() {
    TRANSCRIBE_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_cleanup_error() {
    CLEANUP_ERRORS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_paste_success() {
    PASTE_SUCCESSES.fetch_add(1, Ordering::Relaxed);
}

pub fn record_paste_failure() {
    PASTE_FAILURES.fetch_add(1, Ordering::Relaxed);
}

/// Add `ms` to the total recorded audio duration.
pub fn add_recorded_ms(ms: u64) {
    TOTAL_RECORDED_MS.fetch_add(ms, Ordering::Relaxed);
}

/// Add `ms` to the total transcription time.
pub fn add_transcribe_ms(ms: u64) {
    TOTAL_TRANSCRIBE_MS.fetch_add(ms, Ordering::Relaxed);
}

// ── Snapshot (read for diagnostic export) ──────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SessionMetricsSnapshot {
    pub hotkey_downs: u64,
    pub dictation_starts: u64,
    pub dictations_completed: u64,
    pub dictations_discarded: u64,
    pub audio_errors: u64,
    pub transcribe_errors: u64,
    pub cleanup_errors: u64,
    pub paste_successes: u64,
    pub paste_failures: u64,
    /// Total milliseconds of audio captured across all dictations.
    pub total_recorded_ms: u64,
    /// Total milliseconds spent in transcription.
    pub total_transcribe_ms: u64,
}

/// Atomic snapshot of current counters. Cheap — just Relaxed loads.
pub fn snapshot() -> SessionMetricsSnapshot {
    SessionMetricsSnapshot {
        hotkey_downs: HOTKEY_DOWNS.load(Ordering::Relaxed),
        dictation_starts: DICTATION_STARTS.load(Ordering::Relaxed),
        dictations_completed: DICTATIONS_COMPLETED.load(Ordering::Relaxed),
        dictations_discarded: DICTATIONS_DISCARDED.load(Ordering::Relaxed),
        audio_errors: AUDIO_ERRORS.load(Ordering::Relaxed),
        transcribe_errors: TRANSCRIBE_ERRORS.load(Ordering::Relaxed),
        cleanup_errors: CLEANUP_ERRORS.load(Ordering::Relaxed),
        paste_successes: PASTE_SUCCESSES.load(Ordering::Relaxed),
        paste_failures: PASTE_FAILURES.load(Ordering::Relaxed),
        total_recorded_ms: TOTAL_RECORDED_MS.load(Ordering::Relaxed),
        total_transcribe_ms: TOTAL_TRANSCRIBE_MS.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_counters_start_at_zero() {
        let s = snapshot();
        assert_eq!(s.hotkey_downs, 0);
        assert_eq!(s.dictation_starts, 0);
        assert_eq!(s.dictations_completed, 0);
        assert_eq!(s.dictations_discarded, 0);
        assert_eq!(s.audio_errors, 0);
        assert_eq!(s.transcribe_errors, 0);
        assert_eq!(s.cleanup_errors, 0);
        assert_eq!(s.paste_successes, 0);
        assert_eq!(s.paste_failures, 0);
        assert_eq!(s.total_recorded_ms, 0);
        assert_eq!(s.total_transcribe_ms, 0);
    }

    #[test]
    fn counters_increment_and_snapshot() {
        record_hotkey_down();
        record_hotkey_down();
        record_dictation_started();
        record_dictation_completed();
        record_dictation_discarded();
        record_audio_error();
        record_transcribe_error();
        record_cleanup_error();
        record_paste_success();
        record_paste_failure();
        add_recorded_ms(1500);
        add_transcribe_ms(300);

        let s = snapshot();
        assert_eq!(s.hotkey_downs, 2);
        assert_eq!(s.dictation_starts, 1);
        assert_eq!(s.dictations_completed, 1);
        assert_eq!(s.dictations_discarded, 1);
        assert_eq!(s.audio_errors, 1);
        assert_eq!(s.transcribe_errors, 1);
        assert_eq!(s.cleanup_errors, 1);
        assert_eq!(s.paste_successes, 1);
        assert_eq!(s.paste_failures, 1);
        assert_eq!(s.total_recorded_ms, 1500);
        assert_eq!(s.total_transcribe_ms, 300);
    }
}
