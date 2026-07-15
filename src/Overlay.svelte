<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { commands } from './bindings.ts';
import { invoke } from '@tauri-apps/api/core';

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
  // (transcribed at key-release) or the cleanup pass, so it's a
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
  let liveGlyphWordCount = $state(0);
  let liveGlyphAccumulator = 0;
  let liveGlyphWordsPerSecond = 3.1; // ~185 WPM starter; self-corrects as segments commit.
  let committedPreviewTokens = $derived(
    previewText.trim() ? previewText.trim().split(/\s+/) : []
  );
  let committedPreviewWords = $derived(committedPreviewTokens.length);
  let glyphWordCount = $derived(
    showPreview ? Math.max(liveGlyphWordCount, committedPreviewWords) : 0
  );
  let previewUnits = $derived(
    Array.from({ length: glyphWordCount }, (_, index) => ({
      index,
      text: committedPreviewTokens[index] ?? '',
      width: glyphWidth(index),
      committed: index < committedPreviewWords,
    }))
  );

  let transcribeTimer    = null;
  let transcribeTick     = $state(0); // incremented by timer to keep Svelte rendering during transcription
  let idleTimer          = null; // handle for the 350ms stopTranscribing timeout

  // Job-id tracking so the overlay can ignore stale events from a previous
  // dictation that arrive after a new one has already started.  The backend
  // emits a dictation-stage event with a job_id immediately after ptt-down
  // (in the same critical section), so this is authoritative for "which
  // dictation is currently active."
  let currentJobId = $state(null);

  // ── Overlay event log (rotating ring buffer) ──────────────────────────
  // The last 100 overlay lifecycle events are kept in memory and exposed
  // on `window.__OVERLAY_LOG` so they can be inspected from the browser
  // devtools.  Every entry is also forwarded to the Rust tracing system
  // via `log_overlay_event` so it ends up in the rolling log files under
  // ~/.config/turbotalk/logs/.
  const MAX_LOG = 100;
  const overlayLog = [];
  let logSeq = 0;

  function logOverlay(trigger, detail) {
    const entry = { seq: ++logSeq, ts: Date.now(), trigger, detail };
    overlayLog.push(entry);
    if (overlayLog.length > MAX_LOG) overlayLog.shift();
    // Best-effort: if the IPC call fails (e.g. Tauri not ready yet) the
    // entry is still in the in-memory ring buffer.
    invoke('log_overlay_event', { event: trigger, detail }).catch(() => {});
    window.__OVERLAY_LOG = overlayLog;
  }

  let recordingStart  = null;
  let elapsedSecs     = $state(0);
  let elapsedTimer    = null;

  const WARN_WORDS  = 100; // ~43s speech — faster pulse
  const ALERT_WORDS = 300; // ~2min speech — aggressive glow pulse

  function fmtElapsed(s) {
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  }

  function glyphWidth(index) {
    const loremWordLengths = [
      5, 5, 5, 3, 5, 11, 4, 2, 4, 9, 10, 4, 3, 2, 5, 6,
      7, 3, 5, 2, 9, 4, 8, 3, 5, 2, 4, 3, 5, 9, 5, 3,
      4, 8, 2, 5, 7, 6, 3, 6, 5, 8, 4, 6, 5, 3, 4, 2,
      7, 6, 7, 5, 5, 6, 3, 4, 7, 4, 3, 5, 4, 8, 3, 8,
    ];
    const chars = loremWordLengths[index % loremWordLengths.length];
    return Math.round(Math.max(20, Math.min(112, chars * 8.2 + 14)));
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
  // Live pill count starts from a reasonable dictation pace, then adapts when
  // committed segment text reveals whether the visual estimate was too slow
  // or too fast for the current speaker/phrase.
  const SPEECH_THRESHOLD = 0.008;
  const VISUAL_GATE_FLOOR = SPEECH_THRESHOLD * 0.65;
  const VISUAL_GATE_CEIL  = SPEECH_THRESHOLD * 1.6;
  const DEFAULT_GLYPH_WORDS_PER_SECOND = 3.1; // ~185 WPM
  const MIN_GLYPH_WORDS_PER_SECOND = 1.6;     // ~96 WPM
  const MAX_GLYPH_WORDS_PER_SECOND = 5.2;     // ~312 WPM
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
  let levelsHead = $state(0);

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

  function clampGlyphRate(rate) {
    return Math.max(MIN_GLYPH_WORDS_PER_SECOND, Math.min(MAX_GLYPH_WORDS_PER_SECOND, rate));
  }

  function adaptGlyphRate(committedWords) {
    if (committedWords < 4 || liveGlyphWordCount < 2) return;
    const ratio = committedWords / liveGlyphWordCount;
    if (ratio > 1.12 || ratio < 0.88) {
      liveGlyphWordsPerSecond = clampGlyphRate(
        liveGlyphWordsPerSecond * (0.72 + Math.min(1.7, Math.max(0.55, ratio)) * 0.28)
      );
    }
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
    const start  = (levelsHead - cols + HISTORY) % HISTORY;
    const stepX  = W / (cols - 1);
    const color  = mode === 'recording' ? 'rgba(255,255,255,0.85)' : 'rgba(255,255,255,0.30)';

    // Draw a smooth mirrored waveform — continuous filled path
    // instead of discrete bars, for a clean modern look. Ring buffer:
    // levels start at `start` and wrap around HISTORY.
    ctx.beginPath();
    ctx.moveTo(0, midY);
    for (let i = 0; i < cols; i++) {
      const norm = visualLevel(levels[(start + i) % HISTORY]); // gentle perceptual curve
      ctx.lineTo(i * stepX, midY - norm * midY);
    }
    for (let i = cols - 1; i >= 0; i--) {
      const norm = visualLevel(levels[(start + i) % HISTORY]);
      ctx.lineTo(i * stepX, midY + norm * midY);
    }
    ctx.closePath();

    ctx.fillStyle = color;
    ctx.fill();
  }

  function stopTranscribing(nextMode = 'idle') {
    logOverlay('mode_transition', { from: mode, to: nextMode, trigger: 'stopTranscribing', job_id: currentJobId });
    clearTimeout(idleTimer);
    idleTimer = null;
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

    const uns = [];

    // Backend emits ptt-armed BEFORE ptt-down only when whisper-server is
    // still loading (cold start). On the warm path ptt-down fires directly
    // and the arming state is skipped entirely — no yellow flash.
    listen('ptt-down', () => {
      logOverlay('event_arrived', { event: 'ptt-down', mode, job_id: currentJobId, accepted: true });
      levels = Array(HISTORY).fill(0);
      levelsHead = 0;
      speechFrames = 0;
      wordCount = 0;
      liveGlyphWordCount = 0;
      liveGlyphAccumulator = 0;
      liveGlyphWordsPerSecond = DEFAULT_GLYPH_WORDS_PER_SECOND;
      segPreview = {};
      resetMeterPeak();
      startElapsed();
      mode = 'recording';
      // Clear the job-id anchor — the next dictation-stage event (emitted
      // immediately after ptt-down in the same critical section) will set
      // the correct currentJobId.  Setting null here means completion events
      // from the *previous* job are rejected until the new job_id arrives.
      currentJobId = null;
      // Cancel any stale idle transition from a previous dictation's
      // transcript event.  Without this, the 350ms timeout fires after
      // ptt-down and clobbers mode back to 'idle', making the overlay
      // invisible for the entire new recording.
      logOverlay('timer_cancel', { name: 'idleTimer', reason: 'ptt-down', had_value: idleTimer !== null });
      clearTimeout(idleTimer);
      idleTimer = null;
      // Also cancel any lingering error-panel timer so a stale
      // exitError() doesn't clobber the recording mode mid-dictation.
      logOverlay('timer_cancel', { name: 'errorTimer', reason: 'ptt-down', had_value: errorTimer !== null });
      clearTimeout(errorTimer);
      errorTimer = null;
      // Fire the one-shot connect flash. Re-arm cleanly if a previous flash
      // timer is still pending (rapid re-press).
      justConnected = true;
      clearTimeout(flashTimer);
      flashTimer = setTimeout(() => { justConnected = false; }, 560);
      draw();

    }).then(u => uns.push(u));

    listen('ptt-up', () => {
      logOverlay('event_arrived', { event: 'ptt-up', mode, job_id: currentJobId, accepted: true });
      miniDotLevel = 0;
      mode = 'transcribing';
      stopElapsed(); // keep elapsedSecs visible as recording duration during transcription
      clearInterval(transcribeTimer);
      transcribeTimer = setInterval(() => { transcribeTick++; }, 50);
      draw();
    }).then(u => uns.push(u));

    listen('transcript', () => {
      const accepted = mode === 'transcribing';
      logOverlay('event_arrived', { event: 'transcript', mode, job_id: currentJobId, accepted });
      // Guard: only transition from transcribing → idle.  If the mode is
      // already 'recording', a new dictation started before this event
      // arrived — ignore the stale transcript entirely so its delayed
      // timeout doesn't make the overlay invisible mid-recording.
      if (mode !== 'transcribing') return;
      clearTimeout(idleTimer);
      idleTimer = setTimeout(() => { stopTranscribing('idle'); }, 350);
      logOverlay('timer_set', { name: 'idleTimer', delay: 350, reason: 'transcript_event', job_id: currentJobId });
    }).then(u => uns.push(u));

    // Error panel helpers — toggle overlay cursor events so the user can
    // click the panel to open the main window at the history tab.
    let errorTimer = null;

    function enterError() {
      logOverlay('mode_transition', { from: mode, to: 'error', trigger: 'enterError', job_id: currentJobId });
      stopTranscribing('error');
    }

    function exitError() {
      logOverlay('mode_transition', { from: 'error', to: 'idle', trigger: 'exitError_timer', job_id: currentJobId });
      clearTimeout(errorTimer);
      errorTimer = null;
      mode = 'idle';
    }

    // Guard: don't override the error panel with an idle transition.
    // The error panel owns the display until its timer dismisses it.
    function skipIfError() {
      if (mode === 'error') {
        logOverlay('guard_rejected', { guard: 'skipIfError', event: '(caller)', mode, job_id: currentJobId });
        return true;
      }
      return false;
    }

    // Guard: only allow lifecycle-completion transitions when the overlay
    // is in a terminal display state (transcribing or error).  Ignores
    // stale events from a previous dictation that arrive during a new
    // recording — those would hide the overlay mid-dictation.
    function skipUnlessTranscribingOrError() {
      if (mode !== 'transcribing' && mode !== 'error') {
        logOverlay('guard_rejected', { guard: 'skipUnlessTranscribingOrError', event: '(caller)', mode, job_id: currentJobId });
        return true;
      }
      return false;
    }

    // Hallucination rejection — interrupt whatever the overlay is showing
    // (recording or transcribing) and replace it with a fixed-size error
    // panel that stays for 3 seconds regardless of overlay size mode.
    // All rejections are now advisory (text is always pasted) — the panel
    // alerts the user to review the output.
    listen('transcription-rejected', (e) => {
      const guard = skipIfError();
      logOverlay('event_arrived', { event: 'transcription-rejected', mode, job_id: currentJobId, guarded: guard, flaky: e.payload?.flaky, pasted: e.payload?.pasted });
      if (guard) return;
      enterError();
      errorTimer = setTimeout(exitError, 3000);
      logOverlay('timer_set', { name: 'errorTimer', delay: 3000, reason: 'transcription-rejected', job_id: currentJobId });
    }).then(u => uns.push(u));

    listen('transcript-error', () => {
      const guarded = skipIfError();
      logOverlay('event_arrived', { event: 'transcript-error', mode, job_id: currentJobId, guarded });
      if (guarded) return;
      enterError();
      errorTimer = setTimeout(exitError, 3000);
      logOverlay('timer_set', { name: 'errorTimer', delay: 3000, reason: 'transcript-error', job_id: currentJobId });
    }).then(u => uns.push(u));

    listen('recording-discarded', (e) => {
      logOverlay('event_arrived', { event: 'recording-discarded', mode, job_id: currentJobId, reason: e.payload });
      if (mode === 'idle') return;
      // empty-final-text means transcription ran but cleanup produced nothing
      // (e.g. all non-speech annotations stripped). Show feedback so the user
      // knows the system tried. Other discards (too-short, error-path) are
      // quick-tap or transient — go to idle silently.
      if (e.payload === 'empty-final-text') {
        enterError();
        errorTimer = setTimeout(exitError, 3000);
        logOverlay('timer_set', { name: 'errorTimer', delay: 3000, reason: 'recording-discarded', job_id: currentJobId });
      } else {
        stopTranscribing('idle');
      }
    }).then(u => uns.push(u));

    listen('recording-cancelled', () => {
      logOverlay('event_arrived', { event: 'recording-cancelled', mode, job_id: currentJobId });
      if (mode === 'idle') return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('recording-recovered', () => {
      const guarded = skipUnlessTranscribingOrError();
      logOverlay('event_arrived', { event: 'recording-recovered', mode, job_id: currentJobId, guarded });
      if (guarded) return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('recording-too-short', () => {
      const guarded = skipUnlessTranscribingOrError();
      logOverlay('event_arrived', { event: 'recording-too-short', mode, job_id: currentJobId, guarded });
      if (guarded) return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('device-lost', () => {
      logOverlay('event_arrived', { event: 'device-lost', mode, job_id: currentJobId });
      if (mode === 'idle') return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    listen('paste-error', () => {
      const guarded = skipUnlessTranscribingOrError();
      logOverlay('event_arrived', { event: 'paste-error', mode, job_id: currentJobId, guarded });
      if (guarded) return;
      enterError();
      errorTimer = setTimeout(exitError, 3000);
      logOverlay('timer_set', { name: 'errorTimer', delay: 3000, reason: 'paste-error', job_id: currentJobId });
    }).then(u => uns.push(u));

    listen('paste-copied', () => {
      const guarded = skipUnlessTranscribingOrError();
      logOverlay('event_arrived', { event: 'paste-copied', mode, job_id: currentJobId, guarded });
      if (guarded) return;
      stopTranscribing('idle');
    }).then(u => uns.push(u));

    // Belt-and-suspenders: backend always emits stage=ready when a job ends.
    // Use job_id to prevent a stale ready event from a previous dictation
    // from hiding the overlay during the current dictation's transcription
    // phase.  ptt-down resets currentJobId to null; the first dictation-stage
    // after ptt-down (always emitted in the same critical section) sets it.
    listen('dictation-stage', (e) => {
      const jobId = e.payload?.job_id;
      const stage = e.payload?.stage;
      logOverlay('event_arrived', { event: 'dictation-stage', stage, mode, job_id_in_payload: jobId, current_job_id: currentJobId });
      // Silently update the job-id anchor.  The backend emits this
      // immediately after ptt-down, before any completion events.
      if (jobId != null && currentJobId == null) {
        currentJobId = jobId;
      }
      // Stage=ready → transition to idle, but ONLY for our own job.
      if (stage === 'ready' && mode === 'transcribing' && jobId === currentJobId) {
        stopTranscribing('idle');
      }
    }).then(u => uns.push(u));

    listen('audio-level', (e) => {
      if (mode !== 'recording') return;
      const v = Math.min(1.0, e.payload);
      levels[levelsHead] = v;
      levelsHead = (levelsHead + 1) % HISTORY;

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
        liveGlyphAccumulator += 0.05 * liveGlyphWordsPerSecond;
        while (liveGlyphAccumulator >= 1) {
          liveGlyphWordCount++;
          liveGlyphAccumulator -= 1;
        }
        const newCount = Math.round(speechFrames * 0.05 * liveGlyphWordsPerSecond);
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
      const nextPreview = { ...segPreview, [index]: text };
      const committedWords = Object.keys(nextPreview)
        .map(Number)
        .sort((a, b) => a - b)
        .map(i => nextPreview[i])
        .join(' ')
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .length;
      adaptGlyphRate(committedWords);
      segPreview = nextPreview;
      liveGlyphWordCount = Math.max(liveGlyphWordCount, committedWords);
      wordCount = Math.max(wordCount, committedWords);
    }).then(u => uns.push(u));

    listen('config-update', (e) => {
      overlaySize      = e.payload?.overlay_size ?? 'medium';
      overlayPosition  = e.payload?.overlay_position ?? 'bottom';
    }).then(u => uns.push(u));

    return () => {
      logOverlay('component_destroy', { mode, job_id: currentJobId });
      clearInterval(elapsedTimer);
      clearTimeout(flashTimer);
      clearTimeout(errorTimer);
      clearTimeout(idleTimer);
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
    width: 984px;
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

  /* Large-mode preview box. Live speech renders as glyphic word pills so it
     does not invite proofreading mid-sentence. When a paused segment lands,
     those committed pills become real words and the live tail continues from
     the new edge instead of replaying the whole paragraph distance. */
  .seg-preview {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    bottom: calc(100% + 12px);
    width: 984px;
    box-sizing: border-box;
    max-height: 250px;
    padding: 13px 22px;
    border-radius: 14px;
    background: rgba(16, 16, 16, 0.87);
    border: 1px solid rgba(255, 255, 255, 0.08);
    backdrop-filter: blur(18px) saturate(160%);
    -webkit-backdrop-filter: blur(18px) saturate(160%);
    transition: opacity 180ms ease-out;
    pointer-events: none;
    overflow: hidden;
  }
  .seg-preview.below {
    bottom: auto;
    top: calc(100% + 12px);
  }
  .glyph-preview {
    display: flex;
    flex-wrap: wrap;
    align-content: flex-end;
    align-items: center;
    gap: 9px 8px;
    max-height: 212px;
    overflow: hidden;
    -webkit-mask-image: linear-gradient(to bottom, transparent 0%, #000 18%, #000 100%);
    mask-image: linear-gradient(to bottom, transparent 0%, #000 18%, #000 100%);
  }

  .glyph-word {
    height: 13px;
    border-radius: 999px;
    flex: 0 0 auto;
    background:
      linear-gradient(90deg, rgba(255,255,255,0.32), rgba(255,255,255,0.17));
    box-shadow: inset 0 0 0 1px rgba(255,255,255,0.04);
    opacity: 0.58;
    transform: translateY(0);
    animation: glyph-in 140ms ease-out both;
  }

  .glyph-text-word {
    color: rgba(255, 255, 255, 0.9);
    font-size: 17px;
    font-weight: 520;
    line-height: 1.35;
    overflow-wrap: anywhere;
    user-select: none;
    animation: text-in 150ms ease-out both;
  }

  @keyframes glyph-in {
    0% { transform: translateY(4px) scaleX(0.7); }
    100% { transform: translateY(0) scaleX(1); }
  }

  @keyframes text-in {
    0% { opacity: 0; filter: blur(4px); }
    100% { opacity: 1; filter: blur(0); }
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
    >
      {#if previewUnits.length > 0}
        <div class="glyph-preview" aria-hidden="true">
          {#each previewUnits as word (word.index)}
            {#if word.committed}
              <span class="glyph-text-word">{word.text}</span>
            {:else}
              <span
                class="glyph-word"
                style:width={`${word.width}px`}
              ></span>
            {/if}
          {/each}
        </div>
      {:else}
        <div class="glyph-preview" aria-hidden="true"></div>
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
    style:background={'rgba(16,16,16,0.87)'}
    style:backdrop-filter={'blur(18px) saturate(160%)'}
    style:-webkit-backdrop-filter={'blur(18px) saturate(160%)'}
  >
    <div class="pill-inner">
    {#if mode === 'error'}
      <!-- Fixed-size error panel — same regardless of small/medium/large.
           Auto-dismisses after 3 seconds. Only the overlay shows the toast;
           no main window popup — the text was already pasted in full. -->
      <div class="error-panel">
        <span class="error-label">Error detected!</span>
        <span class="error-hint">Check the output text for errors</span>
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
