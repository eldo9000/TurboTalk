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

  let recordingStart  = null;
  let elapsedSecs     = $state(0);
  let elapsedTimer    = null;

  const WARN_WORDS  = 100; // ~43s speech — faster pulse
  const ALERT_WORDS = 300; // ~2min speech — aggressive glow pulse

  function fmtElapsed(s) {
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  }

  function startElapsed() {
    recordingStart = Date.now();
    elapsedSecs = 0;
    clearInterval(elapsedTimer);
    elapsedTimer = setInterval(() => {
      elapsedSecs = Math.floor((Date.now() - recordingStart) / 1000);
    }, 1000);
  }

  function stopElapsed() {
    clearInterval(elapsedTimer);
    elapsedTimer = null;
  }

  function resetElapsed() {
    stopElapsed();
    elapsedSecs = 0;
    recordingStart = null;
  }

  // Each audio-level event = 50ms. Frames above threshold = speech time.
  // At 140 WPM: words ≈ speech_seconds * 140 / 60
  const SPEECH_THRESHOLD = 0.008;
  let speechFrames = 0;

  // Length counter shown to the right of the pill during recording.
  // VAD-derived wordCount drives the estimate — no real transcription.
  let indicatorEnabled = $state(false);
  let indicatorUnit    = $state('lines'); // 'lines' | 'paragraphs'
  let overlayPosition  = $state('bottom');

  const WORDS_PER_LINE = 11;
  const WORDS_PER_PARA = 80;

  let indicatorCount = $derived(
    indicatorUnit === 'paragraphs'
      ? Math.round(wordCount / WORDS_PER_PARA)
      : Math.round(wordCount / WORDS_PER_LINE)
  );

  let indicatorLabel = $derived(
    indicatorCount === 0
      ? '—'
      : indicatorUnit === 'paragraphs'
        ? (indicatorCount === 1 ? '1 paragraph' : `${indicatorCount} paragraphs`)
        : (indicatorCount === 1 ? '1 line' : `${indicatorCount} lines`)
  );

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
      indicatorUnit    = cfg.length_indicator_unit ?? 'lines';
      overlayPosition  = cfg.overlay_position ?? 'bottom';
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
      startElapsed();
      mode = 'arming';
      draw();
      setTimeout(() => { refreshHoverZone(); }, 200);
    }).then(u => uns.push(u));

    listen('ptt-arm-failed', () => {
      clearInterval(transcribeTimer);
      resetElapsed();
      mode = 'error';
      draw();
      setTimeout(() => { mode = 'idle'; }, 2500);
    }).then(u => uns.push(u));

    listen('ptt-down', () => {
      levels = Array(HISTORY).fill(0);
      speechFrames = 0;
      wordCount = 0;
      startElapsed();
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
      stopElapsed(); // keep elapsedSecs visible as recording duration during transcription
      clearInterval(transcribeTimer);
      transcribeTimer = setInterval(() => { transcribeTick++; }, 50);
      draw();
    }).then(u => uns.push(u));

    listen('transcript', () => {
      clearInterval(transcribeTimer);
      setTimeout(() => { resetElapsed(); mode = 'idle'; }, 350);
    }).then(u => uns.push(u));

    listen('transcript-error', () => {
      clearInterval(transcribeTimer);
      resetElapsed();
      mode = 'error';
      draw();
      setTimeout(() => { mode = 'idle'; }, 2500);
    }).then(u => uns.push(u));

    listen('recording-discarded', () => {
      clearInterval(transcribeTimer);
      resetElapsed();
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('recording-cancelled', () => {
      clearInterval(transcribeTimer);
      resetElapsed();
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('recording-too-short', () => {
      clearInterval(transcribeTimer);
      resetElapsed();
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('device-lost', () => {
      clearInterval(transcribeTimer);
      resetElapsed();
      mode = 'idle';
      draw();
    }).then(u => uns.push(u));

    listen('paste-error', () => {
      clearInterval(transcribeTimer);
      resetElapsed();
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
          wordCount = newCount;
        }
      }
      draw();
    }).then(u => uns.push(u));

    listen('config-update', (e) => {
      indicatorEnabled = e.payload?.transcript_size_indicator ?? false;
      indicatorUnit    = e.payload?.length_indicator_unit ?? 'lines';
      overlayPosition  = e.payload?.overlay_position ?? 'bottom';
      // Rust repositions the window on save; refresh hoverZone so peek-
      // through detection picks up the new frame.
      setTimeout(() => { refreshHoverZone(); }, 200);
    }).then(u => uns.push(u));

    return () => {
      clearInterval(cursorTimer);
      clearInterval(elapsedTimer);
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
  @keyframes pulse-red-warn {
    0%, 100% { border-color: rgba(239, 68, 68, 0.4); }
    50%       { border-color: rgba(239, 68, 68, 1); }
  }
  @keyframes pulse-red-alert {
    0%, 100% { border-color: rgba(239, 68, 68, 0.5); box-shadow: 0 0 0 0 rgba(239,68,68,0); }
    50%       { border-color: rgba(239, 68, 68, 1);   box-shadow: 0 0 16px 5px rgba(239,68,68,0.55); }
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
  /* Past WARN_WORDS: faster pulse, brighter floor */
  .pill.recording.warn {
    animation: pulse-red-warn 2s ease-in-out infinite;
  }
  /* Past ALERT_WORDS: hard glow pulse, 1s cycle */
  .pill.recording.alert {
    animation: pulse-red-alert 1s ease-in-out infinite;
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

  /* Length counter badge — sits to the right of the pill, vertically centred. */
  .line-counter {
    position: absolute;
    left: calc(100% + 10px);
    top: 50%;
    transform: translateY(-50%);
    white-space: nowrap;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.03em;
    color: rgba(255, 255, 255, 0.65);
    background: rgba(16, 16, 16, 0.78);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 8px;
    padding: 4px 9px;
    backdrop-filter: blur(12px) saturate(140%);
    -webkit-backdrop-filter: blur(12px) saturate(140%);
    transition: opacity 180ms ease-out;
    pointer-events: none;
    user-select: none;
  }

</style>

<div class="w-full h-full flex items-center justify-center">
  <div class="relative">
  {#if indicatorEnabled}
    <div
      class="line-counter"
      style:opacity={mode === 'recording' ? (isPeeking ? 0.24 : 1) : 0}
    >
      {indicatorLabel}
    </div>
  {/if}
  <div
    class="pill flex items-center gap-3.5 px-5 py-3.5 rounded-2xl"
    class:show={mode !== 'idle'}
    class:arming={mode === 'arming'}
    class:recording={mode === 'recording'}
    class:transcribing={mode === 'transcribing'}
    class:warn={mode === 'recording' && wordCount >= WARN_WORDS && wordCount < ALERT_WORDS}
    class:alert={mode === 'recording' && wordCount >= ALERT_WORDS}
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
      {#if mode === 'recording' || (mode === 'transcribing' && elapsedSecs > 0)}
        <span class="text-[9px] tabular-nums select-none leading-tight"
              style="color: {mode === 'recording' && wordCount >= ALERT_WORDS
                ? 'rgba(239,100,68,0.85)'
                : mode === 'recording' && wordCount >= WARN_WORDS
                  ? 'rgba(251,191,36,0.75)'
                  : 'rgba(255,255,255,0.4)'};">
          {wordCount > 0 ? `~${wordCount}w · ` : ''}{fmtElapsed(elapsedSecs)}
        </span>
      {/if}
    </div>
    </div>

  </div>
  </div>
</div>
