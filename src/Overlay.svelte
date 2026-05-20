<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow, cursorPosition, primaryMonitor } from '@tauri-apps/api/window';
  import { commands } from './bindings.ts';

  // 'arming' = backend received the press but whisper-server hasn't finished
  // loading yet. Pill chrome shows with a yellow border but the internals
  // (canvas, label, word pills) are hidden — the user can see their press
  // registered without being misled into speaking before capture starts.
  let mode      = $state('idle'); // 'idle' | 'arming' | 'recording' | 'transcribing' | 'error'
  let canvasEl  = $state(null);
  let wordCount = $state(0);

  let transcribeTimer    = null;
  let transcribeTick     = $state(0); // incremented by timer to keep Svelte rendering during transcription

  // Each audio-level event = 50ms. Frames above threshold = speech time.
  // At 140 WPM: words ≈ speech_seconds * 140 / 60
  const SPEECH_THRESHOLD = 0.008;
  let speechFrames = 0;

  // Transcript size indicator: lorem-ipsum-shaped pills that accumulate above
  // the main pill while recording, giving a visual estimate of how much has
  // been said. Driven by VAD-derived wordCount increments — no real
  // transcription. Pills lay out inline (wrap to new rows) so the result reads
  // as a paragraph rather than a stack of full-width bars.
  let indicatorEnabled = $state(false);
  // Overlay placement on the screen: 'bottom' (default) or 'top'. Mirrors
  // the Rust-side `overlay_position` setting. Drives indicator-pillbox CSS:
  // bottom-anchored pill → pillbox grows upward (above pill);
  // top-anchored pill → pillbox grows downward (below pill) so it doesn't
  // disappear off the top of the screen.
  let overlayPosition = $state('bottom');
  let wordPills = $state([]); // [{id, w}] where w is pixel width
  let nextPillId = 0;

  // Empirical English-word length distribution, mapped to pixel widths.
  // ~30% short (1–3 letters), ~45% medium (4–6), ~20% long (7–10), ~5% very
  // long (11+). Width = letters × ~4 px, with mild jitter so identical-length
  // shapes don't visually repeat.
  function makePillWidth() {
    const r = Math.random();
    if (r < 0.30) return 8  + Math.random() * 7;   //  8–15 px (short)
    if (r < 0.75) return 16 + Math.random() * 14;  // 16–30 px (medium)
    if (r < 0.95) return 31 + Math.random() * 14;  // 31–45 px (long)
    return 46 + Math.random() * 12;                // 46–58 px (very long)
  }

  const CANVAS_W   = 140; // CSS pixels
  const CANVAS_H   = 28;
  const HISTORY    = 52;  // columns in the histogram

  let levels = Array(HISTORY).fill(0);
  let cursorInZone = $state(false);
  let isPeeking = $derived(mode === 'recording' && cursorInZone);

  function draw() {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    const W   = canvasEl.width;
    const H   = canvasEl.height;
    const dpr = W / CANVAS_W;

    ctx.clearRect(0, 0, W, H);

    const barW    = W / HISTORY;
    const gap     = Math.max(1, Math.round(dpr));
    ctx.fillStyle = mode === 'recording'
      ? 'rgba(255,255,255,0.95)'
      : 'rgba(255,255,255,0.35)';

    for (let i = 0; i < HISTORY; i++) {
      const norm = Math.sqrt(Math.min(1, levels[i])); // sqrt for perceptual scaling
      const barH = Math.max(2 * dpr, norm * H);
      ctx.fillRect(
        Math.round(i * barW),
        Math.round((H - barH) / 2),
        Math.max(1, Math.round(barW) - gap),
        Math.round(barH),
      );
    }
  }

  onMount(async () => {
    const win = getCurrentWindow();
    const dpr = window.devicePixelRatio || 1;
    canvasEl.width  = CANVAS_W * dpr;
    canvasEl.height = CANVAS_H * dpr;
    draw();

    // Initial config read for the transcript-size indicator + overlay position.
    // Failure here is non-fatal — defaults stay until a config-update event arrives.
    try {
      const cfg = await commands.getConfig();
      indicatorEnabled = cfg.transcript_size_indicator ?? false;
      overlayPosition = cfg.overlay_position ?? 'bottom';
    } catch (_) { /* keep defaults */ }

    // Window placement is owned entirely by the Rust side — see
    // `reposition_overlay_to_cursor_monitor` in src-tauri/src/lib.rs.
    // The frontend only mirrors the resulting frame to compute the
    // cursor-peek-through hoverZone.
    //
    // Coordinate-space note (macOS multi-monitor):
    //   cursorPosition() → NSPoint × primarySf  (primary-monitor-scaled physical px)
    //   outerPosition()  → NSPoint × windowSf   (window-monitor-scaled physical px)
    // On a single Retina display these are the same; on Retina + 1× external
    // they differ. Normalise both to NSPoints before comparing.
    const primary = await primaryMonitor();
    const primarySf = primary?.scaleFactor ?? dpr;

    let hoverZone = { x: 0, y: 0, w: 0, h: 0 }; // NSPoints

    async function refreshHoverZone() {
      try {
        const pos  = await win.outerPosition();
        const size = await win.outerSize();
        const wsf  = window.devicePixelRatio || 1;
        hoverZone = { x: pos.x / wsf, y: pos.y / wsf, w: size.width / wsf, h: size.height / wsf };
      } catch (_) { /* leave previous zone in place */ }
    }
    await refreshHoverZone();

    const cursorTimer = setInterval(async () => {
      try {
        const cur = await cursorPosition();
        const nsX = cur.x / primarySf;
        const nsY = cur.y / primarySf;
        cursorInZone = nsX >= hoverZone.x && nsX <= hoverZone.x + hoverZone.w
                    && nsY >= hoverZone.y && nsY <= hoverZone.y + hoverZone.h;
      } catch (_) {}
    }, 100);

    const uns = [];

    // Backend emits ptt-armed BEFORE ptt-down only when whisper-server is
    // still loading (cold start). On the warm path ptt-down fires directly
    // and the arming state is skipped entirely — no yellow flash.
    listen('ptt-armed', () => {
      levels = Array(HISTORY).fill(0);
      speechFrames = 0;
      wordCount = 0;
      wordPills = [];
      mode = 'arming';
      draw();
      setTimeout(() => { refreshHoverZone(); }, 200);
    }).then(u => uns.push(u));

    listen('ptt-arm-failed', () => {
      clearInterval(transcribeTimer);
      mode = 'error';
      draw();
      setTimeout(() => { mode = 'idle'; }, 2500);
    }).then(u => uns.push(u));

    listen('ptt-down', () => {
      levels = Array(HISTORY).fill(0);
      speechFrames = 0;
      wordCount = 0;
      wordPills = [];
      mode = 'recording';
      draw();
      // Backend has just repositioned the window onto the cursor's monitor
      // (see reposition_overlay_to_cursor_monitor). Pick up the new frame
      // so the cursor-peek-through hoverZone matches reality. Slight delay
      // so the post-set_position frame has settled.
      setTimeout(() => { refreshHoverZone(); }, 200);
    }).then(u => uns.push(u));

    listen('ptt-up', () => {
      mode = 'transcribing';
      clearInterval(transcribeTimer);
      transcribeTimer = setInterval(() => { transcribeTick++; }, 50);
      draw();
    }).then(u => uns.push(u));

    listen('transcript', () => {
      clearInterval(transcribeTimer);
      setTimeout(() => { mode = 'idle'; }, 350);
    }).then(u => uns.push(u));

    listen('transcript-error', () => {
      clearInterval(transcribeTimer);
      mode = 'error';
      draw();
      setTimeout(() => { mode = 'idle'; }, 2500);
    }).then(u => uns.push(u));

    listen('recording-discarded', () => {
      clearInterval(transcribeTimer);
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('recording-cancelled', () => {
      clearInterval(transcribeTimer);
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('recording-too-short', () => {
      clearInterval(transcribeTimer);
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('device-lost', () => {
      clearInterval(transcribeTimer);
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('paste-error', () => {
      clearInterval(transcribeTimer);
      mode = 'error';
      draw();
      setTimeout(() => { mode = 'idle'; }, 2500);
    }).then(u => uns.push(u));

    listen('audio-level', (e) => {
      if (mode !== 'recording') return;
      const v = Math.min(1.0, e.payload);
      levels = [...levels.slice(1), v];
      if (v > SPEECH_THRESHOLD) {
        speechFrames++;
        const newCount = Math.round(speechFrames * 0.05 * 140 / 60);
        if (newCount > wordCount) {
          if (indicatorEnabled) {
            // One pill per estimated word increment. Width drawn from an
            // English-word-length distribution so rows wrap as paragraph-like
            // text rather than identical full-width bars.
            const delta = newCount - wordCount;
            const newPills = [];
            for (let i = 0; i < delta; i++) {
              newPills.push({ id: nextPillId++, w: makePillWidth() });
            }
            wordPills = [...wordPills, ...newPills];
          }
          wordCount = newCount;
        }
      }
      draw();
    }).then(u => uns.push(u));

    listen('config-update', (e) => {
      const next = e.payload?.transcript_size_indicator ?? false;
      indicatorEnabled = next;
      if (!next) wordPills = [];
      overlayPosition = e.payload?.overlay_position ?? 'bottom';
      // Rust repositions the window on save; refresh hoverZone so peek-
      // through detection picks up the new frame.
      setTimeout(() => { refreshHoverZone(); }, 200);
    }).then(u => uns.push(u));

    return () => {
      clearInterval(cursorTimer);
      uns.forEach(u => u());
    };
  });
</script>

<style>
  :global(html, body, #app) {
    background: transparent !important;
    border: none !important;
    outline: none !important;
    box-shadow: none !important;
    width: 100%; height: 100%;
    margin: 0; padding: 0; overflow: hidden;
  }

  @keyframes pulse-red {
    0%, 100% { border-color: rgba(239, 68, 68, 0.15); }
    50%       { border-color: rgba(239, 68, 68, 1); }
  }
  @keyframes pulse-yellow {
    0%, 100% { border-color: rgba(251, 191, 36, 0.15); }
    50%       { border-color: rgba(251, 191, 36, 1); }
  }

  .pill {
    opacity: 0;
    transform: scale(0.88) translateY(6px);
    transition:
      opacity 180ms ease-out,
      transform 180ms ease-out,
      background-color 180ms ease-out,
      backdrop-filter 180ms ease-out,
      -webkit-backdrop-filter 180ms ease-out;
    will-change: opacity, transform, background-color, backdrop-filter;
    pointer-events: none;
    border: 1px solid transparent;
    position: relative;
    overflow: hidden;
  }
  .pill.show {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
  .pill.recording {
    animation: pulse-red 10s ease-in-out infinite;
  }
  .pill.transcribing {
    animation: pulse-yellow 10s ease-in-out infinite;
  }
  /* Arming = press registered, whisper-server still loading. Steady yellow
     border (no pulse) so it visually distinguishes from the transcribing
     pulse. Internals (canvas, label, word pills) are hidden via the
     .pill-inner.hidden rule below — only the chrome is visible. */
  .pill.arming {
    border-color: rgba(251, 191, 36, 0.85);
  }
  .pill-inner {
    display: contents;
    transition: opacity 120ms ease-out;
  }
  .pill-inner.hidden > * {
    visibility: hidden;
  }

  canvas { display: block; }

  /* Transcript size indicator: lorem-ipsum-style word shapes laid out inline,
     wrapping to new rows. Absolutely positioned so growth is upward without
     shifting the main pill. align-content: flex-end keeps the newest row
     pinned to the bottom; older rows clip out the top via overflow hidden. */
  .indicator-pillbox {
    position: absolute;
    bottom: 100%;
    left: 50%;
    transform: translateX(-50%);
    margin-bottom: 8px;
    width: 240px;
    max-height: 180px;
    overflow: hidden;
    background: rgba(16, 16, 16, 0.78);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 12px;
    padding: 5px 10px;
    min-height: 14px;
    display: flex;
    flex-direction: row;
    flex-wrap: wrap;
    align-content: flex-end;
    justify-content: flex-start;
    column-gap: 3px;
    row-gap: 4px;
    opacity: 0;
    transition: opacity 180ms ease-out;
    pointer-events: none;
    backdrop-filter: blur(12px) saturate(140%);
    -webkit-backdrop-filter: blur(12px) saturate(140%);
    /* Soft fade at the top so clipped rows don't hard-cut. */
    -webkit-mask-image: linear-gradient(to top, black 75%, transparent 100%);
    mask-image: linear-gradient(to top, black 75%, transparent 100%);
  }
  /* Top-anchored overlay: flip the pillbox below the pill and grow downward,
     so accumulating pills don't run off the top of the screen. The mask
     direction inverts so the BOTTOM (overflow edge) fades out. */
  .indicator-pillbox.top {
    bottom: auto;
    top: 100%;
    margin-bottom: 0;
    margin-top: 8px;
    align-content: flex-start;
    -webkit-mask-image: linear-gradient(to bottom, black 75%, transparent 100%);
    mask-image: linear-gradient(to bottom, black 75%, transparent 100%);
  }
  .indicator-pillbox.show { opacity: 1; }
  .word-line {
    height: 5px;
    background: rgba(255, 255, 255, 0.5);
    border-radius: 2.5px;
    flex-shrink: 0;
  }

</style>

<div class="w-full h-full flex items-center justify-center">
  <div class="relative">
  {#if indicatorEnabled}
    <div
      class="indicator-pillbox"
      class:show={mode === 'recording'}
      class:top={overlayPosition === 'top'}
      style:opacity={mode === 'recording' ? (isPeeking ? 0.24 : 1) : 0}
    >
      {#each wordPills as p (p.id)}
        <div class="word-line" style="width: {p.w}px;"></div>
      {/each}
    </div>
  {/if}
  <div
    class="pill flex items-center gap-3.5 px-5 py-3.5 rounded-2xl"
    class:show={mode !== 'idle'}
    class:arming={mode === 'arming'}
    class:recording={mode === 'recording'}
    class:transcribing={mode === 'transcribing'}
    class:peek={isPeeking}
    style:background={isPeeking ? 'rgba(16,16,16,0.12)' : 'rgba(16,16,16,0.87)'}
    style:backdrop-filter={isPeeking ? 'blur(1px) saturate(100%)' : 'blur(18px) saturate(160%)'}
    style:-webkit-backdrop-filter={isPeeking ? 'blur(1px) saturate(100%)' : 'blur(18px) saturate(160%)'}
    style:opacity={mode === 'idle' ? 0 : isPeeking ? 0.24 : 1}
  >
    <div class="pill-inner" class:hidden={mode === 'arming'}>
    <canvas
      bind:this={canvasEl}
      style="width: {CANVAS_W}px; height: {CANVAS_H}px;"
    ></canvas>

    <div class="flex flex-col items-start gap-[1px]">
      <span
        class="text-[11px] font-semibold tracking-wide select-none leading-tight"
        style="color: {mode === 'recording' ? '#f87171' : mode === 'error' ? '#f87171' : '#fbbf24'};"
      >
        {mode === 'recording' ? 'Recording' : mode === 'error' ? 'Error' : 'Transcribing…'}
      </span>
      {#if wordCount > 0}
        <span class="text-[9px] tabular-nums select-none leading-tight"
              style="color: rgba(255,255,255,0.4);">
          ~{wordCount}w
        </span>
      {/if}
    </div>
    </div>

  </div>
  </div>
</div>
