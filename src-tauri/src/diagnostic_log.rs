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
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing_appender::non_blocking::NonBlocking;

const MAX_CLIENT_EVENTS: usize = 500;
const MAX_LOG_TAIL_BYTES: usize = 2 * 1024 * 1024;

static CLIENT_EVENTS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn client_events() -> &'static Mutex<VecDeque<String>> {
    CLIENT_EVENTS.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Filename prefix for the full session log. Day-rotated files are named
/// `turbotalk.YYYY-MM-DD.log`.
pub const MAIN_LOG_PREFIX: &str = "turbotalk";
/// Filename prefix for the WARN+ERROR-only log (`errors.YYYY-MM-DD.log`).
pub const ERROR_LOG_PREFIX: &str = "errors";
/// Filename prefix for the transcript debug log (`transcripts.YYYY-MM-DD.log`).
///
/// TEMPORARY (planned removal): captures full transcript text so we can chase
/// repetition / cut-off / mistranslation quirks locally in debug builds. This
/// file is **local-only** — it is written through a dedicated sink (never
/// `tracing`), uses its own filename prefix, and is deliberately excluded from
/// every uploaded diagnostic/bug report. Nothing a user dictates leaves their
/// machine.
pub const TRANSCRIPT_LOG_PREFIX: &str = "transcripts";
pub const LOG_SUFFIX: &str = "log";

/// Dedicated writer for the transcript debug log. Set once at startup. Kept
/// entirely separate from the tracing pipeline so transcript content cannot
/// leak into the session log, errors log, console, or uploaded reports.
static TRANSCRIPT_WRITER: OnceLock<Mutex<Option<NonBlocking>>> = OnceLock::new();

fn transcript_writer() -> &'static Mutex<Option<NonBlocking>> {
    TRANSCRIPT_WRITER.get_or_init(|| Mutex::new(None))
}

/// Install the transcript-log writer (called once during logging init).
pub fn init_transcript_writer(writer: NonBlocking) {
    *transcript_writer().lock() = Some(writer);
}

/// Append one transcript to the local-only transcript debug log. No-op unless
/// the debug-only writer is installed. See [`TRANSCRIPT_LOG_PREFIX`] for the
/// privacy contract — this content is never uploaded.
pub fn record_transcript(backend: &str, text: &str, rejection_dbg: &str) {
    let guard = transcript_writer().lock();
    if let Some(writer) = guard.as_ref() {
        let mut writer = writer.clone();
        let _ = writeln!(
            writer,
            "{} [{backend}] (rejection={rejection_dbg}) {text:?}",
            epoch_ms()
        );
    }
}

pub fn log_dir() -> PathBuf {
    crate::settings::data_dir().join("logs")
}

pub fn ensure_log_dir() -> std::io::Result<()> {
    crate::settings::create_private_dir_all(&log_dir())
}

/// Day-stamped log files for `prefix` (named `{prefix}.YYYY-MM-DD.log`), sorted
/// oldest→newest. The ISO date sorts lexicographically, so a plain sort orders
/// them chronologically.
pub fn log_files_for(prefix: &str) -> Vec<PathBuf> {
    let want_prefix = format!("{prefix}.");
    let want_suffix = format!(".{LOG_SUFFIX}");
    let mut files: Vec<PathBuf> = std::fs::read_dir(log_dir())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&want_prefix) && n.ends_with(&want_suffix))
        })
        .collect();
    files.sort();
    files
}

/// Newest main session-log file, if any exist yet.
pub fn newest_main_log() -> Option<PathBuf> {
    log_files_for(MAIN_LOG_PREFIX).pop()
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Append a frontend-originated line to the in-memory ring buffer and mirror it
/// into the tracing log so it lands in `turbotalk.log`.
///
/// PRIVACY: `event`/`detail` are bundled verbatim into uploaded bug reports via
/// `build_report_text`'s "UI events" section. Callers must NEVER pass dictated
/// transcript text or LLM cleanup output here — log a char count or a category,
/// not the content. See `TRANSCRIPT_LOG_PREFIX` for the local-only sink that is
/// the *only* place raw transcript text may be written.
pub fn record_client_event(event: &str, detail: &str) {
    let line = format!("{} [ui] {event} {detail}", epoch_ms());
    let mut buf = client_events().lock();
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
    overlay_size: String,
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
        overlay_size: cfg.overlay_size.clone(),
        theme: cfg.theme.clone(),
    }
}

/// Concatenate the newest day-files for `prefix`, walking newest→oldest until
/// `cap` bytes are gathered, then emitting them chronologically with per-file
/// headers. Bounds the bundle size regardless of how many days are retained.
fn read_recent_logs(prefix: &str, cap: usize) -> String {
    let files = log_files_for(prefix);
    if files.is_empty() {
        return "(no log files yet)\n".into();
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut total = 0usize;
    let mut truncated = false;
    for path in files.iter().rev() {
        if total >= cap {
            truncated = true;
            break;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        total += bytes.len();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let text = String::from_utf8_lossy(&bytes);
        chunks.push(format!("----- {name} -----\n{text}"));
    }
    chunks.reverse();

    let body = chunks.join("\n");
    if truncated {
        format!("… older logs omitted (newest ~{cap} bytes shown) …\n{body}")
    } else {
        body
    }
}

#[derive(Serialize)]
struct BackendBundleInventory {
    family: String,
    variant: String,
    path: String,
    valid: bool,
}

fn backend_bundle_inventory() -> Vec<BackendBundleInventory> {
    let mut out = Vec::new();

    #[cfg(feature = "parakeet")]
    for variant in ["tdt-0.6b-v2", "tdt-0.6b-v3"] {
        if let Some(path) = crate::transcribe_backends::parakeet::variant_dir(variant) {
            let valid =
                crate::transcribe_backends::parakeet::validate_parakeet_model_dir(&path).is_ok();
            out.push(BackendBundleInventory {
                family: "parakeet".into(),
                variant: variant.into(),
                path: path.to_string_lossy().into_owned(),
                valid,
            });
        }
    }

    out
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

/// Build the full diagnostic report as a single text blob. Shared by the local
/// export and the remote bug-report upload. `note` is the tester's free-text
/// description, included only for bug reports.
pub async fn build_report_text(note: Option<&str>) -> String {
    use std::fmt::Write as _;

    let readiness = crate::permissions::check_readiness();
    let diagnostics = crate::diagnostics::run_diagnostics().await;
    let cfg = crate::settings::load();
    let client_events: Vec<String> = client_events().lock().iter().cloned().collect();
    let log_body = read_recent_logs(MAIN_LOG_PREFIX, MAX_LOG_TAIL_BYTES);
    let error_log_body = read_recent_logs(ERROR_LOG_PREFIX, MAX_LOG_TAIL_BYTES / 2);
    let history_meta = crate::settings::load_history_detailed();
    let model_inventory = crate::settings::scan_models_dir();
    let backend_bundles = backend_bundle_inventory();

    let mut out = String::new();
    let _ = writeln!(out, "=== TurboTalk diagnostic report ===");
    let _ = writeln!(out, "generated_ms: {}", epoch_ms());
    let _ = writeln!(out, "version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "target: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "debug_build: {}", cfg!(debug_assertions));
    let _ = writeln!(
        out,
        "features: parakeet={} telegram_bugreport={}",
        cfg!(feature = "parakeet"),
        cfg!(feature = "dev-telegram-bugreport")
    );
    let _ = writeln!(
        out,
        "current_exe: {}",
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("error: {e}"))
    );
    let _ = writeln!(
        out,
        "current_dir: {}",
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("error: {e}"))
    );

    if let Some(note) = note {
        let _ = writeln!(out);
        let _ = writeln!(out, "=== Reporter note ===");
        let _ = writeln!(out, "{note}");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "=== Runtime context ===");
    let _ = writeln!(out, "data_dir: {}", crate::settings::data_dir().display());
    let _ = writeln!(out, "log_dir: {}", log_dir().display());
    let _ = writeln!(
        out,
        "newest_main_log: {}",
        newest_main_log()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into())
    );
    let _ = writeln!(
        out,
        "history_entries_loaded: {}",
        history_meta.entries.len()
    );
    let _ = writeln!(out, "history_entries_dropped: {}", history_meta.dropped);
    let models_json =
        serde_json::to_string_pretty(&model_inventory).unwrap_or_else(|_| "[]".into());
    let _ = writeln!(out, "installed_whisper_models: {models_json}");
    let backend_bundles_json =
        serde_json::to_string_pretty(&backend_bundles).unwrap_or_else(|_| "[]".into());
    let _ = writeln!(out, "backend_model_bundles: {backend_bundles_json}");
    #[cfg(target_os = "macos")]
    {
        let sw_vers = std::process::Command::new("sw_vers").output();
        let sw_vers = sw_vers
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "(unavailable)".into());
        let _ = writeln!(out, "sw_vers: {}", sw_vers.replace('\n', " | "));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = writeln!(out, "os_version: (not collected on this platform yet)");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "=== Readiness ===");
    let readiness_json =
        serde_json::to_string_pretty(&readiness).unwrap_or_else(|_| readiness.platform.clone());
    let _ = writeln!(out, "{readiness_json}");
    let _ = writeln!(out);

    let _ = writeln!(out, "=== Diagnostics ===");
    let diag_json =
        serde_json::to_string_pretty(&diagnostics).unwrap_or_else(|_| diagnostics.platform.clone());
    let _ = writeln!(out, "{diag_json}");
    let _ = writeln!(out);

    let _ = writeln!(out, "=== Config (sanitized) ===");
    let cfg_json = serde_json::to_string_pretty(&sanitized_config(&cfg))
        .unwrap_or_else(|_| "(serialize failed)".into());
    let _ = writeln!(out, "{cfg_json}");
    let _ = writeln!(out);

    #[cfg(target_os = "windows")]
    {
        let _ = writeln!(out, "=== Hotkey probe ===");
        let probe = crate::hotkey::diagnostic_probe();
        let probe_json =
            serde_json::to_string_pretty(&probe).unwrap_or_else(|_| "(serialize failed)".into());
        let _ = writeln!(out, "{probe_json}");
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "=== UI events ({} lines) ===", client_events.len());
    for line in &client_events {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "=== Session log ===");
    let _ = writeln!(out, "dir: {}", log_dir().display());
    let _ = writeln!(out);
    let _ = write!(out, "{log_body}");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "=== Error log ===");
    let _ = writeln!(out, "dir: {}", log_dir().display());
    let _ = writeln!(out);
    let _ = write!(out, "{error_log_body}");

    out
}

#[tauri::command]
#[specta::specta]
pub async fn export_diagnostic_report(
    note: Option<String>,
) -> Result<ExportDiagnosticResult, String> {
    ensure_log_dir().map_err(|e| e.to_string())?;

    let note_ref = note.as_deref().filter(|s| !s.is_empty());
    let report = build_report_text(note_ref).await;
    let report_name = format!("turbotalk-report-{}.txt", epoch_ms());
    let report_path = log_dir().join(&report_name);
    crate::settings::write_private_file(&report_path, report.as_bytes())
        .map_err(|e| e.to_string())?;

    tracing::info!("[diagnostic] exported report to {}", report_path.display());

    let log_path = newest_main_log().unwrap_or_else(log_dir);
    Ok(ExportDiagnosticResult {
        report_path: report_path.to_string_lossy().into_owned(),
        log_path: log_path.to_string_lossy().into_owned(),
    })
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BugReportResult {
    /// Short, human-quotable id the tester can reference (e.g. "A1B3C5").
    pub report_id: String,
    /// Whether the report was uploaded (vs. only saved locally).
    pub uploaded: bool,
    /// Absolute path to the local report copy.
    pub report_path: String,
}

/// Short id mixing the clock with a per-process counter so concurrent reports
/// don't collide. Not security-sensitive — just a label to correlate reports.
fn short_report_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mix = (epoch_ms() as u32) ^ n.wrapping_mul(0x9E37_79B1);
    format!("{:06X}", mix & 0x00FF_FFFF)
}

/// Truncate to at most `max` chars on a char boundary; substitutes a placeholder
/// when empty so the message caption is never blank.
#[cfg(feature = "dev-telegram-bugreport")]
fn note_for_summary(s: &str, max: usize) -> &str {
    if s.is_empty() {
        return "(no description provided)";
    }
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Upload the report to a Telegram chat via the Bot API `sendDocument` method.
/// The full report rides as a file attachment; a short caption carries the
/// version/OS/id and a snippet of the tester's note.
#[cfg(feature = "dev-telegram-bugreport")]
async fn upload_to_telegram(
    token: &str,
    chat_id: &str,
    report_id: &str,
    note: &str,
    file_bytes: Vec<u8>,
) -> Result<(), String> {
    use reqwest::multipart::{Form, Part};

    // Telegram captions are capped at 1024 chars; keep the note well under that
    // and ship the full report as the attached document.
    let caption = format!(
        "TurboTalk bug report #{report_id}\nv{} · {} {}\n— {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        note_for_summary(note, 800),
    );

    let file_part = Part::bytes(file_bytes)
        .file_name(format!("turbotalk-bugreport-{report_id}.txt"))
        .mime_str("text/plain")
        .map_err(|e| e.to_string())?;

    let form = Form::new()
        .text("chat_id", chat_id.to_string())
        .text("caption", caption)
        .part("document", file_part);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("https://api.telegram.org/bot{token}/sendDocument");
    let resp = client
        .post(url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let body: String = body.chars().take(300).collect();
        return Err(format!("Telegram API returned {status}: {body}"));
    }
    Ok(())
}

/// Bundle the diagnostic report (with the tester's note) and upload it to the
/// configured webhook. Always writes a local copy first so nothing is lost when
/// the network or webhook is unavailable.
///
/// Upload is gated behind the `dev-telegram-bugreport` Cargo feature — public
/// release builds never embed Telegram credentials.
#[tauri::command]
#[specta::specta]
pub async fn submit_bug_report(note: String) -> Result<BugReportResult, String> {
    ensure_log_dir().map_err(|e| e.to_string())?;

    let report_id = short_report_id();
    let note = note.trim();
    let report = build_report_text(if note.is_empty() { None } else { Some(note) }).await;

    // Always save a local copy — the upload may fail, but the report never vanishes.
    let report_name = format!("turbotalk-bugreport-{report_id}.txt");
    let report_path = log_dir().join(&report_name);
    crate::settings::write_private_file(&report_path, report.as_bytes())
        .map_err(|e| format!("Couldn't save the report locally: {e}"))?;

    // Upload attempt — only compiled in dev builds with the feature enabled.
    // Public release builds cannot accidentally embed Telegram credentials.
    #[cfg(feature = "dev-telegram-bugreport")]
    {
        let (Some(token), Some(chat_id)) = (
            option_env!("TURBOTALK_BUGREPORT_TG_TOKEN"),
            option_env!("TURBOTALK_BUGREPORT_TG_CHAT"),
        ) else {
            tracing::warn!(
                "[bugreport] Telegram credentials not configured in this build; saved locally as {}",
                report_path.display()
            );
            return Ok(BugReportResult {
                report_id,
                uploaded: false,
                report_path: report_path.to_string_lossy().into_owned(),
            });
        };

        match upload_to_telegram(token, chat_id, &report_id, note, report.into_bytes()).await {
            Ok(()) => {
                tracing::info!("[bugreport] uploaded report #{report_id}");
                Ok(BugReportResult {
                    report_id,
                    uploaded: true,
                    report_path: report_path.to_string_lossy().into_owned(),
                })
            }
            Err(e) => {
                tracing::error!("[bugreport] upload failed for #{report_id}: {e}");
                Err(format!(
                    "Couldn't send the report ({e}). A local copy was saved as {report_name}."
                ))
            }
        }
    }

    // Default path when the dev feature is not enabled — always local-only.
    #[cfg(not(feature = "dev-telegram-bugreport"))]
    {
        tracing::warn!(
            "[bugreport] Telegram upload not compiled in; saved locally as {} \
             (enable dev-telegram-bugreport feature for uploads)",
            report_path.display()
        );
        Ok(BugReportResult {
            report_id,
            uploaded: false,
            report_path: report_path.to_string_lossy().into_owned(),
        })
    }
}

#[tauri::command]
#[specta::specta]
pub fn open_logs_folder() -> Result<(), String> {
    ensure_log_dir().map_err(|e| e.to_string())?;
    let path = log_dir();

    open::that_detached(&path).map_err(|e| format!("could not open logs folder: {e}"))?;

    Ok(())
}
