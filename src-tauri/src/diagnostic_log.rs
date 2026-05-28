// Session log + one-shot diagnostic export for beta testing (esp. Windows).
//
// Rust tracing writes to `{data_dir}/logs/turbotalk.log`. The frontend can
// append structured UI events via `log_client_event`. `export_diagnostic_report`
// bundles readiness, diagnostics, sanitized config, UI events, and the log tail
// into a single text file the tester can attach to a bug report.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CLIENT_EVENTS: usize = 500;
const MAX_LOG_TAIL_BYTES: usize = 2 * 1024 * 1024;

static CLIENT_EVENTS: LazyLock<Mutex<VecDeque<String>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

pub fn log_dir() -> PathBuf {
    crate::settings::data_dir().join("logs")
}

pub fn log_file_path() -> PathBuf {
    log_dir().join("turbotalk.log")
}

pub fn ensure_log_dir() -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir())
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Append a frontend-originated line to the in-memory ring buffer and mirror it
/// into the tracing log so it lands in `turbotalk.log`.
pub fn record_client_event(event: &str, detail: &str) {
    let line = format!("{} [ui] {event} {detail}", epoch_ms());
    let mut buf = CLIENT_EVENTS.lock();
    buf.push_back(line.clone());
    while buf.len() > MAX_CLIENT_EVENTS {
        buf.pop_front();
    }
    tracing::info!("{line}");
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ExportDiagnosticResult {
    /// Absolute path to the bundled report file.
    pub report_path: String,
    /// Absolute path to the live session log (also embedded in the report).
    pub log_path: String,
}

#[derive(Serialize)]
struct SanitizedConfig {
    backend: String,
    backend_variant: String,
    hotkey_key: String,
    hotkey_mode: String,
    cancel_on_esc: bool,
    cancel_on_hold: bool,
    audio_device: String,
    cleanup_mode: String,
    show_overlay: bool,
    theme: String,
}

fn sanitized_config(cfg: &crate::settings::Config) -> SanitizedConfig {
    SanitizedConfig {
        backend: format!("{:?}", cfg.backend),
        backend_variant: cfg.backend_variant.clone(),
        hotkey_key: cfg.hotkey.key.clone(),
        hotkey_mode: cfg.hotkey.mode.clone(),
        cancel_on_esc: cfg.hotkey.cancel_on_esc,
        cancel_on_hold: cfg.hotkey.cancel_on_hold,
        audio_device: cfg.audio.device.clone(),
        cleanup_mode: format!("{:?}", cfg.cleanup.mode),
        show_overlay: cfg.show_overlay,
        theme: cfg.theme.clone(),
    }
}

fn read_log_tail(path: &Path) -> String {
    let Ok(meta) = std::fs::metadata(path) else {
        return "(log file not created yet)\n".into();
    };
    if meta.len() == 0 {
        return "(log file empty)\n".into();
    }

    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return "(could not open log file)\n".into();
    };

    let start = meta.len().saturating_sub(MAX_LOG_TAIL_BYTES as u64);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return "(could not seek log file)\n".into();
    }
    if start > 0 {
        let mut discard = [0u8; 1];
        let _ = file.read(&mut discard);
    }

    let mut tail = String::new();
    let _ = file.read_to_string(&mut tail);
    if start > 0 {
        format!("… truncated to last {} bytes …\n{tail}", MAX_LOG_TAIL_BYTES)
    } else {
        tail
    }
}

#[tauri::command]
#[specta::specta]
pub fn log_client_event(event: String, detail: Option<String>) -> Result<(), String> {
    let detail = detail.unwrap_or_default();
    // Cap detail length so a runaway JSON blob cannot blow the ring buffer.
    let detail = if detail.len() > 2000 {
        format!("{}…", &detail[..2000])
    } else {
        detail
    };
    record_client_event(&event, &detail);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn export_diagnostic_report() -> Result<ExportDiagnosticResult, String> {
    ensure_log_dir().map_err(|e| e.to_string())?;

    let log_path = log_file_path();
    let readiness = crate::permissions::check_readiness();
    let diagnostics = crate::diagnostics::run_diagnostics().await;
    let cfg = crate::settings::load();

    let report_name = format!("turbotalk-report-{}.txt", epoch_ms());
    let report_path = log_dir().join(&report_name);

    let client_events: Vec<String> = CLIENT_EVENTS.lock().iter().cloned().collect();
    let log_tail = read_log_tail(&log_path);

    let mut out = std::fs::File::create(&report_path).map_err(|e| e.to_string())?;

    writeln!(out, "=== TurboTalk diagnostic report ===").map_err(|e| e.to_string())?;
    writeln!(out, "generated_ms: {}", epoch_ms()).map_err(|e| e.to_string())?;
    writeln!(out, "version: {}", env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "target: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
    .map_err(|e| e.to_string())?;
    writeln!(out).map_err(|e| e.to_string())?;

    writeln!(out, "=== Readiness ===").map_err(|e| e.to_string())?;
    let readiness_json =
        serde_json::to_string_pretty(&readiness).unwrap_or_else(|_| readiness.platform.clone());
    writeln!(out, "{readiness_json}").map_err(|e| e.to_string())?;
    writeln!(out).map_err(|e| e.to_string())?;

    writeln!(out, "=== Diagnostics ===").map_err(|e| e.to_string())?;
    let diag_json =
        serde_json::to_string_pretty(&diagnostics).unwrap_or_else(|_| diagnostics.platform.clone());
    writeln!(out, "{diag_json}").map_err(|e| e.to_string())?;
    writeln!(out).map_err(|e| e.to_string())?;

    writeln!(out, "=== Config (sanitized) ===").map_err(|e| e.to_string())?;
    let cfg_json = serde_json::to_string_pretty(&sanitized_config(&cfg))
        .unwrap_or_else(|_| "(serialize failed)".into());
    writeln!(out, "{cfg_json}").map_err(|e| e.to_string())?;
    writeln!(out).map_err(|e| e.to_string())?;

    writeln!(out, "=== UI events ({} lines) ===", client_events.len())
        .map_err(|e| e.to_string())?;
    for line in &client_events {
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    writeln!(out).map_err(|e| e.to_string())?;

    writeln!(out, "=== Session log ===").map_err(|e| e.to_string())?;
    writeln!(out, "path: {}", log_path.display()).map_err(|e| e.to_string())?;
    writeln!(out).map_err(|e| e.to_string())?;
    write!(out, "{log_tail}").map_err(|e| e.to_string())?;

    tracing::info!(
        "[diagnostic] exported report to {}",
        report_path.display()
    );

    Ok(ExportDiagnosticResult {
        report_path: report_path.to_string_lossy().into_owned(),
        log_path: log_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn open_logs_folder() -> Result<(), String> {
    ensure_log_dir().map_err(|e| e.to_string())?;
    let path = log_dir();

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
