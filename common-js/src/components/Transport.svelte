<script>
  /**
   * Transport — canonical media transport bar (skip-start / J / play-pause / L / skip-end).
   *
   * Pattern lifted from Flicker-App's EditTab transport row.
   *
   * @typedef {{
   *   playing?: boolean,
   *   playbackRate?: number,
   *   onTogglePlay?: () => void,
   *   onSkipStart?: () => void,
   *   onSkipEnd?: () => void,
   *   onRewind?: () => void,
   *   onForward?: () => void,
   *   showFullscreen?: boolean,
   *   fullscreen?: boolean,
   *   onToggleFullscreen?: () => void,
   * }} Props
   *
   * @type {Props}
   */
  let {
    playing = false,
    playbackRate = 1,
    onTogglePlay,
    onSkipStart,
    onSkipEnd,
    onRewind,
    onForward,
    showFullscreen = false,
    fullscreen = false,
    onToggleFullscreen,
  } = $props();

  const bwdActive = $derived(playing && playbackRate < 0);
  const fwdActive = $derived(playing && playbackRate > 0);
</script>

<div class="transport">
  <button
    class="t-btn"
    onclick={onSkipStart}
    title="Go to start"
    aria-label="Go to start"
  >
    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 6h2v12H6zm3.5 6 8.5 6V6z"/>
    </svg>
  </button>

  <button
    class="t-btn"
    class:t-btn-active={bwdActive}
    onclick={onRewind}
    title="Rewind (J)"
    aria-label="Rewind"
  >
    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 6h2v12H6zm12 0-8.5 6 8.5 6V6z"/>
    </svg>
  </button>

  <button
    class="t-play"
    onclick={onTogglePlay}
    title="Play / Pause (Space)"
    aria-label={playing ? 'Pause' : 'Play'}
  >
    {#if playing}
      <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
        <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/>
      </svg>
    {:else}
      <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
        <path d="M8 5v14l11-7z"/>
      </svg>
    {/if}
  </button>

  <button
    class="t-btn"
    class:t-btn-active={fwdActive}
    onclick={onForward}
    title="Play forward (L)"
    aria-label="Play forward"
  >
    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 18l8.5-6L6 6v12zm8.5-6 3.5 6V6z"/>
    </svg>
  </button>

  <button
    class="t-btn"
    onclick={onSkipEnd}
    title="Go to end"
    aria-label="Go to end"
  >
    <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor">
      <path d="M6 18l8.5-6L6 6v12zm2.5-6 8.5 6V6zM16 6h2v12h-2z"/>
    </svg>
  </button>

  {#if showFullscreen}
    <div class="t-divider"></div>
    <button
      class="t-btn"
      class:t-btn-active={fullscreen}
      onclick={onToggleFullscreen}
      title="Fullscreen (F)"
      aria-label="Toggle fullscreen"
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/>
      </svg>
    </button>
  {/if}
</div>

<style>
  .transport {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 0 12px;
    height: 36px;
    background: color-mix(in srgb, var(--surface) 70%, var(--surface-deep, #000));
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .t-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
    transition: background 0.1s, color 0.1s;
  }
  .t-btn:hover {
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
    color: var(--text-secondary);
  }
  .t-btn-active {
    color: var(--accent) !important;
  }

  .t-play {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 999px;
    cursor: pointer;
    padding: 0;
    transition: background 0.1s;
  }
  .t-play:hover { background: var(--accent-hover); }

  .t-divider {
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 8px;
  }
</style>
