<script>
  let {
    ZOOM_LEVELS = [],
    zoomIdx = $bindable(),
    tipText = '',
    onZoomIn,
    onZoomOut,
    onAboutOpen,
  } = $props();
</script>

<div class="shrink-0 h-7 flex items-center justify-between px-2
            select-none relative">
  <div class="flex items-center gap-1">
    <button
      onclick={onZoomOut}
      disabled={zoomIdx === 0}
      class="w-5 h-5 flex items-center justify-center rounded text-xs
             text-[var(--text-tertiary,#666)] hover:text-[var(--text-secondary)]
             disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
    >−</button>
    <button
      onclick={() => { zoomIdx = 0; }}
      title="Reset zoom"
      class="text-[10px] w-8 text-center text-[var(--text-tertiary,#666)] tabular-nums
             hover:text-[var(--text-secondary)] transition-colors"
    >{ZOOM_LEVELS[zoomIdx]}%</button>
    <button
      onclick={onZoomIn}
      disabled={zoomIdx === ZOOM_LEVELS.length - 1}
      class="w-5 h-5 flex items-center justify-center rounded text-xs
             text-[var(--text-tertiary,#666)] hover:text-[var(--text-secondary)]
             disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
    >+</button>
  </div>
  {#if tipText}
    <span class="tt-footer-tip">{tipText}</span>
  {/if}
  <button
    onclick={onAboutOpen}
    class="text-[10px] text-[var(--text-tertiary,#666)] hover:text-[var(--text-secondary)]
           transition-colors"
  >about</button>
</div>
