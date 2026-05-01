<script>
  /**
   * Tabs — tabbed container. Wraps TabBar with associated content panels.
   *
   * Uses TabBar for the tab list (left/right arrow navigation) and renders
   * the matching content panel below it.
   *
   * Props:
   *   tabs        — Array of { id: string, label: string, panel: Snippet }.
   *                 Each panel is a Svelte 5 snippet rendered as the tab content.
   *   activeTab   — bindable string. The id of the active tab.
   *   orientation — 'horizontal' | 'vertical'. Default: 'horizontal'.
   *                 Vertical layout stacks tab list on the left.
   *   class       — extra classes on the container.
   *
   * Usage:
   *   <script>
   *     let active = 'general';
   *   <\/script>
   *
   *   <Tabs bind:activeTab={active} tabs={[
   *     { id: 'general', label: 'General', panel: generalPanel },
   *     { id: 'advanced', label: 'Advanced', panel: advancedPanel },
   *   ]} />
   *
   *   {#snippet generalPanel()}
   *     <p>General settings here</p>
   *   {/snippet}
   *
   * Keyboard (inherited from TabBar for horizontal; handled here for vertical):
   *   ArrowLeft / ArrowRight — horizontal tab navigation
   *   ArrowUp / ArrowDown    — vertical tab navigation
   *   Enter / Space          — activate focused tab
   */

  import TabBar from './TabBar.svelte';

  let {
    tabs = [],
    activeTab = $bindable(''),
    orientation = 'horizontal',
    class: extraClass = '',
  } = $props();

  // For vertical orientation, handle arrow keys ourselves
  const tabListItems = $derived(tabs.map(t => ({ id: t.id, label: t.label })));
  const activePanel = $derived(tabs.find(t => t.id === activeTab)?.panel);

  // Default to first tab if activeTab unset
  $effect(() => {
    if (!activeTab && tabs.length > 0) {
      activeTab = tabs[0].id;
    }
  });

  function handleVerticalKey(e, currentId, index) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      const next = tabs[(index + 1) % tabs.length];
      activeTab = next.id;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = tabs[(index - 1 + tabs.length) % tabs.length];
      activeTab = prev.id;
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      activeTab = currentId;
    } else if (e.key === 'Home') {
      e.preventDefault();
      activeTab = tabs[0].id;
    } else if (e.key === 'End') {
      e.preventDefault();
      activeTab = tabs[tabs.length - 1].id;
    }
  }
</script>

{#if orientation === 'horizontal'}
  <div class="flex flex-col {extraClass}">
    <TabBar
      tabs={tabListItems}
      active={activeTab}
      onSelect={(id) => activeTab = id}
    />
    <div
      role="tabpanel"
      aria-label={tabs.find(t => t.id === activeTab)?.label ?? ''}
      class="flex-1 min-h-0"
    >
      {#if activePanel}
        {@render activePanel()}
      {/if}
    </div>
  </div>

{:else}
  <!-- Vertical layout: tab list on left, panel on right -->
  <div class="flex {extraClass}">
    <div
      role="tablist"
      aria-orientation="vertical"
      class="flex flex-col shrink-0 border-r border-[var(--border)] bg-[var(--surface-raised)] w-36"
    >
      {#each tabs as tab, index}
        <button
          role="tab"
          aria-selected={activeTab === tab.id}
          tabindex={activeTab === tab.id ? 0 : -1}
          onclick={() => activeTab = tab.id}
          onkeydown={(e) => handleVerticalKey(e, tab.id, index)}
          class="px-4 py-2 text-[13px] text-left font-medium border-l-2 transition-colors
                 outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-inset
                 {activeTab === tab.id
                   ? 'border-[var(--accent)] text-[var(--accent)] bg-[var(--surface)]'
                   : 'border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]'}"
        >{tab.label}</button>
      {/each}
    </div>

    <div
      role="tabpanel"
      aria-label={tabs.find(t => t.id === activeTab)?.label ?? ''}
      class="flex-1 min-w-0"
    >
      {#if activePanel}
        {@render activePanel()}
      {/if}
    </div>
  </div>
{/if}
