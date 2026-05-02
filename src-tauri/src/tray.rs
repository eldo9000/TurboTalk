use tauri::image::Image;

pub enum TrayState {
    Idle,
    Recording,
    Transcribing,
}

pub fn make_icon(state: TrayState) -> Image<'static> {
    let size = 44u32; // 44x44 → 22x22 logical at 2x retina
    let mut px = vec![0u8; (size * size * 4) as usize];

    match state {
        TrayState::Idle        => draw_tt(&mut px, size),
        TrayState::Recording   => fill_circle(&mut px, size, 248,  68,  68),
        TrayState::Transcribing=> fill_circle(&mut px, size, 251, 191,  36),
    }

    Image::new_owned(px, size, size)
}

// ── Pixel helpers ─────────────────────────────────────────────────────────────
// Pixel-buffer plotters take RGBA + position + size — naturally many args.

#[allow(clippy::too_many_arguments)]
fn set(px: &mut [u8], w: u32, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    if x < w && y < w {
        let i = ((y * w + x) * 4) as usize;
        px[i] = r; px[i+1] = g; px[i+2] = b; px[i+3] = a;
    }
}

#[allow(clippy::too_many_arguments)]
fn rect(px: &mut [u8], w: u32, x: u32, y: u32, rw: u32, rh: u32, r: u8, g: u8, b: u8) {
    for row in y..y+rh { for col in x..x+rw {
        set(px, w, col, row, r, g, b, 255);
    }}
}

// ── "TT" glyph ───────────────────────────────────────────────────────────────

fn draw_t(px: &mut [u8], w: u32, ox: u32, oy: u32) {
    // 10 wide × 16 tall, white
    let (lw, lh, bar_h, stem_w) = (10u32, 16u32, 3u32, 4u32);
    rect(px, w, ox, oy, lw, bar_h, 255, 255, 255);                          // top bar
    rect(px, w, ox + (lw - stem_w) / 2, oy + bar_h, stem_w, lh - bar_h, 255, 255, 255); // stem
}

fn draw_tt(px: &mut [u8], w: u32) {
    let (lw, lh, gap) = (10u32, 16u32, 4u32);
    let ox = (w - lw * 2 - gap) / 2;
    let oy = (w - lh) / 2;
    draw_t(px, w, ox, oy);
    draw_t(px, w, ox + lw + gap, oy);
}

// ── Filled circle (anti-aliased edge) ─────────────────────────────────────────

fn fill_circle(px: &mut [u8], size: u32, r: u8, g: u8, b: u8) {
    let (cx, cy) = (size as f32 / 2.0, size as f32 / 2.0);
    let radius = size as f32 / 2.0 - 3.5;
    for y in 0..size { for x in 0..size {
        let dist = ((x as f32 - cx + 0.5).powi(2) + (y as f32 - cy + 0.5).powi(2)).sqrt();
        if dist < radius + 1.0 {
            let a = ((radius + 1.0 - dist).clamp(0.0, 1.0) * 255.0) as u8;
            set(px, size, x, y, r, g, b, a);
        }
    }}
}
