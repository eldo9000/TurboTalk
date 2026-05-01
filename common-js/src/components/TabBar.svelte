<script>
  /**
   * TabBar — horizontal content tabs for Libre apps.
   *
   * Renders a row of tabs with an active underline indicator.
   * Keyboard accessible: left/right arrows to navigate between tabs.
   *
   * Props:
   *   tabs     — Array of { id: string, label: string }. Required.
   *   active   — The id of the active tab. Required.
   *   onSelect — Callback fired when a tab is selected: (id: string) => void. Required.
   *   class    — Optional extra classes for the container.
   *
   * Usage:
   *   <TabBar
   *     tabs={[{id:'image',label:'Image'},{id:'video',label:'Video'}]}
   *     active={activeTab}
   *     onSelect={(id) => activeTab = id}
   *   />
   */

  let { tabs = [], active = '', onSelect, class: extraClass = '' } = $props();

  function handleKeydown(e, id, index) {
    if (e.key === 'ArrowRight') {
      const next = tabs[(index + 1) % tabs.length];
      onSelect?.(next.id);
    } else if (e.key === 'ArrowLeft') {
      const prev = tabs[(index - 1 + tabs.length) % tabs.length];
      onSelect?.(prev.id);
    } else if (e.key === 'Enter' || e.key === ' ') {
      onSelect?.(id);
    }
  }
</script>

<div
  role="tablist"
  class="flex border-b border-[var(--border)] bg-[var(--surface-raised)] shrink-0 {extraClass}"
>
  {#each tabs as tab, index}
    <button
      role="tab"
      aria-selected={active === tab.id}
      tabindex={active === tab.id ? 0 : -1}
      onclick={() => onSelect?.(tab.id)}
      onkeydown={(e) => handleKeydown(e, tab.id, index)}
      class="px-5 py-2 text-[13px] font-medium border-b-2 transition-colors outline-none
             focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-inset
             {active === tab.id
               ? 'border-[var(--accent)] text-[var(--accent)]'
               : 'border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}"
    >{tab.label}</button>
  {/each}
</div>
