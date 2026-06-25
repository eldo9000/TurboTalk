<script>
  let {
    open = false,
    closing = false,
    onClose,
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
      if (event.key === 'Enter' || event.key === ' ' || event.key === 'Escape') {
        event.preventDefault();
        onClose?.();
      }
    }}
    role="button"
    tabindex="0"
    aria-label="Close about"
  >
    <div
      class="about-card {closing ? 'about-card-out' : 'about-card-in'}"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <div class="flex flex-col items-center gap-0.5 pb-3">
        <span class="text-[18px] font-semibold tracking-tight text-[var(--text-primary)]">Turbo Talk</span>
        <span class="text-[10px] text-[var(--text-muted)] tabular-nums">v0.9.8</span>
        <p class="text-[var(--text-secondary)] text-[11px] leading-snug mt-1.5 text-center">
          Lightweight voice dictation<br>for getting work done.
        </p>
      </div>
    </div>
  </div>
{/if}
