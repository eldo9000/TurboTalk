<script>
  /**
   * Select — custom dropdown with optional category headers.
   *
   * Markup ported verbatim from Fade-App/src/lib/VideoOptions.svelte
   * (the Video Codec / Encode Preset dropdowns) so the visual treatment
   * stays identical.
   *
   * Items are passed as a flat array. An entry with `category` renders as
   * an uppercase muted section header (non-selectable). An entry with
   * `value` renders as a selectable option row.
   *
   * Props:
   *   items     — Array of { value, label } | { category: string }.
   *   value     — Selected value (use bind:value).
   *   placeholder — Text shown when no value matches. Default: 'Select…'.
   *   disabled  — boolean.
   *   onchange  — Called with the new value after selection.
   *   class     — Extra classes on the trigger.
   */

  let {
    items = [],
    value = $bindable(),
    placeholder = 'Select…',
    disabled = false,
    onchange,
    class: extraClass = '',
  } = $props();

  let open = $state(false);

  const selectedLabel = $derived(
    items.find(it => it && !it.category && it.value === value)?.label ?? placeholder
  );

  function toggle() {
    if (disabled) return;
    open = !open;
  }

  function pick(v) {
    value = v;
    onchange?.(v);
    open = false;
  }
</script>

<div class="relative">
  <button
    type="button"
    onclick={toggle}
    {disabled}
    aria-haspopup="listbox"
    aria-expanded={open}
    class="w-full flex items-center justify-between px-3 py-[5px] rounded-md border
           border-[var(--border)] seg-inactive text-[var(--text-primary)] text-[13px]
           transition-colors disabled:opacity-40 disabled:cursor-not-allowed
           {extraClass}"
  >
    <span class="text-[12px] truncate text-left">{selectedLabel}</span>
    <svg
      class="w-3.5 h-3.5 text-[var(--text-secondary)] shrink-0 transition-transform {open ? 'rotate-180' : ''}"
      viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    >
      <path d="M4 6l4 4 4-4"/>
    </svg>
  </button>

  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="fixed inset-0 z-40" onmousedown={() => open = false}></div>
    <div
      role="listbox"
      class="absolute left-0 right-0 top-full mt-1 z-50
             bg-[var(--surface-panel)] border border-[var(--border)] rounded-lg shadow-xl py-1 animate-fade-in"
    >
      {#each items as item}
        {#if item.category}
          <div class="px-3 pt-2 pb-0.5 text-[10px] font-medium uppercase tracking-wider
                      text-[var(--text-secondary)] select-none">{item.category}</div>
        {:else}
          <button
            type="button"
            role="option"
            aria-selected={item.value === value}
            onmousedown={(e) => { e.stopPropagation(); pick(item.value); }}
            class="w-full text-left px-3 py-[5px] text-[13px] transition-colors cursor-default outline-none
                   hover:bg-[var(--surface-raised)] hover:text-[var(--text-primary)]
                   text-[color-mix(in_srgb,var(--text-primary)_80%,transparent)]"
          >{item.label}</button>
        {/if}
      {/each}
    </div>
  {/if}
</div>
