<script>
  /**
   * Menu — dropdown / context menu anchored to a target element.
   *
   * Props:
   *   open   — bindable boolean. Controls visibility.
   *   anchor — HTMLElement | null. The element to position the menu near.
   *            If null, renders at a fixed position (use x/y props).
   *   x      — fixed x position (px) when anchor is null. Default: 0.
   *   y      — fixed y position (px) when anchor is null. Default: 0.
   *   items  — Array of menu items (see type below).
   *            { label: string, icon?: string, onclick: () => void,
   *              disabled?: boolean, separator?: boolean }
   *            Set separator: true (and omit label/onclick) for a divider row.
   *
   * Usage:
   *   <button bind:this={btnEl} onclick={() => open = !open}>Options</button>
   *   <Menu bind:open items={menuItems} anchor={btnEl} />
   *
   * Keyboard:
   *   ArrowDown / ArrowUp — move focus between items
   *   Enter / Space       — activate focused item
   *   Esc                 — close menu
   *   Home / End          — jump to first / last item
   */

  let {
    open = $bindable(false),
    anchor = null,
    x = 0,
    y = 0,
    items = [],
  } = $props();

  let menuEl = $state(null);
  let focusedIndex = $state(-1);

  // Compute activatable item indices (skip separators + disabled)
  const activeIndices = $derived(
    items.reduce((acc, item, i) => {
      if (!item.separator && !item.disabled) acc.push(i);
      return acc;
    }, [])
  );

  // Position relative to anchor
  let menuStyle = $derived.by(() => {
    if (!anchor) return `left:${x}px; top:${y}px;`;
    const rect = anchor.getBoundingClientRect();
    return `left:${rect.left}px; top:${rect.bottom + 4}px;`;
  });

  function close() {
    open = false;
    focusedIndex = -1;
    anchor?.focus();
  }

  function activateItem(item) {
    if (item.disabled || item.separator) return;
    item.onclick?.();
    close();
  }

  function handleKeydown(e) {
    if (!open) return;
    const navKeys = ['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter', ' ', 'Escape', 'Tab'];
    if (!navKeys.includes(e.key)) return;
    e.preventDefault();

    if (e.key === 'Escape' || e.key === 'Tab') {
      close();
      return;
    }

    const pos = activeIndices.indexOf(focusedIndex);
    if (e.key === 'ArrowDown' || e.key === 'Home') {
      const next = e.key === 'Home' ? 0 : (pos + 1) % activeIndices.length;
      focusedIndex = activeIndices[next];
    } else if (e.key === 'ArrowUp' || e.key === 'End') {
      const prev = e.key === 'End'
        ? activeIndices.length - 1
        : (pos - 1 + activeIndices.length) % activeIndices.length;
      focusedIndex = activeIndices[prev];
    } else if (e.key === 'Enter' || e.key === ' ') {
      if (focusedIndex >= 0) activateItem(items[focusedIndex]);
    }
  }

  // Close on outside click
  function handleOutsideClick(e) {
    if (menuEl && !menuEl.contains(e.target) && e.target !== anchor) {
      close();
    }
  }

  $effect(() => {
    if (open) {
      // Focus first item on open
      focusedIndex = activeIndices[0] ?? -1;
      document.addEventListener('mousedown', handleOutsideClick);
      return () => document.removeEventListener('mousedown', handleOutsideClick);
    } else {
      document.removeEventListener('mousedown', handleOutsideClick);
    }
  });

  // Focus the item button when focusedIndex changes
  $effect(() => {
    if (focusedIndex >= 0 && menuEl) {
      const btn = menuEl.querySelector(`[data-menu-index="${focusedIndex}"]`);
      btn?.focus();
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  onkeydown={handleKeydown}
  style="position:fixed; z-index:200; {menuStyle}"
>
  {#if open}
    <div
      bind:this={menuEl}
      role="menu"
      aria-label="Menu"
      class="min-w-[160px] bg-[var(--surface)] border border-[var(--border)]
             rounded-lg shadow-xl py-1 animate-fade-in"
    >
      {#each items as item, i}
        {#if item.separator}
          <div class="my-1 border-t border-[var(--border)]" role="separator"></div>
        {:else}
          <button
            role="menuitem"
            data-menu-index={i}
            disabled={item.disabled}
            tabindex={focusedIndex === i ? 0 : -1}
            onclick={() => activateItem(item)}
            class="w-full flex items-center gap-2 px-3 py-1.5 text-[13px] text-left
                   transition-colors outline-none
                   {item.disabled
                     ? 'text-[var(--text-secondary)] opacity-50 cursor-not-allowed'
                     : 'text-[var(--text-primary)] hover:bg-[var(--surface-raised)] cursor-default'}
                   focus:bg-[var(--surface-raised)]"
          >
            {#if item.icon}
              <!-- eslint-disable-next-line svelte/no-at-html-tags -->
              <span class="shrink-0 text-[var(--text-secondary)]">{@html item.icon}</span>
            {/if}
            {item.label}
          </button>
        {/if}
      {/each}
    </div>
  {/if}
</div>
