// TurboTalk — personal voice dictation utility
//
// Module map (see ARCHITECTURE.md):
//   audio       mic capture (cpal)
//   recorder    Lifecycle: Ready → Recording → FinalizingAudio → Transcribing
//                          → Cleaning → Pasting → Ready (one in-flight job)
//   transcribe  whisper.cpp sidecar wrapper
//   paste       active-app text injection
//   hotkey      global push-to-talk binding (CGEventTap — Right Alt)
//   cleanup     LLM postprocessor (Chaperone Layer)
//   settings    config persistence

pub mod audio;
pub mod audio_finalizer;
pub mod cleanup;
pub mod diagnostics;
pub mod hotkey;
pub mod ollama;
pub mod paste;
pub mod permissions;
pub mod recorder;
pub mod settings;
pub mod theme;
pub mod transcribe;
pub mod transcribe_backends;
pub mod tray;
pub mod vad;

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
    settings::load()
}

#[tauri::command]
#[specta::specta]
fn save_config(
    app: tauri::AppHandle,
    cfg: settings::Config,
    hotkey_state: tauri::State<'_, HotkeyState>,
) -> Result<(), String> {
    use tauri::Emitter;
    settings::save(&cfg).map_err(|e| e.to_string())?;
    settings::update_cache(&cfg);
    *hotkey_state.write() = cfg.hotkey.clone();
    apply_overlay_visibility(&app, cfg.show_overlay);
    // Pick up overlay_position changes on the next render — repositioning
    // now (rather than waiting for the next PTT) keeps the pill out of the
    // user's way the moment they toggle the setting.
    reposition_overlay_to_cursor_monitor(&app);
    // TASK-20: drop the cached TranscriptionWorker so the next dictation
    // picks up any changes to `whisper.model` or `cleanup.vocabulary`. The
    // rebuild is cheap (path validation only — no model load) so we do not
    // try to detect "did anything actually change".
    transcribe::invalidate_worker();
    // Eagerly rebuild against the new config in the background so the next
    // PTT press doesn't have to sit on the yellow arming tile while the
    // model loads. Mirrors the startup prewarm — same readiness semantics,
    // same `dictation-ready` / `dictation-ready-failed` events.
    transcribe::prewarm(cfg.clone(), app.clone());
    // Notify other windows (overlay) of UI-relevant config changes. The
    // overlay reads transcript_size_indicator and show_overlay on mount and
    // refreshes when this fires.
    let _ = app.emit("config-update", &cfg);
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn prewarm_model(app: tauri::AppHandle) -> Result<(), String> {
    if !permissions::check_readiness().model_present {
        return Err(
            "No transcription model is installed for the selected engine.".to_string(),
        );
    }
    let cfg = settings::load();
    transcribe::prewarm(cfg, app);
    Ok(())
}

/// After an alt-backend model download, persist the variant, invalidate the
/// worker, and prewarm against the new backend.
fn apply_alt_backend_after_download(
    app: &tauri::AppHandle,
    family: settings::BackendFamily,
    variant: &str,
) -> Result<(), String> {
    let mut cfg = settings::load();
    cfg.backend = family;
    cfg.backend_variant = variant.to_string();
    settings::save(&cfg).map_err(|e| e.to_string())?;
    settings::update_cache(&cfg);
    transcribe::invalidate_worker();
    transcribe::prewarm(cfg, app.clone());
    Ok(())
}

/// Logical pill-bottom geometry, shared with the frontend's expectations.
/// The window is 460×280, but the visible pill is a 260×80 rect inside it
/// surrounded by a 100 px gutter (room for blur/shadow). For the pill to
/// land `BOTTOM_GAP` above the screen bottom, the *window's* top-left needs
/// to sit at `screen_bottom - (PILL_H + BOTTOM_GAP + GUTTER)`. We bake that
/// into the y math here so the frontend never re-positions.
const OVERLAY_W_LOGICAL: f64 = 460.0;
const OVERLAY_PILL_BOTTOM_OFFSET: f64 = 290.0; // PILL_H 80 + BOTTOM_GAP 110 + GUTTER 100
/// Top-position equivalent: window top sits `TOP_GAP - GUTTER` below the
/// screen top so the pill (centered inside the 280 px window) lands
/// `TOP_GAP` (110 px) below the screen top. TOP_GAP matches BOTTOM_GAP so
/// the visual breathing room is symmetric. On macOS the menu bar at the
/// very top occupies ~24 px of that gap.
const OVERLAY_PILL_TOP_OFFSET: f64 = 10.0; // TOP_GAP 110 - GUTTER 100

/// Compute the window-top y for the overlay given the monitor origin, monitor
/// height (logical), and the user's overlay_position preference. Centralises
/// the top vs bottom branch so the macOS and Windows/Linux paths agree.
fn overlay_y_for_position(mp_y: f64, mon_h_logical: f64, position: &str) -> f64 {
    if position == "top" {
        mp_y + OVERLAY_PILL_TOP_OFFSET
    } else {
        mp_y + mon_h_logical - OVERLAY_PILL_BOTTOM_OFFSET
    }
}

/// Reposition the overlay window so its content lands on whichever monitor the
/// mouse cursor is currently on. Called from `ptt_down` before the frontend
/// renders any visible content, and once at app startup. Best-effort —
/// silently no-ops if any of the platform queries fail.
///
/// Coordinate-space note (macOS / tao quirk):
///   - `cursor_position()` reports primary-scaled physical pixels — i.e. the
///     NSPoint location of the cursor multiplied by the *primary* monitor's
///     scale factor.
///   - `Monitor::position()` reports the screen origin in logical NSPoints
///     (despite the `PhysicalPosition` type label).
///   - `Monitor::size()` reports actual physical pixels, scaled by that
///     monitor's own scale factor.
///
/// To do a correct point-in-monitor test we have to normalize all three into
/// the same space. We pick logical NSPoints: divide cursor by primary scale,
/// take position as-is, divide size by own scale. Tauri's built-in
/// `monitor_from_point` does *not* handle this mix correctly on multi-scale
/// setups (retina laptop + 1x external) — it returns the wrong monitor —
/// which is why we do the math by hand.
#[cfg(target_os = "macos")]
pub fn reposition_overlay_to_cursor_monitor(app: &tauri::AppHandle) {
    use tauri::{LogicalPosition, Manager};
    let Some(overlay) = app.get_webview_window("overlay") else {
        return;
    };
    let cursor = match app.cursor_position() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("[overlay] cursor_position failed: {:?}", e);
            return;
        }
    };
    let monitors = match overlay.available_monitors() {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            tracing::warn!("[overlay] available_monitors empty — skip reposition");
            return;
        }
        Err(e) => {
            tracing::warn!("[overlay] available_monitors failed: {:?}", e);
            return;
        }
    };

    let primary_scale = overlay
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let cx = cursor.x / primary_scale;
    let cy = cursor.y / primary_scale;

    tracing::info!(
        "[overlay] cursor=({:.0},{:.0}) primary_scale={:.2} → logical=({:.0},{:.0})",
        cursor.x,
        cursor.y,
        primary_scale,
        cx,
        cy
    );
    for (i, m) in monitors.iter().enumerate() {
        let p = m.position();
        let s = m.size();
        let scale = m.scale_factor();
        let lw = s.width as f64 / scale;
        let lh = s.height as f64 / scale;
        tracing::info!(
            "[overlay] monitor[{}] pos=({},{}) size=({},{}) scale={:.2} logical_bounds=[{:.0},{:.0})x[{:.0},{:.0})",
            i,
            p.x,
            p.y,
            s.width,
            s.height,
            scale,
            p.x as f64,
            p.x as f64 + lw,
            p.y as f64,
            p.y as f64 + lh
        );
    }

    let matched_idx = monitors.iter().position(|m| {
        let p = m.position();
        let s = m.size();
        let logical_w = s.width as f64 / m.scale_factor();
        let logical_h = s.height as f64 / m.scale_factor();
        cx >= p.x as f64
            && cx < (p.x as f64 + logical_w)
            && cy >= p.y as f64
            && cy < (p.y as f64 + logical_h)
    });
    let monitor = match matched_idx {
        Some(i) => {
            tracing::info!("[overlay] matched monitor[{}]", i);
            monitors[i].clone()
        }
        None => {
            tracing::warn!(
                "[overlay] cursor logical=({:.0},{:.0}) matched no monitor — falling back to current/primary",
                cx,
                cy
            );
            let Some(m) = overlay
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| overlay.primary_monitor().ok().flatten())
            else {
                return;
            };
            m
        }
    };

    let mp = monitor.position();
    let ms = monitor.size();
    let scale = monitor.scale_factor();
    let mon_w_logical = ms.width as f64 / scale;
    let mon_h_logical = ms.height as f64 / scale;
    let position = settings::load().overlay_position;
    let x = mp.x as f64 + (mon_w_logical - OVERLAY_W_LOGICAL) / 2.0;
    let y = overlay_y_for_position(mp.y as f64, mon_h_logical, &position);

    let pre = overlay.outer_position().ok();
    tracing::info!(
        "[overlay] set_position logical=({:.0},{:.0}) position={} (target monitor pos=({},{}) scale={:.2}) pre_outer={:?}",
        x,
        y,
        position,
        mp.x,
        mp.y,
        scale,
        pre
    );

    // macOS NSPanel quirk: a transparent + decorations-off + alwaysOnTop
    // window is created as an NSPanel with elevated window level, and
    // `setFrameTopLeftPoint:` against that panel is silently dropped — we
    // confirmed this from `pre_outer == post_outer` over multiple presses.
    // Demoting the level (set_always_on_top(false)) takes the panel out of
    // the elevated-level state where the position-pinning behavior applies.
    // We restore alwaysOnTop immediately after the move.
    let _ = overlay.set_always_on_top(false);
    if let Err(e) = overlay.set_position(LogicalPosition::new(x, y)) {
        tracing::warn!("[overlay] set_position failed: {:?}", e);
    }
    let _ = overlay.set_always_on_top(true);

    if let Ok(post) = overlay.outer_position() {
        tracing::info!("[overlay] post_outer={:?}", post);
    }
}

/// Windows / Linux variant — straightforward physical-pixel bounds check
/// since both platforms report cursor, monitor position, and monitor size
/// all in physical pixels of the virtual desktop. Untested on real hardware
/// (TurboTalk currently ships macOS-only); revisit once the Win/Linux audio
/// + hotkey paths come online.
#[cfg(not(target_os = "macos"))]
pub fn reposition_overlay_to_cursor_monitor(app: &tauri::AppHandle) {
    use tauri::{LogicalPosition, Manager};
    let Some(overlay) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(cursor) = app.cursor_position() else {
        return;
    };
    let Ok(monitors) = overlay.available_monitors() else {
        return;
    };

    let monitor = monitors
        .iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            cursor.x >= p.x as f64
                && cursor.x < (p.x as f64 + s.width as f64)
                && cursor.y >= p.y as f64
                && cursor.y < (p.y as f64 + s.height as f64)
        })
        .cloned()
        .or_else(|| overlay.current_monitor().ok().flatten())
        .or_else(|| overlay.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let mp = monitor.position();
    let ms = monitor.size();
    let scale = monitor.scale_factor();
    let mon_w_logical = ms.width as f64 / scale;
    let mon_h_logical = ms.height as f64 / scale;
    let position = settings::load().overlay_position;
    let x = (mp.x as f64 + (ms.width as f64 - OVERLAY_W_LOGICAL * scale) / 2.0) / scale;
    let y = overlay_y_for_position(mp.y as f64 / scale, mon_h_logical, &position);

    let _ = overlay.set_position(LogicalPosition::new(x, y));
}

fn apply_overlay_visibility(app: &tauri::AppHandle, show: bool) {
    // Only toggle when the requested state differs from the current state.
    // Calling `show()` on an already-visible window on macOS reorders it to
    // the front and steals key status from whichever window the user was
    // interacting with — every settings change would defocus the main
    // window mid-click. (TASK-40)
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
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())
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
fn delete_model_file(path: String) -> Result<bool, String> {
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
    std::fs::remove_file(&canon).map_err(|e| format!("delete failed: {}", e))?;
    tracing::info!("[models] deleted {}", canon.display());
    Ok(true)
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

/// Open the TurboTalk data folder (`~/.config/librewin/turbotalk/`) in Finder.
/// macOS only — uses the system `open` command.
#[tauri::command]
#[specta::specta]
fn open_data_folder() -> Result<(), String> {
    let mut path = dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    path.push(".config/librewin/turbotalk");
    // Create the directory if it doesn't exist yet so Finder doesn't error.
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
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

    const MAX_MODEL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

    fn catalog_url(model_id: &str) -> Option<&'static str> {
        match model_id {
            "ggml-large-v3-turbo" => Some(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
            ),
            "ggml-large-v3-turbo-q5_0" => Some(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
            ),
            "ggml-large-v3" => Some(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
            ),
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

    let url = catalog_url(&model_id).ok_or_else(|| format!("unknown model id: {}", model_id))?;
    validate_catalog_url(url)?;

    // Build the destination path — create the directory if it doesn't exist yet.
    // (canonical_models_dir() requires the dir to already exist, so we build manually.)
    let mut dir = dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    dir.push(".config/librewin/turbotalk/models");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let canon_dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let filename = format!("{}.bin", model_id);
    if filename.contains(std::path::MAIN_SEPARATOR) || filename.contains("..") {
        return Err("invalid model id".into());
    }
    let dest = canon_dir.join(filename);

    let client = reqwest::Client::builder()
        .user_agent("TurboTalk/0.0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let total = resp.content_length();
    if total.is_some_and(|t| t > MAX_MODEL_BYTES) {
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
                if downloaded > MAX_MODEL_BYTES {
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
    drop(file);

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

/// Download a Moonshine ONNX model bundle from HuggingFace (TASK-58).
///
/// `variant` must be "tiny" or "base". Files are stored under:
///   `~/.config/librewin/turbotalk/models/moonshine/<variant>/`
///
/// Progress events match the Whisper `download-progress` pattern:
///   `{ "name": "moonshine-<variant>", "pct": 0..100 }`
///
/// The HuggingFace ONNX community repo for each variant:
///   tiny: https://huggingface.co/onnx-community/moonshine-tiny-ONNX
///   base: https://huggingface.co/onnx-community/moonshine-base-ONNX
///
/// The three required files per variant are:
///   encoder_model.onnx
///   decoder_model_merged.onnx
///   tokenizer.json
///
/// Each file is downloaded separately. Progress ticks are per-file (pct within
/// the overall set). The download key used in progress events is
/// `"moonshine-<variant>"` so the frontend can show a per-variant progress bar.
#[tauri::command]
#[specta::specta]
async fn download_moonshine_model(
    variant: String,
    app: tauri::AppHandle,
    cancel_set: tauri::State<'_, DownloadCancelSet>,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    // Validate variant name early so we fail fast.
    if !matches!(variant.as_str(), "tiny" | "base") {
        return Err(format!(
            "unknown Moonshine variant {:?} — expected \"tiny\" or \"base\"",
            variant
        ));
    }

    let repo = match variant.as_str() {
        "tiny" => "onnx-community/moonshine-tiny-ONNX",
        "base" => "onnx-community/moonshine-base-ONNX",
        _ => unreachable!(),
    };

    // FP32 ONNX files live under `onnx/` on HuggingFace. Int8 exports proved
    // unreliable in practice (empty transcripts on real mic audio despite
    // healthy peak levels). transcribe-rs's own Moonshine tests use FP32.
    let files: &[(&str, &str)] = &[
        ("onnx/encoder_model.onnx", "encoder_model.onnx"),
        ("onnx/decoder_model_merged.onnx", "decoder_model_merged.onnx"),
        ("tokenizer.json", "tokenizer.json"),
    ];

    // Build destination directory.
    let mut dest_dir =
        dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    dest_dir.push(".config/librewin/turbotalk/models/moonshine");
    dest_dir.push(&variant);
    tokio::fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| format!("Failed to create model directory: {}", e))?;
    let canon_dir = dest_dir
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize model directory: {}", e))?;

    // Verify destination is inside the expected moonshine models dir.
    {
        let mut expected_base =
            dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
        expected_base.push(".config/librewin/turbotalk/models/moonshine");
        if let Ok(canon_base) = expected_base.canonicalize() {
            if !canon_dir.starts_with(&canon_base) {
                return Err("Download destination is outside the allowed directory".to_string());
            }
        }
    }

    let event_name = format!("moonshine-{}", variant);
    let client = reqwest::Client::builder()
        .user_agent("TurboTalk/0.0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "name": &event_name, "pct": 0u8 }),
    );

    // Drop deprecated int8-only bundles so re-download picks up FP32 weights.
    for legacy in &[
        "encoder_model.int8.onnx",
        "decoder_model_merged.int8.onnx",
    ] {
        let p = canon_dir.join(legacy);
        if p.exists() {
            let _ = tokio::fs::remove_file(&p).await;
            tracing::info!("[moonshine-dl] removed legacy int8 file {}", legacy);
        }
    }

    for (file_idx, (remote_path, local_name)) in files.iter().enumerate() {
        let download_key = format!("{}-{}", event_name, local_name);

        // Check for cancellation before starting each file.
        if cancel_set.lock().remove(&event_name) {
            return Err("cancelled".into());
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            repo, remote_path
        );

        // Validate URL shape — must be huggingface.co.
        {
            let parsed = url::Url::parse(&url).map_err(|e| format!("invalid URL: {}", e))?;
            if parsed.scheme() != "https" || parsed.host_str() != Some("huggingface.co") {
                return Err("model downloads must use https://huggingface.co".to_string());
            }
        }

        // Reject path traversal in local filename.
        if local_name.contains(std::path::MAIN_SEPARATOR) || local_name.contains("..") {
            return Err(format!("invalid filename: {}", local_name));
        }
        let dest_path = canon_dir.join(local_name);
        if !dest_path.starts_with(&canon_dir) {
            return Err("download destination escaped model directory".to_string());
        }

        // Skip if already downloaded (idempotent re-runs).
        if dest_path.exists() {
            tracing::info!("[moonshine-dl] {} already present — skipping", local_name);
            // Still emit progress for the file.
            let base_pct = ((file_idx + 1) * 100 / files.len()).min(99) as u8;
            let _ = app.emit(
                "download-progress",
                serde_json::json!({ "name": &event_name, "pct": base_pct }),
            );
            continue;
        }

        tracing::info!("[moonshine-dl] downloading {} from {}", local_name, url);

        let mut resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed for {}: {}", local_name, e))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {} downloading {}", resp.status(), local_name));
        }

        let total = resp.content_length();
        let mut downloaded: u64 = 0;

        let temp_path = tempfile::Builder::new()
            .prefix("turbotalk-moonshine-")
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
                    if let Err(e) = file.write_all(&chunk).await {
                        drop(file);
                        let _ = tokio::fs::remove_file(&temp_file_path).await;
                        return Err(format!("Write failed for {}: {}", local_name, e));
                    }
                    // Progress: blend per-file progress with overall file index.
                    // Each file contributes an equal fraction of 0..99%.
                    let file_pct = total
                        .filter(|&t| t > 0)
                        .map(|t| (downloaded * 100 / t).min(100) as u8)
                        .unwrap_or(50u8);
                    let overall_pct = ((file_idx as u32 * 100
                        + file_pct as u32)
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
                    return Err(format!("Download interrupted for {}: {}", local_name, e));
                }
            }
        }

        file.flush().await.map_err(|e| e.to_string())?;
        drop(file);

        tokio::fs::rename(&temp_file_path, &dest_path)
            .await
            .map_err(|e| format!("Rename failed for {}: {}", local_name, e))?;

        // Verify the final path is still within canon_dir (rename target check).
        if let Ok(canon_dest) = dest_path.canonicalize() {
            if !canon_dest.starts_with(&canon_dir) {
                let _ = tokio::fs::remove_file(&dest_path).await;
                return Err("downloaded file escaped model directory".to_string());
            }
        }

        tracing::info!("[moonshine-dl] {} complete", local_name);
    }

    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "name": &event_name, "pct": 100u8 }),
    );

    tracing::info!(
        "[moonshine-dl] all files for variant {:?} downloaded to {}",
        variant,
        canon_dir.display()
    );

    apply_alt_backend_after_download(&app, settings::BackendFamily::Moonshine, &variant)?;

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
    if !matches!(variant.as_str(), "tdt-0.6b-v2") {
        return Err(format!(
            "unknown Parakeet variant {:?} — expected \"tdt-0.6b-v2\"",
            variant
        ));
    }

    // HuggingFace repo for the ONNX export of Parakeet TDT 0.6B v2.
    let repo = match variant.as_str() {
        "tdt-0.6b-v2" => "istupakov/parakeet-tdt-0.6b-v2-onnx",
        _ => unreachable!(),
    };

    // Int8 ONNX bundle — matches transcribe-rs ParakeetModel::load(..., Int8).
    let files: &[&str] = &[
        "encoder-model.int8.onnx",
        "decoder_joint-model.int8.onnx",
        "nemo128.onnx",
        "vocab.txt",
    ];

    // Build destination directory.
    let mut dest_dir =
        dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    dest_dir.push(".config/librewin/turbotalk/models/parakeet");
    dest_dir.push(&variant);
    tokio::fs::create_dir_all(&dest_dir)
        .await
        .map_err(|e| format!("Failed to create model directory: {}", e))?;
    let canon_dir = dest_dir
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize model directory: {}", e))?;

    // Verify destination is inside the expected parakeet models dir.
    {
        let mut expected_base =
            dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
        expected_base.push(".config/librewin/turbotalk/models/parakeet");
        if let Ok(canon_base) = expected_base.canonicalize() {
            if !canon_dir.starts_with(&canon_base) {
                return Err("Download destination is outside the allowed directory".to_string());
            }
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

    for (file_idx, filename) in files.iter().enumerate() {
        let download_key = format!("{}-{}", event_name, filename);

        // Check for cancellation before starting each file.
        if cancel_set.lock().remove(&event_name) {
            return Err("cancelled".into());
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            repo, filename
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
            tracing::info!("[parakeet-dl] {} already present — skipping", filename);
            // Still emit progress for the file.
            let base_pct = ((file_idx + 1) * 100 / files.len()).min(99) as u8;
            let _ = app.emit(
                "download-progress",
                serde_json::json!({ "name": &event_name, "pct": base_pct }),
            );
            continue;
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
                    let overall_pct = ((file_idx as u32 * 100
                        + file_pct as u32)
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
    pub label: String,
    pub description: String,
    pub size: String,
    pub download_url: String,
    pub path_hint: String,
    /// True when the required ONNX bundle files are present on disk.
    pub installed: bool,
}

fn moonshine_installed(variant: &str) -> bool {
    crate::transcribe_backends::moonshine::variant_dir(variant)
        .and_then(|d| crate::transcribe_backends::moonshine::validate_moonshine_model_dir(&d).ok())
        .is_some_and(|d| d.join("encoder_model.onnx").exists())
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
/// - Moonshine: returns "tiny" and "base" ONNX variants from the onnx-community repo.
/// - Parakeet: returns the single "tdt-0.6b-v2" variant.
///
/// `family` is a lowercase string: "whisper" | "moonshine" | "parakeet".
/// Unknown values are treated as "whisper".
#[tauri::command]
#[specta::specta]
fn list_models_for_family(family: String) -> Vec<ModelDescriptor> {
    match family.to_lowercase().as_str() {
        "moonshine" => vec![
            ModelDescriptor {
                id: "moonshine-tiny".to_string(),
                label: "Moonshine Tiny".to_string(),
                description: "English-only · low hallucination on silence · fastest".to_string(),
                size: "~110 MB".to_string(),
                download_url: "https://huggingface.co/onnx-community/moonshine-tiny-ONNX".to_string(),
                path_hint: ".config/librewin/turbotalk/models/moonshine/tiny/".to_string(),
                installed: moonshine_installed("tiny"),
            },
            ModelDescriptor {
                id: "moonshine-base".to_string(),
                label: "Moonshine Base".to_string(),
                description: "English-only · low hallucination on silence · more accurate than tiny".to_string(),
                size: "~250 MB".to_string(),
                download_url: "https://huggingface.co/onnx-community/moonshine-base-ONNX".to_string(),
                path_hint: ".config/librewin/turbotalk/models/moonshine/base/".to_string(),
                installed: moonshine_installed("base"),
            },
        ],
        "parakeet" => vec![
            ModelDescriptor {
                id: "parakeet-tdt-0.6b-v2".to_string(),
                label: "Parakeet TDT 0.6B v2".to_string(),
                description: "English-only · fastest · NVIDIA NeMo".to_string(),
                size: "~660 MB (int8)".to_string(),
                download_url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx".to_string(),
                path_hint: ".config/librewin/turbotalk/models/parakeet/tdt-0.6b-v2/".to_string(),
                installed: parakeet_installed("tdt-0.6b-v2"),
            },
        ],
        // "whisper" or anything else
        _ => vec![
            ModelDescriptor {
                id: "ggml-large-v3-turbo".to_string(),
                label: "Large v3 Turbo".to_string(),
                description: "Recommended · best accuracy · multilingual".to_string(),
                size: "1.6 GB".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin".to_string(),
                path_hint: String::new(),
                installed: false,
            },
            ModelDescriptor {
                id: "ggml-large-v3-turbo-q5_0".to_string(),
                label: "Large v3 Turbo (q5_0)".to_string(),
                description: "Low RAM · slightly reduced accuracy".to_string(),
                size: "574 MB".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin".to_string(),
                path_hint: String::new(),
                installed: false,
            },
            ModelDescriptor {
                id: "ggml-large-v3".to_string(),
                label: "Large v3".to_string(),
                description: "High accuracy · high RAM · slow".to_string(),
                size: "3.1 GB".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin".to_string(),
                path_hint: String::new(),
                installed: false,
            },
        ],
    }
}

#[tauri::command]
#[specta::specta]
fn list_audio_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devs) => devs.filter_map(|d| d.name().ok()).collect(),
        Err(_) => vec![],
    }
}

/// Cancel an in-flight recording (Recording or Transcribing state) from the
/// frontend. The hotkey thread calls `recorder.cancel()` directly and does not
/// go through this command — this command exists for future UI use (e.g. an
/// X button on the overlay). Registered in the invoke_handler and specta
/// builder so it appears in `bindings.ts` (TASK-23).
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
    if !matches!(
        state,
        recorder::State::Recording | recorder::State::Transcribing
    ) {
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
type HotkeyState = Arc<RwLock<settings::HotkeyConfig>>;
type RecorderState = Arc<recorder::Recorder>;
type TrayIconState = tauri::tray::TrayIcon;
type DownloadCancelSet = parking_lot::Mutex<std::collections::HashSet<String>>;

use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, RunEvent, WindowEvent,
};

/// Unzoomed advanced-mode width (WINDOW_W * 2 in the frontend).
/// Used as the default anchor when zoom is unknown (tray click before frontend loads).
const ADV_WIN_W_BASE: f64 = 880.0;

/// macOS menu bar clearance so the titlebar isn't tucked under it.
#[cfg(target_os = "macos")]
const CORNER_TOP_OFFSET: f64 = 24.0;

/// Compute the x that places the window's right edge at `monitor_right - adv_width`
/// so in advanced mode (window = adv_width) it snaps flush, and in normal mode it floats.
/// Returns (x, y). For y: macOS top-right, Windows bottom-right.
fn corner_xy(
    win: &tauri::WebviewWindow,
    monitor: &tauri::Monitor,
    adv_width: f64,
) -> (f64, f64) {
    let scale = monitor.scale_factor();
    let mp = monitor.position();
    let ms = monitor.size();
    let mon_x = mp.x as f64;
    let mon_y = mp.y as f64;
    let mon_w = ms.width as f64 / scale;
    let mon_h = ms.height as f64 / scale;

    let x = mon_x + mon_w - adv_width;

    #[cfg(target_os = "macos")]
    let y = {
        let _ = (mon_h, win);
        mon_y + CORNER_TOP_OFFSET
    };
    #[cfg(not(target_os = "macos"))]
    let y = {
        let win_h = win
            .outer_size()
            .map(|s| s.height as f64 / scale)
            .unwrap_or(280.0);
        mon_y + mon_h - win_h
    };

    (x, y)
}

/// Pin the main window to the corner on the monitor containing the cursor.
/// Uses `adv_width` as the anchor width — the right edge of the monitor minus
/// adv_width is the fixed x position, so the window snaps flush when it reaches
/// that width and floats otherwise.
fn position_main_window_inner(
    app: &tauri::AppHandle,
    win: &tauri::WebviewWindow,
    adv_width: f64,
) {
    use tauri::LogicalPosition;

    let primary_scale = win
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);

    let cursor = app.cursor_position().ok();
    let (cx, cy) = match cursor {
        #[cfg(target_os = "macos")]
        Some(c) => (c.x / primary_scale, c.y / primary_scale),
        #[cfg(not(target_os = "macos"))]
        Some(c) => (c.x, c.y),
        None => (f64::NAN, f64::NAN),
    };

    let monitors = win.available_monitors().ok().unwrap_or_default();
    let monitor = monitors
        .iter()
        .find(|m| {
            if cx.is_nan() {
                return false;
            }
            let p = m.position();
            let s = m.size();
            let scale = m.scale_factor();
            let lw = s.width as f64 / scale;
            let lh = s.height as f64 / scale;
            cx >= p.x as f64 && cx < p.x as f64 + lw && cy >= p.y as f64 && cy < p.y as f64 + lh
        })
        .cloned()
        .or_else(|| win.current_monitor().ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else { return };
    let (x, y) = corner_xy(win, &monitor, adv_width);
    let _ = win.set_position(LogicalPosition::new(x, y));
}

fn position_main_window(app: &tauri::AppHandle, win: &tauri::WebviewWindow) {
    position_main_window_inner(app, win, ADV_WIN_W_BASE);
}


/// Called from the frontend after every `setSize`.
/// `adv_width` is the *zoomed* advanced window width (e.g. 1100 at 125% zoom).
/// Fixes x so the right edge stays flush regardless of zoom level.
#[tauri::command]
#[specta::specta]
fn repin_main_window(app: tauri::AppHandle, adv_width: f64) {
    use tauri::{LogicalPosition, Manager};
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let (x, y) = corner_xy(&win, &monitor, adv_width);
    let _ = win.set_position(LogicalPosition::new(x, y));
}

/// Build the tauri-specta type-export descriptor. Lives in its own function
/// so the `#[test]` regenerator below can call it without standing up the
/// full Tauri runtime.
#[cfg(any(debug_assertions, test))]
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
        get_config,
        save_config,
        prewarm_model,
        scan_models_dir,
        list_models_for_family,
        get_launch_at_login,
        set_launch_at_login,
        reset_turbotalk,
        list_audio_devices,
        download_model,
        cancel_download,
        download_moonshine_model,
        download_parakeet_model,
        delete_model_file,
        load_history,
        save_history,
        copy_history_item,
        cancel_recording,
        start_recording,
        stop_recording,
        open_data_folder,
        ollama::check_ollama_model,
        ollama::open_url,
        ollama::ping_ollama,
        ollama::pull_ollama_model,
        diagnostics::run_diagnostics,
        permissions::check_readiness,
        permissions::request_microphone_permission,
        permissions::request_input_monitoring_permission,
        permissions::open_system_settings,
        permissions::restart_app,
        permissions::prompt_for_accessibility,
        permissions::reset_onboarding,
        permissions::clear_force_onboarding,
        permissions::reset_tcc_entry,
        repin_main_window,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let file_appender = tracing_appender::rolling::never("/tmp", "turbotalk-bench.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    let filter = tracing_subscriber::EnvFilter::new("turbotalk_lib=info,warn");
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

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
            scan_models_dir,
            list_models_for_family,
            get_launch_at_login,
            set_launch_at_login,
            reset_turbotalk,
            list_audio_devices,
            download_model,
            cancel_download,
            download_moonshine_model,
            download_parakeet_model,
            delete_model_file,
            load_history,
            save_history,
            copy_history_item,
            cancel_recording,
            start_recording,
            stop_recording,
            open_data_folder,
            ollama::check_ollama_model,
            ollama::open_url,
            ollama::ping_ollama,
            ollama::pull_ollama_model,
            diagnostics::run_diagnostics,
            permissions::check_readiness,
            permissions::request_microphone_permission,
            permissions::request_input_monitoring_permission,
            permissions::open_system_settings,
            permissions::restart_app,
            permissions::prompt_for_accessibility,
            permissions::reset_onboarding,
            permissions::clear_force_onboarding,
            permissions::reset_tcc_entry,
            repin_main_window,
        ])
        .setup(|app| {
            // ── macOS: hide from Dock and Cmd-Tab. The tray icon is the only
            // persistent affordance; the main window is opened on demand.
            // Runtime call (rather than Info.plist LSUIElement alone) so it
            // also applies to `tauri dev` runs, which don't go through the
            // bundle's plist.
            #[cfg(target_os = "macos")]
            {
                use tauri::ActivationPolicy;
                app.set_activation_policy(ActivationPolicy::Accessory);
            }

            // ── Tray icon ──────────────────────────────────────────────────
            let launch_enabled = {
                use tauri_plugin_autostart::ManagerExt;
                app.autolaunch().is_enabled().unwrap_or(false)
            };
            let launch_item = CheckMenuItem::with_id(
                app,
                "launch",
                "Launch at Login",
                true,
                launch_enabled,
                None::<&str>,
            )?;
            let show_item = MenuItem::with_id(app, "show", "Show TurboTalk", true, None::<&str>)?;
            let restart_item = MenuItem::with_id(app, "restart", "Restart", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(
                app,
                &[
                    &launch_item,
                    &sep1,
                    &show_item,
                    &sep2,
                    &restart_item,
                    &quit_item,
                ],
            )?;

            let launch_item_ref = launch_item.clone();
            let tray_icon: TrayIcon = TrayIconBuilder::new()
                .icon(tray::make_icon(tray::TrayState::Idle))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("TurboTalk")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        // If recording is active, cancel it instead of opening the window.
                        // trigger_cancel joins the feeder thread off the main thread
                        // internally. Arm ptt_up suppression first when the user might
                        // still be holding the record key (hold mode), so the eventual
                        // key release no-ops instead of poisoning CANCEL_PENDING.
                        let rec = app.state::<RecorderState>();
                        if matches!(rec.inner().state(), recorder::State::Recording) {
                            let hk = app.state::<HotkeyState>();
                            let hold_mode = hk.read().mode == "hold";
                            if hold_mode {
                                hotkey::arm_ptt_up_suppression();
                            }
                            hotkey::trigger_cancel(rec.inner(), tray, app);
                            return;
                        }
                        if let Some(win) = app.get_webview_window("main") {
                            position_main_window(app, &win);
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                        let _ = app.emit("open-history", ());
                    }
                })
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "launch" => {
                        use tauri_plugin_autostart::ManagerExt;
                        let mgr = app.autolaunch();
                        let new_state = !launch_item_ref.is_checked().unwrap_or(false);
                        if new_state {
                            let _ = mgr.enable();
                        } else {
                            let _ = mgr.disable();
                        }
                        let _ = launch_item_ref.set_checked(new_state);
                    }
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "restart" => app.restart(),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

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

            // ── Launch splash — shown on every app start ───────────────────
            // Window is pre-declared in tauri.conf.json (visible:false) so it
            // doesn't accumulate across hot-reloads; we position and show it here.
            if let Some(splash_win) = app.get_webview_window("splash") {
                tracing::info!("[splash] positioning and showing");
                    const SPLASH_W: f64 = 360.0;
                    const SPLASH_H: f64 = 220.0;
                    // Center on the cursor's monitor (same normalization as the
                    // overlay). NSPanel quirk: demote alwaysOnTop before move,
                    // restore after — identical to the overlay's workaround.
                    #[cfg(target_os = "macos")]
                    {
                        use tauri::LogicalPosition;
                        if let Ok(cursor) = app.handle().cursor_position() {
                            let primary_scale = splash_win
                                .primary_monitor()
                                .ok()
                                .flatten()
                                .map(|m| m.scale_factor())
                                .unwrap_or(1.0);
                            let cx = cursor.x / primary_scale;
                            let cy = cursor.y / primary_scale;
                            if let Ok(monitors) = splash_win.available_monitors() {
                                let monitor = monitors
                                    .iter()
                                    .find(|m| {
                                        let p = m.position();
                                        let s = m.size();
                                        let lw = s.width as f64 / m.scale_factor();
                                        let lh = s.height as f64 / m.scale_factor();
                                        cx >= p.x as f64
                                            && cx < p.x as f64 + lw
                                            && cy >= p.y as f64
                                            && cy < p.y as f64 + lh
                                    })
                                    .cloned()
                                    .or_else(|| splash_win.primary_monitor().ok().flatten());
                                if let Some(m) = monitor {
                                    let mp = m.position();
                                    let ms = m.size();
                                    let scale = m.scale_factor();
                                    let mw = ms.width as f64 / scale;
                                    let mh = ms.height as f64 / scale;
                                    let x = mp.x as f64 + (mw - SPLASH_W) / 2.0;
                                    let y = mp.y as f64 + (mh - SPLASH_H) / 2.0;
                                    tracing::info!(
                                        "[splash] centering at logical=({:.0},{:.0}) monitor pos=({},{})",
                                        x, y, mp.x, mp.y
                                    );
                                    let _ = splash_win.set_always_on_top(false);
                                    let _ = splash_win.set_position(LogicalPosition::new(x, y));
                                    let _ = splash_win.set_always_on_top(true);
                                }
                            }
                        }
                    }
                    let _ = splash_win.show();
            }

            // ── Main window — position below cursor at launch ─────────────
            if let Some(win) = app.get_webview_window("main") {
                position_main_window(app.handle(), &win);

                // First-run / regression gate: if Accessibility, Microphone,
                // or a model are missing, show the window so the onboarding
                // wizard can guide the user. Otherwise leave it hidden (tray-
                // resident agent behaviour).
                let readiness = crate::permissions::check_readiness();
                if !readiness.ready {
                    let _ = win.set_size(tauri::LogicalSize::new(440.0, 420.0));
                    let _ = win.center();
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }

            // ── Overlay — cursor-transparent so clicks always pass through ──
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_ignore_cursor_events(true);
                if !cfg.show_overlay {
                    let _ = overlay.hide();
                }
            }

            // ── Cursor dot — cursor-transparent, always starts hidden ──
            if let Some(dot) = app.get_webview_window("cursor-dot") {
                let _ = dot.set_ignore_cursor_events(true);
            }

            // Pin the overlay to the cursor's monitor at startup so the very
            // first press doesn't have to fight a stale primary-monitor
            // placement from `center: true` in tauri.conf.json.
            reposition_overlay_to_cursor_monitor(app.handle());

            // ── Hotkey ─────────────────────────────────────────────────────
            // Stream opens on first keypress; always re-queries the config device
            // so built-in mic / AirPods switches work without restarting.
            let recorder: RecorderState = Arc::new(recorder::Recorder::new()?);

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
                    level_rec.cancel_after_device_lost();
                    let _ = level_tray.set_icon(Some(tray::make_icon(tray::TrayState::Idle)));
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
                let cfg = settings::load();
                let is_busy = level_rec.state().is_busy();
                if cfg.cursor_dot_indicator && is_busy {
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
                            let lx = cursor.x / cached_primary_scale + DOT_OFFSET_X;
                            let ly = cursor.y / cached_primary_scale + DOT_OFFSET_Y;
                            // macOS NSPanel quirk: demote level before
                            // set_position (same workaround as the overlay).
                            let _ = dot.set_always_on_top(false);
                            let _ = dot.set_position(tauri::LogicalPosition::new(lx, ly));
                            let _ = dot.set_always_on_top(true);
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

            // TASK-23: manage recorder and tray_icon as app state so the
            // `cancel_recording` command can reach them from the invoke handler.
            app.manage(recorder.clone());
            app.manage(tray_icon.clone());
            app.manage(parking_lot::Mutex::new(
                std::collections::HashSet::<String>::new(),
            ) as DownloadCancelSet);

            // Pre-register in the Input Monitoring list so the app appears
            // there before the user opens System Settings during onboarding.
            //
            // On macOS 26+ the bare "request access" APIs (IOHIDRequestAccess,
            // CGRequestListenEventAccess) don't reliably register an ad-hoc
            // signed bundle with TCC. What does register it is actually
            // attempting to open an IOHIDManager — the same path Karabiner,
            // Logi Options+, etc. use to appear in the list.
            //
            // We create the manager, schedule it on the main run loop, and
            // call Open. Even when TCC blocks the open, the *attempt* is
            // what causes the bundle to be added to Privacy & Security →
            // Input Monitoring. The manager is intentionally leaked so the
            // registration persists for the process lifetime.
            #[cfg(target_os = "macos")]
            {
                use std::os::raw::c_void;
                type CFAllocatorRef = *const c_void;
                type CFDictionaryRef = *const c_void;
                type CFRunLoopRef = *const c_void;
                type CFStringRef = *const c_void;
                type IOHIDManagerRef = *mut c_void;
                const K_IO_HID_OPTIONS_TYPE_NONE: u32 = 0;

                #[link(name = "CoreFoundation", kind = "framework")]
                extern "C" {
                    static kCFAllocatorDefault: CFAllocatorRef;
                    static kCFRunLoopDefaultMode: CFStringRef;
                    fn CFRunLoopGetMain() -> CFRunLoopRef;
                }
                #[link(name = "CoreGraphics", kind = "framework")]
                extern "C" {
                    fn CGRequestListenEventAccess() -> bool;
                }
                #[link(name = "IOKit", kind = "framework")]
                extern "C" {
                    fn IOHIDManagerCreate(
                        allocator: CFAllocatorRef,
                        options: u32,
                    ) -> IOHIDManagerRef;
                    fn IOHIDManagerSetDeviceMatching(
                        manager: IOHIDManagerRef,
                        matching: CFDictionaryRef,
                    );
                    fn IOHIDManagerScheduleWithRunLoop(
                        manager: IOHIDManagerRef,
                        runloop: CFRunLoopRef,
                        mode: CFStringRef,
                    );
                    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: u32) -> i32;
                }

                unsafe {
                    // Belt-and-suspenders: the CG request also nudges TCC.
                    CGRequestListenEventAccess();

                    let manager =
                        IOHIDManagerCreate(kCFAllocatorDefault, K_IO_HID_OPTIONS_TYPE_NONE);
                    if !manager.is_null() {
                        // NULL matching dict = match all devices, including keyboards.
                        IOHIDManagerSetDeviceMatching(manager, std::ptr::null());
                        IOHIDManagerScheduleWithRunLoop(
                            manager,
                            CFRunLoopGetMain(),
                            kCFRunLoopDefaultMode,
                        );
                        // The Open attempt is what causes TCC to add the
                        // bundle to the Input Monitoring list. Return value
                        // is ignored — we expect kIOReturnNotPermitted until
                        // the user enables the toggle.
                        let _ = IOHIDManagerOpen(manager, K_IO_HID_OPTIONS_TYPE_NONE);
                        // Manager intentionally leaked: keeping it alive
                        // preserves the TCC registration for this process.
                    }
                }
            }

            hotkey::spawn(recorder, tray_icon, app.handle().clone(), hotkey_state);

            // Kill any whisper-server orphans left by a previous SIGKILL or
            // rapid dev-mode restart before prewarming a fresh one.
            transcribe::kill_orphans();
            // Eagerly warm whisper-server only after first-run setup is done.
            // Onboarding should be passive until the user clicks each step;
            // avoid normal warmed-up-agent work while permissions or a usable
            // model are still missing.
            if crate::permissions::check_readiness().ready {
                transcribe::prewarm(cfg.clone(), app.handle().clone());
            } else {
                tracing::info!("[transcribe] skipping startup prewarm until onboarding is complete");
            }

            Ok(())
        })
        // Close button hides to tray instead of quitting
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
            if let RunEvent::Exit = event {
                transcribe::abort_active();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::specta_builder;

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
