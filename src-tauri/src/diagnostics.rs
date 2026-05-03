// Diagnostics command — surface app runtime health for beta users.
//
// Run via `invoke("run_diagnostics")` from the frontend. Each check is
// wrapped in a `match`/`unwrap_or` so a failing check returns a descriptive
// string rather than crashing. The command never panics.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// All fields are `String` or `bool` for trivial JSON serialisation. String
/// fields use sentinel values ("ok", "missing", "error: …") so the frontend
/// can render them without additional type magic.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DiagnosticsResult {
    /// `std::env::consts::OS` — "macos", "linux", "windows", …
    pub platform: String,

    /// Whether `cpal::default_host().input_devices()` returned at least one
    /// device.
    pub audio_input_available: bool,

    /// Whether the model file configured in `Settings.whisper.model` exists
    /// on disk.
    pub model_file_exists: bool,

    /// Absolute path to the model file as stored in Settings (not resolved).
    pub model_file_path: String,

    /// Whether the whisper-cli sidecar binary exists at the resolved path and
    /// is executable (unix execute-bit check on macOS).
    pub sidecar_available: bool,

    /// Absolute path to the sidecar binary that was checked, or an error
    /// description if it could not be resolved.
    pub sidecar_path: String,

    /// Cleanup mode from Settings: "off", "regex", or "chaperone".
    pub cleanup_mode: String,

    /// Only populated when `cleanup_mode == "chaperone"`. "reachable" if
    /// Ollama responded to an HTTP GET within 2 s; "unreachable: <reason>"
    /// otherwise. Empty string when not checked.
    pub ollama_status: String,

    /// "supported" on macOS; "unsupported" on other platforms.
    pub paste_capability: String,
}

/// Locate the whisper-cli sidecar using the same priority order as
/// `transcribe.rs::find_whisper`, but without failing on a bad configured path.
/// Returns `(resolved_path_string, exists_and_executable)`.
fn check_sidecar() -> (String, bool) {
    let sidecar = "whisper-cli-aarch64-apple-darwin";

    // Release bundle: next to the running executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join(sidecar);
            if p.exists() {
                let ok = is_executable(&p);
                return (p.to_string_lossy().into_owned(), ok);
            }
        }
    }

    // Dev mode: src-tauri/binaries/ at compile time.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(sidecar);
    if dev.exists() {
        let ok = is_executable(&dev);
        return (dev.to_string_lossy().into_owned(), ok);
    }

    (
        format!(
            "not found (checked exe dir and {}/binaries/{})",
            env!("CARGO_MANIFEST_DIR"),
            sidecar
        ),
        false,
    )
}

/// Returns true if the path exists and has the user-executable bit set (Unix).
/// On non-Unix platforms, existence is sufficient.
fn is_executable(p: &std::path::Path) -> bool {
    if !p.exists() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(p) {
            Ok(m) => m.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[tauri::command]
#[specta::specta]
pub async fn run_diagnostics() -> DiagnosticsResult {
    let cfg = crate::settings::load();

    // ── Platform ─────────────────────────────────────────────────────────────
    let platform = std::env::consts::OS.to_string();

    // ── Audio input ──────────────────────────────────────────────────────────
    let audio_input_available = {
        use cpal::traits::HostTrait;
        match cpal::default_host().input_devices() {
            Ok(mut devs) => devs.next().is_some(),
            Err(_) => false,
        }
    };

    // ── Model file ───────────────────────────────────────────────────────────
    let model_file_path = cfg.whisper.model.clone();
    let model_file_exists = std::path::Path::new(&model_file_path).exists();

    // ── Sidecar binary ───────────────────────────────────────────────────────
    let (sidecar_path, sidecar_available) = check_sidecar();

    // ── Cleanup mode ─────────────────────────────────────────────────────────
    let cleanup_mode = match cfg.cleanup.mode {
        crate::settings::CleanupMode::Off => "off".to_string(),
        crate::settings::CleanupMode::Regex => "regex".to_string(),
        crate::settings::CleanupMode::Chaperone => "chaperone".to_string(),
    };

    // ── Ollama reachability (chaperone mode only) ─────────────────────────────
    let ollama_status = if cfg.cleanup.mode == crate::settings::CleanupMode::Chaperone {
        let url = cfg.cleanup.ollama_url.clone();
        match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
        {
            Ok(client) => match client.get(&url).send().await {
                Ok(resp) => {
                    if resp.status().is_success() || resp.status().as_u16() < 500 {
                        "reachable".to_string()
                    } else {
                        format!("unreachable: HTTP {}", resp.status())
                    }
                }
                Err(e) => format!("unreachable: {}", e),
            },
            Err(e) => format!("error building client: {}", e),
        }
    } else {
        String::new()
    };

    // ── Paste capability ─────────────────────────────────────────────────────
    let paste_capability = if cfg!(target_os = "macos") {
        "supported"
    } else {
        "unsupported"
    }
    .to_string();

    DiagnosticsResult {
        platform,
        audio_input_available,
        model_file_exists,
        model_file_path,
        sidecar_available,
        sidecar_path,
        cleanup_mode,
        ollama_status,
        paste_capability,
    }
}
