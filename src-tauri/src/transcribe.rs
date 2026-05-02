// Spawns whisper-cli, feeds it the WAV, reads back the transcript.
// Output strategy: -otxt writes <wav_path>.wav.txt; we read and delete that file.
use std::path::{Path, PathBuf};

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

/// Locate the whisper-cli binary.
/// Priority: bundled sidecar (next to exe) → dev binaries dir → configured path.
/// The configured-path fallback is only honored if the path canonicalizes to
/// a location inside an allowed root; otherwise an error is returned.
fn find_whisper(configured_bin: &str) -> anyhow::Result<PathBuf> {
    let sidecar = "whisper-cli-aarch64-apple-darwin";

    // Release bundle: sidecar is placed next to the main executable in Contents/MacOS/
    if let Ok(exe) = std::env::current_exe() {
        let p = exe.parent().unwrap_or_else(|| Path::new(".")).join(sidecar);
        if p.exists() {
            tracing::debug!("[transcribe] using bundled sidecar: {:?}", p);
            return Ok(p);
        }
    }

    // Dev mode: sidecar lives in src-tauri/binaries/ at compile time
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(sidecar);
    if dev.exists() {
        tracing::debug!("[transcribe] using dev sidecar: {:?}", dev);
        return Ok(dev);
    }

    // Last resort: configured path. Validated against the allow-list to prevent
    // arbitrary code execution via a tampered config.toml.
    let configured = PathBuf::from(configured_bin);
    if !is_allowed_whisper_path(&configured) {
        tracing::error!(
            "[transcribe] rejected configured whisper bin (outside allowed roots): {}",
            configured_bin
        );
        anyhow::bail!(
            "configured whisper.bin is outside allowed locations: {} — \
             remove it from config.toml so the bundled sidecar is used",
            configured_bin
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
            "model path does not exist or could not be resolved: {}",
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

/// Run whisper-cli on `wav` and return the **raw** trimmed transcript text.
///
/// This function is responsible only for the Whisper stage: locating the
/// sidecar binary, validating the model path, spawning the process, and
/// reading back the `.txt` output. It does **not** call `cleanup::process` —
/// the caller is expected to drive the `Transcribing → Cleaning → Pasting`
/// stages explicitly so each stage's latency is observable (TASK-15).
pub fn run_raw(wav: &Path) -> anyhow::Result<String> {
    let cfg = crate::settings::load();
    let bin = find_whisper(&cfg.whisper.bin)?;

    // Canonicalize the model path and verify it lives inside the allowed
    // models directory. Blocks `model = "/etc/passwd"` style attacks and
    // symlink escapes from the models dir.
    let canon_models_dir = crate::settings::canonical_models_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "models directory does not exist — create ~/.config/librewin/turbotalk/models/ \
             and place a ggml model there"
        )
    })?;
    let canon_model = validate_model_path(&cfg.whisper.model, &canon_models_dir)?;
    let model_str = canon_model
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("model path is not valid UTF-8: {:?}", canon_model))?;

    // whisper-cli appends .txt to the full input filename: <wav>.wav.txt
    let txt_path = PathBuf::from(format!("{}.txt", wav.display()));

    // Flags tuned for short-form push-to-talk dictation (not long-form transcription):
    //   -mc 0            max-context 0 = don't carry prior-segment text into decoding
    //                    (whisper.cpp's equivalent of OpenAI Whisper's --no-context)
    //   --beam-size 5    moderate bump from default 1; better short-utterance accuracy
    //   --temperature 0  deterministic decoding; whisper.cpp still falls back internally on no-speech
    //   --suppress-nst   suppress non-speech tokens (e.g. <|nospeech|>); pairs with VAD
    // The user-editable `cleanup.vocabulary` (already used by the Chaperone classifier) is
    // also fed to whisper as `--prompt` to bias spelling of names/jargon/identifiers.
    let mut args: Vec<String> = vec![
        "-m".into(), model_str.to_string(),
        "-f".into(), wav.to_str().unwrap().to_string(),
        "-otxt".into(),
        "-np".into(),
        "-nt".into(),
        "-l".into(), "en".into(),
        "-mc".into(), "0".into(),
        "--beam-size".into(), "5".into(),
        "--temperature".into(), "0".into(),
        "--suppress-nst".into(),
    ];
    if !cfg.cleanup.vocabulary.is_empty() {
        args.push("--prompt".into());
        args.push(cfg.cleanup.vocabulary.join(", "));
    }

    let output = std::process::Command::new(&bin)
        .args(&args)
        .output()?;

    // Even on exit-0, stderr can contain warnings ("argument not recognized") that
    // explain why the .txt below is missing. Log it at debug; promote to warn if
    // the next read fails.
    if !output.stderr.is_empty() {
        tracing::debug!("[transcribe] whisper-cli stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("whisper-cli failed ({}): {}", output.status, stderr);
    }

    let text = std::fs::read_to_string(&txt_path).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::anyhow!(
            "whisper output file not found: {:?}\n--- whisper-cli stderr ---\n{}",
            txt_path,
            stderr
        )
    })?;
    let _ = std::fs::remove_file(&txt_path);

    Ok(text.trim().to_string())
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
            err.contains("does not exist") || err.contains("could not be resolved"),
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
}
