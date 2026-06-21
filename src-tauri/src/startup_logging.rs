// TurboTalk — startup logging initialisation.
//
// Extracted from `lib.rs::run()` to keep the bootstrap entry point focused on
// orchestrating Tauri lifecycle.  Owns the tracing subscriber, rolling-file
// appenders, and the LOG_DIR_CELL static that the health watchdog reads.

#[cfg(debug_assertions)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_appender::rolling::{RollingFileAppender, Rotation};

use tracing_subscriber::{
    filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt, Layer,
};

/// Cached log directory path, set once at startup so the tracing health
/// watchdog (spawned in `.setup()`) can stat files without going through
/// the Settings RwLock.
pub static LOG_DIR_CELL: OnceLock<PathBuf> = OnceLock::new();

/// Keeps all non-blocking tracing writer guards alive for the process
/// lifetime so logs flush on every write.
static LOG_GUARDS: OnceLock<Vec<tracing_appender::non_blocking::WorkerGuard>> = OnceLock::new();

/// Initialise the tracing subscriber, rolling-file appenders, and the shared
/// logging library.  Returns the resolved log directory path for callers that
/// need it before init (the caller has already set `LOG_DIR_CELL` on return).
///
/// File appenders are best-effort.  On macOS 26 and later, sandbox policy can
/// deny file creation under `~/.config/turbotalk/logs/` even when the directory
/// exists.  When that happens the app falls back to stderr-only tracing and
/// keeps running — the dictation loop does not depend on file logging.
pub fn init() -> PathBuf {
    let data_dir = crate::settings::data_dir();
    shared::logging::init(env!("CARGO_PKG_NAME"), &data_dir);
    let _ = crate::diagnostic_log::ensure_log_dir();
    let log_dir = crate::diagnostic_log::log_dir();

    // Try to build both file appenders.  If either fails, fall back to
    // stderr-only — the dictation loop does not depend on file logging.
    let main_result = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(crate::diagnostic_log::MAIN_LOG_PREFIX)
        .filename_suffix(crate::diagnostic_log::LOG_SUFFIX)
        .max_log_files(14)
        .build(&log_dir);

    let error_result = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(crate::diagnostic_log::ERROR_LOG_PREFIX)
        .filename_suffix(crate::diagnostic_log::LOG_SUFFIX)
        .max_log_files(60)
        .build(&log_dir);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("turbotalk_lib=debug,warn"));

    match (main_result, error_result) {
        (Ok(main_appender), Ok(error_appender)) => {
            // Full logging: stderr console layer + daily-rotated file layers.
            let (main_nb, main_guard) = tracing_appender::non_blocking(main_appender);
            let (error_nb, error_guard) = tracing_appender::non_blocking(error_appender);

            #[cfg(debug_assertions)]
            let mut log_guards = vec![main_guard, error_guard];
            #[cfg(not(debug_assertions))]
            let log_guards = vec![main_guard, error_guard];

            // Transcript debug log (dev-only, kept off the tracing pipeline).
            #[cfg(debug_assertions)]
            if let Some(guard) = build_transcript_writer(&log_dir) {
                log_guards.push(guard);
            }

            let _ = LOG_GUARDS.set(log_guards);

            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(main_nb)
                        .with_ansi(false),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(error_nb)
                        .with_ansi(false)
                        .with_filter(LevelFilter::WARN),
                )
                .init();
        }
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("[turbotalk] log appender unavailable — stderr only: {e}");
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                .init();

            // Transcript debug log is also best-effort; skip if the main
            // appenders already failed (filesystem won't accept this one).
            #[cfg(debug_assertions)]
            {
                let _ = build_transcript_writer(&log_dir);
            }
        }
    }

    // Cache the log dir for the health watchdog.
    let _ = LOG_DIR_CELL.set(log_dir.clone());

    tracing::info!(
        "[startup] TurboTalk v{} logging to {}",
        env!("CARGO_PKG_VERSION"),
        log_dir.display()
    );
    crate::diagnostic_log::emergency_trace(format!(
        "[startup] TurboTalk v{} log_dir={}",
        env!("CARGO_PKG_VERSION"),
        log_dir.display()
    ));

    log_dir
}

/// Build the transcript debug-writer (dev-only).  Returns `None` if the
/// filesystem rejects the file — transcript logging is diagnostic sugar, not
/// part of the dictation pipeline, so this is never fatal.
#[cfg(debug_assertions)]
fn build_transcript_writer(log_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    match RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(crate::diagnostic_log::TRANSCRIPT_LOG_PREFIX)
        .filename_suffix(crate::diagnostic_log::LOG_SUFFIX)
        .max_log_files(14)
        .build(log_dir)
    {
        Ok(appender) => {
            let (nb, guard) = tracing_appender::non_blocking(appender);
            crate::diagnostic_log::init_transcript_writer(nb);
            Some(guard)
        }
        Err(e) => {
            eprintln!("[turbotalk] transcript log unavailable (non-fatal): {e}");
            None
        }
    }
}
