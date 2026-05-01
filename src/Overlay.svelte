<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow, primaryMonitor } from '@tauri-apps/api/window';
  import { LogicalPosition } from '@tauri-apps/api/dpi';

  let mode = $state('idle'); // 'idle' | 'recording' | 'transcribing'
  let canvasEl = $state(null);

  const CANVAS_W   = 140; // CSS pixels
  const CANVAS_H   = 28;
  const HISTORY    = 52;  // columns in the histogram
  const WIN_W      = 260;
  const WIN_H      = 80;
  const BOTTOM_GAP = 110;

  let levels = Array(HISTORY).fill(0);

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

    const mon = await primaryMonitor();
    if (mon) {
      const sf = mon.scaleFactor;
      const mw = mon.size.width  / sf;
      const mh = mon.size.height / sf;
      await win.setPosition(new LogicalPosition(
        Math.round((mw - WIN_W) / 2),
        Math.round(mh - WIN_H - BOTTOM_GAP),
      ));
    }

    const uns = [];

    listen('ptt-down', () => {
      levels = Array(HISTORY).fill(0);
      mode = 'recording';
      draw();
    }).then(u => uns.push(u));

    listen('ptt-up', () => {
      mode = 'transcribing';
      draw();
    }).then(u => uns.push(u));

    listen('transcript', () => {
      setTimeout(() => { mode = 'idle'; }, 350);
    }).then(u => uns.push(u));

    listen('audio-level', (e) => {
      if (mode !== 'recording') return;
      levels = [...levels.slice(1), Math.min(1.0, e.payload)];
      draw();
    }).then(u => uns.push(u));

    return () => uns.forEach(u => u());
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

  .pill {
    opacity: 0;
    transform: scale(0.88) translateY(6px);
    transition: opacity 180ms ease-out, transform 180ms ease-out;
    pointer-events: none;
  }
  .pill.show {
    opacity: 1;
    transform: scale(1) translateY(0);
  }

  canvas { display: block; }
</style>

<div class="w-full h-full flex items-center justify-center">
  <div
    class="pill flex items-center gap-3.5 px-5 py-3.5 rounded-2xl"
    class:show={mode !== 'idle'}
    style="background: rgba(16,16,16,0.87); backdrop-filter: blur(18px) saturate(160%);
           box-shadow: 0 8px 32px rgba(0,0,0,0.5), inset 0 1px 0 rgba(255,255,255,0.06);"
  >
    <canvas
      bind:this={canvasEl}
      style="width: {CANVAS_W}px; height: {CANVAS_H}px;"
    ></canvas>

    <span
      class="text-[11px] font-semibold tracking-wide select-none"
      style="color: {mode === 'recording' ? '#f87171' : '#fbbf24'};"
    >
      {mode === 'recording' ? 'Recording' : 'Transcribing…'}
    </span>
  </div>
</div>
