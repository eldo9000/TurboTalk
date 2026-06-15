<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow, cursorPosition, primaryMonitor } from '@tauri-apps/api/window';
  import { commands } from './bindings.ts';

  // 'recording' | 'transcribing' | 'error' | 'idle'
  // Arming (warm-up) is now handled by the separate Status window.
  let mode      = $state('idle'); // 'idle' | 'recording' | 'transcribing' | 'error'
  let canvasEl  = $state(null);
  let wordCount = $state(0);

  // One-shot bright-red flash when the audio stream goes truly live (ptt-down).
  // Cleared after the keyframe so the steady-state slow pulse takes over. This
  // is the visual confirmation that capture started — see the audio-live gate
  // in src-tauri/src/hotkey.rs.
  let justConnected = $state(false);
  let flashTimer    = null;

  // Live draft preview (prototype). `seg-preview` events from the backend
  // segment transcriber fill this map keyed by segment index; we render the
  // values in index order so concurrent completions still read left-to-right.
  // This is the RAW per-segment text — it does not include the final tail
  // (transcribed at key-release) or the Chaperone cleanup pass, so it's a
  // draft, not the pasted result. Only surfaced in the 'large' overlay.
  let segPreview = $state({});
  let previewText = $derived(
    Object.keys(segPreview)
      .map(Number)
      .sort((a, b) => a - b)
      .map(i => segPreview[i])
      .join(' ')
  );
  let showPreview = $derived(
    overlaySize === 'large' && (mode === 'recording' || mode === 'transcribing')
  );

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
  const VISUAL_GATE_FLOOR = SPEECH_THRESHOLD * 0.65;
  const VISUAL_GATE_CEIL  = SPEECH_THRESHOLD * 1.6;
  let speechFrames = 0;

  let overlaySize      = $state('medium'); // 'small' | 'medium' | 'large'
  let overlayPosition  = $state('bottom');

  const CANVAS_W   = 140; // CSS pixels
  const CANVAS_H   = 28;
  const HISTORY    = 312;  // columns in the histogram (≈15.6s of 20Hz history — a long waveform for the wide large mode)
  // Medium meter renders only the most recent slice of the shared history so it
  // scrolls ~4× faster than large (large uses the full HISTORY). 312/4 = 78.
  const MEDIUM_COLS = 78;

  let levels = Array(HISTORY).fill(0);
  let cursorInZone = $state(false);
  // Peek-through (dim the pill so you can see what's behind it) is disabled in
  // 'large' mode: the window is tall to hold the transcript bubble, so the
  // hover zone would cover most of the screen and over-trigger — and in large
  // mode the whole point is to read the transcript, not see through it.
  let isPeeking = $derived(mode === 'recording' && cursorInZone && overlaySize !== 'large');

  // Visual AGC — track a rolling target RMS so quiet mics still show
  // full-scale bars, then ease the visual scale toward that target. The
  // target stays responsive; the displayed scale avoids "all bars resized
  // this frame" snaps when the first word or trailing quiet speech changes
  // the normalization baseline. Speech detection still uses raw RMS below.
  const METER_START = 0.02;  // start at a reasonable floor (~2% of full scale)
  const METER_FLOOR = 0.005; // never divide by smaller than this
  const METER_DECAY = 0.98;  // target peak decay per frame at 20 Hz
  const METER_ATTACK_SMOOTH = 0.16;
  const METER_RELEASE_SMOOTH = 0.045;
  const MINI_DOT_ATTACK = 0.28;
  const MINI_DOT_RELEASE = 0.12;
  let meterPeak = METER_START;
  let meterVisualPeak = METER_START;
  let miniDotLevel = $state(0);

  function resetMeterPeak() {
    meterPeak = METER_START;
    meterVisualPeak = METER_START;
    miniDotLevel = 0;
  }

  function visualLevel(v) {
    const gate = Math.min(
      1,
      Math.max(0, (v - VISUAL_GATE_FLOOR) / (VISUAL_GATE_CEIL - VISUAL_GATE_FLOOR))
    );
    if (gate <= 0) return 0;

    const peak = Math.max(meterVisualPeak, METER_FLOOR);
    const gainNorm = Math.min(1, v / peak);
    return Math.pow(gainNorm, 0.45) * gate;
  }

  function draw() {
    if (!canvasEl) return;
    const rect = canvasEl.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    if (rect.width > 0 && rect.height > 0) {
      const nextW = Math.max(1, Math.round(rect.width * dpr));
      const nextH = Math.max(1, Math.round(rect.height * dpr));
      if (canvasEl.width !== nextW || canvasEl.height !== nextH) {
        canvasEl.width = nextW;
        canvasEl.height = nextH;
      }
    }
    const ctx = canvasEl.getContext('2d');
    const W   = canvasEl.width;
    const H   = canvasEl.height;

    // Keep the waveform surface transparent so the pill background/backdrop
    // reads as one continuous material.
    ctx.clearRect(0, 0, W, H);

    const midY   = H / 2;
    // Large uses the full history; medium/small render only the most recent
    // MEDIUM_COLS columns of the same buffer, so the same audio scrolls ~8×
    // faster across the narrower meter.
    const cols   = overlaySize === 'large' ? HISTORY : MEDIUM_COLS;
    const start  = HISTORY - cols;
    const stepX  = W / (cols - 1);
    const color  = mode === 'recording' ? 'rgba(255,255,255,0.85)' : 'rgba(255,255,255,0.30)';

    // Draw a smooth mirrored waveform — continuous filled path
    // instead of discrete bars, for a clean modern look.
    ctx.beginPath();
    ctx.moveTo(0, midY);
    for (let i = 0; i < cols; i++) {
      const norm = visualLevel(levels[start + i]); // gentle perceptual curve
      ctx.lineTo(i * stepX, midY - norm * midY);
    }
    for (let i = cols - 1; i >= 0; i--) {
      const norm = visualLevel(levels[start + i]);
      ctx.lineTo(i * stepX, midY + norm * midY);
    }
    ctx.closePath();

    ctx.fillStyle = color;
    ctx.fill();
  }

  function stopTranscribing(nextMode = 'idle') {
    clearInterval(transcribeTimer);
    transcribeTimer = null;
    resetElapsed();
    mode = nextMode;
    draw();
  }

  onMount(async () => {
    const win = getCurrentWindow();
    const dpr = window.devicePixelRatio || 1;
    draw();

    // Initial config read for overlay size + position. Failure here is
    // non-fatal — defaults stay until a config-update event arrives.
    try {
      const cfg = await commands.getConfig();
      overlaySize      = cfg.overlay_size ?? 'medium';
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
      if (mode !== 'recording') {
        cursorInZone = false;
        return;
      }
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
    listen('ptt-down', () => {
      levels = Array(HISTORY).fill(0);
      speechFrames = 0;
      wordCount = 0;
      segPreview = {};
      resetMeterPeak();
      startElapsed();
      mode = 'recording';
      // Fire the one-shot connect flash. Re-arm cleanly if a previous flash
      // timer is still pending (rapid re-press).
      justConnected = true;
      clearTimeout(flashTimer);
      flashTimer = setTimeout(() => { justConnected = false; }, 560);
      draw();
      // Backend has just repositioned the window onto the cursor's monitor
      // (see reposition_overlay_to_cursor_monitor). Pick up the new frame
      // so the cursor-peek-through hoverZone matches reality. Slight delay
      // so the post-set_position frame has settled.
      setTimeout(() => { refreshHoverZone(); }, 200);
    }).then(u => uns.push(u));

    listen('ptt-up', () => {
      miniDotLevel = 0;
      mode = 'transcribing';
      stopElapsed(); // keep elapsedSecs visible as recording duration during transcription
      clearInterval(transcribeTimer);
      transcribeTimer = setInterval(() => { transcribeTick++; }, 50);
      draw();
    }).then(u => uns.push(u));

    listen('transcript', () => {
      // If the error panel is showing (flaky paste), don't dismiss it early.
      if (mode === 'error') return;
      setTimeout(() => { stopTranscribing('idle'); }, 350);
    }).then(u => uns.push(u));

    // Error panel helpers — toggle overlay cursor events so the user can
    // click the panel to open the main window at the history tab.
    let errorTimer = null;

    function enterError() {
      stopTranscribing('error');
    }

    function exitError() {
      clearTimeout(errorTimer);
      errorTimer = null;
      mode = 'idle';
    }

    // Guard: don't override the error panel with an idle transition.
    // The error panel owns the display until its timer dismisses it.
    function skipIfError() {
      return mode === 'error';
    }

    // Hallucination rejection — interrupt whatever the overlay is showing
    // (recording or transcribing) and replace it with a fixed-size error
    // panel that stays for 3 seconds regardless of overlay size mode.
    listen('transcription-rejected', (e) => {
      const payload = e.payload || {};
      if (skipIfError()) return;
      // Only interrupt for flaky (pasted despite detection) or blocked
      // (nothing pasted). Partial salvage follows transcript normally.
      if (payload.flaky || payload.pasted === false) {
        enterError();
        errorTimer = setTimeout(exitError, 3000);
      }
    }).then(u => uns.push(u));

    listen('transcript-error', () => {
      if (skipIfError()) return;
      enterError();
      errorTimer = setTimeout(exitError, 3000);
    }).then(u => uns.push(u));

    listen('recording-discarded', (e) => {
      if (skipIfError()) return;
      // empty-final-text means transcription ran but cleanup produced nothing
      // (e.g. all non-speech annotations stripped). Show feedback so the user
      // knows the system tried. Other discards (too-short, error-path) are
      // quick-tap or transient — go to idle silently.
      if (e.payload === 'empty-final-text') {
        enterError();
        errorTimer = setTimeout(exitError, 3000);
      } else {
        stopTranscribing('idle');
      }
    }).then(u => uns.push(u));

    listen('recording-cancelled', () => {
      if (skipIfError()) return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('recording-recovered', () => {
      if (skipIfError()) return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('recording-too-short', () => {
      if (skipIfError()) return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('device-lost', () => {
      if (skipIfError()) return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('paste-error', () => {
      if (skipIfError()) return;
      enterError();
      errorTimer = setTimeout(exitError, 3000);
    }).then(u => uns.push(u));

    // Belt-and-suspenders: backend always emits stage=ready when a job ends.
    listen('dictation-stage', (e) => {
      if (e.payload?.stage === 'ready' && mode === 'transcribing') {
        stopTranscribing('idle');
      }
    }).then(u => uns.push(u));

    listen('audio-level', (e) => {
      if (mode !== 'recording') return;
      const v = Math.min(1.0, e.payload);
      levels = [...levels.slice(1), v];

      // Visual AGC: responsive target, smoothed display scale.
      if (v > meterPeak) {
        meterPeak = v;
      } else {
        meterPeak = Math.max(METER_FLOOR, meterPeak * METER_DECAY);
      }
      const smooth = meterPeak > meterVisualPeak ? METER_ATTACK_SMOOTH : METER_RELEASE_SMOOTH;
      meterVisualPeak += (meterPeak - meterVisualPeak) * smooth;
      const dotTarget = visualLevel(v);
      const dotSmooth = dotTarget > miniDotLevel ? MINI_DOT_ATTACK : MINI_DOT_RELEASE;
      miniDotLevel += (dotTarget - miniDotLevel) * dotSmooth;

      if (v > SPEECH_THRESHOLD) {
        speechFrames++;
        const newCount = Math.round(speechFrames * 0.05 * 140 / 60);
        if (newCount > wordCount) {
          wordCount = newCount;
        }
      }
      draw();
    }).then(u => uns.push(u));

    // Live draft preview (prototype): each mid-recording segment's raw text
    // as soon as it transcribes. Accumulate by index; the derived previewText
    // joins them in order.
    listen('seg-preview', (e) => {
      const index = e.payload?.index;
      const text  = e.payload?.text;
      if (typeof index !== 'number' || !text) return;
      segPreview = { ...segPreview, [index]: text };
    }).then(u => uns.push(u));

    listen('config-update', (e) => {
      overlaySize      = e.payload?.overlay_size ?? 'medium';
      overlayPosition  = e.payload?.overlay_position ?? 'bottom';
      // Rust repositions the window on save; refresh hoverZone so peek-
      // through detection picks up the new frame.
      setTimeout(() => { refreshHoverZone(); }, 200);
    }).then(u => uns.push(u));

    return () => {
      clearInterval(cursorTimer);
      clearInterval(elapsedTimer);
      clearTimeout(flashTimer);
      clearTimeout(errorTimer);
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
  /* One-shot bright-red flash the instant the audio stream goes live, then
     hand off to the steady slow pulse (delayed so they don't fight). This is
     the "it's connected, talk now" cue. */
  .pill.recording.flash {
    animation:
      connect-flash 0.55s ease-out,
      pulse-red 10s ease-in-out infinite 0.55s;
  }
  @keyframes connect-flash {
    0% {
      border-color: rgba(239, 68, 68, 1);
      box-shadow:
        0 0 0 3px rgba(239, 68, 68, 0.35),
        0 0 24px 7px rgba(239, 68, 68, 0.6);
    }
    100% {
      border-color: rgba(239, 68, 68, 0.15);
      box-shadow: 0 0 0 0 rgba(239, 68, 68, 0);
    }
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
  .pill-inner {
    display: contents;
    transition: opacity 120ms ease-out;
  }
  .pill.small {
    width: auto;
    min-width: 68px;
    height: 38px;
    box-sizing: border-box;
    gap: 7px;
    padding: 0 12px 0 11px;
    border-radius: 999px;
  }
  .pill.large {
    min-width: 984px;
    min-height: 92px;
    box-sizing: border-box;
    gap: 18px;
    padding: 18px 22px;
    border-radius: 20px;
  }

  /* Error / rejection panel — fixed size regardless of overlay size mode.
     Interrupts and replaces whatever recording/transcribing state was active.
     Yellow pulsing border (same as warm-up tile), content centered, no
     waveform or timer. Clickable — opens main window at the history tab. */
  .pill.error {
    width: 260px;
    height: 80px;
    box-sizing: border-box;
    justify-content: center;
    gap: 0;
    padding: 0;
    border-radius: 20px;
    border-color: rgba(251, 191, 36, 0.85);
    animation: pulse-error 1.15s ease-in-out infinite;
    pointer-events: auto;
    cursor: pointer;
  }

  @keyframes pulse-error {
    0%, 100% {
      border-color: rgba(251, 191, 36, 0.28);
      box-shadow: 0 0 0 0 rgba(251, 191, 36, 0);
    }
    45%, 55% {
      border-color: rgba(251, 191, 36, 1);
      box-shadow:
        0 0 0 3px rgba(251, 191, 36, 0.22),
        0 0 22px 5px rgba(251, 191, 36, 0.34);
    }
  }

  .error-panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
  }

  .error-label {
    font-size: 13px;
    font-weight: 700;
    line-height: 1;
    letter-spacing: 0;
    color: #fbbf24;
    user-select: none;
  }

  .error-hint {
    font-size: 9px;
    font-weight: 500;
    line-height: 1;
    letter-spacing: 0.08em;
    color: rgba(251, 191, 36, 0.55);
    user-select: none;
  }
  .status-dot {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    flex: 0 0 auto;
    background: #f87171;
    box-shadow: 0 0 0 3px rgba(248, 113, 113, 0.14), 0 0 14px rgba(248, 113, 113, 0.4);
    transition: opacity 55ms linear;
  }
  .status-dot.transcribing {
    background: #fbbf24;
    box-shadow: 0 0 0 3px rgba(251, 191, 36, 0.14), 0 0 14px rgba(251, 191, 36, 0.36);
  }
  .small-time {
    font-size: 12px;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    font-weight: 650;
    color: rgba(255, 255, 255, 0.78);
    user-select: none;
  }
  .large-stack {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 5px;
    min-width: 112px;
  }
  .large-title {
    font-size: 14px;
    font-weight: 700;
    line-height: 1;
    letter-spacing: 0;
    color: #f87171;
    user-select: none;
  }
  .large-title.transcribing {
    color: #fbbf24;
  }
  .large-meta {
    font-size: 11px;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.48);
    user-select: none;
  }
  .large-meter {
    flex: 1 1 auto;
    min-width: 0;
    height: 38px;
  }

  canvas {
    display: block;
    background: transparent;
    /* Fade the waveform out toward the left as it scrolls off, instead of a
       hard clip at the edge. The gradient ramps from transparent at the far
       left to fully opaque at 25% of the bar width — same proportion on both
       the large and medium meters. */
    -webkit-mask-image: linear-gradient(to right, transparent 0%, #000 25%);
    mask-image: linear-gradient(to right, transparent 0%, #000 25%);
  }

  /* Live draft preview box. Floats above the pill (bottom overlay) or below it
     (top overlay) and grows with the transcript — no clip, no fade. For bottom
     position the box's bottom edge is pinned just above the pill, so new text
     pushes the box upward as you talk; for top position the top edge is pinned
     and it grows downward. The 'large' window (see lib.rs) gives the room to
     expand. Purpose of this mode: see exactly how much will be pasted. */
  .seg-preview {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    bottom: calc(100% + 12px);
    width: 984px;
    box-sizing: border-box;
    padding: 14px 20px;
    border-radius: 14px;
    background: rgba(16, 16, 16, 0.87);
    border: 1px solid rgba(255, 255, 255, 0.08);
    backdrop-filter: blur(18px) saturate(160%);
    -webkit-backdrop-filter: blur(18px) saturate(160%);
    transition: opacity 180ms ease-out;
    pointer-events: none;
  }
  .seg-preview.below {
    bottom: auto;
    top: calc(100% + 12px);
  }
  .seg-preview-text {
    font-size: 17px;
    line-height: 1.45;
    text-align: left;
    color: rgba(255, 255, 255, 0.92);
    word-break: break-word;
    user-select: none;
  }
  .seg-preview-placeholder {
    font-size: 16px;
    font-style: italic;
    color: rgba(255, 255, 255, 0.4);
    user-select: none;
  }

</style>

<!-- In 'large' mode the window is tall (see overlay_height_for_size in lib.rs).
     Anchor the pill to the screen-edge side — bottom of the window for bottom
     position, top for top position — with a 100 px gutter, so the pill lands
     in the same on-screen spot as the default size while the transcript bubble
     grows into the rest of the window. Small/medium stay centered.
     Error mode always centers regardless of overlay size. -->
<div
  class="w-full h-full flex justify-center"
  class:items-center={overlaySize !== 'large' || mode === 'error'}
  class:items-end={overlaySize === 'large' && overlayPosition !== 'top' && mode !== 'error'}
  class:items-start={overlaySize === 'large' && overlayPosition === 'top' && mode !== 'error'}
  style:padding-bottom={overlaySize === 'large' && overlayPosition !== 'top' && mode !== 'error' ? '100px' : '0'}
  style:padding-top={overlaySize === 'large' && overlayPosition === 'top' && mode !== 'error' ? '100px' : '0'}
>
  <div class="relative">
  {#if showPreview}
    <div
      class="seg-preview"
      class:below={overlayPosition === 'top'}
      style:opacity={isPeeking ? 0.12 : 1}
    >
      {#if previewText}
        <span class="seg-preview-text">{previewText}</span>
      {:else}
        <span class="seg-preview-placeholder">Listening…</span>
      {/if}
    </div>
  {/if}
  <div
    class="pill flex items-center gap-3.5 px-5 py-3.5 rounded-2xl"
    class:show={mode !== 'idle'}
    class:small={overlaySize === 'small' && mode !== 'error'}
    class:large={overlaySize === 'large' && mode !== 'error'}
    class:error={mode === 'error'}
    class:recording={mode === 'recording'}
    class:flash={mode === 'recording' && justConnected}
    class:transcribing={mode === 'transcribing'}
    class:warn={mode === 'recording' && wordCount >= WARN_WORDS && wordCount < ALERT_WORDS}
    class:alert={mode === 'recording' && wordCount >= ALERT_WORDS}
    class:peek={isPeeking}
    style:background={isPeeking ? 'rgba(16,16,16,0.12)' : 'rgba(16,16,16,0.87)'}
    style:backdrop-filter={isPeeking ? 'blur(1px) saturate(100%)' : 'blur(18px) saturate(160%)'}
    style:-webkit-backdrop-filter={isPeeking ? 'blur(1px) saturate(100%)' : 'blur(18px) saturate(160%)'}
    style:opacity={mode === 'idle' ? 0 : isPeeking ? 0.24 : 1}
  >
    <div class="pill-inner">
    {#if mode === 'error'}
      <!-- Fixed-size error panel — same regardless of small/medium/large.
           Auto-dismisses after 3 seconds. The main window opens to History
           automatically so the user can inspect the problematic text. -->
      <div class="error-panel">
        <span class="error-label">Error captured</span>
        <span class="error-hint">checking history…</span>
      </div>
    {:else if overlaySize === 'small'}
      <canvas
        bind:this={canvasEl}
        aria-hidden="true"
        style="position: absolute; width: 1px; height: 1px; opacity: 0; pointer-events: none;"
      ></canvas>
      <div
        class="status-dot"
        class:transcribing={mode === 'transcribing'}
        style:opacity={mode === 'recording' ? miniDotLevel : 1}
      ></div>
      <div class="small-time">{fmtElapsed(elapsedSecs)}</div>
    {:else if overlaySize === 'large'}
      <canvas
        bind:this={canvasEl}
        class="large-meter"
        style="height: 38px;"
      ></canvas>

      <div class="large-stack">
        <span
          class="large-title"
          class:transcribing={mode === 'transcribing'}
        >
          {mode === 'recording' ? 'Recording' : 'Transcribing'}
        </span>
        <span class="large-meta">
          {wordCount > 0 ? `About ${wordCount} words · ` : ''}{fmtElapsed(elapsedSecs)}
        </span>
      </div>
    {:else}
    <canvas
      bind:this={canvasEl}
      style="width: {CANVAS_W}px; height: {CANVAS_H}px;"
    ></canvas>

    <div class="flex flex-col items-start gap-[1px]">
      <span
        class="text-[11px] font-semibold tracking-wide select-none leading-tight"
        style="color: {mode === 'recording' ? '#f87171' : '#fbbf24'};"
      >
        {mode === 'recording' ? 'Recording' : 'Transcribing…'}
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
    {/if}
    </div>

  </div>
  </div>
</div>
