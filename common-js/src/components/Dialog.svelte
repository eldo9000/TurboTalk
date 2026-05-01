<script>
  /**
   * Dialog — modal dialog with backdrop, focus trap, and escape-to-close.
   *
   * Props:
   *   open    — bindable boolean. Controls visibility.
   *   title   — string. Dialog heading text.
   *   size    — 'sm' | 'md' | 'lg'. Default: 'md'.
   *   onclose — Callback fired when the dialog requests closing.
   *   children — Dialog body content.
   *   footer  — Optional footer slot (action buttons).
   *
   * Usage:
   *   <Dialog bind:open={showDialog} title="Confirm Delete" onclose={() => showDialog = false}>
   *     <p>Are you sure?</p>
   *     {#snippet footer()}
   *       <Button variant="secondary" onclick={() => showDialog = false}>Cancel</Button>
   *       <Button onclick={handleConfirm}>Delete</Button>
   *     {/snippet}
   *   </Dialog>
   *
   * Keyboard:
   *   Esc   — closes the dialog
   *   Tab   — traps focus inside the dialog
   */

  let {
    open = $bindable(false),
    title = '',
    size = 'md',
    onclose,
    children,
    footer,
  } = $props();

  let dialogEl = $state(null);

  const sizes = {
    sm: 'w-80',
    md: 'w-[480px]',
    lg: 'w-[640px]',
  };

  function close() {
    open = false;
    onclose?.();
  }

  function handleBackdropClick(e) {
    if (e.target === e.currentTarget) close();
  }

  function handleKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'Tab' && dialogEl) {
      const focusable = dialogEl.querySelectorAll(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        }
      } else {
        if (document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    }
  }

  // Focus the dialog when it opens; restore focus when it closes
  let previouslyFocused = $state(null);

  $effect(() => {
    if (open) {
      previouslyFocused = document.activeElement;
      // Defer so the DOM is rendered
      requestAnimationFrame(() => {
        const focusable = dialogEl?.querySelector(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
        );
        focusable?.focus();
      });
    } else {
      previouslyFocused?.focus();
    }
  });
</script>

{#if open}
  <!-- Backdrop -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center p-4"
    style="background: rgb(0 0 0 / 40%)"
    onclick={handleBackdropClick}
    onkeydown={handleKeydown}
    role="presentation"
  >
    <!-- Dialog panel -->
    <div
      bind:this={dialogEl}
      role="dialog"
      aria-modal="true"
      aria-labelledby="dialog-title"
      class="relative flex flex-col bg-[var(--surface)] border border-[var(--border)]
             rounded-lg shadow-xl max-h-[80vh] {sizes[size] ?? sizes.md}
             animate-fade-in"
    >
      <!-- Header -->
      <div class="flex items-center justify-between px-5 py-3 border-b border-[var(--border)] shrink-0">
        <h2 id="dialog-title" class="text-[14px] font-semibold text-[var(--text-primary)]">
          {title}
        </h2>
        <button
          onclick={close}
          class="w-7 h-7 flex items-center justify-center rounded text-[var(--text-secondary)]
                 hover:text-[var(--text-primary)] hover:bg-[var(--surface-raised)] transition-colors
                 text-[18px] leading-none"
          aria-label="Close dialog"
        >×</button>
      </div>

      <!-- Body -->
      <div class="flex-1 min-h-0 overflow-y-auto px-5 py-4 text-[13px] text-[var(--text-primary)]">
        {@render children?.()}
      </div>

      <!-- Footer -->
      {#if footer}
        <div class="flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--border)] shrink-0">
          {@render footer?.()}
        </div>
      {/if}
    </div>
  </div>
{/if}
