<script>
  /**
   * Timecode — canonical playhead/duration timecode display.
   *
   * Two display modes (toggle by clicking the body):
   *  - 'timecode' — HH:MM:SS:FF with dim/bright digit treatment (leading zeros dim)
   *  - 'frames'   — total frame counter
   *
   * Bottom row shows remaining (left) and duration (right) in the same mode.
   *
   * Pattern lifted from Flicker-App's App.svelte timecode block.
   *
   * @typedef {{
   *   time?: number,         // playhead position in seconds
   *   duration?: number,     // total duration in seconds
   *   fps?: number,          // frames per second (default 30)
   *   mode?: 'timecode' | 'frames',
   *   onModeChange?: (mode: 'timecode' | 'frames') => void,
   * }} Props
   *
   * @type {Props}
   */
  let {
    time = 0,
    duration = 0,
    fps = 30,
    mode = $bindable('timecode'),
    onModeChange,
  } = $props();

  function segsFor(t) {
    const clamped = Math.max(0, t ?? 0);
    const h = Math.floor(clamped / 3600);
    const m = Math.floor((clamped % 3600) / 60);
    const s = Math.floor(clamped % 60);
    const f = Math.floor((clamped % 1) * fps);
    return {
      h: { v: pad(h), bright: h > 0 },
      m: { v: pad(m), bright: m > 0 || h > 0 },
      s: { v: pad(s), bright: s > 0 || m > 0 || h > 0 },
      f: { v: pad(f), bright: f > 0 || s > 0 || m > 0 || h > 0 },
    };
  }

  function pad(n) { return String(n).padStart(2, '0'); }

  const playSegs      = $derived(segsFor(time));
  const remainingSegs = $derived(segsFor((duration ?? 0) - (time ?? 0)));
  const durationSegs  = $derived(segsFor(duration ?? 0));

  const playFrames      = $derived(Math.max(0, Math.floor((time ?? 0) * fps)));
  const remainingFrames = $derived(Math.max(0, Math.floor(((duration ?? 0) - (time ?? 0)) * fps)));
  const durationFrames  = $derived(Math.max(0, Math.floor((duration ?? 0) * fps)));

  function toggle() {
    const next = mode === 'timecode' ? 'frames' : 'timecode';
    mode = next;
    onModeChange?.(next);
  }
</script>

<button
  class="tc"
  onclick={toggle}
  title="Click to toggle timecode ⟷ frames"
  aria-label="Toggle timecode mode"
>
  {#if mode === 'timecode'}
    <span class="tc-main">
      <span class:bright={playSegs.h.bright}>{playSegs.h.v}</span><span class="tc-sep">:</span><span class:bright={playSegs.m.bright}>{playSegs.m.v}</span><span class="tc-sep">:</span><span class:bright={playSegs.s.bright}>{playSegs.s.v}</span><span class="tc-sep">:</span><span class:bright={playSegs.f.bright}>{playSegs.f.v}</span>
    </span>
    <div class="tc-meta">
      <span title="Time remaining">
        <span class:bright={remainingSegs.h.bright}>{remainingSegs.h.v}</span><span class="tc-sep">:</span><span class:bright={remainingSegs.m.bright}>{remainingSegs.m.v}</span><span class="tc-sep">:</span><span class:bright={remainingSegs.s.bright}>{remainingSegs.s.v}</span>
      </span>
      <span title="Total duration">
        <span class:bright={durationSegs.h.bright}>{durationSegs.h.v}</span><span class="tc-sep">:</span><span class:bright={durationSegs.m.bright}>{durationSegs.m.v}</span><span class="tc-sep">:</span><span class:bright={durationSegs.s.bright}>{durationSegs.s.v}</span>
      </span>
    </div>
  {:else}
    <span class="tc-main tc-frames" class:bright={playFrames > 0}>{playFrames}</span>
    <div class="tc-meta">
      <span title="Frames remaining" class:bright={remainingFrames > 0}>{remainingFrames}</span>
      <span title="Total frames" class:bright={durationFrames > 0}>{durationFrames}</span>
    </div>
  {/if}
</button>

<style>
  .tc {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: stretch;
    min-width: 144px;
    padding: 4px 16px;
    background: var(--surface-deep, #0c0c0c);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: #3a3a3a;
    font-family: 'Geist Mono', ui-monospace, monospace;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.05em;
    cursor: default;
    transition: background 0.1s;
  }
  .tc:hover { background: color-mix(in srgb, var(--surface-deep, #0c0c0c) 70%, var(--text-primary)); }

  .tc-main {
    display: block;
    font-size: 15px;
    line-height: 1;
    text-align: left;
  }
  .tc-frames { text-align: center; }

  .tc-meta {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-top: 4px;
    font-size: 8px;
    line-height: 1;
    letter-spacing: 0.04em;
  }

  .bright { color: #c8c8c8; }
  .tc-sep { color: #2e2e2e; }
</style>
