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
pub mod diagnostic_log;
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
        return Err("No transcription model is installed for the selected engine.".to_string());
    }
    let cfg = settings::load();
    transcribe::prewarm(cfg, app);
    Ok(())
}

fn reset_warmup_cache_inner(recorder: &recorder::Recorder) -> Result<(), String> {
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

#[cfg(target_os = "macos")]
fn position_main_window_on_cursor_monitor(app: &tauri::AppHandle) {
    use tauri::{LogicalPosition, Manager};
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let cursor = match app.cursor_position() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("[main-window] cursor_position failed: {:?}", e);
            return;
        }
    };
    let monitors = match win.available_monitors() {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            tracing::warn!("[main-window] available_monitors empty — skip first placement");
            return;
        }
        Err(e) => {
            tracing::warn!("[main-window] available_monitors failed: {:?}", e);
            return;
        }
    };

    let primary_scale = win
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let cx = cursor.x / primary_scale;
    let cy = cursor.y / primary_scale;

    let monitor = monitors
        .iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            let logical_w = s.width as f64 / m.scale_factor();
            let logical_h = s.height as f64 / m.scale_factor();
            cx >= p.x as f64
                && cx < p.x as f64 + logical_w
                && cy >= p.y as f64
                && cy < p.y as f64 + logical_h
        })
        .cloned()
        .or_else(|| win.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        return;
    };
    let mp = monitor.position();
    let ms = monitor.size();
    let scale = monitor.scale_factor();
    let mon_w_logical = ms.width as f64 / scale;
    let mon_h_logical = ms.height as f64 / scale;
    let size = win
        .outer_size()
        .ok()
        .map(|s| (s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or((550.0, 560.0));
    let x = mp.x as f64 + (mon_w_logical - size.0) / 2.0;
    let y = mp.y as f64 + (mon_h_logical - size.1) / 2.0;

    tracing::info!(
        "[main-window] first tray placement logical=({:.0},{:.0}) monitor pos=({},{}) scale={:.2}",
        x,
        y,
        mp.x,
        mp.y,
        scale
    );
    let _ = win.set_position(LogicalPosition::new(x, y));
}

#[cfg(not(target_os = "macos"))]
fn position_main_window_on_cursor_monitor(app: &tauri::AppHandle) {
    use tauri::{LogicalPosition, Manager};
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(cursor) = app.cursor_position() else {
        return;
    };
    let Ok(monitors) = win.available_monitors() else {
        return;
    };
    let monitor = monitors
        .into_iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            let scale = m.scale_factor();
            let pl = p.x as f64 / scale;
            let pt = p.y as f64 / scale;
            let wl = s.width as f64 / scale;
            let hl = s.height as f64 / scale;
            cursor.x / scale >= pl
                && cursor.x / scale < pl + wl
                && cursor.y / scale >= pt
                && cursor.y / scale < pt + hl
        })
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let mp = monitor.position();
    let ms = monitor.size();
    let scale = monitor.scale_factor();
    let size = win
        .outer_size()
        .ok()
        .map(|s| (s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or((550.0, 560.0));
    // All math in logical coordinates. monitor.position() returns physical
    // pixels on non-macOS; / scale converts to logical.
    let x = mp.x as f64 / scale + (ms.width as f64 / scale - size.0) / 2.0;
    let y = mp.y as f64 / scale + (ms.height as f64 / scale - size.1) / 2.0;
    let _ = win.set_position(LogicalPosition::new(x, y));
}

fn show_main_window(app: &tauri::AppHandle, first_manual_show: &std::sync::atomic::AtomicBool) {
    use std::sync::atomic::Ordering;
    if let Some(win) = app.get_webview_window("main") {
        let visible = win.is_visible().unwrap_or(false);
        if !visible && !first_manual_show.swap(true, Ordering::AcqRel) {
            position_main_window_on_cursor_monitor(app);
        }
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Windows / Linux variant.
///
/// Coordinate conventions (Tauri 2):
/// - `monitor.position()` — physical pixels on all platforms.
/// - `monitor.size()` — physical pixels.
/// - `cursor_position()` — physical pixels.
/// - `set_position(LogicalPosition)` — logical points.
///
/// All math is done in logical (DPI-scaled) coordinates for consistency.
/// Physical → logical: divide by `scale_factor()`.
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
            let scale = m.scale_factor();
            let pl = p.x as f64 / scale; // physical → logical
            let pt = p.y as f64 / scale;
            let wl = s.width as f64 / scale;
            let hl = s.height as f64 / scale;
            cursor.x / scale >= pl
                && cursor.x / scale < pl + wl
                && cursor.y / scale >= pt
                && cursor.y / scale < pt + hl
        })
        .cloned()
        .or_else(|| overlay.current_monitor().ok().flatten())
        .or_else(|| overlay.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let mp = monitor.position();
    let ms = monitor.size();
    let scale = monitor.scale_factor();
    // Convert monitor origin + size to logical coordinates.
    let mon_x_logical = mp.x as f64 / scale;
    let mon_y_logical = mp.y as f64 / scale;
    let mon_w_logical = ms.width as f64 / scale;
    let mon_h_logical = ms.height as f64 / scale;
    let position = settings::load().overlay_position;
    // Center overlay horizontally on the cursor's monitor in logical space.
    let x = mon_x_logical + (mon_w_logical - OVERLAY_W_LOGICAL) / 2.0;
    let y = overlay_y_for_position(mon_y_logical, mon_h_logical, &position);

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

/// Delete an ONNX model bundle directory for Moonshine or Parakeet.
///
/// `family` is "moonshine" or "parakeet"; `variant` is the variant slug
/// (e.g. "tiny", "tdt-0.6b-v2"). Clears `backend_variant` when the removed
/// bundle was the active selection. Returns `Ok(true)` when a directory was
/// removed, `Ok(false)` when it was already gone.
#[tauri::command]
#[specta::specta]
fn delete_backend_model(family: String, variant: String) -> Result<bool, String> {
    use crate::settings::BackendFamily;

    let (dir, base) = match family.to_lowercase().as_str() {
        "moonshine" => (
            crate::transcribe_backends::moonshine::variant_dir(&variant),
            crate::transcribe_backends::moonshine::moonshine_models_dir(),
        ),
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

    std::fs::remove_dir_all(&canon).map_err(|e| format!("delete failed: {}", e))?;
    tracing::info!("[models] deleted backend bundle {}", canon.display());

    let mut cfg = settings::load();
    if cfg.backend_variant == variant {
        match cfg.backend {
            BackendFamily::Moonshine | BackendFamily::Parakeet => {
                cfg.backend_variant.clear();
                settings::save(&cfg).map_err(|e| format!("failed to save config: {}", e))?;
                settings::update_cache(&cfg);
            }
            _ => {}
        }
    }

    transcribe::invalidate_worker();
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
    // (canonical_models_dir() requires the dir to already exist, so we build manually.)
    let mut dir = dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    dir.push(".config/librewin/turbotalk/models");
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
    let files: &[RuntimeModelFileSpec] = match variant.as_str() {
        "tiny" => &[
            RuntimeModelFileSpec {
                remote_path: "onnx/encoder_model.onnx",
                local_name: "encoder_model.onnx",
                max_bytes: 40 * MIB,
                sha256: Some("cbbf580f703b2af2137e0f6d14cd87f31cc67bd858bfd8715403a9489982d1a5"),
            },
            RuntimeModelFileSpec {
                remote_path: "onnx/decoder_model_merged.onnx",
                local_name: "decoder_model_merged.onnx",
                max_bytes: 100 * MIB,
                sha256: Some("4131cef00b62942e9cdef691101f2cc7dbbcd828d71eee8c6c46c28fd051d6cb"),
            },
            RuntimeModelFileSpec {
                remote_path: "tokenizer.json",
                local_name: "tokenizer.json",
                max_bytes: 8 * MIB,
                sha256: Some("e1f9d42221e82686d50cfa0cebfa9e26d3770aa785db0937449409a20b5e7118"),
            },
        ],
        "base" => &[
            RuntimeModelFileSpec {
                remote_path: "onnx/encoder_model.onnx",
                local_name: "encoder_model.onnx",
                max_bytes: 100 * MIB,
                sha256: Some("153e128e7abd64a74ee47f2c3f585c3171c4d46cbb368b032827934c4e01e779"),
            },
            RuntimeModelFileSpec {
                remote_path: "onnx/decoder_model_merged.onnx",
                local_name: "decoder_model_merged.onnx",
                max_bytes: 200 * MIB,
                sha256: Some("58778763ca8438963190244d6b26572bdca2cedec56a4b91e828f3f2d69ef3c5"),
            },
            RuntimeModelFileSpec {
                remote_path: "tokenizer.json",
                local_name: "tokenizer.json",
                max_bytes: 8 * MIB,
                sha256: Some("e1f9d42221e82686d50cfa0cebfa9e26d3770aa785db0937449409a20b5e7118"),
            },
        ],
        _ => unreachable!(),
    };

    // Build destination directory.
    let mut dest_dir =
        dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    dest_dir.push(".config/librewin/turbotalk/models/moonshine");
    dest_dir.push(&variant);
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
    for legacy in &["encoder_model.int8.onnx", "decoder_model_merged.int8.onnx"] {
        let p = canon_dir.join(legacy);
        if p.exists() {
            let _ = tokio::fs::remove_file(&p).await;
            tracing::info!("[moonshine-dl] removed legacy int8 file {}", legacy);
        }
    }

    for (file_idx, spec) in files.iter().enumerate() {
        let remote_path = spec.remote_path;
        let local_name = spec.local_name;
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
            match verify_runtime_model_file(&dest_path, spec).await {
                Ok(()) => {
                    tracing::info!("[moonshine-dl] {} already present — skipping", local_name);
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
                    tracing::warn!(
                        "[moonshine-dl] removed invalid existing {}: {}",
                        local_name,
                        e
                    );
                }
            }
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
        if total.is_some_and(|t| t > spec.max_bytes) {
            return Err(format!("{} is larger than the allowed limit", local_name));
        }
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
                    if downloaded > spec.max_bytes {
                        drop(file);
                        let _ = tokio::fs::remove_file(&temp_file_path).await;
                        let _ = tokio::fs::remove_file(&dest_path).await;
                        return Err(format!("{} is larger than the allowed limit", local_name));
                    }
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
                    return Err(format!("Download interrupted for {}: {}", local_name, e));
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
                sha256: Some("20eefde5cae181c8c19481f6d6f8b2abdc44b3243c946bd1967f98281bbe5739"),
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
                sha256: Some("6c3109e5fb3769941c1ce19580f0008c3c6687a58bd99ac7c097c4cb98f37304"),
            },
        ],
        _ => unreachable!(),
    };

    // Build destination directory.
    let mut dest_dir =
        dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    dest_dir.push(".config/librewin/turbotalk/models/parakeet");
    dest_dir.push(&variant);
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

    for (file_idx, spec) in files.iter().enumerate() {
        let filename = spec.local_name;
        let download_key = format!("{}-{}", event_name, filename);

        // Check for cancellation before starting each file.
        if cancel_set.lock().remove(&event_name) {
            return Err("cancelled".into());
        }

        let url = format!("https://huggingface.co/{}/resolve/main/{}", repo, spec.remote_path);

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
                    tracing::warn!(
                        "[parakeet-dl] removed invalid existing {}: {}",
                        filename,
                        e
                    );
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
    /// Technical model name shown in the monospace pill (e.g. `moonshine-tiny`).
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
/// - Parakeet: returns "tdt-0.6b-v2" (English) and "tdt-0.6b-v3" (multilingual).
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
                tier: "Recommended".to_string(),
                label: "moonshine-tiny".to_string(),
                description: "english-only · fastest · low silence hallucination".to_string(),
                size: "110 MB".to_string(),
                download_url: "https://huggingface.co/onnx-community/moonshine-tiny-ONNX".to_string(),
                path_hint: ".config/librewin/turbotalk/models/moonshine/tiny/".to_string(),
                installed: moonshine_installed("tiny"),
                recommended: true,
            },
            ModelDescriptor {
                id: "moonshine-base".to_string(),
                tier: "Large".to_string(),
                label: "moonshine-base".to_string(),
                description: "english-only · more accurate".to_string(),
                size: "250 MB".to_string(),
                download_url: "https://huggingface.co/onnx-community/moonshine-base-ONNX".to_string(),
                path_hint: ".config/librewin/turbotalk/models/moonshine/base/".to_string(),
                installed: moonshine_installed("base"),
                recommended: false,
            },
        ],
        "parakeet" => vec![
            ModelDescriptor {
                id: "parakeet-tdt-0.6b-v2".to_string(),
                tier: "Recommended".to_string(),
                label: "parakeet-en-v2".to_string(),
                description: "english-only · fastest".to_string(),
                size: "660 MB".to_string(),
                download_url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx".to_string(),
                path_hint: ".config/librewin/turbotalk/models/parakeet/tdt-0.6b-v2/".to_string(),
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
                path_hint: ".config/librewin/turbotalk/models/parakeet/tdt-0.6b-v3/".to_string(),
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
        scan_models_dir,
        list_models_for_family,
        get_launch_at_login,
        set_launch_at_login,
        reset_turbotalk,
        list_audio_devices,
        detect_logitech_mouse,
        download_model,
        cancel_download,
        download_moonshine_model,
        download_parakeet_model,
        delete_model_file,
        delete_backend_model,
        load_history,
        save_history,
        copy_history_item,
        cancel_recording,
        start_recording,
        stop_recording,
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
        permissions::open_system_settings,
        permissions::restart_app,
        permissions::prompt_for_accessibility,
        permissions::reset_onboarding,
        permissions::clear_force_onboarding,
        permissions::reset_tcc_entry,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    librewin_common::logging::init(env!("CARGO_PKG_NAME"));
    let _ = diagnostic_log::ensure_log_dir();
    let log_dir = diagnostic_log::log_dir();

    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    // Full session log: one file per day (`turbotalk.YYYY-MM-DD.log`), keeping
    // ~2 weeks so the directory can't grow without bound on a daily driver.
    let main_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(diagnostic_log::MAIN_LOG_PREFIX)
        .filename_suffix(diagnostic_log::LOG_SUFFIX)
        .max_log_files(14)
        .build(&log_dir)
        .expect("init main log appender");
    let (main_nb, main_guard) = tracing_appender::non_blocking(main_appender);

    // Errors-only log: WARN+ERROR across all targets, retained longer so a
    // "what broke over the last week/month" query reads one short file.
    let error_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(diagnostic_log::ERROR_LOG_PREFIX)
        .filename_suffix(diagnostic_log::LOG_SUFFIX)
        .max_log_files(60)
        .build(&log_dir)
        .expect("init error log appender");
    let (error_nb, error_guard) = tracing_appender::non_blocking(error_appender);

    // Keep all non-blocking writers alive for the process lifetime so logs flush.
    static LOG_GUARDS: std::sync::OnceLock<Vec<tracing_appender::non_blocking::WorkerGuard>> =
        std::sync::OnceLock::new();
    #[cfg(debug_assertions)]
    let mut log_guards = vec![main_guard, error_guard];
    #[cfg(not(debug_assertions))]
    let log_guards = vec![main_guard, error_guard];

    #[cfg(debug_assertions)]
    {
        // Transcript debug log: a dedicated, local-only sink kept off the
        // tracing pipeline entirely (see diagnostic_log::TRANSCRIPT_LOG_PREFIX).
        // TEMPORARY — used to chase transcription quirks in dev builds; never
        // included in uploaded reports.
        let transcript_appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(diagnostic_log::TRANSCRIPT_LOG_PREFIX)
            .filename_suffix(diagnostic_log::LOG_SUFFIX)
            .max_log_files(14)
            .build(&log_dir)
            .expect("init transcript log appender");
        let (transcript_nb, transcript_guard) = tracing_appender::non_blocking(transcript_appender);
        diagnostic_log::init_transcript_writer(transcript_nb);
        log_guards.push(transcript_guard);
    }

    let _ = LOG_GUARDS.set(log_guards);

    use tracing_subscriber::{
        filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt, Layer,
    };
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

    tracing::info!(
        "[startup] TurboTalk v{} logging to {}",
        env!("CARGO_PKG_VERSION"),
        log_dir.display()
    );

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
            scan_models_dir,
            list_models_for_family,
            get_launch_at_login,
            set_launch_at_login,
            reset_turbotalk,
            list_audio_devices,
            detect_logitech_mouse,
            download_model,
            cancel_download,
            download_moonshine_model,
            download_parakeet_model,
            delete_model_file,
            delete_backend_model,
            load_history,
            save_history,
            copy_history_item,
            cancel_recording,
            start_recording,
            stop_recording,
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
            permissions::open_system_settings,
            permissions::restart_app,
            permissions::prompt_for_accessibility,
            permissions::reset_onboarding,
            permissions::clear_force_onboarding,
            permissions::reset_tcc_entry,
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
            let reset_warmup_item =
                MenuItem::with_id(app, "reset-warmup", "Clear Warmup Cache", true, None::<&str>)?;
            let restart_item = MenuItem::with_id(app, "restart", "Restart", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(
                app,
                &[
                    &launch_item,
                    &sep1,
                    &show_item,
                    &sep2,
                    &reset_warmup_item,
                    &sep3,
                    &restart_item,
                    &quit_item,
                ],
            )?;

            let launch_item_ref = launch_item.clone();
            let first_manual_main_show = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let tray_first_manual_main_show = first_manual_main_show.clone();
            let menu_first_manual_main_show = first_manual_main_show.clone();
            let tray_icon: TrayIcon = TrayIconBuilder::new()
                .icon(tray::make_icon(tray::TrayState::Idle))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("TurboTalk")
                .on_tray_icon_event(move |tray, event| {
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
                        show_main_window(app, &tray_first_manual_main_show);
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
                        show_main_window(app, &menu_first_manual_main_show);
                    }
                    "reset-warmup" => {
                        let recorder = app.state::<RecorderState>();
                        match reset_warmup_cache_inner(recorder.inner()) {
                            Ok(()) => {
                                tracing::info!("[transcribe] warmup cache cleared from tray menu");
                            }
                            Err(message) => {
                                emit_ui_error(app, "warmup-cache", message, true);
                            }
                        }
                    }
                    "restart" => app.restart(),
                    "quit" => {
                        // Release the warmed transcription backend before the
                        // process starts tearing down. `RunEvent::Exit` also
                        // handles this, but doing it here makes tray Quit
                        // deterministic and avoids carrying model-sized memory
                        // until the final event-loop tick.
                        transcribe::abort_active();
                        app.exit(0);
                    }
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

            // ── Main window — hidden until tray click unless onboarding ───
            if let Some(win) = app.get_webview_window("main") {
                use tauri::LogicalSize;
                // Spawn height is the floor; width stays at 550 (JS also sets min/max).
                let _ = win.set_min_size(Some(LogicalSize::new(550.0, 560.0)));
                // First-run / regression gate: if Accessibility, Microphone,
                // or a model are missing, show the window so the onboarding
                // wizard can guide the user. Otherwise leave it hidden (tray-
                // resident agent behaviour).
                let readiness = crate::permissions::check_readiness();
                if !readiness.ready {
                    let _ = win.set_size(tauri::LogicalSize::new(550.0, 420.0));
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
            // rapid dev-mode restart before this run can build a fresh one.
            transcribe::kill_orphans();
            // Do not eagerly warm the transcription model on ordinary launch.
            // large-v3-turbo and the ONNX backends can reserve hundreds of MB
            // to multiple GB, so a quit/relaunch cycle can push macOS memory
            // pressure yellow before the user has dictated anything. The
            // hotkey path already performs the same prewarm on first press and
            // shows the yellow arming tile while it loads.
            if crate::permissions::check_readiness().ready {
                tracing::info!("[transcribe] startup prewarm deferred until first dictation");
            } else {
                tracing::info!("[transcribe] startup prewarm skipped until onboarding is complete");
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
            match event {
                RunEvent::ExitRequested { .. } | RunEvent::Exit => {
                    transcribe::abort_active();
                }
                _ => {}
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
