<script>
  /**
   * Toaster — singleton toast manager.
   *
   * Place once inside your WindowFrame. Call the exported toast() function
   * from anywhere to show a notification. Toasts auto-dismiss after 4s.
   *
   * Usage:
   *   In App.svelte:
   *     import { Toaster } from '@libre/ui';
   *     <WindowFrame>
   *       <Toaster bind:this={toaster} />
   *       ...
   *     </WindowFrame>
   *
   *   To show a toast:
   *     toaster.show({ message: 'File saved', detail: 'my-file.md' })
   *     toaster.show({ message: 'Error', detail: 'Something went wrong', variant: 'error' })
   *
   * Toast shape: { message: string, detail?: string, variant?: 'default'|'success'|'error' }
   */

  import { onDestroy } from 'svelte';

  let current = $state(null); // { message, detail, variant } | null
  let timer = null;

  export function show(toast, duration = 4000) {
    if (timer) clearTimeout(timer);
    current = toast;
    timer = setTimeout(() => { current = null; }, duration);
  }

  export function dismiss() {
    if (timer) clearTimeout(timer);
    current = null;
  }

  onDestroy(() => { if (timer) clearTimeout(timer); });

  const iconColor = {
    default: 'text-[var(--accent)]',
    success: 'text-green-500',
    error:   'text-red-500',
  };
</script>

{#if current}
  <div
    role="status"
    aria-live="polite"
    aria-atomic="true"
    class="absolute bottom-4 right-4 z-50 flex items-center gap-3 px-4 py-3
           bg-[var(--surface)] border border-[var(--border)]
           rounded-xl shadow-lg text-sm max-w-xs animate-fade-in"
  >
    <!-- Icon: success checkmark, error X, default info dot -->
    {#if current.variant === 'success'}
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
           stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
           class="shrink-0 {iconColor.success}">
        <polyline points="20 6 9 17 4 12"/>
      </svg>
    {:else if current.variant === 'error'}
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
           stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
           class="shrink-0 {iconColor.error}">
        <circle cx="12" cy="12" r="10"/>
        <line x1="12" y1="8" x2="12" y2="12"/>
        <line x1="12" y1="16" x2="12.01" y2="16"/>
      </svg>
    {:else}
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
           stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
           class="shrink-0 {iconColor.default}">
        <circle cx="12" cy="12" r="10"/>
        <line x1="12" y1="8" x2="12" y2="12"/>
        <line x1="12" y1="16" x2="12.01" y2="16"/>
      </svg>
    {/if}

    <div class="min-w-0">
      <div class="font-medium text-[var(--text-primary)] truncate">{current.message}</div>
      {#if current.detail}
        <div class="text-[11px] text-[var(--text-secondary)] truncate">{current.detail}</div>
      {/if}
    </div>

    <button
      onclick={dismiss}
      aria-label="Dismiss notification"
      class="shrink-0 w-5 h-5 flex items-center justify-center rounded
             text-[var(--text-secondary)] hover:text-[var(--text-primary)]
             hover:bg-[var(--surface-raised)] transition-colors text-[13px] ml-1"
    >×</button>
  </div>
{/if}
