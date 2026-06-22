use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::image::Image;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

#[derive(Clone, Copy, PartialEq)]
pub enum TrayState {
    Idle,
    Recording,
    Transcribing,
}

pub fn make_icon(state: TrayState) -> Image<'static> {
    #[cfg(target_os = "windows")]
    let size = 32u32;
    #[cfg(not(target_os = "windows"))]
    let size = 22u32;

    let mut px = vec![0u8; (size * size * 4) as usize];

    #[cfg(target_os = "windows")]
    fill_opaque(&mut px, size, 17, 17, 17);

    // Idle: draw "TT" glyph with template mode so macOS renders it in the
    // system menu bar text color. Recording/Transcribing: non-template mode
    // so actual RGB colors render (red circle, amber circle).
    match state {
        TrayState::Idle => draw_tt(&mut px, size),
        TrayState::Recording => {
            fill_circle(&mut px, size, 248, 68, 68);
            draw_x(&mut px, size);
        }
        TrayState::Transcribing => fill_circle(&mut px, size, 251, 191, 36),
    }

    Image::new_owned(px, size, size)
}

// ── Pixel helpers ─────────────────────────────────────────────────────────────
// Pixel-buffer plotters take RGBA + position + size — naturally many args.

#[cfg(target_os = "windows")]
fn fill_opaque(px: &mut [u8], w: u32, r: u8, g: u8, b: u8) {
    for y in 0..w {
        for x in 0..w {
            set(px, w, x, y, r, g, b, 255);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn set(px: &mut [u8], w: u32, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    if x < w && y < w {
        let i = ((y * w + x) * 4) as usize;
        px[i] = r;
        px[i + 1] = g;
        px[i + 2] = b;
        px[i + 3] = a;
    }
}

#[allow(clippy::too_many_arguments)]
fn rect(px: &mut [u8], w: u32, x: u32, y: u32, rw: u32, rh: u32, r: u8, g: u8, b: u8) {
    for row in y..y + rh {
        for col in x..x + rw {
            set(px, w, col, row, r, g, b, 255);
        }
    }
}

// ── "TT" glyph ───────────────────────────────────────────────────────────────
// Draws "TT" into the pixel buffer for the idle state. The icon uses template
// mode so macOS renders it in the system menu bar text color. Recording and
// transcribing states use non-template icons so actual RGB colors show.

fn draw_t(px: &mut [u8], w: u32, ox: u32, oy: u32, lw: u32, lh: u32, bar_h: u32, stem_w: u32) {
    rect(px, w, ox, oy, lw, bar_h, 255, 255, 255);
    rect(
        px,
        w,
        ox + (lw - stem_w) / 2,
        oy + bar_h,
        stem_w,
        lh - bar_h,
        255,
        255,
        255,
    );
}

fn draw_tt(px: &mut [u8], w: u32) {
    // Scale glyph dimensions proportionally from the 44px reference design.
    let s = w as f32 / 44.0;
    let lw = (10.0 * s).round().max(2.0) as u32;
    let lh = (16.0 * s).round().max(3.0) as u32;
    let bar_h = (3.0 * s).round().max(1.0) as u32;
    let stem_w = (4.0 * s).round().max(1.0) as u32;
    let gap = (4.0 * s).round().max(1.0) as u32;
    let total = lw * 2 + gap;
    if total >= w {
        return;
    }
    let ox = (w - total) / 2;
    let oy = (w - lh) / 2;
    draw_t(px, w, ox, oy, lw, lh, bar_h, stem_w);
    draw_t(px, w, ox + lw + gap, oy, lw, lh, bar_h, stem_w);
}

// ── Filled circle (anti-aliased edge) ─────────────────────────────────────────

fn fill_circle(px: &mut [u8], size: u32, r: u8, g: u8, b: u8) {
    let (cx, cy) = (size as f32 / 2.0, size as f32 / 2.0);
    let radius = size as f32 / 2.0 - 3.5;
    for y in 0..size {
        for x in 0..size {
            let dist = ((x as f32 - cx + 0.5).powi(2) + (y as f32 - cy + 0.5).powi(2)).sqrt();
            if dist < radius + 1.0 {
                let a = ((radius + 1.0 - dist).clamp(0.0, 1.0) * 255.0) as u8;
                set(px, size, x, y, r, g, b, a);
            }
        }
    }
}

// ── White X glyph (drawn over an existing filled circle) ──────────────────────

fn draw_x(px: &mut [u8], size: u32) {
    let cx = size as f32 / 2.0;
    let arm = 5.0f32; // Euclidean half-arm length in pixels
    let half_w = 1.0f32; // line half-width in pixels
    let s = std::f32::consts::SQRT_2;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cx;

            // Decompose into the two diagonal axes.
            // a = perpendicular distance to arm1 = along-axis distance for arm2, and vice-versa.
            let a = (dy - dx).abs() / s; // perp to arm1 / along arm2
            let b = (dx + dy).abs() / s; // along arm1 / perp to arm2

            let on_arm1 = a < half_w && b <= arm;
            let on_arm2 = b < half_w && a <= arm;

            if on_arm1 || on_arm2 {
                let i = ((y * size + x) * 4) as usize;
                // Only paint inside the circle (where the background alpha is set).
                if px[i + 3] > 128 {
                    px[i] = 255;
                    px[i + 1] = 255;
                    px[i + 2] = 255;
                }
            }
        }
    }
}

// ── Tray builder ──────────────────────────────────────────────────────────

/// Build the tray icon and its context menu.  Populates `LAUNCH_MENU_ITEM` for
/// live sync from the Settings toggle.  The returned `TrayIcon` is also managed
/// as app state so commands (`cancel_recording`, tray click handlers) can reach
/// it.
pub fn build(app: &tauri::App) -> tauri::Result<TrayIcon> {
    let launch_enabled = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().unwrap_or(false)
    };
    // Build the "Launch at Login" menu item as a plain MenuItem with a
    // visual indicator in the label (✓ when enabled). Tauri 2's
    // CheckMenuItem has a macOS tray bug where clicks don't fire menu
    // events and set_checked doesn't update the native menu — so we
    // manage the state ourselves via set_text.
    let launch_label = if launch_enabled {
        "\u{2713} Launch at Login"
    } else {
        "  Launch at Login"
    };
    let launch_item = MenuItem::with_id(app, "launch", launch_label, true, None::<&str>)?;
    // Store in global so set_launch_at_login (Settings toggle) can
    // sync the tray menu item text.
    {
        let slot = crate::LAUNCH_MENU_ITEM.get_or_init(|| std::sync::Mutex::new(None));
        *slot.lock().unwrap() = Some(launch_item.clone());
    }
    let show_item = MenuItem::with_id(app, "show", "Show TurboTalk", true, None::<&str>)?;
    let reset_warmup_item = MenuItem::with_id(
        app,
        "reset-warmup",
        "Clear Warmup Cache",
        true,
        None::<&str>,
    )?;
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
    let first_manual_main_show = Arc::new(AtomicBool::new(false));
    let tray_first_manual_main_show = first_manual_main_show.clone();
    let menu_first_manual_main_show = first_manual_main_show.clone();
    let tray_icon: TrayIcon = TrayIconBuilder::new()
        .icon(make_icon(TrayState::Idle))
        .icon_as_template(true)
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
                let rec = app.state::<crate::RecorderState>();
                if matches!(rec.inner().state(), crate::recorder::State::Recording) {
                    let hk = app.state::<crate::HotkeyState>();
                    let hold_mode = hk.read().mode == "hold";
                    if hold_mode {
                        crate::hotkey::arm_ptt_up_suppression();
                    }
                    crate::hotkey::trigger_cancel(rec.inner(), tray, &app);
                    return;
                }
                crate::show_main_window(&app, &tray_first_manual_main_show);
                let _ = app.emit("open-history", ());
            }
        })
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "launch" => {
                use tauri_plugin_autostart::ManagerExt;
                let mgr = app.autolaunch();
                let new_state = !mgr.is_enabled().unwrap_or(false);
                if new_state {
                    let _ = mgr.enable();
                } else {
                    let _ = mgr.disable();
                }
                let label = if new_state {
                    "\u{2713} Launch at Login"
                } else {
                    "  Launch at Login"
                };
                let _ = launch_item_ref.set_text(label);
            }
            "show" => {
                crate::show_main_window(app, &menu_first_manual_main_show);
            }
            "reset-warmup" => {
                let recorder = app.state::<crate::RecorderState>();
                match crate::reset_warmup_cache_inner(recorder.inner()) {
                    Ok(()) => {
                        tracing::info!("[transcribe] warmup cache cleared from tray menu");
                    }
                    Err(message) => {
                        crate::emit_ui_error(app, "warmup-cache", message, true);
                    }
                }
            }
            "restart" => app.restart(),
            "quit" => {
                crate::transcribe::abort_active();
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    tracing::info!("[tray] tray icon created");

    Ok(tray_icon)
}

/// Set the tray icon state. Idle renders the "TT" glyph in template mode
/// (system menu bar text color). Recording/Transcribing use non-template
/// mode so the actual RGB colors render (red circle, amber circle).
pub fn set_tray_icon(tray: &TrayIcon, state: TrayState) {
    let _ = tray.set_icon(Some(make_icon(state)));
    #[cfg(target_os = "macos")]
    {
        let _ = tray.set_icon_as_template(matches!(state, TrayState::Idle));
    }
}

