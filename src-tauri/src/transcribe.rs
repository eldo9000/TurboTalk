// Spawns whisper-server as a long-lived sidecar, feeds it the WAV via HTTP
// POST /inference, and reads back the JSON transcript.
//
// TASK-47: replace the per-call whisper-cli spawn (TASK-20 option 3) with a
// persistent whisper-server that keeps the model loaded across dictations.
// The server is spawned once per worker lifetime (one per model config); the
// worker is cached in the process-wide WORKER slot and rebuilt only when the
// model changes or after an abort.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tauri::Emitter;

/// Allowed roots for the whisper binary:
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

    for sidecar in &sidecars {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(sidecar);
        if dev.exists() {
            tracing::debug!("[transcribe] using dev sidecar: {:?}", dev);
            return Ok(dev);
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
/// Priority: dev binaries dir → bundled sidecar (next to exe) → configured path.
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
    for sidecar in &sidecars {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(sidecar);
        if dev.exists() {
            tracing::debug!("[transcribe] using dev whisper-server sidecar: {:?}", dev);
            return Ok(dev);
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
    tracing::debug!("[transcribe] using configured whisper-server bin: {}", configured_bin);
    Ok(configured)
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

/// Lifecycle owner for whisper transcription. TASK-47: this struct spawns a
/// long-lived `whisper-server` process at construction time and reuses it for
/// all subsequent `transcribe` calls — keeping the model warm across
/// dictations and eliminating the per-call model-reload latency.
///
/// The internal `spawn_lock` enforces the one-in-flight invariant from
/// TASK-14: any second concurrent caller blocks here rather than racing the
/// HTTP POST. `server_child` is protected by a separate `parking_lot::Mutex`
/// so `abort()` can kill the server from another thread without taking the
/// coarser `spawn_lock`.
pub struct TranscriptionWorker {
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

impl TranscriptionWorker {
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
        for var in &["HOME", "PATH", "TMPDIR", "USER", "LOGNAME"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
        // SAFETY: setsid() is async-signal-safe and has no Rust invariants.
        // It must be called after fork but before exec, which is exactly what
        // pre_exec guarantees. Failure is intentionally ignored: setsid()
        // returns EPERM if the process is already a group leader (harmless).
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
        let http_client = reqwest::blocking::Client::new();
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

    /// POST the WAV file to `/inference` and return the **raw** trimmed
    /// transcript text. Holds `spawn_lock` for the whole call.
    pub fn transcribe(&self, wav: &Path) -> anyhow::Result<String> {
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
        let text = json["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("whisper-server response missing 'text' field"))?
            .trim()
            .to_string();

        let whisper_ms = t_whisper_start.elapsed().as_millis();
        tracing::info!("[transcribe] whisper took {} ms", whisper_ms);
        tracing::info!("[transcribe] transcript: {:?}", text);

        Ok(text)
    }

    /// Kill the whisper-server subprocess. Best-effort: logs at warn on
    /// failure. No-op if the server has already exited.
    ///
    /// After `abort()` the worker is in a broken state — the caller must
    /// rebuild. `abort_active()` calls `invalidate_worker()` after this.
    pub fn abort(&self) {
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

impl Drop for TranscriptionWorker {
    fn drop(&mut self) {
        let mut slot = self.server_child.lock();
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
            tracing::info!("[transcribe] whisper-server stopped");
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
}

/// Get-or-build the worker against the current settings snapshot. If the
/// cached worker's model differs from the current `cfg.whisper.model`, it is
/// dropped and rebuilt. Returns an `Arc` so the spawn call can drop the outer
/// mutex before the (potentially long) HTTP POST.
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
    READY.store(true, Ordering::Release);
    PREWARM_FAILED.store(false, Ordering::Release);
    tracing::info!(
        "[transcribe] worker built for model {:?}",
        fresh.model_path()
    );
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

/// True if dictation is ready (whisper-server loaded). Cheap atomic load.
pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// True if the last prewarm attempt failed and the worker is not loadable
/// against the current settings. Cleared by a successful rebuild.
pub fn prewarm_failed() -> bool {
    PREWARM_FAILED.load(Ordering::Acquire)
}

/// Eagerly spawn the whisper-server worker at app startup so the model is warm
/// before the first dictation and the diagnostic log exists immediately.
/// Runs on a background thread; on success flips `READY` and emits the
/// `dictation-ready` event so the overlay can drop the yellow arming tile if
/// a press is currently waiting. On failure: emits `dictation-ready-failed`
/// with the error message so the frontend can surface it.
pub fn prewarm(cfg: crate::settings::Config, app: tauri::AppHandle) {
    std::thread::spawn(move || {
        tracing::info!("[transcribe] prewarming whisper-server worker");
        match worker_for(&cfg) {
            Ok(_) => {
                READY.store(true, Ordering::Release);
                PREWARM_FAILED.store(false, Ordering::Release);
                tracing::info!("[transcribe] prewarm complete — worker ready");
                if let Err(e) = app.emit("dictation-ready", ()) {
                    tracing::warn!("[transcribe] failed to emit dictation-ready: {:?}", e);
                }
            }
            Err(e) => {
                PREWARM_FAILED.store(true, Ordering::Release);
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

/// Run whisper transcription on `wav` and return the **raw** trimmed transcript.
///
/// This function is responsible only for the Whisper stage: locating the
/// sidecar binary, validating the model path, and sending the HTTP POST.
/// It does **not** call `cleanup::process` — the caller drives the stages.
///
/// TASK-47: routes through `TranscriptionWorker` which keeps whisper-server
/// alive across calls. On worker-build failure (e.g. invalid model path) the
/// function returns the error directly — the cached worker remains absent so
/// a fixed config is picked up on the next call.
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
