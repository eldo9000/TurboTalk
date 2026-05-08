// Spawns whisper-cli, feeds it the WAV, reads back the transcript.
// Output strategy: -otxt writes <wav_path>.wav.txt; we read and delete that file.
//
// TASK-20 (option 3): the spawn lifecycle is wrapped in `TranscriptionWorker`
// so callers go through one type with a `Mutex` enforcing the one-in-flight
// invariant from TASK-14. The worker still spawns `whisper-cli` per call —
// there is **no model warmup** in this option. Options 1 (`whisper-rs`) and
// 2 (`whisper-server` long-lived sidecar) were both blocked on this host on
// 2026-05-02:
//   - Option 1: `cargo check` of `whisper-rs = "0.16"` (metal feature) hung
//     in `whisper-rs-sys`'s build script for 300+ s, same `cmTC_*` cmake
//     probe symptom as the original TASK-18 deferral. Repro confirmed.
//   - Option 2: `whisper-server` is not bundled in `src-tauri/binaries/`,
//     and downloading external binaries is out of scope for this task.
// See `tasks/done/TASK-18-persistent-whisper-transcription-worker.md` for
// full deferral evidence.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Allowed roots for the whisper-cli binary:
/// - the directory containing the running executable (release bundle sidecar)
/// - the cargo target/ tree of this crate (dev builds)
/// - the `src-tauri/binaries/` directory bundled at compile time (dev fallback)
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

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Ok(canon) = manifest_dir.join("target").canonicalize() {
        roots.push(canon);
    }
    if let Ok(canon) = manifest_dir.join("binaries").canonicalize() {
        roots.push(canon);
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

/// Build the list of candidate sidecar filenames to search for, in priority
/// order. Tauri convention is to suffix the externalBin name with the target
/// triple at bundle time (and `.exe` on Windows), so the per-target candidate
/// matches what ends up in the release bundle. The unsuffixed `whisper-cli`
/// candidate covers dev builds where the binary may have been dropped in
/// `src-tauri/binaries/` without the triple suffix.
///
/// TARGET_TRIPLE is injected by `build.rs` from Cargo's `TARGET` build-script
/// env var.
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

/// Locate the whisper-cli binary.
/// Priority: bundled sidecar (next to exe) → dev binaries dir → configured path.
/// The configured-path fallback is only honored if the path canonicalizes to
/// a location inside an allowed root; otherwise an error is returned.
fn find_whisper(configured_bin: &str) -> anyhow::Result<PathBuf> {
    let sidecars = sidecar_candidates();

    // Release bundle: sidecar is placed next to the main executable in Contents/MacOS/
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

    // Dev mode: sidecar lives in src-tauri/binaries/ at compile time
    for sidecar in &sidecars {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(sidecar);
        if dev.exists() {
            tracing::debug!("[transcribe] using dev sidecar: {:?}", dev);
            return Ok(dev);
        }
    }

    // Last resort: configured path. Validated against the allow-list to prevent
    // arbitrary code execution via a tampered config.toml.
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

/// Canonicalize `raw_model` and verify it lives inside `canon_models_dir`.
/// Blocks `model = "/etc/passwd"` style attacks and symlink escapes from the
/// models dir. Returns the canonicalized model path on success.
///
/// Extracted from `run()` so unit tests can exercise the path-traversal
/// guard against a temp dir without spawning whisper-cli.
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

/// Lifecycle owner for whisper transcription. TASK-20 option 3: this struct
/// holds the validated model path and prompt for the current configuration so
/// callers don't re-run path validation per recording. The internal `Mutex`
/// makes the one-in-flight invariant from TASK-14 explicit at the type level
/// — any second concurrent caller blocks here rather than racing the spawn.
///
/// **This implementation does NOT keep the whisper model warm.** Each
/// `transcribe` call still spawns `whisper-cli` and reloads the model. Options
/// 1 (`whisper-rs`) and 2 (`whisper-server`) were both blocked on this host
/// on 2026-05-02 — see the module-level deferral note. A real warm worker
/// remains future work; this struct is the seam where it will eventually
/// plug in.
pub struct TranscriptionWorker {
    /// Canonicalized whisper-cli binary path. Validated at construction.
    bin: PathBuf,
    /// Canonicalized model path. Validated at construction; lives inside
    /// `~/.config/librewin/turbotalk/models/`.
    model: PathBuf,
    /// Vocabulary phrases passed to whisper as `--prompt`. Empty = no prompt.
    vocabulary: Vec<String>,
    /// Spawn serialization. Held across the whole `transcribe` call so the
    /// process tree can never have two whisper-cli instances at once.
    /// (Option 3 has no warm context to protect, but this matches the shape
    /// options 1 and 2 will need.)
    spawn_lock: Mutex<()>,
    /// The active whisper-cli child process. Set immediately after `spawn()`
    /// and cleared on completion. Allows `abort()` to kill the subprocess
    /// mid-transcription (TASK-23).
    active_child: parking_lot::Mutex<Option<std::process::Child>>,
}

impl TranscriptionWorker {
    /// Build a worker from a snapshot of the current settings. Validates the
    /// binary path and the model path eagerly so a misconfigured `config.toml`
    /// surfaces at construction time, not deep inside a spawn call.
    pub fn from_config(cfg: &crate::settings::Config) -> anyhow::Result<Self> {
        let bin = find_whisper(&cfg.whisper.bin)?;
        let canon_models_dir = crate::settings::canonical_models_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "models directory does not exist — create ~/.config/librewin/turbotalk/models/ \
                 and place a ggml model there"
            )
        })?;
        let model = validate_model_path(&cfg.whisper.model, &canon_models_dir)?;
        Ok(Self {
            bin,
            model,
            vocabulary: cfg.cleanup.vocabulary.clone(),
            spawn_lock: Mutex::new(()),
            active_child: parking_lot::Mutex::new(None),
        })
    }

    /// The canonicalized model path this worker was built against. Callers
    /// can compare against a fresh `cfg.whisper.model` to decide whether to
    /// rebuild the worker.
    pub fn model_path(&self) -> &Path {
        &self.model
    }

    /// Run whisper-cli on `wav` and return the **raw** trimmed transcript text.
    /// Holds `spawn_lock` for the whole call.
    ///
    /// TASK-23: uses `spawn()` + `wait_with_output()` (instead of `output()`)
    /// so the active `Child` is stored in `self.active_child` while running.
    /// This allows `abort()` to kill the subprocess from another thread.
    /// Behavior on the happy path is identical to the prior `output()` approach.
    pub fn transcribe(&self, wav: &Path) -> anyhow::Result<String> {
        let _guard = self.spawn_lock.lock().unwrap_or_else(|e| e.into_inner());

        let model_str = self
            .model
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("model path is not valid UTF-8: {:?}", self.model))?;

        // whisper-cli appends .txt to the full input filename: <wav>.wav.txt
        let txt_path = PathBuf::from(format!("{}.txt", wav.display()));

        // Flags tuned for short-form push-to-talk dictation (not long-form transcription):
        //   -mc 0            max-context 0 = don't carry prior-segment text into decoding
        //                    (whisper.cpp's equivalent of OpenAI Whisper's --no-context)
        //   --beam-size 1    greedy decode; set alongside --best-of 1 for true greedy
        //   --best-of 1      required for true greedy — default best_of=5 would still run
        //                    5 candidate decodes even with beam-size 1
        //   --temperature 0  deterministic decoding; whisper.cpp still falls back internally on no-speech
        //   --suppress-nst   suppress non-speech tokens (e.g. <|nospeech|>); pairs with VAD
        // The user-editable `cleanup.vocabulary` (already used by the Chaperone classifier) is
        // also fed to whisper as `--prompt` to bias spelling of names/jargon/identifiers.
        let mut args: Vec<String> = vec![
            "-m".into(),
            model_str.to_string(),
            "-f".into(),
            wav.to_str().unwrap().to_string(),
            "-otxt".into(),
            "-np".into(),
            "-nt".into(),
            "-l".into(),
            "en".into(),
            "-mc".into(),
            "0".into(),
            "--beam-size".into(),
            "1".into(),
            "--best-of".into(),
            "1".into(),
            "--temperature".into(),
            "0".into(),
            "--suppress-nst".into(),
        ];
        if !self.vocabulary.is_empty() {
            args.push("--prompt".into());
            args.push(self.vocabulary.join(", "));
        }

        // TASK-21: capture per-recording whisper subprocess wall time so we can
        // compare it against the audio-finalization stage sum and decide whether
        // the streaming finalizer in TASK-19 is worth implementing. The
        // measurement brackets exactly the spawn → exit window — the model load
        // and the actual decode are both inside it, which is what we want.
        let t_whisper_start = Instant::now();

        // Compute the GGML backend search path so whisper-cli loads the bundled
        // Metal backend instead of falling back to the hardcoded Homebrew path
        // baked into libggml.0.dylib.
        //
        // The libggml.0.dylib bundled in src-tauri/binaries/ (version 0.10.1) has
        // "/opt/homebrew/Cellar/ggml/0.10.1/libexec" as its compile-time default
        // backend search path.  That path does not exist on a Homebrew-free Mac,
        // so ggml would find no backends and fall back to a slow software path —
        // or fail entirely.  Setting GGML_BACKEND_PATH overrides the default and
        // points ggml at the .so files we bundle alongside the app.
        //
        // Path resolution:
        //   dev:      self.bin lives in src-tauri/binaries/ → .so files are there too
        //   packaged: self.bin lives in Contents/MacOS/whisper-cli,
        //             Tauri bundles resources to Contents/Resources/,
        //             so the .so files are one level up and over.
        let backend_dir: std::path::PathBuf = {
            let bin_parent = self.bin.parent().unwrap_or_else(|| std::path::Path::new("."));
            let resources_candidate = bin_parent.join("../Resources");
            // In a packaged .app the Resources dir is a real directory.
            // In dev (binaries/ dir) it does not exist, so we stay with bin_parent.
            if resources_candidate.exists() {
                resources_candidate
            } else {
                bin_parent.to_path_buf()
            }
        };
        tracing::debug!("[transcribe] GGML_BACKEND_PATH = {:?}", backend_dir);

        // TASK-23: spawn the child and immediately store it in `active_child` so
        // `abort()` can kill it mid-transcription. On completion we clear the slot.
        let mut child = std::process::Command::new(&self.bin)
            .args(&args)
            .env("GGML_BACKEND_PATH", &backend_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child.stderr.take();
        let stderr_reader = stderr.map(|mut stderr| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = stderr.read_to_end(&mut buf);
                buf
            })
        });
        *self.active_child.lock() = Some(child);

        // Poll instead of `wait_with_output()` so `abort()` can take and kill
        // the child while transcription is in flight.
        let status = loop {
            let maybe_status = {
                let mut slot = self.active_child.lock();
                let Some(child) = slot.as_mut() else {
                    anyhow::bail!("Transcription cancelled.");
                };
                child.try_wait()?
            };
            if let Some(status) = maybe_status {
                *self.active_child.lock() = None;
                break status;
            }
            std::thread::sleep(Duration::from_millis(20));
        };

        let stderr = stderr_reader
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        let whisper_ms = t_whisper_start.elapsed().as_millis();
        tracing::info!("[transcribe] whisper took {} ms", whisper_ms);

        // Even on exit-0, stderr can contain warnings ("argument not recognized") that
        // explain why the .txt below is missing. Log it at debug; promote to warn if
        // the next read fails.
        if !stderr.is_empty() {
            tracing::debug!(
                "[transcribe] whisper-cli stderr: {}",
                String::from_utf8_lossy(&stderr)
            );
        }

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            anyhow::bail!(
                "Transcription failed (whisper-cli error {}). Check that the model file is valid.",
                code
            );
        }

        let text = std::fs::read_to_string(&txt_path).map_err(|_| {
            let stderr = String::from_utf8_lossy(&stderr);
            anyhow::anyhow!(
                "whisper output file not found: {:?}\n--- whisper-cli stderr ---\n{}",
                txt_path,
                stderr
            )
        })?;
        let _ = std::fs::remove_file(&txt_path);

        let trimmed = text.trim().to_string();
        tracing::info!("[transcribe] transcript: {:?}", trimmed);
        Ok(trimmed)
    }

    /// Kill any active whisper-cli subprocess. Best-effort: if the process has
    /// already exited, or if the kill syscall fails for any reason, we log at
    /// warn and move on. No-op if no transcription is currently in flight.
    ///
    /// Called by `Recorder::cancel()` when the user triggers a cancel
    /// while a recording is in the `Transcribing` state.
    ///
    /// The abort path is fully unit-testable — see the `abort_noop_when_idle`
    /// test in the tests module below.
    pub fn abort(&self) {
        let mut slot = self.active_child.lock();
        if let Some(mut child) = slot.take() {
            if let Err(e) = child.kill() {
                tracing::warn!("[transcribe] abort: child.kill() failed: {}", e);
            } else {
                tracing::info!("[transcribe] abort: whisper-cli subprocess killed");
            }
            let _ = child.wait();
        }
    }
}

/// Process-wide handle to the active worker. `None` on cold start and after
/// model invalidation; rebuilt lazily by `run_raw`.
///
/// The outer `Mutex` is the sequencing point — it serializes `take` /
/// `replace` / read accesses across the recorder, settings, and any future
/// app-shutdown drop site. The inner `Option` represents "worker not yet
/// built (or invalidated)".
static WORKER: Mutex<Option<std::sync::Arc<TranscriptionWorker>>> = Mutex::new(None);

/// Abort any in-flight whisper-cli subprocess. Called by `Recorder::cancel()`
/// when a cancel is triggered while in `Transcribing` state (TASK-23).
/// If no worker is cached or no subprocess is active, this is a no-op.
pub fn abort_active() {
    let slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(worker) = &*slot {
        worker.abort();
    }
}

/// Drop the cached worker. Called by `settings::save` (via `lib.rs`) when the
/// user changes the model — the next `run_raw` will rebuild against the new
/// config. Idempotent.
pub fn invalidate_worker() {
    let mut slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        tracing::info!("[transcribe] worker invalidated");
    }
    *slot = None;
}

/// Get-or-build the worker against the current settings snapshot. If the
/// cached worker's model differs from the current `cfg.whisper.model`, it is
/// dropped and rebuilt. Returns an `Arc` so the spawn call can drop the outer
/// mutex before the (potentially long) whisper invocation.
fn worker_for(
    cfg: &crate::settings::Config,
) -> anyhow::Result<std::sync::Arc<TranscriptionWorker>> {
    let mut slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());

    // Cheap cache-validity check: compare canonicalized model paths.
    // canonicalize() may fail if the file was deleted out from under us —
    // treat that as "rebuild and let the build error surface".
    let configured_canon = std::path::PathBuf::from(&cfg.whisper.model)
        .canonicalize()
        .ok();
    let cached_matches = match (&*slot, configured_canon.as_ref()) {
        (Some(w), Some(c)) => w.model_path() == c.as_path(),
        _ => false,
    };

    if cached_matches {
        return Ok(slot.as_ref().unwrap().clone());
    }

    let fresh = std::sync::Arc::new(TranscriptionWorker::from_config(cfg)?);
    *slot = Some(fresh.clone());
    tracing::info!(
        "[transcribe] worker built for model {:?}",
        fresh.model_path()
    );
    Ok(fresh)
}

/// Run whisper-cli on `wav` and return the **raw** trimmed transcript text.
///
/// This function is responsible only for the Whisper stage: locating the
/// sidecar binary, validating the model path, spawning the process, and
/// reading back the `.txt` output. It does **not** call `cleanup::process` —
/// the caller is expected to drive the `Transcribing → Cleaning → Pasting`
/// stages explicitly so each stage's latency is observable (TASK-15).
///
/// TASK-20: routes through `TranscriptionWorker` for lifecycle ownership.
/// On worker-build failure (e.g. invalid model path) the function returns
/// the error directly — the cached worker remains absent so a fixed config
/// is picked up on the next call.
pub fn run_raw(wav: &Path) -> anyhow::Result<String> {
    let cfg = crate::settings::load();
    let worker = worker_for(&cfg)?;
    worker.transcribe(wav)
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

    // ----------------------------------------------------------------------
    // TASK-23: TranscriptionWorker::abort() no-op test.
    //
    // `abort()` on a worker with no active Child must return cleanly without
    // panicking. We build a minimal worker directly (bypassing the model-path
    // validation that `from_config` would run) to keep the test self-contained.

    #[test]
    fn abort_noop_when_idle() {
        // Build a minimal worker with an empty active_child slot.
        let worker = TranscriptionWorker {
            bin: PathBuf::from("/nonexistent"),
            model: PathBuf::from("/nonexistent"),
            vocabulary: vec![],
            spawn_lock: Mutex::new(()),
            active_child: parking_lot::Mutex::new(None),
        };
        // Must return cleanly, no panic.
        worker.abort();
        // Slot remains None.
        assert!(worker.active_child.lock().is_none());
    }
}
