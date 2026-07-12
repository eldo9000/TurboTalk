// Window / monitor geometry — placement, off-screen rescue, and macOS NSPanel
// position workaround.
//
// Coordinate conventions (Tauri 2 / macOS tao quirk):
//   - `cursor_position()` reports primary-scaled physical pixels — i.e. the
//     NSPoint location of the cursor multiplied by the *primary* monitor's
//     scale factor (on macOS). On other platforms it's physical pixels in the
//     virtual screen coordinate space.
//   - `Monitor::position()` reports the screen origin in logical NSPoints
//     (despite the `PhysicalPosition` type label) on macOS. On other platforms
//     it's physical pixels.
//   - `Monitor::size()` reports actual physical pixels, scaled by that
//     monitor's own scale factor on all platforms.
//
// To do a correct point-in-monitor test we have to normalise all three into
// the same space. We pick logical NSPoints: divide cursor by primary scale
// (macOS), divide size by own scale. Tauri's built-in `monitor_from_point`
// does *not* handle this mix correctly on multi-scale setups (retina laptop +
// 1x external) — hence the manual math.
//
// macOS NSPanel quirk: a transparent + decorations-off + alwaysOnTop window is
// created as an NSPanel with elevated window level, and `setFrameTopLeftPoint:`
// against that panel is silently dropped. Demoting the level (set_always_on_top
// false) before the move and restoring afterwards is the known workaround.

use tauri::{
    AppHandle, LogicalPosition, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow,
};

// ── Constants ──────────────────────────────────────────────────────────────

pub const MAIN_WINDOW_MIN_W: f64 = 420.0;
pub const MAIN_WINDOW_MIN_H: f64 = 420.0;
pub const MAIN_WINDOW_DEFAULT_W: f64 = 550.0;
pub const MAIN_WINDOW_DEFAULT_H: f64 = 560.0;

/// Default overlay window width (small / medium pill). The pill is centered
/// horizontally inside it.
const OVERLAY_W_DEFAULT: f64 = 460.0;
/// Large-mode window width — 3× the default so the waveform and transcript
/// bubble get a much wider canvas. The pill stays horizontally centred, so
/// its on-screen centre is unchanged; only the window's left/right edges move
/// outward (it's transparent + click-through, so the extra width is invisible
/// and harmless).
const OVERLAY_W_LARGE: f64 = 1380.0;
/// Default overlay window height (small / medium pill). The visible pill is a
/// ~260×80 rect centred inside it with a 100 px gutter on each side.
const OVERLAY_H_DEFAULT: f64 = 280.0;
/// Large-mode window height. The live transcript bubble grows into the extra
/// space — for bottom position that space is all *above* the pill, for top
/// position all *below* it. The pill's on-screen position is unchanged from
/// default because the window bottom (bottom pos) / top (top pos) stays pinned;
/// only the far edge moves.
const OVERLAY_H_LARGE: f64 = 460.0;
/// The overlay window's bottom edge sits this far above the screen bottom
/// (bottom position). Window top = screen_bottom - this - height, so the pill
/// — anchored `height - 100 - 80` from the window top (default) or to the
/// window bottom via CSS (large) — always lands `BOTTOM_GAP` (110) above the
/// screen bottom regardless of window height.
const OVERLAY_BOTTOM_MARGIN: f64 = 10.0;
/// Top-position equivalent: window top sits `TOP_GAP - GUTTER` below the
/// screen top so the pill lands `TOP_GAP` (110 px) below the screen top.
/// TOP_GAP matches BOTTOM_GAP so the visual breathing room is symmetric. On
/// macOS the menu bar at the very top occupies ~24 px of that gap. Independent
/// of window height — the window grows *downward* from this fixed top.
const OVERLAY_PILL_TOP_OFFSET: f64 = 10.0; // TOP_GAP 110 - GUTTER 100

const STATUS_W: f64 = 280.0;
const STATUS_H: f64 = 80.0;

// ── Overlay sizing helpers ─────────────────────────────────────────────────

fn overlay_height_for_size(size: &str) -> f64 {
    if size == "large" {
        OVERLAY_H_LARGE
    } else {
        OVERLAY_H_DEFAULT
    }
}

fn overlay_width_for_size(size: &str) -> f64 {
    if size == "large" {
        OVERLAY_W_LARGE
    } else {
        OVERLAY_W_DEFAULT
    }
}

/// Compute the window-top y for the overlay given the monitor origin, monitor
/// height (logical), the user's overlay_position preference, and the current
/// window height. Centralises the top vs bottom branch so the macOS and
/// Windows/Linux paths agree. Bottom: pin the window bottom; top: pin the
/// window top. Either way the pill stays put as height changes.
pub fn overlay_y_for_position(mp_y: f64, mon_h_logical: f64, position: &str, height: f64) -> f64 {
    if position == "top" {
        mp_y + OVERLAY_PILL_TOP_OFFSET
    } else {
        mp_y + mon_h_logical - OVERLAY_BOTTOM_MARGIN - height
    }
}

// ── Monitor-finding primitive ──────────────────────────────────────────────

/// Find the monitor containing the cursor, normalising coordinates into logical
/// NSPoints. Falls back through the provided chain. The macOS variant applies
/// the primary-scale cursor quirk; the non-macOS variant just divides cursor by
/// every monitor's own scale factor (cursor is in virtual-screen physical pixels).
#[cfg(target_os = "macos")]
fn find_cursor_monitor(
    cursor: PhysicalPosition<f64>,
    monitors: &[Monitor],
    primary_scale: f64,
    fallback_current: Option<Monitor>,
    fallback_primary: Option<Monitor>,
) -> Option<Monitor> {
    // On macOS, cursor_position() reports primary-scaled physical pixels, so
    // / primary_scale is our best-available normalisation to logical NSPoints.
    let cx = cursor.x / primary_scale;
    let cy = cursor.y / primary_scale;

    monitors
        .iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            let lw = s.width as f64 / m.scale_factor();
            let lh = s.height as f64 / m.scale_factor();
            cx >= p.x as f64 && cx < p.x as f64 + lw && cy >= p.y as f64 && cy < p.y as f64 + lh
        })
        .cloned()
        .or(fallback_current)
        .or(fallback_primary)
}

#[cfg(not(target_os = "macos"))]
fn find_cursor_monitor(
    cursor: PhysicalPosition<f64>,
    monitors: &[Monitor],
    _fallback_current: Option<Monitor>,
    fallback_primary: Option<Monitor>,
) -> Option<Monitor> {
    monitors
        .iter()
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
        .cloned()
        .or(fallback_primary)
}

/// Read the monitor's logical origin x, origin y, width, height.
fn monitor_logical_bounds(m: &Monitor) -> (f64, f64, f64, f64) {
    let p = m.position();
    let s = m.size();
    let scale = m.scale_factor();
    #[cfg(target_os = "macos")]
    {
        // On macOS, position() is already in logical NSPoints.
        (
            p.x as f64,
            p.y as f64,
            s.width as f64 / scale,
            s.height as f64 / scale,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        (
            p.x as f64 / scale,
            p.y as f64 / scale,
            s.width as f64 / scale,
            s.height as f64 / scale,
        )
    }
}

// ── macOS NSPanel position helper ──────────────────────────────────────────

/// macOS NSPanel quirk workaround: demote always-on-top, move, restore.
/// Safe to call on any platform — on non-macOS it's just set_position.
#[cfg(target_os = "macos")]
fn position_nspanel(win: &WebviewWindow, pos: LogicalPosition<f64>) {
    // Only demote/promote the window level when necessary. The overlay now uses
    // a native AppKit frame move, but status/cursor-dot still share this helper.
    let was_top = win.is_always_on_top().unwrap_or(true);
    if was_top {
        let _ = win.set_position(pos);
    } else {
        let _ = win.set_always_on_top(false);
        let _ = win.set_position(pos);
        let _ = win.set_always_on_top(true);
    }
}

// ── Overlay window ─────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn appkit_position_overlay_on_cursor_monitor(
    overlay: &WebviewWindow,
    position: &str,
    win_w: f64,
    win_h: f64,
) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSFloatingWindowLevel, NSScreen, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("[overlay] AppKit overlay positioning called off main thread");
        return false;
    };

    let Ok(ns_window_ptr) = overlay.ns_window() else {
        tracing::warn!("[overlay] ns_window unavailable — cannot position overlay");
        return false;
    };
    if ns_window_ptr.is_null() {
        tracing::warn!("[overlay] ns_window was null — cannot position overlay");
        return false;
    }

    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let mut target_frame = None;
    for i in 0..screens.count() {
        let screen = screens.objectAtIndex(i);
        let frame = screen.frame();
        let in_x = mouse.x >= frame.origin.x && mouse.x < frame.origin.x + frame.size.width;
        let in_y = mouse.y >= frame.origin.y && mouse.y < frame.origin.y + frame.size.height;
        if in_x && in_y {
            target_frame = Some(frame);
            break;
        }
    }

    let frame = target_frame
        .or_else(|| NSScreen::mainScreen(mtm).map(|screen| screen.frame()))
        .or_else(|| {
            if screens.count() > 0 {
                Some(screens.objectAtIndex(0).frame())
            } else {
                None
            }
        });
    let Some(screen_frame) = frame else {
        tracing::warn!("[overlay] no NSScreen available — cannot position overlay");
        return false;
    };

    let x = (screen_frame.origin.x + (screen_frame.size.width - win_w) / 2.0)
        .max(screen_frame.origin.x);
    let y = if position == "top" {
        screen_frame.origin.y + screen_frame.size.height - OVERLAY_PILL_TOP_OFFSET - win_h
    } else {
        screen_frame.origin.y + OVERLAY_BOTTOM_MARGIN
    };
    let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(win_w, win_h));

    unsafe {
        let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
        ns_window.setFrame_display(frame, true);
        ns_window.setLevel(NSFloatingWindowLevel);
    }

    tracing::debug!(
        "[overlay] AppKit positioned frame=({:.0},{:.0} {:.0}x{:.0}) mouse=({:.0},{:.0}) screen=({:.0},{:.0} {:.0}x{:.0})",
        x,
        y,
        win_w,
        win_h,
        mouse.x,
        mouse.y,
        screen_frame.origin.x,
        screen_frame.origin.y,
        screen_frame.size.width,
        screen_frame.size.height,
    );
    true
}

/// Reposition the overlay window so its content lands on whichever monitor the
/// mouse cursor is currently on. On macOS this uses AppKit's own screen/window
/// coordinate space (`NSEvent::mouseLocation`, `NSScreen::screens`, and
/// `NSWindow::setFrame`) instead of Tauri's mixed logical/physical monitor APIs.
#[cfg(target_os = "macos")]
pub fn reposition_overlay_to_cursor_monitor(app: &AppHandle) {
    use std::sync::mpsc;
    use std::time::Duration;

    let Some(overlay) = app.get_webview_window("overlay") else {
        return;
    };

    let position = crate::settings::overlay_position();
    let overlay_size = crate::settings::overlay_size();
    let win_h = overlay_height_for_size(&overlay_size);
    let win_w = overlay_width_for_size(&overlay_size);

    if objc2::MainThreadMarker::new().is_some() {
        let _ = appkit_position_overlay_on_cursor_monitor(&overlay, &position, win_w, win_h);
        return;
    }

    let overlay_for_main = overlay.clone();
    let (tx, rx) = mpsc::channel();
    if let Err(e) = overlay.run_on_main_thread(move || {
        let ok =
            appkit_position_overlay_on_cursor_monitor(&overlay_for_main, &position, win_w, win_h);
        let _ = tx.send(ok);
    }) {
        tracing::warn!("[overlay] failed to dispatch AppKit positioning: {:?}", e);
        return;
    }

    match rx.recv_timeout(Duration::from_millis(250)) {
        Ok(true) => {}
        Ok(false) => tracing::warn!("[overlay] AppKit positioning did not complete"),
        Err(_) => tracing::warn!("[overlay] timed out waiting for AppKit positioning"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn reposition_overlay_to_cursor_monitor(app: &AppHandle) {
    use tauri::LogicalSize;

    let Some(overlay) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(cursor) = app.cursor_position() else {
        return;
    };
    let Ok(monitors) = overlay.available_monitors() else {
        return;
    };

    let monitor = find_cursor_monitor(
        cursor,
        &monitors,
        overlay.current_monitor().ok().flatten(),
        overlay.primary_monitor().ok().flatten(),
    );
    let Some(monitor) = monitor else {
        return;
    };

    let (mx, my, mw, mh) = monitor_logical_bounds(&monitor);
    let position = crate::settings::overlay_position();
    let overlay_size = crate::settings::overlay_size();
    let win_h = overlay_height_for_size(&overlay_size);
    let win_w = overlay_width_for_size(&overlay_size);
    let x = (mx + (mw - win_w) / 2.0).max(mx);
    let y = overlay_y_for_position(my, mh, &position, win_h);

    let _ = overlay.set_size(LogicalSize::new(win_w, win_h));
    let _ = overlay.set_position(LogicalPosition::new(x, y));
}

// ── Status window ──────────────────────────────────────────────────────────

/// Position the status window on the cursor's monitor, centred horizontally
/// and placed near the top portion of the screen so it's visible but not
/// obscuring content. The status window is fixed-size (280×80 from conf).
#[cfg(target_os = "macos")]
fn appkit_position_status_on_cursor_monitor(status: &WebviewWindow) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Ok(ns_window_ptr) = status.ns_window() else {
        return false;
    };
    if ns_window_ptr.is_null() {
        return false;
    }

    let mouse = NSEvent::mouseLocation();
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    let mut target_frame = None;
    for i in 0..screens.count() {
        let screen = screens.objectAtIndex(i);
        let frame = screen.visibleFrame();
        if mouse.x >= frame.origin.x
            && mouse.x < frame.origin.x + frame.size.width
            && mouse.y >= frame.origin.y
            && mouse.y < frame.origin.y + frame.size.height
        {
            target_frame = Some(frame);
            break;
        }
    }
    let Some(frame) = target_frame.or_else(|| {
        objc2_app_kit::NSScreen::mainScreen(mtm).map(|screen| screen.visibleFrame())
    }) else {
        return false;
    };

    let x = frame.origin.x + ((frame.size.width - STATUS_W) / 2.0).max(0.0);
    // AppKit coordinates originate at the lower left. Keep the tile about 200
    // points below the usable top edge, clear of the menu bar/notch.
    let y = frame.origin.y + (frame.size.height - STATUS_H - 200.0).max(0.0);
    unsafe {
        let ns_window = &*(ns_window_ptr.cast::<NSWindow>());
        ns_window.setFrame_display(
            NSRect::new(NSPoint::new(x, y), NSSize::new(STATUS_W, STATUS_H)),
            true,
        );
    }
    true
}

pub fn reposition_status_to_cursor(status: &WebviewWindow) {
    use std::sync::mpsc;
    use std::time::Duration;

    if objc2::MainThreadMarker::new().is_some() {
        let _ = appkit_position_status_on_cursor_monitor(status);
        return;
    }

    let status_for_main = status.clone();
    let (tx, rx) = mpsc::channel();
    if let Err(e) = status.run_on_main_thread(move || {
        let _ = tx.send(appkit_position_status_on_cursor_monitor(&status_for_main));
    }) {
        tracing::warn!("[status] failed to dispatch cursor placement: {e:?}");
        return;
    }
    if !matches!(rx.recv_timeout(Duration::from_millis(250)), Ok(true)) {
        tracing::warn!("[status] cursor placement did not complete");
    }
}

/// Non-macOS stub: position is best-effort.
#[cfg(not(target_os = "macos"))]
pub fn reposition_status_to_cursor(_win: &WebviewWindow) {
    // No-op — the window is already centred via tauri.conf.json.
}

// ── Main window (first tray placement) ─────────────────────────────────────

#[cfg(target_os = "macos")]
pub fn position_main_window_on_cursor_monitor(app: &AppHandle) {
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

    let monitor = find_cursor_monitor(
        cursor,
        &monitors,
        primary_scale,
        None,
        win.primary_monitor().ok().flatten(),
    );
    let Some(monitor) = monitor else {
        return;
    };

    let (mx, my, mw, mh) = monitor_logical_bounds(&monitor);
    let scale = monitor.scale_factor();
    let size = win
        .outer_size()
        .ok()
        .map(|s| (s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or((MAIN_WINDOW_DEFAULT_W, MAIN_WINDOW_DEFAULT_H));
    let x = mx + (mw - size.0) / 2.0;
    let y = my + (mh - size.1) / 2.0;

    tracing::info!(
        "[main-window] first tray placement logical=({:.0},{:.0}) \
         monitor pos=({:.0},{:.0}) scale={:.2}",
        x,
        y,
        mx,
        my,
        scale,
    );
    let _ = win.set_position(LogicalPosition::new(x, y));
    ensure_main_webview_window_visible(&win);
}

#[cfg(not(target_os = "macos"))]
pub fn position_main_window_on_cursor_monitor(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(cursor) = app.cursor_position() else {
        return;
    };
    let Ok(monitors) = win.available_monitors() else {
        return;
    };

    let monitor = find_cursor_monitor(
        cursor,
        &monitors,
        win.current_monitor().ok().flatten(),
        win.primary_monitor().ok().flatten(),
    );
    let Some(monitor) = monitor else {
        return;
    };

    let (mx, my, mw, mh) = monitor_logical_bounds(&monitor);
    let scale = monitor.scale_factor();
    let size = win
        .outer_size()
        .ok()
        .map(|s| (s.width as f64 / scale, s.height as f64 / scale))
        .unwrap_or((MAIN_WINDOW_DEFAULT_W, MAIN_WINDOW_DEFAULT_H));
    let x = mx + (mw - size.0) / 2.0;
    let y = my + (mh - size.1) / 2.0;
    let _ = win.set_position(LogicalPosition::new(x, y));
    ensure_main_webview_window_visible(&win);
}

// ── Off-screen rescue (main window only) ───────────────────────────────────

/// Off-screen rescue for the main window. Nudges the window back onto a visible
/// monitor's work area if it has drifted off (e.g. a monitor was disconnected).
/// Position-only — never resizes. Called when the window is shown, not on every
/// move/resize event.
pub fn ensure_main_webview_window_visible(win: &WebviewWindow) {
    if win.label() != "main" {
        return;
    }

    let Ok(pos) = win.outer_position() else {
        return;
    };
    let Ok(size) = win.outer_size() else {
        return;
    };
    let Ok(monitors) = win.available_monitors() else {
        return;
    };
    if monitors.is_empty() {
        return;
    }

    let current = win.current_monitor().ok().flatten();
    let primary = win.primary_monitor().ok().flatten();
    let Some((x, y, work_x, work_y, work_w, work_h)) =
        main_window_visible_geometry(pos, size, &monitors, current, primary)
    else {
        return;
    };

    if x != pos.x || y != pos.y {
        tracing::info!(
            "[main-window] nudging on-screen from ({},{}) to ({},{}) \
             work_area=({},{} {}x{})",
            pos.x,
            pos.y,
            x,
            y,
            work_x,
            work_y,
            work_w,
            work_h,
        );
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

/// Position-only off-screen rescue: given the window's current position and
/// size, return where its top-left should go so it stays inside a monitor's
/// work area. Never changes the window size — sizing is owned entirely by the
/// frontend (which knows the UI zoom level). Returns (x, y, work_x, work_y,
/// work_w, work_h).
fn main_window_visible_geometry(
    pos: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    monitors: &[Monitor],
    current: Option<Monitor>,
    primary: Option<Monitor>,
) -> Option<(i32, i32, i32, i32, u32, u32)> {
    let monitor = monitors
        .iter()
        .max_by_key(|m| {
            let work = m.work_area();
            intersection_area(
                pos.x,
                pos.y,
                size.width as i32,
                size.height as i32,
                work.position.x,
                work.position.y,
                work.size.width as i32,
                work.size.height as i32,
            )
        })
        .cloned()
        .or(current)
        .or(primary)?;

    let work = monitor.work_area();
    let (x, y) = clamp_window_position_to_work_area(
        pos.x,
        pos.y,
        size.width as i32,
        size.height as i32,
        work.position.x,
        work.position.y,
        work.size.width as i32,
        work.size.height as i32,
    );

    Some((
        x,
        y,
        work.position.x,
        work.position.y,
        work.size.width,
        work.size.height,
    ))
}

pub(crate) fn intersection_area(
    ax: i32,
    ay: i32,
    aw: i32,
    ah: i32,
    bx: i32,
    by: i32,
    bw: i32,
    bh: i32,
) -> i64 {
    let left = ax.max(bx);
    let top = ay.max(by);
    let right = (ax + aw).min(bx + bw);
    let bottom = (ay + ah).min(by + bh);
    let w = (right - left).max(0) as i64;
    let h = (bottom - top).max(0) as i64;
    w * h
}

pub(crate) fn clamp_window_position_to_work_area(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    work_x: i32,
    work_y: i32,
    work_w: i32,
    work_h: i32,
) -> (i32, i32) {
    // Nudge right edge: window right > work right → slide left.
    let x = if x + w > work_x + work_w {
        (work_x + work_w - w).max(work_x)
    } else {
        x
    };
    // Nudge bottom edge: window bottom > work bottom → slide up.
    let y = if y + h > work_y + work_h {
        (work_y + work_h - h).max(work_y)
    } else {
        y
    };
    // Nudge left edge: window left < work left → slide right.
    let x = if x < work_x { work_x } else { x };
    // Nudge top edge: window top < work top → slide down.
    let y = if y < work_y { work_y } else { y };
    (x, y)
}

// ── Generic centre-on-cursor ───────────────────────────────────────────────

/// Centre a window on the monitor currently containing the cursor. Returns
/// false if positioning could not be completed.
#[cfg(target_os = "macos")]
pub fn center_window_on_cursor_monitor(win: &WebviewWindow, window_w: f64, window_h: f64) -> bool {
    let Ok(cursor) = win.app_handle().cursor_position() else {
        return false;
    };
    let primary_scale = win
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let Ok(monitors) = win.available_monitors() else {
        return false;
    };

    let monitor = find_cursor_monitor(
        cursor,
        &monitors,
        primary_scale,
        None,
        win.primary_monitor().ok().flatten(),
    );
    let Some(m) = monitor else {
        return false;
    };

    let (mx, my, mw, mh) = monitor_logical_bounds(&m);
    let x = mx + (mw - window_w) / 2.0;
    let y = my + (mh - window_h) / 2.0;

    tracing::info!(
        "[windowing] centering at logical=({:.0},{:.0}) monitor pos=({:.0},{:.0})",
        x,
        y,
        mx,
        my,
    );

    position_nspanel(win, LogicalPosition::new(x, y));
    true
}

#[cfg(not(target_os = "macos"))]
pub fn center_window_on_cursor_monitor(win: &WebviewWindow, window_w: f64, window_h: f64) -> bool {
    let Ok(cursor) = win.app_handle().cursor_position() else {
        return false;
    };
    let Ok(monitors) = win.available_monitors() else {
        return false;
    };

    let monitor = find_cursor_monitor(
        cursor,
        &monitors,
        win.current_monitor().ok().flatten(),
        win.primary_monitor().ok().flatten(),
    );
    let Some(m) = monitor else {
        return false;
    };

    let (mx, my, mw, mh) = monitor_logical_bounds(&m);
    let x = mx + (mw - window_w) / 2.0;
    let y = my + (mh - window_h) / 2.0;

    let _ = win.set_position(LogicalPosition::new(x, y));
    true
}

// ── Cursor-dot position ────────────────────────────────────────────────────

/// Position the cursor-dot indicator at the given offset from the cursor
/// hotspot. Handles the macOS NSPanel quirk. Uses the provided primary scale
/// factor (which should be refreshed when the dot becomes visible).
pub fn position_cursor_dot(
    dot: &WebviewWindow,
    cursor: PhysicalPosition<f64>,
    scale: f64,
    offset_x: f64,
    offset_y: f64,
) {
    let lx = cursor.x / scale + offset_x;
    let ly = cursor.y / scale + offset_y;
    #[cfg(target_os = "macos")]
    {
        let _ = dot.set_always_on_top(false);
        let _ = dot.set_position(LogicalPosition::new(lx, ly));
        let _ = dot.set_always_on_top(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dot.set_position(LogicalPosition::new(lx, ly));
    }
}
