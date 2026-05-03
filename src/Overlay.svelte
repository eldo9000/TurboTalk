<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow, primaryMonitor, monitorFromPoint, cursorPosition } from '@tauri-apps/api/window';
  import { LogicalPosition } from '@tauri-apps/api/dpi';

  let mode      = $state('idle'); // 'idle' | 'recording' | 'transcribing'
  let canvasEl  = $state(null);
  let wordCount = $state(0);

  let transcribeProgress = $state(0); // 0 → 1 as countdown elapses
  let transcribeTimer    = null;

  // Each audio-level event = 50ms. Frames above threshold = speech time.
  // At 140 WPM: words ≈ speech_seconds * 140 / 60
  const SPEECH_THRESHOLD = 0.008;
  let speechFrames = 0;

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

    // Pick the monitor where the user's cursor lives — that's the screen
    // the focused app is most likely on. Falls back to the primary monitor
    // if the cursor query fails (early-boot, multi-user fast-switch, etc.).
    // Position math is done in logical units relative to the chosen
    // monitor's origin so the overlay lands on the correct screen on
    // multi-display setups.
    async function positionOverlay() {
      let mon = null;
      try {
        const cur = await cursorPosition();
        mon = await monitorFromPoint(cur.x, cur.y);
      } catch (_) { /* fall through to primary */ }
      if (!mon) mon = await primaryMonitor();
      if (!mon) return;
      const sf = mon.scaleFactor;
      const mw = mon.size.width  / sf;
      const mh = mon.size.height / sf;
      const ox = mon.position.x / sf;
      const oy = mon.position.y / sf;
      await win.setPosition(new LogicalPosition(
        Math.round(ox + (mw - WIN_W) / 2),
        Math.round(oy + mh - WIN_H - BOTTOM_GAP),
      ));
    }
    await positionOverlay();

    const uns = [];

    listen('ptt-down', () => {
      levels = Array(HISTORY).fill(0);
      speechFrames = 0;
      wordCount = 0;
      mode = 'recording';
      draw();
      // Move to the monitor the user is currently on — they may have
      // dragged their focused app to a different screen since last time.
      // Fire-and-forget so we don't delay the recording-start signal.
      positionOverlay().catch(() => {});
    }).then(u => uns.push(u));

    listen('ptt-up', () => {
      mode = 'transcribing';
      transcribeProgress = 0;
      // Estimate: audio duration ≈ wordCount * 60/140 s; whisper ~0.4× real-time
      const audioSec = wordCount > 0 ? (wordCount * 60 / 140) : 3;
      const estMs    = Math.max(2000, audioSec * 480);
      const start    = Date.now();
      clearInterval(transcribeTimer);
      transcribeTimer = setInterval(() => {
        transcribeProgress = Math.min(1, (Date.now() - start) / estMs);
        if (transcribeProgress >= 1) clearInterval(transcribeTimer);
      }, 50);
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
        wordCount = Math.round(speechFrames * 0.05 * 140 / 60);
      }
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
    transition: opacity 180ms ease-out, transform 180ms ease-out;
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

  .progress-bar {
    position: absolute;
    bottom: 0;
    left: 0;
    height: 2px;
    background: rgba(255, 255, 255, 0.9);
    transition: width 50ms linear;
  }

  canvas { display: block; }
</style>

<div class="w-full h-full flex items-center justify-center">
  <div
    class="pill flex items-center gap-3.5 px-5 py-3.5 rounded-2xl"
    class:show={mode !== 'idle'}
    class:recording={mode === 'recording'}
    class:transcribing={mode === 'transcribing'}
    style="background: rgba(16,16,16,0.87); backdrop-filter: blur(18px) saturate(160%);"
  >
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

    {#if mode === 'transcribing'}
      <div
        class="progress-bar"
        style="width: {(1 - transcribeProgress) * 100}%;"
      ></div>
    {/if}
  </div>
</div>
