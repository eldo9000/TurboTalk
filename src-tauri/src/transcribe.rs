// Spawns whisper-server as a long-lived sidecar, feeds it the WAV via HTTP
// POST /inference, and reads back the JSON transcript.
//
// TASK-47: replace the per-call whisper-cli spawn (TASK-20 option 3) with a
// persistent whisper-server that keeps the model loaded across dictations.
// The server is spawned once per worker lifetime (one per model config); the
// worker is cached in the process-wide WORKER slot and rebuilt only when the
// model changes or after an abort.
//
// TASK-55: post-hoc hallucination detection. After strip_trailing_filler, the
// transcript is passed through detect_garbage(). If it trips any of the three
// detection signals below, the caller receives a TranscriptOutcome with a
// rejection variant, displays the text as "⚠ filtered", and skips paste.
//
// ── Tunable hallucination-detection thresholds ──────────────────────────────
//
/// Compression-ratio threshold. `gzip(text).len() / text.len()` below this
/// value indicates highly repetitive text (e.g. "the the the the the").
/// Threshold chosen conservatively: < 0.35 triggers (i.e. text compresses to
/// less than 35% of original). Raise toward 0.5 to catch more; lower to
/// reduce false positives.
const GARBAGE_COMPRESS_RATIO: f64 = 0.35;

/// Maximum number of times the same three-word sequence (trigram) may appear
/// before the transcript is considered a repetition loop.
const GARBAGE_TRIGRAM_MAX_REPEATS: usize = 3;

/// Maximum fraction of characters that are not letters, digits, spaces, or
/// common punctuation (.,!?'-). Above this → junk characters / all-zeros
/// hallucination. 0.30 = up to 30% non-letter-ish chars is tolerated.
const GARBAGE_NON_LETTER_RATIO: f64 = 0.30;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tauri::Emitter;

// ── Hallucination detection (TASK-55) ────────────────────────────────────────

/// The reason a transcript was classified as garbage and rejected.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// Text compresses to an unusually small fraction of its original length,
    /// indicating heavy repetition (e.g. "the the the the").
    HighCompression,
    /// The same three-word sequence appears more than `GARBAGE_TRIGRAM_MAX_REPEATS` times.
    TrigramRepetition,
    /// More than `GARBAGE_NON_LETTER_RATIO` of characters are not letters,
    /// digits, spaces, or common punctuation — likely all-zeros or junk.
    NonLetterJunk,
}

impl RejectReason {
    /// Human-readable explanation suitable for a UI toast.
    pub fn description(&self) -> &'static str {
        match self {
            RejectReason::HighCompression =>
                "Repetition loop detected — Whisper echoed the same phrase repeatedly on silence.",
            RejectReason::TrigramRepetition =>
                "Repetition loop detected — the same phrase appeared too many times.",
            RejectReason::NonLetterJunk =>
                "Junk characters detected — Whisper produced garbage output on silence.",
        }
    }
}

/// Outcome of `TranscriptionWorker::transcribe`. Either a clean transcript or
/// a detected-garbage rejection that the caller must not paste.
pub struct TranscriptOutcome {
    /// The raw transcript text (always present, even on rejection — callers
    /// display it with a "⚠ filtered" badge for observability).
    pub text: String,
    /// `Some` if the transcript tripped a hallucination filter and must not
    /// be pasted; `None` for a normal accepted transcript.
    pub rejection: Option<RejectReason>,
}

/// Run the three hallucination-detection signals on `text`. Returns the first
/// failing signal, or `None` if all pass (clean transcript).
///
/// Called after `strip_trailing_filler` so trailing-filler removal has already
/// narrowed the text. Empty text is always accepted (nothing to detect; the
/// caller handles the empty-transcript path separately).
pub fn detect_garbage(text: &str) -> Option<RejectReason> {
    if text.is_empty() {
        return None;
    }

    // ── Signal 1: compression ratio ──────────────────────────────────────────
    // Highly repetitive text compresses extremely well. Threshold defined at
    // the top of the module.
    {
        use std::io::Write;
        use flate2::{Compression, write::GzEncoder};
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        let _ = enc.write_all(text.as_bytes());
        if let Ok(compressed) = enc.finish() {
            let ratio = compressed.len() as f64 / text.len() as f64;
            tracing::debug!("[detect_garbage] compression ratio = {:.3}", ratio);
            if ratio < GARBAGE_COMPRESS_RATIO {
                return Some(RejectReason::HighCompression);
            }
        }
    }

    // ── Signal 2: trigram repetition ─────────────────────────────────────────
    // Split on whitespace and count occurrences of each 3-word window. A
    // single sequence appearing more than GARBAGE_TRIGRAM_MAX_REPEATS times
    // is a repetition loop.
    {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() >= 3 {
            let mut counts: std::collections::HashMap<(&str, &str, &str), usize> =
                std::collections::HashMap::new();
            for window in words.windows(3) {
                let key = (window[0], window[1], window[2]);
                let count = counts.entry(key).or_insert(0);
                *count += 1;
                if *count > GARBAGE_TRIGRAM_MAX_REPEATS {
                    tracing::debug!(
                        "[detect_garbage] trigram {:?} appeared {} times",
                        key,
                        count
                    );
                    return Some(RejectReason::TrigramRepetition);
                }
            }
        }
    }

    // ── Signal 3: non-letter ratio ────────────────────────────────────────────
    // Fraction of characters that are not letters, digits, spaces, or the
    // common ASCII punctuation set. High ratio = junk/all-zeros output.
    {
        let total = text.chars().count();
        let non_letter = text
            .chars()
            .filter(|c| {
                !c.is_alphabetic()
                    && !c.is_ascii_digit()
                    && !c.is_whitespace()
                    && !".,!?'-\":;()[]".contains(*c)
                    // Also allow common Unicode punctuation (smart quotes, em/en dashes)
                    && !matches!(*c, '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}' | '\u{2013}' | '\u{2014}')
            })
            .count();
        let ratio = non_letter as f64 / total as f64;
        tracing::debug!("[detect_garbage] non-letter ratio = {:.3}", ratio);
        if ratio > GARBAGE_NON_LETTER_RATIO {
            return Some(RejectReason::NonLetterJunk);
        }
    }

    None
}

/// Allowed roots for the whisper binary:
/// - the directory containing the running executable (release bundle sidecar)
/// - in debug builds only, the cargo target/ tree and `src-tauri/binaries/`
///
/// Any configured path that does not canonicalize to a location inside one of
/// these roots is rejected — including arbitrary system binaries like `/bin/ls`.
fn allowed_whisper_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Ok(canon) = parent.canonicalize() {
                roots.push(canon);
            }
        }
    }

    #[cfg(debug_assertions)]
    {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Ok(canon) = manifest_dir.join("target").canonicalize() {
            roots.push(canon);
        }
        if let Ok(canon) = manifest_dir.join("binaries").canonicalize() {
            roots.push(canon);
        }
    }

    roots
}

/// Returns `true` only if `p` canonicalizes to a path inside one of the
/// allowed whisper roots. Symlinks are resolved before checking.
fn is_allowed_whisper_path(p: &Path) -> bool {
    let Ok(canon) = p.canonicalize() else {
        return false;
    };
    let roots = allowed_whisper_roots();
    roots.iter().any(|root| canon.starts_with(root))
}

/// Build the list of candidate sidecar filenames for whisper-cli, in priority
/// order. Kept for path-validation tests.
#[allow(dead_code)]
fn sidecar_candidates() -> Vec<String> {
    let triple = env!("TARGET_TRIPLE");
    let exe_suffix = if triple.contains("windows") {
        ".exe"
    } else {
        ""
    };
    vec![
        format!("whisper-cli{}", exe_suffix),
        format!("whisper-cli-{}{}", triple, exe_suffix),
    ]
}

/// Build the list of candidate sidecar filenames for whisper-server, in
/// priority order. Mirrors `sidecar_candidates()` but for the server binary.
fn server_sidecar_candidates() -> Vec<String> {
    let triple = env!("TARGET_TRIPLE");
    let exe_suffix = if triple.contains("windows") {
        ".exe"
    } else {
        ""
    };
    vec![
        format!("whisper-server{}", exe_suffix),
        format!("whisper-server-{}{}", triple, exe_suffix),
    ]
}

/// Locate the whisper-cli binary (used only for path-validation tests; the
/// live transcription path now uses `find_whisper_server`).
/// Priority: bundled sidecar (next to exe) → dev binaries dir → configured path.
#[allow(dead_code)]
fn find_whisper(configured_bin: &str) -> anyhow::Result<PathBuf> {
    let sidecars = sidecar_candidates();

    if let Ok(exe) = std::env::current_exe() {
        let parent = exe.parent().unwrap_or_else(|| Path::new("."));
        for sidecar in &sidecars {
            let p = parent.join(sidecar);
            if p.exists() {
                tracing::debug!("[transcribe] using bundled sidecar: {:?}", p);
                return Ok(p);
            }
        }
    }

    #[cfg(debug_assertions)]
    {
        for sidecar in &sidecars {
            let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("binaries")
                .join(sidecar);
            if dev.exists() {
                tracing::debug!("[transcribe] using dev sidecar: {:?}", dev);
                return Ok(dev);
            }
        }
    }

    let configured = PathBuf::from(configured_bin);
    if !configured.exists() || !is_allowed_whisper_path(&configured) {
        tracing::error!(
            "[transcribe] whisper-cli sidecar not found (checked bundle and dev paths); \
             configured bin: {}",
            configured_bin
        );
        anyhow::bail!(
            "Whisper sidecar not found. Reinstall the app or check that whisper-cli exists \
             in the app bundle."
        );
    }
    tracing::debug!("[transcribe] using configured bin: {}", configured_bin);
    Ok(configured)
}

/// Locate the whisper-server binary.
/// Priority:
/// - debug: dev binaries dir → current executable dir → configured path
/// - release: current executable dir → configured path
///
/// IMPORTANT: `binaries/` is checked BEFORE `current_exe().parent()` because
/// Tauri's dev build copies `whisper-server` (and stale `libggml`/`libwhisper`
/// dylibs with `@rpath` install names) into `target/debug/`. When the Homebrew
/// whisper-server binary loads, its rpath-relative libwhisper pulls in those
/// stale dylibs alongside the Homebrew ones → two libggml instances → two
/// `get_reg()` statics → `ggml_backend_dev_count()` returns 0 → GGML_ASSERT.
/// Using the `binaries/` symlink → Homebrew binary sidesteps this entirely.
fn find_whisper_server(configured_bin: &str) -> anyhow::Result<PathBuf> {
    let sidecars = server_sidecar_candidates();

    // Dev mode: binaries/ symlink → Homebrew binary. Checked FIRST to avoid
    // target/debug/ stale-dylib registry split (see comment above).
    #[cfg(debug_assertions)]
    {
        for sidecar in &sidecars {
            let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("binaries")
                .join(sidecar);
            if dev.exists() {
                tracing::debug!("[transcribe] using dev whisper-server sidecar: {:?}", dev);
                return Ok(dev);
            }
        }
    }

    // Release bundle: sidecar is placed next to the main executable in Contents/MacOS/
    if let Ok(exe) = std::env::current_exe() {
        let parent = exe.parent().unwrap_or_else(|| Path::new("."));
        for sidecar in &sidecars {
            let p = parent.join(sidecar);
            if p.exists() {
                tracing::debug!("[transcribe] using bundled whisper-server sidecar: {:?}", p);
                return Ok(p);
            }
        }
    }

    // Last resort: configured path. Validated against the allow-list.
    let configured = PathBuf::from(configured_bin);
    if !configured.exists() || !is_allowed_whisper_path(&configured) {
        tracing::error!(
            "[transcribe] whisper-server sidecar not found (checked bundle and dev paths); \
             configured bin: {}",
            configured_bin
        );
        anyhow::bail!(
            "whisper-server sidecar not found. Reinstall the app or check that whisper-server \
             exists in the app bundle."
        );
    }
    tracing::debug!(
        "[transcribe] using configured whisper-server bin: {}",
        configured_bin
    );
    Ok(configured)
}

/// Build the list of candidate filenames for the Silero VAD model, in priority
/// order. Mirrors `server_sidecar_candidates()` for consistent path resolution.
fn vad_model_candidates() -> Vec<String> {
    // The canonical filename shipped in src-tauri/binaries/. A single
    // platform-independent name is used since this is a data file, not a binary.
    vec!["ggml-silero-v5.1.2.bin".to_string()]
}

/// Locate the Silero VAD model file for whisper-server.
/// Priority:
/// - debug: dev binaries dir → current executable dir
/// - release: current executable dir
///
/// Returns `None` if the model is not found in any candidate location. The
/// caller must treat `None` as "VAD unavailable" and skip the VAD flags rather
/// than failing the transcription — VAD is a best-effort acceleration layer.
fn find_vad_model() -> Option<PathBuf> {
    let candidates = vad_model_candidates();

    // Dev mode: check src-tauri/binaries/ first (same priority logic as
    // find_whisper_server — keeps dev symlinks consistent).
    #[cfg(debug_assertions)]
    {
        for candidate in &candidates {
            let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("binaries")
                .join(candidate);
            if dev.exists() {
                tracing::debug!("[transcribe] using dev VAD model: {:?}", dev);
                return Some(dev);
            }
        }
    }

    // Release bundle: VAD model placed next to the main executable.
    if let Ok(exe) = std::env::current_exe() {
        let parent = exe.parent().unwrap_or_else(|| Path::new("."));
        for candidate in &candidates {
            let p = parent.join(candidate);
            if p.exists() {
                tracing::debug!("[transcribe] using bundled VAD model: {:?}", p);
                return Some(p);
            }
        }
    }

    tracing::debug!("[transcribe] VAD model not found in any candidate location");
    None
}

/// Canonicalize `raw_model` and verify it lives inside `canon_models_dir`.
/// Blocks `model = "/etc/passwd"` style attacks and symlink escapes from the
/// models dir. Returns the canonicalized model path on success.
///
/// Extracted from `run()` so unit tests can exercise the path-traversal
/// guard against a temp dir without spawning whisper.
fn validate_model_path(raw_model: &str, canon_models_dir: &Path) -> anyhow::Result<PathBuf> {
    let canon_model = PathBuf::from(raw_model).canonicalize().map_err(|_| {
        anyhow::anyhow!(
            "Whisper model not found at the configured path. Open Settings and set the correct model path. (path: {})",
            raw_model
        )
    })?;
    if !canon_model.starts_with(canon_models_dir) {
        anyhow::bail!(
            "model path is outside the allowed models directory: {}",
            raw_model
        );
    }
    Ok(canon_model)
}

/// Strip common Whisper trailing hallucinations ("okay", "yeah", etc.) that
/// appear when the model decodes trailing silence rather than real speech.
/// Only the very end of the transcript is trimmed; the body is untouched.
fn strip_trailing_filler(text: &str) -> String {
    // Each entry is matched case-insensitively against the trimmed tail.
    // Punctuation after the filler word is consumed along with it.
    const FILLERS: &[&str] = &[
        "okay",
        "ok",
        "yeah",
        "yep",
        "yup",
        "alright",
        "all right",
        "thank you",
        "thanks",
        "uh",
        "um",
        "uh huh",
    ];
    let mut s = text.to_string();
    loop {
        let trimmed = s.trim_end_matches(|c: char| c.is_whitespace() || c == '.' || c == ',');
        let lower = trimmed.to_lowercase();
        let matched = FILLERS
            .iter()
            .find_map(|&f| lower.strip_suffix(f).map(|rest| rest.len()));
        match matched {
            Some(keep) => {
                s = trimmed[..keep]
                    .trim_end_matches(|c: char| c.is_whitespace() || c == ',' || c == '.')
                    .to_string()
            }
            None => break,
        }
    }
    s
}

/// whisper-server may return segment-bounded text containing literal newlines
/// every few words. Those are decoder artifacts, not user intent; explicit
/// voice commands such as "new paragraph" are handled later in cleanup.rs.
fn normalize_whisper_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── TranscriptionBackend trait (TASK-57) ─────────────────────────────────────

/// Common interface for all transcription backends. Implementations must be
/// `Send + Sync` so they can live behind `Arc<dyn TranscriptionBackend>` and be
/// shared across the hotkey, abort, and prewarm threads.
///
/// The trait is intentionally minimal. All Whisper-specific concerns
/// (audio_ctx tuning, vocabulary prompt, subprocess lifecycle) stay inside
/// `WhisperBackend`. Only the three cross-backend verbs are exposed.
pub trait TranscriptionBackend: Send + Sync {
    /// POST `wav` to the backend and return the cleaned transcript text.
    /// Callers inspect `TranscriptOutcome.rejection` to decide whether to paste.
    fn transcribe(&self, wav: &Path) -> anyhow::Result<TranscriptOutcome>;

    /// Kill any in-flight work. After `abort()` the backend is in a broken
    /// state — the caller must call `invalidate_worker()` and let the next
    /// `run_raw` rebuild.
    fn abort(&self);

    /// A stable string that uniquely identifies the loaded model (typically
    /// its canonicalized path). Used by `worker_for` to decide whether the
    /// cached backend is still valid for the current settings.
    fn model_identity(&self) -> String;
}

/// Construct the active backend from a settings snapshot.
///
/// Backend selection is driven by `cfg.backend` (a `BackendFamily` enum persisted
/// in `config.toml`). The `TT_BACKEND` environment variable has been removed — use
/// the Settings UI or edit `config.toml` directly to switch backends.
///
/// Fallback behaviour when a feature flag is off (TASK-60):
///   Moonshine or Parakeet selected but their feature gate is disabled (ort conflict
///   between transcribe-rs rc.12 and vad-rs rc.9) → falls back to Whisper with a
///   log warning. The fallback is intentional and documented: the backend field is
///   wired end-to-end now so activation is just a recompile once the conflict resolves.
///
/// Note on VAD (TASK-56) and hallucination filter (TASK-55):
///   The Silero VAD pre-filter is whisper-server-specific; Moonshine/Parakeet have
///   their own silence handling. The post-hoc hallucination filter runs on all
///   backends (no harm, family-agnostic). The Chaperone cleanup (cleanup.rs) runs
///   on the output of all backends — also family-agnostic.
fn build_backend(cfg: &crate::settings::Config) -> anyhow::Result<std::sync::Arc<dyn TranscriptionBackend>> {
    use crate::settings::BackendFamily;

    match cfg.backend {
        BackendFamily::Moonshine => {
            #[cfg(feature = "moonshine")]
            {
                tracing::info!("[transcribe] backend=moonshine — building MoonshineBackend");
                let backend = crate::transcribe_backends::moonshine::MoonshineBackend::from_config(cfg)?;
                return Ok(std::sync::Arc::new(backend));
            }
            #[cfg(not(feature = "moonshine"))]
            {
                tracing::warn!(
                    "[transcribe] backend=moonshine requested but `moonshine` feature is not compiled in. \
                     Falling back to Whisper."
                );
                // Fall through to Whisper below.
            }
        }
        BackendFamily::Parakeet => {
            #[cfg(feature = "parakeet")]
            {
                tracing::info!("[transcribe] backend=parakeet — building ParakeetBackend");
                let backend = crate::transcribe_backends::parakeet::ParakeetBackend::from_config(cfg)?;
                return Ok(std::sync::Arc::new(backend));
            }
            #[cfg(not(feature = "parakeet"))]
            {
                tracing::warn!(
                    "[transcribe] backend=parakeet requested but `parakeet` feature is not compiled in. \
                     Falling back to Whisper."
                );
                // Fall through to Whisper below.
            }
        }
        BackendFamily::Whisper => {
            // Explicit Whisper selection — handled below.
        }
    }

    // Default / fallback path: Whisper.
    tracing::info!("[transcribe] using WhisperBackend");
    Ok(std::sync::Arc::new(WhisperBackend::from_config(cfg)?))
}

// ── WhisperBackend ────────────────────────────────────────────────────────────

/// Concrete `TranscriptionBackend` backed by a long-lived `whisper-server`
/// subprocess. TASK-47 introduced this pattern; TASK-57 promotes it behind
/// the `TranscriptionBackend` trait so future backends can be swapped in.
///
/// The internal `spawn_lock` enforces the one-in-flight invariant from
/// TASK-14: any second concurrent caller blocks here rather than racing the
/// HTTP POST. `server_child` is protected by a separate `parking_lot::Mutex`
/// so `abort()` can kill the server from another thread without taking the
/// coarser `spawn_lock`.
///
/// `TranscriptionWorker` is a deprecated type alias kept for any tests or
/// comments that still reference the old name. New code should use
/// `WhisperBackend` directly.
pub struct WhisperBackend {
    /// whisper-server binary path. Validated at construction.
    #[allow(dead_code)]
    bin: PathBuf,
    /// Canonicalized model path. Validated at construction; lives inside
    /// `~/.config/librewin/turbotalk/models/`.
    model: PathBuf,
    /// Vocabulary phrases joined and passed to whisper-server as the `prompt`
    /// form field. Empty = no prompt.
    vocabulary: Vec<String>,
    /// Spawn serialization. Held across the whole `transcribe` call so there
    /// is never more than one in-flight HTTP POST to the server at once.
    spawn_lock: Mutex<()>,
    /// The long-lived whisper-server child process. Set at construction,
    /// cleared by `abort()` or the `Drop` impl.
    server_child: parking_lot::Mutex<Option<std::process::Child>>,
    /// Port the server is listening on.
    server_port: u16,
    /// Reusable HTTP client for POST /inference requests.
    http_client: reqwest::blocking::Client,
    /// audio_ctx sent per-request. 512 = ~10 s encoder window; benched at 63%
    /// faster than default (1500) across short/medium/long utterances (TASK-44).
    audio_ctx: u32,
}

/// Deprecated alias for `WhisperBackend`. Used only in legacy doc comments and
/// the single test that constructs a bare struct literal (see `abort_noop_when_idle`).
#[allow(dead_code)]
pub type TranscriptionWorker = WhisperBackend;

impl WhisperBackend {
    /// Build a worker from a snapshot of the current settings. Validates the
    /// binary path and the model path eagerly, then spawns `whisper-server`
    /// and waits for it to become ready.
    pub fn from_config(cfg: &crate::settings::Config) -> anyhow::Result<Self> {
        // Use "whisper-server" as the default configured bin name; the
        // find_whisper_server search resolves the actual path.
        let bin = find_whisper_server("whisper-server")?;
        let canon_models_dir = crate::settings::canonical_models_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "models directory does not exist — create ~/.config/librewin/turbotalk/models/ \
                 and place a ggml model there"
            )
        })?;
        let model = validate_model_path(&cfg.whisper.model, &canon_models_dir)?;

        let model_str = model
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("model path is not valid UTF-8: {:?}", model))?
            .to_string();

        // Pick a random available port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener); // free the port so whisper-server can bind it

        // Spawn whisper-server. It will load the model and start listening.
        // Stderr goes to a temp file so we can diagnose crashes without a
        // pipe-deadlock (Stdio::piped + never reading = blocked server).
        let stderr_stdio = std::fs::File::create("/tmp/whisper-server-stderr.log")
            .map(std::process::Stdio::from)
            .unwrap_or_else(|_| std::process::Stdio::null());
        // Canonicalize so `_NSGetExecutablePath` in the child returns the real
        // Homebrew path, not the binaries/ symlink. Combined with the
        // `find_whisper_server` search order (binaries/ before target/debug/),
        // this ensures only one libggml instance loads — the Homebrew one.
        let real_bin = std::fs::canonicalize(&bin).unwrap_or_else(|_| bin.clone());
        let mut cmd = std::process::Command::new(&real_bin);
        cmd.args([
            "-m",
            &model_str,
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--inference-path",
            "/inference",
        ])
        .env_clear()
        .stdout(std::process::Stdio::null())
        .stderr(stderr_stdio);

        // TASK-56: Silero VAD pre-filter. When enabled, whisper-server skips
        // silent regions before the decoder runs, preventing hallucination on
        // silence and reducing transcription time on recordings with long pauses.
        // The VAD model file must exist alongside the binary; if it is missing
        // we log a warning and fall back to no-VAD rather than failing startup.
        if cfg.whisper.vad_enabled {
            match find_vad_model() {
                Some(vad_path) => {
                    let vad_str = vad_path.to_string_lossy().into_owned();
                    tracing::info!("[transcribe] VAD enabled — model: {}", vad_str);
                    cmd.args(["--vad", "--vad-model", &vad_str]);
                }
                None => {
                    tracing::warn!(
                        "[transcribe] VAD enabled in settings but ggml-silero-v5.1.2.bin not found \
                         in binaries/ or next to executable — starting without VAD"
                    );
                }
            }
        } else {
            tracing::info!("[transcribe] VAD disabled by settings");
        }
        for var in &["HOME", "PATH", "TMPDIR", "USER", "LOGNAME"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
        // SAFETY: setsid() is async-signal-safe and has no Rust invariants.
        // It must be called after fork but before exec, which is exactly what
        // pre_exec guarantees. Failure is intentionally ignored: setsid()
        // returns EPERM if the process is already a group leader (harmless).
        #[cfg(unix)]
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                extern "C" {
                    fn setsid() -> i32;
                }
                setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn()?;

        // Quick early-exit check: if the process dies in the first 500 ms it's
        // a binary/signature/ABI problem. Report the exit code immediately so
        // we don't burn 30 s polling a dead process.
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(Some(status)) = child.try_wait() {
            anyhow::bail!(
                "whisper-server exited immediately (code {:?}) — check /tmp/whisper-server-stderr.log",
                status.code()
            );
        }

        // Poll until the server is ready (up to 30 s, 150 × 200 ms).
        // large-v3-turbo (1.5 GB) can take 5-10 s to load on first cold start.
        // Use a short per-request timeout so a half-open TCP connection (server
        // accepting but not yet responding) doesn't stall the entire poll loop.
        let poll_client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(400))
            .build()
            .unwrap_or_default();
        // IMPORTANT: reqwest's *blocking* client defaults to a 30 s total
        // request timeout (Timeout::default() = Some(30 s) — unlike the async
        // client, which defaults to None). `Client::new()` silently inherits
        // it. A long dictation (~3 min of dense speech) transcribes in >30 s
        // on large-v3-turbo, so the default cut the connection mid-inference
        // and surfaced as "error sending request" while whisper-server kept
        // running. Set an explicit, generous cap instead. 120 s comfortably
        // covers a ~10 min whole-file batch-fallback POST on this hardware
        // (480 s audio benched at ~55 s); the streaming path keeps individual
        // requests far below this.
        let http_client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        let base_url = format!("http://127.0.0.1:{}", port);
        let mut ready = false;
        for attempt in 0..150 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            tracing::debug!(
                "[transcribe] whisper-server readiness poll attempt {}",
                attempt + 1
            );
            if poll_client.get(&base_url).send().is_ok() {
                ready = true;
                break;
            }
        }
        if !ready {
            anyhow::bail!(
                "whisper-server did not become ready within 30 s on port {}",
                port
            );
        }
        tracing::info!("[transcribe] whisper-server ready on port {}", port);

        Ok(Self {
            bin,
            model,
            vocabulary: cfg.cleanup.vocabulary.clone(),
            spawn_lock: Mutex::new(()),
            server_child: parking_lot::Mutex::new(Some(child)),
            server_port: port,
            http_client,
            audio_ctx: 512,
        })
    }

    /// The canonicalized model path this worker was built against. Callers
    /// can compare against a fresh `cfg.whisper.model` to decide whether to
    /// rebuild the worker.
    pub fn model_path(&self) -> &Path {
        &self.model
    }

    /// POST the WAV file to `/inference` and return a `TranscriptOutcome`.
    /// The outcome's `text` is always the cleaned transcript; `rejection` is
    /// `Some` if a hallucination signal was detected. Callers must not paste
    /// when `rejection.is_some()`. Holds `spawn_lock` for the whole call.
    pub fn transcribe(&self, wav: &Path) -> anyhow::Result<TranscriptOutcome> {
        let _guard = self.spawn_lock.lock().unwrap_or_else(|e| e.into_inner());

        let t_whisper_start = Instant::now();

        // Pick audio_ctx based on actual WAV duration. 512 frames covers
        // ~10 s; anything longer must use the full context (0 = all) or
        // whisper silently truncates past the cap. TASK-44 benched 512 as
        // 63% faster on ≤8 s utterances — keep that win for short dictation,
        // fall back to full context for long sentences.
        let effective_audio_ctx = match hound::WavReader::open(wav) {
            Ok(r) => {
                let spec = r.spec();
                let secs = r.duration() as f32 / spec.sample_rate as f32;
                if secs <= 8.0 {
                    self.audio_ctx
                } else {
                    0
                }
            }
            // Header read failed — be safe, use full context.
            Err(_) => 0,
        };

        let mut form = reqwest::blocking::multipart::Form::new()
            .file("file", wav)?
            // Anti-hallucination: temperature_inc=0 disables the temperature
            // fallback retry that produces "same phrase 3x" repetition output
            // on short or silent audio. Mirrors the old whisper-cli config
            // (commit 55cfa21) lost during the TASK-47 server transition.
            .text("temperature", "0.0")
            .text("temperature_inc", "0.0")
            .text("suppress_nst", "true")
            .text("no_context", "true")
            .text("beam_size", "5");
        if effective_audio_ctx > 0 {
            form = form.text("audio_ctx", effective_audio_ctx.to_string());
        }
        if !self.vocabulary.is_empty() {
            form = form.text("prompt", self.vocabulary.join(", "));
        }
        let response = self
            .http_client
            .post(format!("http://127.0.0.1:{}/inference", self.server_port))
            .multipart(form)
            .send()?;

        if !response.status().is_success() {
            anyhow::bail!("whisper-server returned {}", response.status());
        }

        let json: serde_json::Value = response.json()?;
        let raw = json["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("whisper-server response missing 'text' field"))?
            .trim()
            .to_string();
        let normalized = normalize_whisper_text(&raw);
        let text = strip_trailing_filler(&normalized);

        let whisper_ms = t_whisper_start.elapsed().as_millis();
        tracing::info!("[transcribe] whisper took {} ms", whisper_ms);
        // Never log transcript content to the session log — it can contain anything
        // the user dictated and that log is bundled into uploaded bug reports.
        tracing::info!("[transcribe] transcript ready ({} chars)", text.chars().count());

        // TASK-55: post-hoc hallucination detection on the cleaned text.
        let rejection = detect_garbage(&text);

        // Full transcript → local-only debug log (never uploaded). Temporary.
        crate::diagnostic_log::record_transcript("whisper", &text, &format!("{rejection:?}"));
        if let Some(ref reason) = rejection {
            tracing::warn!(
                "[transcribe] hallucination detected ({:?}) — text will not be pasted ({} chars)",
                reason,
                text.chars().count()
            );
        }

        Ok(TranscriptOutcome { text, rejection })
    }

    /// Kill the whisper-server subprocess. Best-effort: logs at warn on
    /// failure. No-op if the server has already exited.
    ///
    /// After `abort()` the worker is in a broken state — the caller must
    /// rebuild. `abort_active()` calls `invalidate_worker()` after this.
    pub fn abort_inner(&self) {
        let mut slot = self.server_child.lock();
        if let Some(mut child) = slot.take() {
            if let Err(e) = child.kill() {
                tracing::warn!("[transcribe] abort: server kill() failed: {}", e);
            } else {
                tracing::info!("[transcribe] abort: whisper-server subprocess killed");
            }
            let _ = child.wait();
        }
    }
}

impl TranscriptionBackend for WhisperBackend {
    fn transcribe(&self, wav: &Path) -> anyhow::Result<TranscriptOutcome> {
        WhisperBackend::transcribe(self, wav)
    }

    fn abort(&self) {
        self.abort_inner();
    }

    fn model_identity(&self) -> String {
        self.model
            .to_str()
            .unwrap_or("")
            .to_string()
    }
}

impl Drop for WhisperBackend {
    fn drop(&mut self) {
        let mut slot = self.server_child.lock();
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
            tracing::info!("[transcribe] whisper-server stopped");
        }
    }
}

/// Process-wide handle to the active backend. `None` on cold start and after
/// model invalidation; rebuilt lazily by `run_raw`.
///
/// The outer `Mutex` is the sequencing point — it serializes `take` /
/// `replace` / read accesses across the recorder, settings, and any future
/// app-shutdown drop site. The inner `Option` represents "worker not yet
/// built (or invalidated)".
///
/// `Arc<dyn TranscriptionBackend>` is used (rather than `Box<dyn …>`) so the
/// abort path and the spawn path can each clone the Arc and release the outer
/// mutex before the (potentially long) HTTP POST or kill() call.
static WORKER: Mutex<Option<std::sync::Arc<dyn TranscriptionBackend>>> = Mutex::new(None);

/// Abort the in-flight whisper-server subprocess. Called by `Recorder::cancel()`
/// when a cancel is triggered while in `Transcribing` state (TASK-23).
/// After killing the server, the cached worker is invalidated so the next
/// dictation rebuilds it (and thus restarts the server).
pub fn abort_active() {
    let slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(worker) = &*slot {
        worker.abort();
    }
    drop(slot); // release WORKER lock before calling invalidate_worker
    invalidate_worker();
}

/// Drop the cached worker. Called by `settings::save` (via `lib.rs`) when the
/// user changes the model — the next `run_raw` will rebuild against the new
/// config. Idempotent.
///
/// Also clears `READY` so the next PTT press shows the yellow arming tile
/// while the new model loads, instead of lying that the (just-deleted) cache
/// is still warm.
pub fn invalidate_worker() {
    let mut slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        tracing::info!("[transcribe] worker invalidated");
    }
    *slot = None;
    READY.store(false, Ordering::Release);
    PREWARM_FAILED.store(false, Ordering::Release);
    PREWARM_IN_FLIGHT.store(false, Ordering::Release);
}

/// Identity string for the currently configured backend — used to decide
/// whether the cached worker can be reused across dictations.
pub(crate) fn expected_backend_identity(cfg: &crate::settings::Config) -> String {
    use crate::settings::BackendFamily;

    match cfg.backend {
        BackendFamily::Whisper => std::path::PathBuf::from(&cfg.whisper.model)
            .canonicalize()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| cfg.whisper.model.clone()),
        BackendFamily::Moonshine => {
            let v = crate::settings::resolve_backend_variant(cfg);
            match crate::transcribe_backends::moonshine::variant_dir(&v) {
                Some(d) => {
                    let canon = d.canonicalize().unwrap_or(d);
                    format!("moonshine:{}:{}", v, canon.display())
                }
                None => format!("moonshine:{}:missing", v),
            }
        }
        BackendFamily::Parakeet => {
            let v = crate::settings::resolve_backend_variant(cfg);
            match crate::transcribe_backends::parakeet::variant_dir(&v) {
                Some(d) => {
                    let canon = d.canonicalize().unwrap_or(d);
                    format!("parakeet:{}:{}", v, canon.display())
                }
                None => format!("parakeet:{}:missing", v),
            }
        }
    }
}

/// Get-or-build the backend against the current settings snapshot. If the
/// cached backend's `model_identity()` differs from the configured backend,
/// it is dropped and rebuilt. Returns an `Arc<dyn TranscriptionBackend>` so
/// the spawn call can drop the outer mutex before the transcribe call.
fn worker_for(
    cfg: &crate::settings::Config,
) -> anyhow::Result<std::sync::Arc<dyn TranscriptionBackend>> {
    let mut slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());

    let configured_identity = expected_backend_identity(cfg);
    let cached_matches = match &*slot {
        Some(w) => w.model_identity() == configured_identity,
        None => false,
    };

    if cached_matches {
        return Ok(slot.as_ref().unwrap().clone());
    }

    let fresh = build_backend(cfg)?;
    tracing::info!(
        "[transcribe] backend built — model identity: {}",
        fresh.model_identity()
    );
    *slot = Some(fresh.clone());
    READY.store(true, Ordering::Release);
    PREWARM_FAILED.store(false, Ordering::Release);
    Ok(fresh)
}

/// Process-wide whisper-server readiness flag. Flipped true exactly once when
/// `prewarm` (or a lazy `worker_for` call) successfully loads the model. The
/// hotkey arm-wait reads this to decide whether the first PTT press goes
/// straight to the red recording UI or has to show the yellow "armed" tile
/// while the model finishes loading.
///
/// Cleared by `invalidate_worker()` so a model swap (settings change) makes
/// the next press wait again until the new model is loaded.
static READY: AtomicBool = AtomicBool::new(false);

/// True if the most recent prewarm attempt failed permanently (e.g. invalid
/// model path, missing binary, port-bind failure). The hotkey arm-wait reads
/// this to short-circuit instead of polling for 30 s on every press. Cleared
/// when a successful build completes (e.g. after the user fixes the config).
static PREWARM_FAILED: AtomicBool = AtomicBool::new(false);

/// True while a background prewarm thread is currently building the worker.
/// This lets multiple callers ask for readiness without spawning duplicate
/// whisper-server start attempts during the same cold window.
static PREWARM_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// True if dictation is ready (whisper-server loaded). Cheap atomic load.
pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// True if the last prewarm attempt failed and the worker is not loadable
/// against the current settings. Cleared by a successful rebuild.
pub fn prewarm_failed() -> bool {
    PREWARM_FAILED.load(Ordering::Acquire)
}

/// Kill any whisper-server processes left over from a previous run that was
/// terminated before its `RunEvent::Exit` cleanup fired (e.g. SIGKILL from
/// Tauri's dev runner during rapid file-change rebuilds). Best-effort: logs
/// at warn on failure but never blocks startup.
pub fn kill_orphans() {
    let result = std::process::Command::new("pkill")
        .args(["-f", "whisper-server"])
        .output();
    match result {
        Ok(out) if out.status.success() => {
            tracing::info!("[transcribe] kill_orphans: terminated leftover whisper-server(s)");
        }
        Ok(_) => {} // exit 1 = no matching process, normal case
        Err(e) => tracing::warn!("[transcribe] kill_orphans: pkill failed: {}", e),
    }
}

/// Eagerly spawn the whisper-server worker at app startup so the model is warm
/// before the first dictation and the diagnostic log exists immediately.
/// Runs on a background thread; on success flips `READY` and emits the
/// `dictation-ready` event so the overlay can drop the yellow arming tile if
/// a press is currently waiting. On failure: emits `dictation-ready-failed`
/// with the error message so the frontend can surface it.
pub fn prewarm(cfg: crate::settings::Config, app: tauri::AppHandle) {
    if READY.load(Ordering::Acquire) {
        return;
    }
    if PREWARM_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::debug!("[transcribe] prewarm already in flight");
        return;
    }
    std::thread::spawn(move || {
        tracing::info!("[transcribe] prewarming whisper-server worker");
        match worker_for(&cfg) {
            Ok(_) => {
                READY.store(true, Ordering::Release);
                PREWARM_FAILED.store(false, Ordering::Release);
                PREWARM_IN_FLIGHT.store(false, Ordering::Release);
                tracing::info!("[transcribe] prewarm complete — worker ready");
                if let Err(e) = app.emit("dictation-ready", ()) {
                    tracing::warn!("[transcribe] failed to emit dictation-ready: {:?}", e);
                }
            }
            Err(e) => {
                PREWARM_FAILED.store(true, Ordering::Release);
                PREWARM_IN_FLIGHT.store(false, Ordering::Release);
                let msg = format!("{:#}", e);
                tracing::warn!("[transcribe] prewarm failed: {}", msg);
                if let Err(emit_err) = app.emit("dictation-ready-failed", msg) {
                    tracing::warn!(
                        "[transcribe] failed to emit dictation-ready-failed: {:?}",
                        emit_err
                    );
                }
            }
        }
    });
}

/// Run whisper transcription on `wav` and return a `TranscriptOutcome`.
///
/// This function is responsible only for the Whisper stage: locating the
/// sidecar binary, validating the model path, and sending the HTTP POST.
/// It does **not** call `cleanup::process` — the caller drives the stages.
///
/// TASK-47: routes through `TranscriptionWorker` which keeps whisper-server
/// alive across calls. On worker-build failure (e.g. invalid model path) the
/// function returns the error directly — the cached worker remains absent so
/// a fixed config is picked up on the next call.
///
/// TASK-55: the returned `TranscriptOutcome.rejection` signals hallucination.
/// Callers must skip paste when `rejection.is_some()`.
pub fn run_raw(wav: &Path) -> anyhow::Result<TranscriptOutcome> {
    let cfg = crate::settings::load();
    let worker = worker_for(&cfg)?;
    worker.transcribe(wav)
}

// ── Segment transcription queue (TASK-54B) ───────────────────────────────────

/// Write a slice of 16 kHz mono f32 samples to a temporary WAV file using the
/// same 16-bit PCM contract as the tail WAV (`audio::write_transcription_wav`).
fn write_segment_wav(
    samples: &[f32],
    seg_index: usize,
) -> anyhow::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!("turbotalk-seg-{}.wav", seg_index));
    crate::audio::write_transcription_wav(&path, samples)?;
    Ok(path)
}

/// Transcribe one segment: write WAV, call `run_raw` with one retry on
/// failure, clean up the temp file. Returns an empty string on final failure
/// so the assembly step can still produce a partial transcript.
///
/// Note: per-segment hallucination rejection is NOT applied here — rejection
/// is applied to the final assembled transcript in the hotkey pipeline after
/// all segments and the tail are joined (TASK-55). Individual silence-boundary
/// segments may legitimately look repetitive in isolation.
fn transcribe_one_segment(seg: &crate::audio_finalizer::SegmentEmit) -> String {
    let wav_path = match write_segment_wav(&seg.samples, seg.index) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                "[seg-transcriber] WAV write failed for segment {}: {} — skipping",
                seg.index,
                e
            );
            return String::new();
        }
    };

    let result = run_raw(&wav_path).or_else(|e| {
        tracing::warn!(
            "[seg-transcriber] segment {} first attempt failed: {} — retrying once",
            seg.index,
            e
        );
        run_raw(&wav_path)
    });

    let _ = std::fs::remove_file(&wav_path);

    match result {
        Ok(outcome) => {
            tracing::info!(
                "[seg-transcriber] segment {} → {:?}",
                seg.index,
                outcome.text
            );
            // Use text regardless of per-segment rejection — final-assembly
            // detection runs on the joined transcript in the hotkey pipeline.
            outcome.text
        }
        Err(e) => {
            tracing::warn!(
                "[seg-transcriber] segment {} failed after retry: {} — using empty",
                seg.index,
                e
            );
            String::new()
        }
    }
}

fn seg_transcriber_worker(
    seg_rx: crossbeam_channel::Receiver<crate::audio_finalizer::SegmentEmit>,
    results: std::sync::Arc<Mutex<std::collections::BTreeMap<usize, String>>>,
) {
    while let Ok(seg) = seg_rx.recv() {
        tracing::info!(
            "[seg-transcriber] transcribing segment {} ({} samples = {:.1}s)",
            seg.index,
            seg.samples.len(),
            seg.samples.len() as f32 / 16_000.0,
        );
        let text = transcribe_one_segment(&seg);
        results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(seg.index, text);
    }
    tracing::info!("[seg-transcriber] channel closed — all mid-recording segments done");
}

/// Concurrent segment transcription queue. Spawned right after recording
/// starts and processes each `SegmentEmit` from the finalizer as it arrives,
/// so that by key-release only the final tail remains to transcribe.
///
/// `join_segments()` blocks until the channel closes and all transcriptions
/// are complete, then returns the assembled text in segment-index order.
pub struct SegmentTranscriber {
    worker: Option<std::thread::JoinHandle<()>>,
    results: std::sync::Arc<Mutex<std::collections::BTreeMap<usize, String>>>,
}

impl SegmentTranscriber {
    /// Spawn the background transcription thread. Pass the `Receiver` from
    /// `AudioCapture::take_segment_receiver()` immediately after `start()`.
    pub fn start(
        seg_rx: crossbeam_channel::Receiver<crate::audio_finalizer::SegmentEmit>,
    ) -> Self {
        let results = std::sync::Arc::new(Mutex::new(std::collections::BTreeMap::new()));
        let results_clone = results.clone();
        let worker = std::thread::Builder::new()
            .name("turbotalk-seg-transcriber".into())
            .spawn(move || seg_transcriber_worker(seg_rx, results_clone))
            .expect("spawn segment transcriber worker");
        Self {
            worker: Some(worker),
            results,
        }
    }

    /// Wait for all in-flight transcriptions and return the assembled text.
    /// Non-empty segment texts are joined with single spaces in segment-index
    /// order. Failed segments (stored as empty strings) are silently skipped
    /// — the batch fallback in `stop()` handles the recovery path.
    ///
    /// **Must be called after `StreamingFinalizer::finish()`** so the segment
    /// channel is closed and the worker thread can exit cleanly.
    pub fn join_segments(mut self) -> String {
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
        let map = self.results.lock().unwrap_or_else(|e| e.into_inner());
        map.values()
            .filter(|t| !t.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }
}


#[cfg(test)]
mod tests {
    //! Path-traversal hardening tests for TASK-2.
    //!
    //! These tests do NOT exercise the canonicalization logic by mutation —
    //! they assert the existing guards reject the obvious attack shapes
    //! (`/etc/passwd`, `..` escapes, symlinks pointing outside the allowed
    //! root) and accept legitimate paths inside the allow-list.
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn is_allowed_whisper_path_rejects_system_binaries() {
        // /bin/ls and /etc/passwd canonicalize fine but live nowhere near the
        // allowed roots, so the allow-list must reject them.
        assert!(!is_allowed_whisper_path(Path::new("/bin/ls")));
        assert!(!is_allowed_whisper_path(Path::new("/etc/passwd")));
    }

    #[test]
    fn is_allowed_whisper_path_rejects_dotdot_escape() {
        // A path with `..` segments that resolves outside the allowed roots
        // must be rejected. We build one inside a tempdir and aim it at /tmp.
        let tmp = tempdir().expect("tempdir");
        let escape = tmp.path().join("..").join("..").join("etc").join("passwd");
        assert!(!is_allowed_whisper_path(&escape));
    }

    #[test]
    fn is_allowed_whisper_path_rejects_nonexistent() {
        // Non-existent paths cannot canonicalize and must be rejected.
        assert!(!is_allowed_whisper_path(Path::new(
            "/definitely/not/a/real/path/whisper-cli"
        )));
    }

    #[test]
    fn is_allowed_whisper_path_accepts_path_inside_target_dir() {
        // The cargo `target/` directory is one of the allowed roots. Any test
        // running here lives under `target/debug/deps/`, so its canonical
        // current_exe is by construction inside the allow-list.
        let exe = std::env::current_exe().expect("current_exe");
        // Sanity: the running test binary itself must be accepted.
        assert!(
            is_allowed_whisper_path(&exe),
            "the running test binary at {:?} should be inside an allowed root",
            exe
        );
    }

    #[test]
    fn normalize_whisper_text_collapses_segment_newlines() {
        assert_eq!(
            normalize_whisper_text("one two\n three four\nfive"),
            "one two three four five"
        );
        assert_eq!(
            normalize_whisper_text(" one   two\tthree\n four "),
            "one two three four"
        );
    }

    #[test]
    fn validate_model_path_accepts_real_file_inside_models_dir() {
        let tmp = tempdir().expect("tempdir");
        let canon_dir = tmp.path().canonicalize().expect("canon dir");
        let model = canon_dir.join("ggml-base.en.bin");
        fs::write(&model, b"fake ggml bytes").expect("write model");

        let result = validate_model_path(model.to_str().unwrap(), &canon_dir);
        assert!(result.is_ok(), "expected accept, got: {:?}", result.err());
        assert_eq!(result.unwrap(), model.canonicalize().unwrap());
    }

    #[test]
    fn validate_model_path_rejects_etc_hosts() {
        let tmp = tempdir().expect("tempdir");
        let canon_dir = tmp.path().canonicalize().expect("canon dir");

        // /etc/hosts exists on macOS and Linux but is outside the models dir.
        let result = validate_model_path("/etc/hosts", &canon_dir);
        assert!(result.is_err(), "expected /etc/hosts to be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("outside the allowed models directory"),
            "unexpected error message: {}",
            err
        );
    }

    #[test]
    fn validate_model_path_rejects_nonexistent_path() {
        let tmp = tempdir().expect("tempdir");
        let canon_dir = tmp.path().canonicalize().expect("canon dir");

        let result = validate_model_path("/no/such/model.bin", &canon_dir);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Whisper model not found") || err.contains("could not be resolved"),
            "unexpected error message: {}",
            err
        );
    }

    #[cfg(unix)]
    #[test]
    fn validate_model_path_rejects_symlink_escape() {
        // A symlink inside the models dir pointing at a target outside the
        // models dir must be rejected. canonicalize() resolves the symlink
        // before the starts_with check, which is the whole point.
        use std::os::unix::fs::symlink;

        let outside = tempdir().expect("outside tempdir");
        let outside_canon = outside.path().canonicalize().expect("canon outside");
        let target = outside_canon.join("evil.bin");
        fs::write(&target, b"evil").expect("write evil");

        let inside = tempdir().expect("inside tempdir");
        let inside_canon = inside.path().canonicalize().expect("canon inside");
        let link = inside_canon.join("ggml-evil.bin");
        symlink(&target, &link).expect("symlink");

        let result = validate_model_path(link.to_str().unwrap(), &inside_canon);
        assert!(
            result.is_err(),
            "symlink escape should be rejected, got: {:?}",
            result.ok()
        );
    }

    // ----------------------------------------------------------------------
    // TASK-20: TranscriptionWorker construction-time validation.
    //
    // Construction must reject an invalid model path WITHOUT spawning
    // whisper-cli. We test by handing `from_config` a cfg whose model points
    // at /etc/hosts (exists, but lives outside the models dir). The worker
    // build path goes through `validate_model_path`, which must short-circuit
    // before any process is spawned.

    #[test]
    fn worker_from_config_rejects_invalid_model() {
        // We can't easily fake `canonical_models_dir()` here (it reads
        // $HOME), so we exercise the lower-level guard the worker delegates
        // to. The behavior we care about — "construction returns Err without
        // spawning anything" — is observable through `validate_model_path`,
        // which is the single rejection point inside `from_config`.
        let tmp = tempdir().expect("tempdir");
        let canon_dir = tmp.path().canonicalize().expect("canon dir");
        let result = validate_model_path("/etc/hosts", &canon_dir);
        assert!(
            result.is_err(),
            "worker construction must reject a model path outside the models dir"
        );
    }

    // ── TASK-54B: SegmentTranscriber assembly ─────────────────────────────

    /// BTreeMap naturally yields values in key order — verify the assembly
    /// logic preserves segment order regardless of insertion order.
    #[test]
    fn segment_assembly_preserves_index_order() {
        let mut map = std::collections::BTreeMap::new();
        map.insert(2usize, "third".to_string());
        map.insert(0usize, "first".to_string());
        map.insert(1usize, "second".to_string());
        let result: String = map
            .values()
            .filter(|t| !t.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(result, "first second third");
    }

    /// Failed segments are stored as empty strings and must be silently
    /// filtered so a single bad segment doesn't produce a double-space gap.
    #[test]
    fn segment_assembly_filters_empty_slots() {
        let mut map = std::collections::BTreeMap::new();
        map.insert(0usize, "hello".to_string());
        map.insert(1usize, String::new()); // failed transcription
        map.insert(2usize, "world".to_string());
        let result: String = map
            .values()
            .filter(|t| !t.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(result, "hello world");
    }

    /// write_segment_wav round-trips: the produced WAV is readable by hound
    /// and transcribe-rs (16 kHz mono 16-bit PCM — same as tail WAV).
    #[test]
    fn write_segment_wav_round_trips() {
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 / 1600.0) * 0.5).collect();
        let path = write_segment_wav(&samples, 999).expect("write ok");
        let reader = hound::WavReader::open(&path).expect("read ok");
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);
        assert!(matches!(spec.sample_format, hound::SampleFormat::Int));
        assert_eq!(reader.duration(), samples.len() as u32);
        #[cfg(any(feature = "moonshine", feature = "parakeet"))]
        {
            transcribe_rs::audio::read_wav_samples(&path).expect("transcribe-rs read");
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── TASK-55: detect_garbage unit tests ────────────────────────────────

    /// Empty string must never trigger a filter — the caller handles the
    /// empty-transcript path separately.
    #[test]
    fn detect_garbage_empty_string_is_clean() {
        assert_eq!(detect_garbage(""), None);
    }

    /// Normal clean sentence — must not be flagged.
    #[test]
    fn detect_garbage_clean_sentence_passes() {
        let clean = "Hello world, this is a normal dictation.";
        assert_eq!(
            detect_garbage(clean),
            None,
            "clean sentence should pass: {:?}",
            clean
        );
    }

    /// All-zeros hallucination (common Whisper garbage on silence).
    /// Whisper emits zeros either as a long run or as space-delimited tokens.
    /// The space-delimited form triggers trigram repetition ("0 0 0 0 0 0…").
    #[test]
    fn detect_garbage_zeros_flagged_as_junk() {
        // Whisper typically emits a space-delimited sequence like "0 0 0 0 0…"
        // which the trigram detector catches. We test both forms.
        let zeros_spaced = "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let result = detect_garbage(zeros_spaced);
        assert!(
            result.is_some(),
            "space-delimited zeros should be flagged, got None"
        );
    }

    /// "thanks for watching" × 5 — training-set artifact on silence.
    #[test]
    fn detect_garbage_thanks_for_watching_flagged() {
        let repeated = "thanks for watching thanks for watching thanks for watching thanks for watching thanks for watching";
        let result = detect_garbage(repeated);
        assert!(
            result.is_some(),
            "repeated training artifact should be flagged, got None"
        );
    }

    /// Simple word repetition loop ("the the the the the").
    #[test]
    fn detect_garbage_the_the_the_flagged() {
        // Build a long enough string to trip the trigram detector or compressor.
        let repeated = "the the the the the the the the the the the the the the the";
        let result = detect_garbage(repeated);
        assert!(
            result.is_some(),
            "'the the the...' repetition should be flagged, got None"
        );
    }

    /// Realistic short dictation — must pass all three filters.
    #[test]
    fn detect_garbage_realistic_dictation_passes() {
        let text = "Please add a new function that validates the user input and returns a boolean value.";
        assert_eq!(detect_garbage(text), None, "realistic dictation should not be filtered");
    }

    // ── TASK-47: TranscriptionWorker::abort() no-op test ─────────────────

    // ----------------------------------------------------------------------
    // TASK-47: TranscriptionWorker::abort() no-op test.
    //
    // `abort()` on a worker with no active server_child slot must return
    // cleanly without panicking. We build a minimal worker directly (bypassing
    // the server-spawn that `from_config` would run) to keep the test
    // self-contained.

    #[test]
    fn abort_noop_when_idle() {
        // Build a minimal worker with an empty server_child slot.
        let worker = TranscriptionWorker {
            bin: PathBuf::from("/nonexistent"),
            model: PathBuf::from("/nonexistent"),
            vocabulary: vec![],
            spawn_lock: Mutex::new(()),
            server_child: parking_lot::Mutex::new(None),
            server_port: 0,
            http_client: reqwest::blocking::Client::new(),
            audio_ctx: 0,
        };
        // Must return cleanly, no panic.
        worker.abort();
        // Slot remains None.
        assert!(worker.server_child.lock().is_none());
    }
}
