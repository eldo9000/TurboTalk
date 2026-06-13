//! Crash-forensics logging infrastructure shared across all LibreWin apps.
//!
//! Writes structured JSON-lines to `<base_dir>/crash-logs/<app>.jsonl` with
//! rotation (10 MB per file, 5 files retained). Two tiers:
//!   - **basic**:   panic hooks + error capture only (always on, near-zero cost)
//!   - **comprehensive**: basic + Tauri IPC, file I/O, permission checks
//!
//! Environment variables:
//!   - `LOG_LEVEL`  → `basic` (default) | `comprehensive`
//!   - `LOG_DIR`    → log directory path (default: `<base_dir>/crash-logs/`)

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use once_cell::sync::Lazy;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warn,
    Info,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Basic,
    Comprehensive,
}

#[derive(serde::Serialize)]
struct LogEntry {
    timestamp: String,
    level: Level,
    source: String,
    event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

// ── Global state ─────────────────────────────────────────────────────────────

static LOGGER: Lazy<Mutex<Option<BufWriter<File>>>> = Lazy::new(|| Mutex::new(None));

static TIER: Lazy<Tier> = Lazy::new(|| match std::env::var("LOG_LEVEL").as_deref() {
    Ok("comprehensive") => Tier::Comprehensive,
    _ => Tier::Basic,
});

// ── Init ─────────────────────────────────────────────────────────────────────

/// Initialise the logger. Call once at the very top of `main()`.
///
/// Panic hooks are installed automatically. The app name determines the
/// log filename: `<base_dir>/crash-logs/<app_name>.jsonl`.
///
/// `base_dir` must be a writable absolute path (e.g. the app's data directory).
/// When the `LOG_DIR` environment variable is set it takes precedence over
/// `base_dir`, making the logger relocatable for testing.
pub fn init(app_name: &str, base_dir: &std::path::Path) {
    let log_dir = std::env::var("LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| base_dir.join("crash-logs"));
    fs::create_dir_all(&log_dir)
        .unwrap_or_else(|e| panic!("Failed to create crash-log dir {}: {e}", log_dir.display()));

    let log_path = log_dir.join(format!("{}.jsonl", app_name));
    rotate(&log_path);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|e| panic!("Failed to open crash log file {}: {e}", log_path.display()));

    *LOGGER.lock().unwrap() = Some(BufWriter::new(file));

    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_event(
            Level::Error,
            "panic::hook",
            "process panicked",
            Some(serde_json::json!({
                "message": info.to_string(),
                "location": info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            })),
        );
        default(info);
    }));
}

/// Current tier (reads `LOG_LEVEL` env once, on first access).
pub fn tier() -> Tier {
    *TIER
}

// ── Core logging ─────────────────────────────────────────────────────────────

/// Write a single structured log line. Flushes after every write so crash
/// recovery sees the most recent events.
pub fn log_event(level: Level, source: &str, event: &str, data: Option<serde_json::Value>) {
    let entry = LogEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        level,
        source: source.to_string(),
        event: event.to_string(),
        data,
    };
    if let Some(ref mut w) = *LOGGER.lock().unwrap() {
        serde_json::to_writer(&mut *w, &entry).ok();
        writeln!(w).ok();
        w.flush().ok();
    }
}

/// Convenience helpers.
pub fn log_error(source: &str, event: &str) {
    log_event(Level::Error, source, event, None);
}

pub fn log_warn(source: &str, event: &str) {
    log_event(Level::Warn, source, event, None);
}

pub fn log_info(source: &str, event: &str) {
    log_event(Level::Info, source, event, None);
}

/// Trace-level event — only emitted when tier is Comprehensive.
pub fn log_trace(source: &str, event: &str, data: impl serde::Serialize) {
    if *TIER == Tier::Comprehensive {
        log_event(
            Level::Trace,
            source,
            event,
            Some(serde_json::to_value(data).unwrap_or_default()),
        );
    }
}

// ── Comprehensive-tier helpers ───────────────────────────────────────────────

/// Log a file read (comprehensive tier only).
pub fn trace_file_read(path: &std::path::Path, bytes: usize) {
    if *TIER == Tier::Comprehensive {
        log_event(
            Level::Trace,
            "io::read",
            "file read",
            Some(serde_json::json!({
                "path": path.display().to_string(),
                "bytes": bytes,
            })),
        );
    }
}

/// Log a file write (comprehensive tier only).
pub fn trace_file_write(path: &std::path::Path, bytes: usize) {
    if *TIER == Tier::Comprehensive {
        log_event(
            Level::Trace,
            "io::write",
            "file write",
            Some(serde_json::json!({
                "path": path.display().to_string(),
                "bytes": bytes,
            })),
        );
    }
}

// ── Rotation ─────────────────────────────────────────────────────────────────

const MAX_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_FILES: u32 = 5;

fn rotate(path: &PathBuf) {
    if !path.exists() {
        return;
    }
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return,
    };
    if meta.len() <= MAX_SIZE {
        return;
    }
    // Shift existing backups: .jsonl.4 → .jsonl.5, … , .jsonl.1 → .jsonl.2
    for i in (1..MAX_FILES).rev() {
        let old = path.with_extension(format!("{}.jsonl", i));
        let new = path.with_extension(format!("{}.jsonl", i + 1));
        if old.exists() {
            fs::rename(&old, &new).ok();
        }
    }
    let first = path.with_extension("1.jsonl");
    fs::rename(path, &first).ok();
}
