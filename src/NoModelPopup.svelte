<script>
  let {
    open = false,
    closing = false,
    onClose,
    onOpenModels,
  } = $props();
</script>

{#if open}
  <div
    class="about-backdrop {closing ? 'about-backdrop-out' : 'about-backdrop-in'}"
    onclick={(event) => {
      if (event.target === event.currentTarget) {
        onClose?.();
      }
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose?.();
      }
    }}
    role="button"
    tabindex="0"
    aria-label="Close no-model alert"
  >
    <div
      class="about-card no-model-card {closing ? 'about-card-out' : 'about-card-in'}"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <div class="flex flex-col items-center gap-2 pb-3">
        <span class="no-model-icon">⚠</span>
        <span class="no-model-title">NO MODEL INSTALLED</span>
        <p class="no-model-body">
          TurboTalk needs a transcription model before it can transcribe. Download one in the Models tab to get started.
        </p>
      </div>
      <div class="flex flex-col gap-2 pt-2">
        <button
          onclick={() => { onClose?.(); onOpenModels?.(); }}
          class="no-model-cta"
        >
          Open Models
        </button>
        <button
          onclick={onClose}
          class="no-model-dismiss"
        >
          Dismiss
        </button>
      </div>
    </div>
  </div>
{/if}
