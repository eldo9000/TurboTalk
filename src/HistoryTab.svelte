<script>
  let {
    history, copiedTs, transcriptError, transcriptNotice,
    filteredEntry, recording, transcribing, hotkeyLabel, cfgHotkeyMode,
    actions,
  } = $props();

  function fadeIfOverflow(node) {
    function check() {
      node.classList.toggle('tt-history-text-fade', node.scrollHeight > node.clientHeight);
    }
    check();
    const ro = new ResizeObserver(check);
    ro.observe(node);
    return { destroy() { ro.disconnect(); } };
  }
</script>

<div class="tt-history flex-1 min-h-0 flex flex-col">
  {#if transcriptError}
    <div class="tt-banner-error">
      <span class="tt-banner-error-msg">{transcriptError}</span>
      <button onclick={actions.dismissTranscriptError} class="tt-banner-close">×</button>
    </div>
  {/if}
  {#if transcriptNotice}
    <div class="tt-banner-notice">
      <span class="tt-banner-notice-msg">{transcriptNotice}</span>
      <button onclick={actions.dismissTranscriptNotice} class="tt-banner-notice-close">×</button>
    </div>
  {/if}
  {#if filteredEntry}
    <div class="tt-banner-error" style="border-color: var(--warning, #c97d00); background: var(--warning-bg, #fff8e0);">
      <span style="font-size: 0.72rem; font-weight: 600; color: var(--warning, #c97d00);">⚠ Filtered: {filteredEntry.reason}</span>
      <button onclick={actions.dismissFilteredEntry} class="tt-banner-close">×</button>
    </div>
  {/if}
  {#if history.length === 0}
    <div class="tt-history-empty">
      {#if recording || transcribing}
        <p class="tt-history-empty-status">{recording ? 'Recording…' : 'Transcribing…'}</p>
      {:else}
        <kbd class="tt-kbd">{hotkeyLabel}</kbd>
        <p class="tt-history-empty-hint">
          {cfgHotkeyMode === 'toggle' ? 'Press to start · press again to stop'
            : cfgHotkeyMode === 'auto' ? 'Tap to start · tap again to stop · hold to talk'
            : 'Hold to record'}
        </p>
      {/if}
    </div>
  {:else}
    <div class="tt-history-list">
      {#each history as item (item.ts)}
        <button
          onclick={() => actions.copyHistoryItem(item)}
          title="Click to copy"
          class="tt-history-item"
          class:tt-history-item-flaky={item.flaky}
        >
          <span use:fadeIfOverflow class="tt-history-text" class:tt-history-text-hidden={copiedTs === item.ts}>
            {item.text}
          </span>
          {#if copiedTs === item.ts}
            <span class="tt-history-copied">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="20 6 9 17 4 12"/>
              </svg>
              Copied
            </span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
  <div class="tt-history-actions">
    <button
      onclick={actions.toggleRecording}
      disabled={transcribing}
      title="Record into the history list. Transcript stays here — won't paste into another app."
      class="tt-btn tt-btn-icon"
      class:tt-btn-recording={recording}
    >
      <span class="tt-rec-dot"></span>
      {recording ? 'Stop' : 'Record'}
    </button>
    {#if history.length > 0}
      <button onclick={actions.clearHistory} class="tt-btn tt-btn-icon tt-btn-danger-hover">
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"/>
        </svg>
        Clear all
      </button>
    {/if}
  </div>
</div>
