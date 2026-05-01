<script>
  /**
   * Toast — ephemeral notification message.
   *
   * Renders as an absolutely positioned card in the bottom-right corner.
   * Animates in via the fadeIn keyframe defined in tokens.css (ghost/app.css).
   * The parent is responsible for showing/hiding it (bind a boolean or null).
   *
   * Props:
   *   children — Toast content (slot). Include icon + text layout.
   *   class    — Extra classes.
   *
   * Usage:
   *   {#if toast}
   *     <Toast>
   *       <svg ...></svg>
   *       <div>{toast.message}</div>
   *     </Toast>
   *   {/if}
   *
   * For the full toaster singleton pattern, use Toaster + toast() instead.
   */

  let { class: extraClass = '', children } = $props();
</script>

<div
  role="status"
  aria-live="polite"
  aria-atomic="true"
  class="absolute bottom-4 right-4 z-50 flex items-center gap-3 px-4 py-3
         bg-[var(--surface)] border border-[var(--border)]
         rounded-xl shadow-lg text-sm max-w-xs animate-fade-in {extraClass}"
>
  {@render children?.()}
</div>
