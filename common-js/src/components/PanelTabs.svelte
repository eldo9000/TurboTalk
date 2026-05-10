<script>
  /**
   * PanelTabs — secondary text-only tab strip with an accent underline.
   *
   * Pattern lifted from Flicker-App's Edit-page panel headers (Media /
   * Effects / Source). Visually quiet by design: no background, no border
   * around the group, no fill on the active tab — just a 2px accent
   * border on the bottom of the active tab to mark selection. Use for
   * sub-navigation inside a panel where heavier tab styling would compete
   * with the panel's own chrome.
   *
   * @typedef {{ id: string, label: string }} Tab
   * @typedef {{
   *   tabs?: Tab[],
   *   active?: string,
   *   onSelect?: (id: string) => void,
   *   ariaLabel?: string,
   * }} Props
   *
   * @type {Props}
   */
  let {
    tabs = [],
    active = $bindable(''),
    onSelect,
    ariaLabel = 'Section',
  } = $props();

  function pick(id) {
    active = id;
    onSelect?.(id);
  }
</script>

<div class="pt-group" role="tablist" aria-label={ariaLabel}>
  {#each tabs as t}
    <button
      role="tab"
      aria-selected={active === t.id}
      class="pt-tab"
      class:pt-tab-active={active === t.id}
      onclick={() => pick(t.id)}
    >
      {t.label}
    </button>
  {/each}
</div>

<style>
  .pt-group {
    display: inline-flex;
    align-items: stretch;
    height: 28px;
    gap: 0;
  }
  .pt-tab {
    height: 100%;
    padding: 0 12px;
    display: flex;
    align-items: center;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 0.1s, border-color 0.1s;
  }
  .pt-tab:hover {
    color: var(--text-secondary);
  }
  .pt-tab-active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-bright, var(--accent));
  }
</style>
