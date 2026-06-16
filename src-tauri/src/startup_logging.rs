// TurboTalk — startup logging initialisation.
//
// Extracted from `lib.rs::run()` to keep the bootstrap entry point focused on
// orchestrating Tauri lifecycle.  Owns the tracing subscriber, rolling-file
// appenders, and the LOG_DIR_CELL static that the health watchdog reads.

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
static LOG_GUARDS: OnceLock<Vec<tracing_appender::non_blocking::WorkerGuard>> =
    OnceLock::new();

/// Initialise the tracing subscriber, rolling-file appenders, and the shared
/// logging library.  Returns the resolved log directory path for callers that
/// need it before init (the caller has already set `LOG_DIR_CELL` on return).
///
/// # Panics
/// Panics if the rolling-file appenders cannot be created (filesystem error
/// at ~/.config/turbotalk/logs/).
pub fn init() -> PathBuf {
    let data_dir = crate::settings::data_dir();
    shared::logging::init(env!("CARGO_PKG_NAME"), &data_dir);
    let _ = crate::diagnostic_log::ensure_log_dir();
    let log_dir = crate::diagnostic_log::log_dir();

    // Full session log: one file per day (`turbotalk.YYYY-MM-DD.log`),
    // keeping ~2 weeks so the directory can't grow without bound.
    let main_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(crate::diagnostic_log::MAIN_LOG_PREFIX)
        .filename_suffix(crate::diagnostic_log::LOG_SUFFIX)
        .max_log_files(14)
        .build(&log_dir)
        .expect("init main log appender");
    let (main_nb, main_guard) = tracing_appender::non_blocking(main_appender);

    // Errors-only log: WARN+ERROR across all targets, retained longer so a
    // "what broke over the last week/month" query reads one short file.
    let error_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(crate::diagnostic_log::ERROR_LOG_PREFIX)
        .filename_suffix(crate::diagnostic_log::LOG_SUFFIX)
        .max_log_files(60)
        .build(&log_dir)
        .expect("init error log appender");
    let (error_nb, error_guard) = tracing_appender::non_blocking(error_appender);

    // Keep all non-blocking writers alive for the process lifetime.
    #[cfg(debug_assertions)]
    let mut log_guards = vec![main_guard, error_guard];
    #[cfg(not(debug_assertions))]
    let log_guards = vec![main_guard, error_guard];

    #[cfg(debug_assertions)]
    {
        // Transcript debug log: a dedicated, local-only sink kept off the
        // tracing pipeline entirely.  TEMPORARY — used to chase transcription
        // quirks in dev builds; never included in uploaded reports.
        let transcript_appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(crate::diagnostic_log::TRANSCRIPT_LOG_PREFIX)
            .filename_suffix(crate::diagnostic_log::LOG_SUFFIX)
            .max_log_files(14)
            .build(&log_dir)
            .expect("init transcript log appender");
        let (transcript_nb, transcript_guard) =
            tracing_appender::non_blocking(transcript_appender);
        crate::diagnostic_log::init_transcript_writer(transcript_nb);
        log_guards.push(transcript_guard);
    }

    let _ = LOG_GUARDS.set(log_guards);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("turbotalk_lib=debug,warn"));
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

    // Cache the log dir for the health watchdog.
    let _ = LOG_DIR_CELL.set(log_dir.clone());

    tracing::info!(
        "[startup] TurboTalk v{} logging to {}",
        env!("CARGO_PKG_VERSION"),
        log_dir.display()
    );

    log_dir
}
