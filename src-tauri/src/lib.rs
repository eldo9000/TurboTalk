// TurboTalk — personal voice dictation utility
//
// Module map (see ARCHITECTURE.md):
//   audio       mic capture (cpal)
//   recorder    Lifecycle: Ready → Recording → FinalizingAudio → Transcribing
//                          → Cleaning → Pasting → Ready (one in-flight job)
//   transcribe  whisper.cpp sidecar wrapper
//   paste       active-app text injection
//   hotkey      global push-to-talk binding (CGEventTap — Right Alt)
// cleanup     Text post-processor (formatting, anti-vocabulary)
//   settings    config persistence

pub mod audio;
pub mod audio_finalizer;
pub mod cleanup;
pub mod pre_format;
pub mod diagnostic_log;
pub mod diagnostics;
pub mod hotkey;
pub mod macos_input_monitoring;
pub mod media_control;
pub mod ollama;
pub mod paste;
pub mod permissions;
pub mod recorder;
pub mod session_metrics;
pub mod settings;
pub mod startup_logging;
pub mod theme;
pub mod transcribe;
pub mod transcribe_backends;
pub mod tray;
pub mod vad;
pub mod windowing;

pub use theme::{get_accent, get_theme};

/// Unified error channel for the frontend. Any backend error path that the
/// user should see (failed save, malformed config, dropped history entry, etc.)
/// goes through here so the frontend has a single listener and uniform UX.
///
/// Payload shape is intentionally simple:
///   { kind: String, message: String, recoverable: bool }
pub fn emit_ui_error(
    app: &tauri::AppHandle,
    kind: &str,
    message: impl Into<String>,
    recoverable: bool,
) {
    use tauri::Emitter;
    let payload = serde_json::json!({
        "kind": kind,
        "message": message.into(),
        "recoverable": recoverable,
    });
    if let Err(e) = app.emit("ui-error", payload) {
        tracing::warn!("[ui-error] failed to emit {}: {}", kind, e);
    }
}

#[tauri::command]
#[specta::specta]
fn get_config() -> settings::Config {
    (*settings::load()).clone()
}

#[tauri::command]
#[specta::specta]
fn save_config(
    app: tauri::AppHandle,
    cfg: settings::Config,
    hotkey_state: tauri::State<'_, HotkeyState>,
) -> Result<(), String> {
    use tauri::Emitter;
    let old_cfg = settings::load();
    settings::save(&cfg).map_err(|e| e.to_string())?;
    settings::update_cache(&cfg);
    *hotkey_state.write() = cfg.hotkey.clone();
    // Position before showing so the overlay never renders at a stale location.
    windowing::reposition_overlay_to_cursor_monitor(&app);
    apply_overlay_visibility(&app, cfg.show_overlay);
    // Rebuild the transcription worker only when backend-affecting config
    // fields change. Non-backend fields (theme, sound, overlay, cursor dot,
    // etc.) still persist to disk and update the cache but do NOT destroy
    // the warm worker.
    let needs_rebuild = old_cfg.backend != cfg.backend
        || old_cfg.backend_variant != cfg.backend_variant
        || old_cfg.whisper.model != cfg.whisper.model
        || old_cfg.whisper.vad_enabled != cfg.whisper.vad_enabled
        || old_cfg.cleanup.vocabulary != cfg.cleanup.vocabulary;
    if needs_rebuild {
        transcribe::invalidate_worker();
        transcribe::prewarm(cfg.clone(), app.clone());
    } else {
        tracing::debug!("[settings] no backend change — skipping worker invalidation");
    }
    // Notify other windows (overlay) of UI-relevant config changes.
    let _ = app.emit("config-update", &cfg);
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn prewarm_model(app: tauri::AppHandle) -> Result<(), String> {
    if !permissions::check_readiness().model_present {
        return Err("No transcription model is installed for the selected engine.".to_string());
    }
    let cfg = (*settings::load()).clone();
    transcribe::prewarm(cfg, app);
    Ok(())
}

pub(crate) fn reset_warmup_cache_inner(recorder: &recorder::Recorder) -> Result<(), String> {
    let state = recorder.state();
    if state.is_busy() {
        return Err(format!(
            "cannot clear warmup cache while dictation is {}",
            state
        ));
    }
    transcribe::abort_active();
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn reset_warmup_cache(recorder_state: tauri::State<'_, RecorderState>) -> Result<(), String> {
    reset_warmup_cache_inner(recorder_state.inner())
}

/// Debug: simulate a hallucination-rejection event so the user can test the
/// error UX in the overlay without waiting for a real false-positive. Emits
/// the exact same `transcription-rejected` event as the hotkey pipeline.
/// Log a structured overlay lifecycle event to the tracing system.
/// Called by the overlay frontend on every mode transition, event
/// arrival, timer operation, and guard decision so we can reconstruct
/// what caused the overlay to disappear.
///
/// `event` is a short snake_case name (e.g. "mode_transition",
/// "event_arrived", "guard_rejected").  `detail` is a JSON object with
/// the relevant fields (mode, job_id, trigger, from, to, etc.).
///
/// This is fire-and-forget with no error return — logging should never
/// block or perturb the UI.
#[tauri::command]
fn log_overlay_event(event: String, detail: serde_json::Value) {
    tracing::info!("[overlay] {event} detail={}", detail);
}

#[tauri::command]
#[specta::specta]
fn simulate_rejection(app: tauri::AppHandle) {
    use tauri::Emitter;
    let _ = app.emit(
        "transcription-rejected",
        serde_json::json!({
            "text": "Simulated repetition: but but but but but but buttons",
            "reason": "Repetition loop detected — the same phrase appeared too many times.",
            "label": "Error detected",
            "pasted": true,
            "flaky": true,
        }),
    );
    crate::hotkey::common::play_chime(crate::hotkey::common::ChimeEvent::Error);
    show_main_and_open_history(app.clone());
    tracing::info!("[debug] simulate_rejection emitted transcription-rejected event");
}

/// After an alt-backend model download, persist the variant, invalidate the
/// worker, and prewarm against the new backend.
fn apply_alt_backend_after_download(
    app: &tauri::AppHandle,
    family: settings::BackendFamily,
    variant: &str,
) -> Result<(), String> {
    let mut cfg = (*settings::load()).clone();
    cfg.backend = family;
    cfg.backend_variant = variant.to_string();
    settings::save(&cfg).map_err(|e| e.to_string())?;
    settings::update_cache(&cfg);
    transcribe::invalidate_worker();
    transcribe::prewarm(cfg, app.clone());
    Ok(())
}

pub(crate) fn show_main_window(
    app: &tauri::AppHandle,
    first_manual_show: &std::sync::atomic::AtomicBool,
) {
    use std::sync::atomic::Ordering;
    if let Some(win) = app.get_webview_window("main") {
        let visible = win.is_visible().unwrap_or(false);
        if !visible && !first_manual_show.swap(true, Ordering::AcqRel) {
            windowing::position_main_window_on_cursor_monitor(app);
        }
        windowing::ensure_main_webview_window_visible(&win);
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn apply_overlay_visibility(app: &tauri::AppHandle, show: bool) {
    // Only toggle when the requested state differs from the current state.
    // Calling `show()` on an already-visible window on macOS reorders it to
    // the front and steals key status from whichever window the user was
    // interacting with — every settings change would defocus the main
    // window mid-click.
    if let Some(overlay) = app.get_webview_window("overlay") {
        let visible = overlay.is_visible().unwrap_or(false);
        if show && !visible {
            let _ = overlay.show();
        } else if !show && visible {
            let _ = overlay.hide();
        }
    }
}

#[tauri::command]
#[specta::specta]
fn scan_models_dir() -> Vec<String> {
    settings::scan_models_dir()
}

#[tauri::command]
#[specta::specta]
fn get_launch_at_login(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;

    // Sync the tray menu item text.
    let label = if enabled {
        "\u{2713} Launch at Login"
    } else {
        "  Launch at Login"
    };
    if let Some(slot) = LAUNCH_MENU_ITEM.get() {
        if let Some(item) = slot.lock().unwrap().as_ref() {
            let _ = item.set_text(label);
        }
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
fn reset_turbotalk(
    app: tauri::AppHandle,
    hotkey_state: tauri::State<'_, HotkeyState>,
    delete_models: bool,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;

    let _ = app.autolaunch().disable();
    // Sync the tray menu item text.
    if let Some(slot) = LAUNCH_MENU_ITEM.get() {
        if let Some(item) = slot.lock().unwrap().as_ref() {
            let _ = item.set_text("  Launch at Login");
        }
    }

    for path in [settings::config_path(), settings::history_path()] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("failed to remove {}: {}", path.display(), e)),
        }
    }

    let mut reset_cfg = settings::Config::default();

    if delete_models {
        if let Some(models_dir) = settings::canonical_models_dir() {
            match std::fs::remove_dir_all(&models_dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("failed to remove {}: {}", models_dir.display(), e)),
            }
        }
    } else {
        let models = settings::scan_models_dir();
        if let Some(model) = models.first() {
            reset_cfg.whisper.model = model.clone();
            reset_cfg.whisper.models = models;
            settings::save(&reset_cfg)
                .map_err(|e| format!("failed to save reset settings: {}", e))?;
        }
    }

    settings::update_cache(&reset_cfg);
    *hotkey_state.write() = reset_cfg.hotkey.clone();
    apply_overlay_visibility(&app, reset_cfg.show_overlay);
    transcribe::invalidate_worker();
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn copy_history_item(text: String) -> Result<(), String> {
    use arboard::Clipboard;
    let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text).map_err(|e| e.to_string())?;
    Ok(())
}

/// Best-effort delete of a model `.bin` file from the canonical models
/// directory. Returns `Ok(true)` if the file was actually deleted,
/// `Ok(false)` for safe skips (file gone, custom path outside the models
/// dir, or models dir doesn't exist), and `Err` only for genuine failures
/// (permission denied, .bin extension check failed, etc).
#[tauri::command]
#[specta::specta]
async fn delete_model_file(path: String) -> Result<bool, String> {
    let Some(canon_models_dir) = settings::canonical_models_dir() else {
        return Ok(false);
    };
    let canon = match std::path::PathBuf::from(&path).canonicalize() {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };
    if !canon.starts_with(&canon_models_dir) {
        return Ok(false);
    }
    if !canon.extension().is_some_and(|e| e == "bin") {
        return Err(format!("refusing to delete non-.bin file: {}", path));
    }
    let canon2 = canon.clone();
    tokio::task::spawn_blocking(move || std::fs::remove_file(&canon2))
        .await
        .map_err(|e| format!("delete failed: {}", e))?
        .map_err(|e| format!("delete failed: {}", e))?;
    tracing::info!("[models] deleted {}", canon.display());
    Ok(true)
}

/// Delete an ONNX model bundle directory for Parakeet.
///
/// `family` is "parakeet"; `variant` is the variant slug (e.g.
/// "tdt-0.6b-v2"). Clears `backend_variant` when the removed bundle was the
/// active selection. Returns `Ok(true)` when a directory was removed,
/// `Ok(false)` when it was already gone.
#[tauri::command]
#[specta::specta]
async fn delete_backend_model(family: String, variant: String) -> Result<bool, String> {
    use crate::settings::BackendFamily;

    let (dir, base) = match family.to_lowercase().as_str() {
        "parakeet" => (
            crate::transcribe_backends::parakeet::variant_dir(&variant),
            crate::transcribe_backends::parakeet::parakeet_models_dir(),
        ),
        other => return Err(format!("unsupported backend family {:?}", other)),
    };

    let Some(dir) = dir else {
        return Ok(false);
    };
    if !dir.exists() {
        return Ok(false);
    }

    let Some(base) = base else {
        return Ok(false);
    };
    let canon = dir
        .canonicalize()
        .map_err(|e| format!("path error: {}", e))?;
    let canon_base = base.canonicalize().unwrap_or(base);
    if !canon.starts_with(&canon_base) {
        return Err("refusing to delete path outside the models directory".into());
    }

    let canon2 = canon.clone();
    tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&canon2))
        .await
        .map_err(|e| format!("delete failed: {}", e))?
        .map_err(|e| format!("delete failed: {}", e))?;
    tracing::info!("[models] deleted backend bundle {}", canon.display());

    let mut cfg = (*settings::load()).clone();
    if cfg.backend_variant == variant {
        if matches!(cfg.backend, BackendFamily::Parakeet) {
            cfg.backend_variant.clear();
            settings::save(&cfg).map_err(|e| format!("failed to save config: {}", e))?;
            settings::update_cache(&cfg);
        }
    }

    transcribe::invalidate_worker();
    Ok(true)
}

#[tauri::command]
#[specta::specta]
fn show_main_and_open_history(app: tauri::AppHandle) {
    use tauri::Emitter;
    // Show and focus the main window
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
    // Tell the main window to switch to the history tab
    let _ = app.emit("open-history", ());
    tracing::info!("[cmd] show_main_and_open_history");
}

#[tauri::command]
#[specta::specta]
fn load_history(app: tauri::AppHandle) -> Vec<settings::HistoryEntry> {
    let result = settings::load_history_detailed();
    if result.dropped > 0 {
        emit_ui_error(
            &app,
            "history-load-malformed",
            format!(
                "{} history entr{} skipped (malformed)",
                result.dropped,
                if result.dropped == 1 {
                    "y was"
                } else {
                    "ies were"
                },
            ),
            true,
        );
    }
    let cfg = settings::load();
    let original_len = result.entries.len();
    let filtered = settings::filter_history_by_policy(result.entries, &cfg.history_auto_delete);
    if filtered.len() < original_len {
        let _ = settings::save_history(&filtered);
    }
    filtered
}

#[tauri::command]
#[specta::specta]
fn save_history(entries: Vec<settings::HistoryEntry>, app: tauri::AppHandle) -> Result<(), String> {
    // Respect the "never save history" toggle — skip the write silently.
    let cfg = settings::load();
    if !cfg.save_history {
        return Ok(());
    }
    if let Err(e) = settings::save_history(&entries) {
        let msg = e.to_string();
        emit_ui_error(
            &app,
            "history-save",
            format!("Couldn't save history: {}", msg),
            true,
        );
        return Err(msg);
    }
    Ok(())
}

/// Open the TurboTalk data folder in the platform's file manager.
#[tauri::command]
#[specta::specta]
fn open_data_folder() -> Result<(), String> {
    let path = crate::settings::data_dir();
    // Create the directory if it doesn't exist yet so the file manager doesn't error.
    crate::settings::create_private_dir_all(&path).map_err(|e| e.to_string())?;

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
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Open the canonical TurboTalk GitHub releases page in the user's browser.
/// Uses the `open` crate for non-shell URL opening on all platforms.
#[tauri::command]
#[specta::specta]
fn open_releases_page() -> Result<(), String> {
    const RELEASES_URL: &str = "https://github.com/eldo9000/TurboTalk/releases/latest";

    open::that_in_background(RELEASES_URL);
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn download_model(
    model_id: String,
    app: tauri::AppHandle,
    cancel_set: tauri::State<'_, DownloadCancelSet>,
) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;

    /// Per-Whisper-model metadata including the pinned SHA-256 digest.
    /// Hashes were derived from the HuggingFace LFS pointer files at:
    ///   https://huggingface.co/ggerganov/whisper.cpp/raw/main/<filename>
    struct WhisperModelSpec {
        url: &'static str,
        sha256: &'static str,
        max_bytes: u64,
    }

    fn catalog_model(model_id: &str) -> Option<WhisperModelSpec> {
        match model_id {
            "ggml-large-v3-turbo" => Some(WhisperModelSpec {
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
                sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
                max_bytes: 1_624_555_275,
            }),
            "ggml-large-v3-turbo-q5_0" => Some(WhisperModelSpec {
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
                sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
                max_bytes: 574_041_195,
            }),
            "ggml-large-v3" => Some(WhisperModelSpec {
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
                sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
                max_bytes: 3_095_033_483,
            }),
            _ => None,
        }
    }

    fn validate_catalog_url(raw: &str) -> Result<(), String> {
        let parsed = url::Url::parse(raw).map_err(|e| format!("invalid model URL: {}", e))?;
        if parsed.scheme() != "https" {
            return Err("model downloads must use https".into());
        }
        if parsed.host_str() != Some("huggingface.co") {
            return Err("model downloads must come from huggingface.co".into());
        }
        if !parsed
            .path()
            .starts_with("/ggerganov/whisper.cpp/resolve/main/")
        {
            return Err("model URL is outside the whisper.cpp catalog".into());
        }
        if !parsed.path().ends_with(".bin") {
            return Err("model URL must point to a .bin file".into());
        }
        Ok(())
    }

    let spec = catalog_model(&model_id).ok_or_else(|| format!("unknown model id: {}", model_id))?;
    validate_catalog_url(spec.url)?;

    // Build the destination path — create the directory if it doesn't exist yet.
    // NOTE: must match data_dir() (which returns ~/Library/Application Support/turbotalk/
    // on macOS), NOT ~/.config/turbotalk. The scan_models_dir and WhisperBackend both
    // use data_dir().join("models"), so the download must land in the same tree.
    let dir = crate::settings::data_dir().join("models");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let canon_dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let filename = format!("{}.bin", model_id);
    if filename.contains(std::path::MAIN_SEPARATOR) || filename.contains("..") {
        return Err("invalid model id".into());
    }
    let dest = canon_dir.join(filename);

    // If the destination file already exists, verify its hash before
    // treating it as installed. This covers the case where the file was
    // placed by a previous download that may have been interrupted or
    // tampered with.
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
        match sha256_file_hex(&dest).await {
            Ok(actual) if actual.eq_ignore_ascii_case(spec.sha256) => {
                let canonical = dest.canonicalize().map_err(|e| e.to_string())?;
                if !canonical.starts_with(&canon_dir) {
                    let _ = tokio::fs::remove_file(&canonical).await;
                    return Err("download destination escaped models directory".into());
                }
                let _ = app.emit(
                    "download-progress",
                    serde_json::json!({ "name": &model_id, "pct": 100u8 }),
                );
                return Ok(canonical.to_string_lossy().into_owned());
            }
            _ => {
                // Hash mismatch or unreadable — remove the stale file
                // and proceed with fresh download.
                let _ = tokio::fs::remove_file(&dest).await;
            }
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("TurboTalk/0.0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let mut resp = client
        .get(spec.url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let total = resp.content_length();
    if total.is_some_and(|t| t > spec.max_bytes) {
        return Err("model file is larger than the allowed limit".into());
    }
    let mut downloaded: u64 = 0;
    let temp_path = tempfile::Builder::new()
        .prefix("turbotalk-model-")
        .suffix(".download")
        .tempfile_in(&canon_dir)
        .map_err(|e| e.to_string())?
        .into_temp_path();
    let temp_file_path = temp_path.to_path_buf();
    let mut file = tokio::fs::File::create(&temp_file_path)
        .await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "name": &model_id, "pct": 0u8 }),
    );

    loop {
        if cancel_set.lock().remove(&model_id) {
            drop(file);
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            return Err("cancelled".into());
        }
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                downloaded += chunk.len() as u64;
                if downloaded > spec.max_bytes {
                    drop(file);
                    let _ = tokio::fs::remove_file(&temp_file_path).await;
                    return Err("model file is larger than the allowed limit".into());
                }
                if let Err(e) = file.write_all(&chunk).await {
                    drop(file);
                    let _ = tokio::fs::remove_file(&temp_file_path).await;
                    return Err(e.to_string());
                }
                let pct = total
                    .filter(|&t| t > 0)
                    .map(|t| ((downloaded * 100) / t).min(99) as u8)
                    .unwrap_or(0);
                let _ = app.emit(
                    "download-progress",
                    serde_json::json!({ "name": &model_id, "pct": pct }),
                );
            }
            Ok(None) => break,
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&temp_file_path).await;
                return Err(format!("Download interrupted: {}", e));
            }
        }
    }

    file.flush().await.map_err(|e| e.to_string())?;
    // Sync metadata so the file is fully on disk before hashing.
    file.sync_all().await.map_err(|e| e.to_string())?;
    drop(file);

    // Verify SHA-256 before persisting. If the hash doesn't match,
    // remove the temp file and fail closed.
    let actual_hash = sha256_file_hex(&temp_file_path).await?;
    if !actual_hash.eq_ignore_ascii_case(spec.sha256) {
        let _ = tokio::fs::remove_file(&temp_file_path).await;
        return Err(format!(
            "SHA-256 mismatch for {}: expected {} got {}",
            model_id, spec.sha256, actual_hash
        ));
    }

    tokio::fs::rename(&temp_file_path, &dest)
        .await
        .map_err(|e| e.to_string())?;

    let canonical = dest.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(&canon_dir) {
        let _ = tokio::fs::remove_file(&canonical).await;
        return Err("download destination escaped models directory".into());
    }
    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "name": &model_id, "pct": 100u8 }),
    );
    Ok(canonical.to_string_lossy().into_owned())
}

#[tauri::command]
#[specta::specta]
fn cancel_download(model_id: String, cancel_set: tauri::State<'_, DownloadCancelSet>) {
    cancel_set.lock().insert(model_id);
}

struct RuntimeModelFileSpec {
    remote_path: &'static str,
    local_name: &'static str,
    max_bytes: u64,
    sha256: Option<&'static str>,
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

async fn sha256_file_hex(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Failed to open {} for hashing: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Failed to hash {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut out, "{:02x}", byte).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

async fn verify_runtime_model_file(
    path: &std::path::Path,
    spec: &RuntimeModelFileSpec,
) -> Result<(), String> {
    let len = tokio::fs::metadata(path)
        .await
        .map_err(|e| format!("Failed to inspect {}: {}", spec.local_name, e))?
        .len();
    if len > spec.max_bytes {
        return Err(format!(
            "{} is larger than the allowed limit ({} > {} bytes)",
            spec.local_name, len, spec.max_bytes
        ));
    }
    if let Some(expected) = spec.sha256 {
        let actual = sha256_file_hex(path).await?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!("{} failed SHA-256 verification", spec.local_name));
        }
    }
    Ok(())
}

/// Download a Parakeet TDT ONNX model bundle from HuggingFace.
///
/// Model source: https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx
///
/// Files downloaded per variant (int8 quantized — ~660 MB total):
///   encoder-model.int8.onnx
///   decoder_joint-model.int8.onnx
///   nemo128.onnx
///   vocab.txt
///
/// Each file is downloaded separately. Progress ticks are per-file (pct within
/// the overall set). The download key used in progress events is
/// `"parakeet-<variant>"` so the frontend can show a per-variant progress bar.
#[tauri::command]
#[specta::specta]
async fn download_parakeet_model(
    variant: String,
    app: tauri::AppHandle,
    cancel_set: tauri::State<'_, DownloadCancelSet>,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    // Validate variant name early so we fail fast.
    if !matches!(variant.as_str(), "tdt-0.6b-v2" | "tdt-0.6b-v3") {
        return Err(format!(
            "unknown Parakeet variant {:?} — expected \"tdt-0.6b-v2\" or \"tdt-0.6b-v3\"",
            variant
        ));
    }

    // HuggingFace repo for the ONNX export of Parakeet TDT 0.6B.
    let repo = match variant.as_str() {
        "tdt-0.6b-v2" => "istupakov/parakeet-tdt-0.6b-v2-onnx",
        "tdt-0.6b-v3" => "istupakov/parakeet-tdt-0.6b-v3-onnx",
        _ => unreachable!(),
    };

    // Int8 ONNX bundle — matches transcribe-rs ParakeetModel::load(..., Int8).
    let files: &[RuntimeModelFileSpec] = match variant.as_str() {
        "tdt-0.6b-v2" => &[
            RuntimeModelFileSpec {
                remote_path: "encoder-model.int8.onnx",
                local_name: "encoder-model.int8.onnx",
                max_bytes: 750 * MIB,
                sha256: Some("3e0581fda6ab843888b51e56d7ee78b6d5bc3237ec113af1f732d1d5286aa155"),
            },
            RuntimeModelFileSpec {
                remote_path: "decoder_joint-model.int8.onnx",
                local_name: "decoder_joint-model.int8.onnx",
                max_bytes: 16 * MIB,
                sha256: Some("a449f49acd68979d418651dd2dcb737cc0f1bf0225e009e29ee326354edbf7d3"),
            },
            RuntimeModelFileSpec {
                remote_path: "nemo128.onnx",
                local_name: "nemo128.onnx",
                max_bytes: 512 * KIB,
                sha256: Some("a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f"),
            },
            RuntimeModelFileSpec {
                remote_path: "vocab.txt",
                local_name: "vocab.txt",
                max_bytes: 64 * KIB,
                sha256: Some("ec182b70dd42113aff6c5372c75cac58c952443eb22322f57bbd7f53977d497d"),
            },
        ],
        "tdt-0.6b-v3" => &[
            RuntimeModelFileSpec {
                remote_path: "encoder-model.int8.onnx",
                local_name: "encoder-model.int8.onnx",
                max_bytes: 750 * MIB,
                sha256: Some("6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09"),
            },
            RuntimeModelFileSpec {
                remote_path: "decoder_joint-model.int8.onnx",
                local_name: "decoder_joint-model.int8.onnx",
                max_bytes: 32 * MIB,
                sha256: Some("eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70"),
            },
            RuntimeModelFileSpec {
                remote_path: "nemo128.onnx",
                local_name: "nemo128.onnx",
                max_bytes: 512 * KIB,
                sha256: Some("a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f"),
            },
            RuntimeModelFileSpec {
                remote_path: "vocab.txt",
                local_name: "vocab.txt",
                max_bytes: 256 * KIB,
                sha256: Some("d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d"),
            },
        ],
        _ => unreachable!(),
    };

    // Build destination directory using the canonical data_dir (not a hardcoded
    // ~/.config path, which differs on Windows — see TASK-71).
    let models_dir = crate::transcribe_backends::parakeet::parakeet_models_dir()
        .ok_or_else(|| "Could not locate Parakeet models directory".to_string())?;
    let dest_dir = models_dir.join(&variant);
    tokio::fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| format!("Failed to create model directory: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest_dir, std::fs::Permissions::from_mode(0o700));
    }
    let canon_dir = dest_dir
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize model directory: {}", e))?;

    // Verify destination is inside the expected parakeet models dir.
    if let Ok(canon_base) = models_dir.canonicalize() {
        if !canon_dir.starts_with(&canon_base) {
            return Err("Download destination is outside the allowed directory".to_string());
        }
    }

    let event_name = format!("parakeet-{}", variant);
    let client = reqwest::Client::builder()
        .user_agent("TurboTalk/0.0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "name": &event_name, "pct": 0u8 }),
    );

    for (file_idx, spec) in files.iter().enumerate() {
        let filename = spec.local_name;
        let download_key = format!("{}-{}", event_name, filename);

        // Check for cancellation before starting each file.
        if cancel_set.lock().remove(&event_name) {
            return Err("cancelled".into());
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            repo, spec.remote_path
        );

        // Validate URL shape — must be huggingface.co.
        {
            let parsed = url::Url::parse(&url).map_err(|e| format!("invalid URL: {}", e))?;
            if parsed.scheme() != "https" || parsed.host_str() != Some("huggingface.co") {
                return Err("model downloads must use https://huggingface.co".to_string());
            }
        }

        // Reject path traversal in filename.
        if filename.contains(std::path::MAIN_SEPARATOR) || filename.contains("..") {
            return Err(format!("invalid filename: {}", filename));
        }
        let dest_path = canon_dir.join(filename);
        if !dest_path.starts_with(&canon_dir) {
            return Err("download destination escaped model directory".to_string());
        }

        // Skip if already downloaded (idempotent re-runs).
        if dest_path.exists() {
            match verify_runtime_model_file(&dest_path, spec).await {
                Ok(()) => {
                    tracing::info!("[parakeet-dl] {} already present — skipping", filename);
                    // Still emit progress for the file.
                    let base_pct = ((file_idx + 1) * 100 / files.len()).min(99) as u8;
                    let _ = app.emit(
                        "download-progress",
                        serde_json::json!({ "name": &event_name, "pct": base_pct }),
                    );
                    continue;
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&dest_path).await;
                    tracing::warn!("[parakeet-dl] removed invalid existing {}: {}", filename, e);
                }
            }
        }

        tracing::info!("[parakeet-dl] downloading {} from {}", filename, url);

        let mut resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed for {}: {}", filename, e))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {} downloading {}", resp.status(), filename));
        }

        let total = resp.content_length();
        if total.is_some_and(|t| t > spec.max_bytes) {
            return Err(format!("{} is larger than the allowed limit", filename));
        }
        let mut downloaded: u64 = 0;

        let temp_path = tempfile::Builder::new()
            .prefix("turbotalk-parakeet-")
            .suffix(".download")
            .tempfile_in(&canon_dir)
            .map_err(|e| format!("Failed to create temp file: {}", e))?
            .into_temp_path();
        let temp_file_path = temp_path.to_path_buf();
        let mut file = tokio::fs::File::create(&temp_file_path)
            .await
            .map_err(|e| format!("Failed to create file: {}", e))?;

        loop {
            if cancel_set.lock().remove(&event_name) {
                drop(file);
                let _ = tokio::fs::remove_file(&temp_file_path).await;
                return Err("cancelled".into());
            }
            // Also check per-file cancel key.
            if cancel_set.lock().remove(&download_key) {
                drop(file);
                let _ = tokio::fs::remove_file(&temp_file_path).await;
                return Err("cancelled".into());
            }
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    downloaded += chunk.len() as u64;
                    if downloaded > spec.max_bytes {
                        drop(file);
                        let _ = tokio::fs::remove_file(&temp_file_path).await;
                        let _ = tokio::fs::remove_file(&dest_path).await;
                        return Err(format!("{} is larger than the allowed limit", filename));
                    }
                    if let Err(e) = file.write_all(&chunk).await {
                        drop(file);
                        let _ = tokio::fs::remove_file(&temp_file_path).await;
                        return Err(format!("Write failed for {}: {}", filename, e));
                    }
                    // Progress: blend per-file progress with overall file index.
                    // Each file contributes an equal fraction of 0..99%.
                    let file_pct = total
                        .filter(|&t| t > 0)
                        .map(|t| (downloaded * 100 / t).min(100) as u8)
                        .unwrap_or(50u8);
                    let overall_pct = ((file_idx as u32 * 100 + file_pct as u32)
                        / files.len() as u32)
                        .min(99) as u8;
                    let _ = app.emit(
                        "download-progress",
                        serde_json::json!({ "name": &event_name, "pct": overall_pct }),
                    );
                }
                Ok(None) => break,
                Err(e) => {
                    drop(file);
                    let _ = tokio::fs::remove_file(&temp_file_path).await;
                    return Err(format!("Download interrupted for {}: {}", filename, e));
                }
            }
        }

        file.flush().await.map_err(|e| e.to_string())?;
        drop(file);

        if let Err(e) = verify_runtime_model_file(&temp_file_path, spec).await {
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            let _ = tokio::fs::remove_file(&dest_path).await;
            return Err(e);
        }

        tokio::fs::rename(&temp_file_path, &dest_path)
            .await
            .map_err(|e| format!("Rename failed for {}: {}", filename, e))?;

        // Verify the final path is still within canon_dir (rename target check).
        if let Ok(canon_dest) = dest_path.canonicalize() {
            if !canon_dest.starts_with(&canon_dir) {
                let _ = tokio::fs::remove_file(&dest_path).await;
                return Err("downloaded file escaped model directory".to_string());
            }
        }

        tracing::info!("[parakeet-dl] {} complete", filename);
    }

    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "name": &event_name, "pct": 100u8 }),
    );

    tracing::info!(
        "[parakeet-dl] all files for variant {:?} downloaded to {}",
        variant,
        canon_dir.display()
    );

    apply_alt_backend_after_download(&app, settings::BackendFamily::Parakeet, &variant)?;

    Ok(())
}

/// A model descriptor returned by `list_models_for_family`.
/// The `id` is a stable short identifier used as a download key and
/// in progress events. The `label`, `description`, and `size` fields
/// are display strings for the UI. `download_url` is the canonical
/// HuggingFace URL (empty string if not directly downloadable via the
/// existing `download_model` command). `path_hint` is the expected
/// on-disk path after download (empty for Whisper — those use `scan_models_dir`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelDescriptor {
    pub id: String,
    /// Human tier label (e.g. "Recommended", "Large") — matches Whisper Models UI.
    pub tier: String,
    /// Technical model name shown in the monospace pill (e.g. `parakeet-en-v2`).
    pub label: String,
    pub description: String,
    pub size: String,
    pub download_url: String,
    pub path_hint: String,
    /// True when the required ONNX bundle files are present on disk.
    pub installed: bool,
    /// True for the recommended starter model within this backend family.
    pub recommended: bool,
}

fn parakeet_installed(variant: &str) -> bool {
    crate::transcribe_backends::parakeet::variant_dir(variant)
        .and_then(|d| crate::transcribe_backends::parakeet::validate_parakeet_model_dir(&d).ok())
        .is_some()
}

/// Return the available models for a given backend family.
///
/// - Whisper: returns the three models in the existing catalog (same IDs as
///   `download_model` accepts). The UI should show installed vs. not-installed
///   state by cross-referencing with `scan_models_dir`.
/// - Parakeet: returns "tdt-0.6b-v2" (English) and "tdt-0.6b-v3" (multilingual).
///
/// `family` is a lowercase string: "whisper" | "parakeet".
/// Unknown values are treated as "whisper".
#[tauri::command]
#[specta::specta]
fn list_models_for_family(family: String) -> Vec<ModelDescriptor> {
    match family.to_lowercase().as_str() {
        "parakeet" => vec![
            ModelDescriptor {
                id: "parakeet-tdt-0.6b-v2".to_string(),
                tier: "Recommended".to_string(),
                label: "parakeet-en-v2".to_string(),
                description: "english-only · fastest".to_string(),
                size: "660 MB".to_string(),
                download_url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx".to_string(),
                path_hint: ".config/turbotalk/models/parakeet/tdt-0.6b-v2/".to_string(),
                installed: parakeet_installed("tdt-0.6b-v2"),
                recommended: true,
            },
            ModelDescriptor {
                id: "parakeet-tdt-0.6b-v3".to_string(),
                tier: "Multilingual".to_string(),
                label: "parakeet-multi-v3".to_string(),
                description: "multilingual · 25 european languages".to_string(),
                size: "660 MB".to_string(),
                download_url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx".to_string(),
                path_hint: ".config/turbotalk/models/parakeet/tdt-0.6b-v3/".to_string(),
                installed: parakeet_installed("tdt-0.6b-v3"),
                recommended: false,
            },
        ],
        // "whisper" or anything else
        _ => vec![
            ModelDescriptor {
                id: "ggml-large-v3-turbo".to_string(),
                tier: "Recommended".to_string(),
                label: "ggml-large-v3-turbo".to_string(),
                description: "multilingual · best accuracy".to_string(),
                size: "1.6 GB".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin".to_string(),
                path_hint: String::new(),
                installed: false,
                recommended: true,
            },
            ModelDescriptor {
                id: "ggml-large-v3-turbo-q5_0".to_string(),
                tier: "Small".to_string(),
                label: "ggml-large-v3-turbo-q5_0".to_string(),
                description: "low RAM · english only".to_string(),
                size: "574 MB".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin".to_string(),
                path_hint: String::new(),
                installed: false,
                recommended: false,
            },
            ModelDescriptor {
                id: "ggml-large-v3".to_string(),
                tier: "Large".to_string(),
                label: "ggml-large-v3".to_string(),
                description: "high accuracy · high RAM · slow".to_string(),
                size: "3.1 GB".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin".to_string(),
                path_hint: String::new(),
                installed: false,
                recommended: false,
            },
        ],
    }
}

#[tauri::command]
#[specta::specta]
fn list_audio_devices() -> Vec<String> {
    // Gate on microphone permission so the Settings tab never triggers the
    // macOS TCC prompt. cpal's input_devices() probes CoreAudio which can
    // cause the microphone permission dialog to appear immediately. When
    // permission is not yet granted, return an empty list — the dropdown
    // still shows "System default".
    if !crate::permissions::mic_granted() {
        return vec![];
    }
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devs) => devs.filter_map(|d| d.name().ok()).collect(),
        Err(_) => vec![],
    }
}

/// Check whether any Logitech HID mouse (VendorID 0x046d) is currently
/// connected. Uses `ioreg` on macOS — fast (< 50 ms) and requires no new
/// FFI. On non-macOS platforms always returns false (Logitech software
/// interception is a macOS-only problem).
#[tauri::command]
#[specta::specta]
fn detect_logitech_mouse() -> bool {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ioreg")
            .args(["-r", "-c", "IOHIDDevice", "-k", "VendorID", "-w0"])
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout.contains("\"VendorID\" = 1133")
            }
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Cancel an in-flight dictation job from the frontend. The hotkey thread
/// calls `recorder.cancel()` directly and does not go through this command —
/// this command exists for future UI use (e.g. an X button on the overlay).
/// Registered in the invoke_handler and specta builder so it appears in
/// `bindings.ts`.
#[tauri::command]
#[specta::specta]
fn cancel_recording(
    recorder_state: tauri::State<'_, RecorderState>,
    tray_icon_state: tauri::State<'_, TrayIconState>,
    hotkey_state: tauri::State<'_, HotkeyState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let rec = recorder_state.inner();
    let tray = tray_icon_state.inner();
    let state = rec.state();
    if !state.is_busy() {
        return Err(format!("nothing to cancel (recorder is {})", state));
    }
    // Hold mode + Recording: a matching key-release will dispatch ptt_up.
    // Arm one suppression slot so it no-ops instead of cascading into
    // CANCEL_PENDING. Toggle mode releases are already no-ops, so skip.
    let hold_mode = hotkey_state.read().mode == "hold";
    if hold_mode && matches!(state, recorder::State::Recording) {
        hotkey::arm_ptt_up_suppression();
    }
    hotkey::trigger_cancel(rec, tray, &app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn start_recording(
    recorder_state: tauri::State<'_, RecorderState>,
    tray_icon_state: tauri::State<'_, TrayIconState>,
    app: tauri::AppHandle,
) {
    hotkey::trigger_start(recorder_state.inner(), tray_icon_state.inner(), &app);
}

#[tauri::command]
#[specta::specta]
fn stop_recording(
    recorder_state: tauri::State<'_, RecorderState>,
    tray_icon_state: tauri::State<'_, TrayIconState>,
    app: tauri::AppHandle,
) {
    hotkey::trigger_stop(recorder_state.inner(), tray_icon_state.inner(), &app);
}

use parking_lot::RwLock;
use std::sync::Arc;

// Shared hotkey config — hotkey thread reads this on every event so
// settings changes take effect without restarting the app.
pub(crate) type HotkeyState = Arc<RwLock<settings::HotkeyConfig>>;
pub(crate) type RecorderState = Arc<recorder::Recorder>;
pub(crate) type TrayIconState = tauri::tray::TrayIcon;
type DownloadCancelSet = parking_lot::Mutex<std::collections::HashSet<String>>;

use tauri::{Emitter, Manager, RunEvent, WindowEvent};

/// Global handle to the tray's "Launch at Login" menu item. Populated once
/// during tray construction in `run()`. `set_launch_at_login` reads this to
/// keep the tray menu in sync when toggled from Settings.
pub(crate) static LAUNCH_MENU_ITEM: std::sync::OnceLock<
    std::sync::Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
> = std::sync::OnceLock::new();

/// Build the tauri-specta type-export descriptor. Lives in its own function
/// so the `#[test]` regenerator below can call it without standing up the
/// full Tauri runtime.
#[cfg(any(debug_assertions, test))]
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
        get_config,
        save_config,
        prewarm_model,
        reset_warmup_cache,
        simulate_rejection,
        scan_models_dir,
        list_models_for_family,
        get_launch_at_login,
        set_launch_at_login,
        reset_turbotalk,
        list_audio_devices,
        detect_logitech_mouse,
        download_model,
        cancel_download,
        download_parakeet_model,
        delete_model_file,
        delete_backend_model,
        load_history,
        save_history,
        copy_history_item,
        cancel_recording,
        start_recording,
        stop_recording,
        show_main_and_open_history,
        open_data_folder,
        open_releases_page,
        ollama::check_ollama_model,
        ollama::check_ollama_partial_blobs,
        ollama::open_url,
        ollama::ping_ollama,
        ollama::prewarm_ollama,
        ollama::pull_ollama_model,
        diagnostics::run_diagnostics,
        diagnostic_log::log_client_event,
        diagnostic_log::export_diagnostic_report,
        diagnostic_log::submit_bug_report,
        diagnostic_log::open_logs_folder,
        permissions::check_readiness,
        permissions::request_microphone_permission,
        permissions::request_input_monitoring_permission,
        permissions::request_system_audio_permission,
        permissions::open_system_settings,
        permissions::restart_app,
        permissions::prompt_for_accessibility,
        permissions::reset_onboarding,
        permissions::clear_force_onboarding,
        permissions::set_setup_complete,
        permissions::reset_tcc_entry,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    startup_logging::init();

    // ── Typed Rust↔TS contract ─────────────────────────────────────────────
    // Every command crossing the IPC boundary that the frontend talks to is
    // listed in `specta_builder()`. In debug builds, `.export()` writes
    // `src/bindings.ts` so adding/removing/renaming a field on `Config` (or
    // any sub-struct) shows up as a TypeScript compile error in the
    // frontend. `get_theme`/`get_accent` stay free-form because the
    // frontend reaches them through `@libre/ui`.
    #[cfg(debug_assertions)]
    {
        use specta_typescript::Typescript;
        if let Err(e) = specta_builder().export(
            Typescript::default()
                .header("// AUTO-GENERATED by tauri-specta. Do not edit by hand.\n// Run `cargo test --manifest-path src-tauri/Cargo.toml export_bindings`\n// (or launch the app in dev) to regenerate.\n"),
            "../src/bindings.ts",
        ) {
            tracing::warn!("[specta] failed to export bindings.ts: {:?}", e);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_theme,
            get_accent,
            get_config,
            save_config,
            prewarm_model,
            reset_warmup_cache,
            simulate_rejection,
            log_overlay_event,
            scan_models_dir,
            list_models_for_family,
            get_launch_at_login,
            set_launch_at_login,
            reset_turbotalk,
            list_audio_devices,
            detect_logitech_mouse,
            download_model,
            cancel_download,
            download_parakeet_model,
            delete_model_file,
            delete_backend_model,
            load_history,
            save_history,
            copy_history_item,
            cancel_recording,
            start_recording,
            stop_recording,
            show_main_and_open_history,
            open_data_folder,
            open_releases_page,
            ollama::check_ollama_model,
            ollama::check_ollama_partial_blobs,
            ollama::open_url,
            ollama::ping_ollama,
            ollama::prewarm_ollama,
            ollama::pull_ollama_model,
            diagnostics::run_diagnostics,
            diagnostic_log::log_client_event,
            diagnostic_log::export_diagnostic_report,
            diagnostic_log::submit_bug_report,
            diagnostic_log::open_logs_folder,
            permissions::check_readiness,
            permissions::request_microphone_permission,
            permissions::request_input_monitoring_permission,
            permissions::request_system_audio_permission,
            permissions::open_system_settings,
            permissions::restart_app,
            permissions::prompt_for_accessibility,
            permissions::reset_onboarding,
            permissions::clear_force_onboarding,
            permissions::set_setup_complete,
            permissions::reset_tcc_entry,
        ])
        .setup(|app| {
            // ── Hide dock icon (tray-only app) ─────────────────────────────
            #[cfg(target_os = "macos")]
            {
                use objc2::msg_send;
                let app_cls = objc2::class!(NSApplication);
                let shared_app: *mut objc2::runtime::AnyObject =
                    unsafe { msg_send![app_cls, sharedApplication] };
                // NSApplicationActivationPolicyAccessory = 1 (no dock icon)
                unsafe { msg_send![shared_app, setActivationPolicy: 1i64] }
            }

            // ── Tray icon ──────────────────────────────────────────────────
            let tray_icon = crate::tray::build(app)?;

            // ── Config (write defaults on first run) ───────────────────────
            let cfg_result = settings::load_detailed();
            let cfg = cfg_result.config;
            if let Some(err_msg) = cfg_result.parse_error {
                emit_ui_error(&app.handle().clone(), "config-parse", err_msg, true);
            }
            if let Err(e) = settings::save(&cfg) {
                tracing::warn!("[settings] could not write config: {:?}", e);
            }
            // Populate the process-wide settings cache so PTT-down readers
            // (audio.rs and friends) skip the per-press disk read. Idempotent.
            settings::update_cache(&cfg);
            settings::prime_cache();

            // ── Shared hotkey state — updated live when settings are saved ──
            let hotkey_state: HotkeyState = Arc::new(RwLock::new(cfg.hotkey.clone()));
            app.manage(hotkey_state.clone());

            // ── Launch splash — shown on every app start unless disabled ────
            // Window is pre-declared in tauri.conf.json (visible:false) so it
            // doesn't accumulate across hot-reloads; we position and show it here.
            if cfg.show_splash {
                if let Some(splash_win) = app.get_webview_window("splash") {
                    tracing::info!("[splash] positioning and showing");
                    const SPLASH_W: f64 = 360.0;
                    const SPLASH_H: f64 = 220.0;
                    windowing::center_window_on_cursor_monitor(&splash_win, SPLASH_W, SPLASH_H);
                    let _ = splash_win.show();
                }
            }

            // ── Main window — hidden until tray click unless onboarding ───
            if let Some(win) = app.get_webview_window("main") {
                use tauri::LogicalSize;
                // Compact floor keeps resize handles reachable on small displays;
                // the frontend still restores the preferred 550×560 when it fits.
                let _ = win.set_min_size(Some(LogicalSize::new(
                    windowing::MAIN_WINDOW_MIN_W,
                    windowing::MAIN_WINDOW_MIN_H,
                )));
                windowing::ensure_main_webview_window_visible(&win);
                // Keep the main window visible while tray/status-item
                // visibility is unreliable on this macOS setup. The close
                // handler still hides instead of quitting, and the Dock icon
                // is now the reliable way back in.
                let readiness = crate::permissions::check_readiness();
                let height = if readiness.ready {
                    windowing::MAIN_WINDOW_DEFAULT_H
                } else {
                    windowing::MAIN_WINDOW_MIN_H
                };
                let _ = win.set_size(tauri::LogicalSize::new(
                    windowing::MAIN_WINDOW_DEFAULT_W,
                    height,
                ));
                let _ = win.center();
                windowing::ensure_main_webview_window_visible(&win);
                let _ = win.show();
                let _ = win.set_focus();
            }

            // ── Overlay — cursor-transparent so clicks always pass through ──
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_ignore_cursor_events(true);
            }

            // ── Cursor dot — cursor-transparent, always starts hidden ──
            if let Some(dot) = app.get_webview_window("cursor-dot") {
                let _ = dot.set_ignore_cursor_events(true);
            }

            // ── Status window — clickable, starts hidden ──────────────────
            // Positioned near the cursor so the warm-up / rejection status
            // tile appears wherever the user is focused.  Cursor events are
            // NOT ignored — the X button on rejection messages must work.
            if let Some(status_win) = app.get_webview_window("status") {
                windowing::reposition_status_to_cursor(&status_win);
            }

            // Pin the overlay to the cursor's monitor at startup so the very
            // first press doesn't have to fight a stale primary-monitor
            // placement from `center: true` in tauri.conf.json.
            windowing::reposition_overlay_to_cursor_monitor(app.handle());
            apply_overlay_visibility(app.handle(), cfg.show_overlay);

            // ── Hotkey ─────────────────────────────────────────────────────
            // Stream opens on first keypress; always re-queries the config device
            // so built-in mic / AirPods switches work without restarting.
            let recorder: RecorderState = Arc::new(recorder::Recorder::new()?);

            // ── Tracing health watchdog ──────────────────────────────────
            // The tracing-appender NonBlocking writer can die silently if its
            // background thread panics (e.g. a disk-I/O error on the log file).
            // When that happens every `tracing::info!()` / `warn!()` etc.
            // becomes a no-op and we lose all observability — no errors log,
            // no session log, nothing. This watchdog stats the main log file
            // every 60 s; if the mtime is stale, it writes to stderr only.
            // Idle apps naturally stop writing logs, so this must never raise
            // a user-facing error toast.
            spawn_tracing_watchdog(app.handle().clone());

            // Emit live audio level to the overlay at 20 Hz while recording.
            // Same thread also services the device-lost edge: if the cpal
            // error callback flagged it, we cancel the recorder, reset the
            // tray, and emit `device-lost` to the frontend so the overlay
            // clears and the main window can surface a banner.
            let level_rec = recorder.clone();
            let level_app = app.handle().clone();
            let level_tray = tray_icon.clone();
            std::thread::spawn(move || {
                // Cursor-dot: offset from cursor hotspot in logical points.
                const DOT_OFFSET_X: f64 = 12.0;
                const DOT_OFFSET_Y: f64 = 16.0;
                let mut cached_primary_scale = 1.0f64;
                let mut dot_was_visible = false;

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if level_rec.device_lost() {
                        tracing::warn!("[lib] observed device-lost flag — cancelling recorder");
                        // Hold mode: the user is still holding the record key when the
                        // device drops, so a `ptt_up` is still coming. Arm one
                        // suppression slot before cancelling so that key-up no-ops in
                        // `ptt_up` instead of calling `stop()` on the now-Ready recorder,
                        // hitting IllegalTransition, and arming CANCEL_PENDING — which
                        // would fake-cancel the user's *next* press. Mirrors the
                        // `trigger_cancel` callers in `cancel_recording` and the tray
                        // click handler. Toggle-mode releases are already no-ops.
                        if matches!(level_rec.state(), recorder::State::Recording)
                            && level_app.state::<HotkeyState>().read().mode == "hold"
                        {
                            hotkey::arm_ptt_up_suppression();
                        }
                        level_rec.cancel_after_device_lost();
                        tray::set_tray_icon(&level_tray, tray::TrayState::Idle);
                        let _ = level_app.emit("device-lost", ());
                        // `recording-discarded` keeps the overlay's existing
                        // catch-all listener happy without the frontend needing
                        // to learn a new clear-overlay path.
                        let _ = level_app.emit("recording-discarded", ());
                    }
                    let is_recording = level_rec.is_recording();
                    if is_recording {
                        let _ = level_app.emit("audio-level", level_rec.level());
                    }

                    // Cursor-dot indicator: follow mouse while recording or transcribing.
                    let is_busy = level_rec.state().is_busy();
                    if settings::cursor_dot_indicator_enabled() && is_busy {
                        if let Ok(cursor) = level_app.cursor_position() {
                            if let Some(dot) = level_app.get_webview_window("cursor-dot") {
                                if !dot_was_visible {
                                    // Refresh scale factor on first show.
                                    cached_primary_scale = dot
                                        .primary_monitor()
                                        .ok()
                                        .flatten()
                                        .map(|m| m.scale_factor())
                                        .unwrap_or(1.0);
                                }
                                windowing::position_cursor_dot(
                                    &dot,
                                    cursor,
                                    cached_primary_scale,
                                    DOT_OFFSET_X,
                                    DOT_OFFSET_Y,
                                );
                                if !dot_was_visible {
                                    let _ = dot.show();
                                    dot_was_visible = true;
                                }
                            }
                        }
                    } else if dot_was_visible && !is_busy {
                        if let Some(dot) = level_app.get_webview_window("cursor-dot") {
                            let _ = dot.hide();
                        }
                        dot_was_visible = false;
                    }
                } // end loop
            });

            // Manage recorder and tray_icon as app state so the
            // `cancel_recording` command can reach them from the invoke handler.
            app.manage(recorder.clone());
            app.manage(tray_icon.clone());
            app.manage(
                parking_lot::Mutex::new(std::collections::HashSet::<String>::new())
                    as DownloadCancelSet,
            );

            // Pre-register in the Input Monitoring list so the app appears
            // there before the user opens System Settings during onboarding.
            macos_input_monitoring::register();

            hotkey::spawn(recorder, tray_icon, app.handle().clone(), hotkey_state);

            // Kill any whisper-server orphans left by a previous SIGKILL or
            // rapid dev-mode restart before this run can build a fresh one.
            transcribe::kill_orphans();
            // Eagerly prewarm the transcription model at startup so the first
            // dictation press arms instantly without the yellow "connecting"
            // tile. Memory cost is front-loaded once instead of repeating on
            // first press, and the user doesn't have to wait.
            let app_h = app.handle().clone();
            std::thread::spawn(move || {
                // Let the splash screen and initial UI render complete (~2s)
                // before loading the model so startup feels instant.
                std::thread::sleep(std::time::Duration::from_secs(2));
                crate::transcribe::prewarm((*crate::settings::load()).clone(), app_h);
            });
            if crate::permissions::check_readiness().ready {
                crate::permissions::clear_onboarding_active();
            } else {
                tracing::info!("[transcribe] startup prewarm skipped until onboarding is complete");
            }

            Ok(())
        })
        // Close hides the main window instead of quitting — the Dock icon is
        // the reliable way back in since the tray icon does not render on
        // macOS 26. Right-click Dock → Quit to actually exit.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building TurboTalk")
        .run(|_app, event| {
            // Kill the persistent whisper-server child on app exit. Statics
            // do not run Drop on process termination, so without this the
            // setsid'd child survives every quit and accumulates ~1.6 GB
            // resident per leak.
            match event {
                RunEvent::ExitRequested { .. } | RunEvent::Exit => {
                    transcribe::abort_active();
                }
                _ => {}
            }
        });
}

// ── Tracing health watchdog ──────────────────────────────────────────────
//
// If the tracing-appender NonBlocking writer thread panics or dies, every
// `tracing::info!()` / `warn!()` / `error!()` becomes a silent no-op. This
// watchdog stats the newest main session-log file every 60 s.  When the
// file's mtime is more than TRACING_STALE_SECS old, the watchdog writes a
// one-shot warning to stderr. It deliberately does not emit a UI toast:
// quiet/idle app sessions are normal and should not look like failures.
//
// One-shot semantics: the first stale detection writes stderr; subsequent
// checks remain quiet until the app restarts.

const TRACING_HEALTH_INTERVAL_SECS: u64 = 60;
const TRACING_STALE_SECS: u64 = 900;

fn spawn_tracing_watchdog(_app: tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::UNIX_EPOCH;

    let already_fired = Arc::new(AtomicBool::new(false));

    std::thread::spawn(move || {
        let log_dir = match startup_logging::LOG_DIR_CELL.get() {
            Some(d) => d.clone(),
            None => {
                eprintln!("[tracing-watchdog] LOG_DIR_CELL not set — watchdog disabled");
                return;
            }
        };

        let now = || -> u64 {
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };

        // Give the app time to start and emit the first log lines before
        // the watchdog begins checking.
        std::thread::sleep(std::time::Duration::from_secs(TRACING_HEALTH_INTERVAL_SECS));

        loop {
            // Find the newest turbotalk.YYYY-MM-DD.log
            let newest = diagnostic_log::log_files_for(diagnostic_log::MAIN_LOG_PREFIX).pop();
            let age_secs = newest
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok())
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| now().saturating_sub(d.as_secs()))
                .unwrap_or(u64::MAX);

            if age_secs > TRACING_STALE_SECS {
                if !already_fired.swap(true, Ordering::AcqRel) {
                    let log_path = newest
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| log_dir.display().to_string());
                    eprintln!(
                        "[tracing-watchdog] Main session log has not been written in {age_secs}s \
                         (path={log_path}). The tracing writer may have died — \
                         subsequent `tracing::info!()` calls are likely no-ops. \
                         Restart TurboTalk to restore full logging."
                    );
                }
            }

            std::thread::sleep(std::time::Duration::from_secs(TRACING_HEALTH_INTERVAL_SECS));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::specta_builder;
    use super::windowing::{clamp_window_position_to_work_area, intersection_area};

    #[test]
    fn clamp_window_position_keeps_window_inside_work_area_when_it_fits() {
        assert_eq!(
            clamp_window_position_to_work_area(-200, 40, 420, 420, 0, 25, 1440, 875),
            (0, 40)
        );
        assert_eq!(
            clamp_window_position_to_work_area(1300, 760, 420, 420, 0, 25, 1440, 875),
            (1020, 480)
        );
    }

    #[test]
    fn clamp_window_position_anchors_oversized_windows_to_work_area_origin() {
        assert_eq!(
            clamp_window_position_to_work_area(200, 200, 1600, 1000, 0, 25, 1440, 875),
            (0, 25)
        );
    }

    #[test]
    fn intersection_area_returns_zero_for_disjoint_rectangles() {
        assert_eq!(intersection_area(0, 0, 100, 100, 200, 200, 100, 100), 0);
        assert_eq!(intersection_area(0, 0, 100, 100, 50, 50, 100, 100), 2500);
    }

    /// Regenerate `src/bindings.ts` on demand. Run with:
    ///   cargo test --manifest-path src-tauri/Cargo.toml export_bindings
    ///
    /// We also run it implicitly in dev builds via `run()`, but a unit test
    /// is the cleanest way to refresh the file from CI / the worker without
    /// launching a full Tauri window.
    #[test]
    fn export_bindings() {
        use specta_typescript::Typescript;
        specta_builder()
            .export(
                Typescript::default().header(
                    "// AUTO-GENERATED by tauri-specta. Do not edit by hand.\n\
                     // Run `cargo test --manifest-path src-tauri/Cargo.toml export_bindings`\n\
                     // (or launch the app in dev) to regenerate.\n",
                ),
                "../src/bindings.ts",
            )
            .expect("specta export failed");
    }
}
