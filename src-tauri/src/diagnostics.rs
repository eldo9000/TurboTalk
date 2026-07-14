// Diagnostics command — surface app runtime health for beta users.
//
// Run via `invoke("run_diagnostics")` from the frontend. Each check is
// wrapped in a `match`/`unwrap_or` so a failing check returns a descriptive
// string rather than crashing. The command never panics.

use serde::{Deserialize, Serialize};
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

    /// Cleanup mode from Settings: "off", "regex", or "text_formatter".
    pub cleanup_mode: String,

    /// Ollama reachability (legacy — Ollama is no longer used by the
    /// TextFormatter mode). Kept for diagnostics on existing Ollama config.
    pub ollama_status: String,

    /// "supported" on macOS; "unsupported" on other platforms.
    pub paste_capability: String,

    // ── Added 2026-06-16 for cross-platform testing ───────────────────────────
    /// OS version string (e.g. "macOS 15.4", "Windows 11 23H2", "Linux 6.8.0").
    /// Collected via platform-specific commands at diagnostic time.
    pub os_version: String,

    /// Keyboard layout identifier. Windows: locale name from GetKeyboardLayout.
    /// macOS/Linux: empty string (not collected).
    pub keyboard_layout: String,

    /// Default input device name as reported by cpal, or "none" / "error: …".
    pub default_input_device: String,

    /// Number of input channels on the default device ("1", "2", "unknown").
    pub default_input_channels: String,

    /// Preferred sample rate of the default device ("16000", "44100", "unknown").
    pub default_input_sample_rate: String,

    /// Whether the whisper-server sidecar process is currently running.
    /// "running", "not running", or "unknown (prewarm in flight)".
    pub whisper_server_running: String,

    /// Paste injection method: "CGEventPost Cmd+V", "enigo Ctrl+V",
    /// "unsupported (wayland)".
    pub paste_method: String,
}

/// Locate the whisper-cli sidecar using the same priority order as
/// `transcribe.rs::find_whisper`, but without failing on a bad configured path.
/// Returns `(resolved_path_string, exists_and_executable)`.
fn check_sidecar() -> (String, bool) {
    let sidecars = ["whisper-cli", "whisper-cli-aarch64-apple-darwin"];

    // Release bundle: next to the running executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for sidecar in sidecars {
                let p = parent.join(sidecar);
                if p.exists() {
                    let ok = is_executable(&p);
                    return (p.to_string_lossy().into_owned(), ok);
                }
            }
        }
    }

    // Dev mode: src-tauri/binaries/ at compile time. Release diagnostics must
    // not mask a broken install by reaching back into the source checkout.
    #[cfg(debug_assertions)]
    {
        for sidecar in sidecars {
            let dev = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("binaries")
                .join(sidecar);
            if dev.exists() {
                let ok = is_executable(&dev);
                return (dev.to_string_lossy().into_owned(), ok);
            }
        }
    }

    (
        format!(
            "not found (checked exe dir and {}/binaries for whisper-cli)",
            env!("CARGO_MANIFEST_DIR")
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

fn ollama_version_endpoint(raw_url: &str) -> anyhow::Result<url::Url> {
    crate::ollama::validate_ollama_url(raw_url)?
        .join("api/version")
        .map_err(|e| anyhow::anyhow!("could not build Ollama diagnostics URL: {e}"))
}

async fn check_ollama_status(raw_url: &str) -> String {
    let endpoint = match ollama_version_endpoint(raw_url) {
        Ok(endpoint) => endpoint,
        Err(e) => return format!("unreachable: invalid Ollama URL: {e}"),
    };

    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(client) => match client.get(endpoint).send().await {
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
}

// ── Platform-aware OS version collection ────────────────────────────────────

/// Collect the OS version string using platform-specific commands.
/// Never panics — returns a best-effort description or "unavailable".
fn collect_os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
        {
            Ok(o) => {
                let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if v.is_empty() {
                    "macOS (version unavailable)".into()
                } else {
                    format!("macOS {v}")
                }
            }
            Err(_) => "macOS (sw_vers unavailable)".into(),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match std::process::Command::new("cmd")
            .args(["/c", "ver"])
            .output()
        {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout);
                let s = s.trim();
                if s.is_empty() {
                    "Windows (ver command empty)".into()
                } else {
                    s.to_string()
                }
            }
            Err(_) => "Windows (ver unavailable)".into(),
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(contents) = std::fs::read_to_string("/etc/os-release") {
            let mut pretty = None;
            for line in contents.lines() {
                if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                    let val = val.trim_matches('"');
                    pretty = Some(val.to_string());
                    break;
                }
            }
            if let Some(name) = pretty {
                if let Ok(o) = std::process::Command::new("uname").arg("-r").output() {
                    let kernel = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    return format!("{name} ({kernel})");
                }
                return name;
            }
        }
        match std::process::Command::new("uname").arg("-r").output() {
            Ok(o) => {
                let k = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if k.is_empty() {
                    "Linux (uname empty)".into()
                } else {
                    format!("Linux {k}")
                }
            }
            Err(_) => "Linux (version unavailable)".into(),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        format!("{} (unknown OS)", std::env::consts::OS)
    }
}

// ── Keyboard layout (Windows only) ──────────────────────────────────────────

fn collect_keyboard_layout() -> String {
    #[cfg(target_os = "windows")]
    {
        let ps_script = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class KB {
    [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint id);
}
"@
[KB]::GetKeyboardLayout(0).ToString()
"#;
        match std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", ps_script])
            .output()
        {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    "unavailable".into()
                } else {
                    s
                }
            }
            Err(_) => "unavailable".into(),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        String::new()
    }
}

// ── Audio device details ────────────────────────────────────────────────────

fn collect_audio_device_details() -> (String, String, String) {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();

    let device_name = match host.default_input_device() {
        Some(d) => d.name().unwrap_or_else(|_| "unknown".into()),
        None => "none".to_string(),
    };

    let device = host.default_input_device();

    let (channels, sample_rate) = match device.and_then(|d| d.default_input_config().ok()) {
        Some(cfg) => (cfg.channels().to_string(), cfg.sample_rate().0.to_string()),
        None => ("unknown".to_string(), "unknown".to_string()),
    };

    (device_name, channels, sample_rate)
}

// ── Whisper-server running probe ────────────────────────────────────────────

fn check_whisper_server_running() -> String {
    if crate::transcribe::prewarm_in_flight() {
        return "unknown (prewarm in flight)".into();
    }
    if crate::transcribe::is_ready() {
        return "running".into();
    }
    let server_path = crate::transcribe::find_whisper_server("whisper-server");
    match server_path {
        Ok(_) => "not running".into(),
        Err(e) => format!("sidecar unavailable: {e}"),
    }
}

// ── Paste method ────────────────────────────────────────────────────────────

fn collect_paste_method() -> String {
    #[cfg(target_os = "macos")]
    {
        "CGEventPost Cmd+V".into()
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
        {
            "unsupported (wayland)".into()
        } else {
            "enigo Ctrl+V".into()
        }
    }

    #[cfg(target_os = "windows")]
    {
        "enigo Ctrl+V".into()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown".into()
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
        crate::settings::CleanupMode::TextFormatter => "text_formatter".to_string(),
    };

    // ── Ollama reachability (legacy) ──────────────────────────────────────────
    let ollama_status = if cfg.cleanup.ollama_url.is_empty() {
        String::new()
    } else {
        check_ollama_status(&cfg.cleanup.ollama_url).await
    };

    // ── Paste capability ─────────────────────────────────────────────────────
    let paste_capability = if cfg!(target_os = "macos") {
        "supported"
    } else {
        "unsupported"
    }
    .to_string();

    // ── Platform details (added 2026-06-16) ───────────────────────────────────
    let os_version = collect_os_version();
    let keyboard_layout = collect_keyboard_layout();
    let (default_input_device, default_input_channels, default_input_sample_rate) =
        collect_audio_device_details();
    let whisper_server_running = check_whisper_server_running();
    let paste_method = collect_paste_method();

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
        os_version,
        keyboard_layout,
        default_input_device,
        default_input_channels,
        default_input_sample_rate,
        whisper_server_running,
        paste_method,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_diagnostics_accepts_loopback_urls() {
        assert_eq!(
            ollama_version_endpoint("http://localhost:11434")
                .expect("localhost")
                .as_str(),
            "http://localhost:11434/api/version"
        );
        assert_eq!(
            ollama_version_endpoint("http://127.0.0.1:11434")
                .expect("ipv4 loopback")
                .as_str(),
            "http://127.0.0.1:11434/api/version"
        );
        assert_eq!(
            ollama_version_endpoint("http://[::1]:11434")
                .expect("ipv6 loopback")
                .as_str(),
            "http://[::1]:11434/api/version"
        );
    }

    #[test]
    fn ollama_diagnostics_rejects_non_loopback_urls() {
        for url in [
            "http://10.0.0.1:11434",
            "http://192.168.1.50:11434",
            "http://169.254.169.254/latest/meta-data",
            "https://example.com",
        ] {
            assert!(
                ollama_version_endpoint(url).is_err(),
                "diagnostics must not permit outbound Ollama probe to {url}"
            );
        }
    }
}
