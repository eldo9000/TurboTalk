#!/usr/bin/env python3
"""Generate TurboTalk app icons. Run from src-tauri/icons/:  python3 gen_icons.py"""

import math, os, struct, zlib

# ── PNG writer (stdlib only) ──────────────────────────────────────────────────

def _chunk(tag, data):
    crc = zlib.crc32(tag + data) & 0xFFFFFFFF
    return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', crc)

def write_png(path, size, pixels):
    """Write RGBA pixel list (size*size*4 bytes) as PNG."""
    raw = b''
    for y in range(size):
        raw += b'\x00'
        base = y * size * 4
        raw += bytes(pixels[base:base + size * 4])
    png = (b'\x89PNG\r\n\x1a\n'
           + _chunk(b'IHDR', struct.pack('>II', size, size) + bytes([8, 6, 0, 0, 0]))
           + _chunk(b'IDAT', zlib.compress(raw, 9))
           + _chunk(b'IEND', b''))
    with open(path, 'wb') as f:
        f.write(png)

# ── Pixel helpers ─────────────────────────────────────────────────────────────

def _set(px, w, x, y, r, g, b, a):
    if 0 <= x < w and 0 <= y < w:
        i = (y * w + x) * 4
        px[i] = r; px[i+1] = g; px[i+2] = b; px[i+3] = a

def _rect(px, w, x, y, rw, rh, r, g, b, a=255):
    for row in range(y, y + rh):
        for col in range(x, x + rw):
            _set(px, w, col, row, r, g, b, a)

# ── Background: dark rounded square ──────────────────────────────────────────

def _bg(size, radius):
    """Dark #111 background with anti-aliased rounded corners."""
    px = bytearray(size * size * 4)
    R, G, B = 17, 17, 17
    for y in range(size):
        for x in range(size):
            cx = min(x, size - 1 - x)
            cy = min(y, size - 1 - y)
            if cx < radius and cy < radius:
                d = math.sqrt((radius - cx - 0.5)**2 + (radius - cy - 0.5)**2)
                a = int(max(0.0, min(1.0, radius - d)) * 255)
            else:
                a = 255
            i = (y * size + x) * 4
            px[i] = R; px[i+1] = G; px[i+2] = B; px[i+3] = a
    return px

# ── TT glyph ─────────────────────────────────────────────────────────────────

def _draw_t(px, w, ox, oy, lw, lh, bar_h, stem_w):
    _rect(px, w, ox, oy, lw, bar_h, 255, 255, 255)
    stem_x = ox + (lw - stem_w) // 2
    _rect(px, w, stem_x, oy + bar_h, stem_w, lh - bar_h, 255, 255, 255)

def make_icon(size):
    radius = round(size * 0.225)           # macOS ~22% corner radius
    px = _bg(size, radius)

    # Scale from the 44-px tray reference: lw=10, lh=16, bar_h=3, stem_w=4, gap=4
    s  = size / 44.0
    lw     = max(1, round(10 * s))
    lh     = max(1, round(16 * s))
    bar_h  = max(1, round( 3 * s))
    stem_w = max(1, round( 4 * s))
    gap    = max(1, round( 4 * s))

    total_w = lw * 2 + gap
    ox = (size - total_w) // 2
    oy = (size - lh) // 2

    _draw_t(px, size, ox,            oy, lw, lh, bar_h, stem_w)
    _draw_t(px, size, ox + lw + gap, oy, lw, lh, bar_h, stem_w)
    return px

# ── Emit all sizes ────────────────────────────────────────────────────────────

SIZES = {
    '32x32.png':      32,
    '128x128.png':   128,
    '128x128@2x.png':256,
    'icon.png':      512,
}

script_dir = os.path.dirname(os.path.abspath(__file__))
for name, size in SIZES.items():
    path = os.path.join(script_dir, name)
    write_png(path, size, make_icon(size))
    print(f'  {name}  ({size}×{size})')
