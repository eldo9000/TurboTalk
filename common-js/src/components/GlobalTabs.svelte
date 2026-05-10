<script>
  /**
   * GlobalTabs — top-bar workspace tab selector with a sliding active pill.
   *
   * Tabs sit in a darkened rounded backdrop. A single pill animates between
   * positions (150ms ease-out). The accent underline lives inside the pill
   * so it slides with it instead of swapping at the endpoints.
   *
   * Pill geometry is driven by measured `offsetLeft` / `offsetWidth` of the
   * active tab — not a calc-based equal-slot assumption — so labels with
   * different lengths (e.g. "Overview" vs "Components") still align.
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
    ariaLabel = 'Workspace',
  } = $props();

  const activeIndex = $derived(tabs.findIndex(t => t.id === active));

  let tabRefs = $state([]);
  let pillStyle = $state('');

  $effect(() => {
    // Re-measure on activeIndex / tabs changes. Also re-runs when refs
    // populate after first render.
    const i = activeIndex;
    const el = tabRefs[i];
    if (i < 0 || !el) {
      pillStyle = '';
      return;
    }
    pillStyle = `width: ${el.offsetWidth}px; left: ${el.offsetLeft}px;`;
  });

  function pick(id) {
    active = id;
    onSelect?.(id);
  }
</script>

<div class="gt-group" role="tablist" aria-label={ariaLabel}>
  {#if pillStyle}
    <div class="gt-pill" style={pillStyle}>
      <span class="gt-underline"></span>
    </div>
  {/if}
  {#each tabs as t, i}
    <button
      bind:this={tabRefs[i]}
      role="tab"
      aria-selected={active === t.id}
      class="gt-tab"
      class:gt-tab-active={active === t.id}
      onclick={() => pick(t.id)}
    >
      {t.label}
    </button>
  {/each}
</div>

<style>
  .gt-group {
    position: relative;
    display: inline-flex;
    align-items: stretch;
    height: 28px;
    padding: 2px;
    border-radius: 6px;
    background: rgb(0 0 0 / 0.22);
  }
  .gt-pill {
    position: absolute;
    top: 2px;
    bottom: 2px;
    border-radius: 4px;
    background: color-mix(in srgb, white 18%, var(--surface-raised));
    box-shadow: 0 1px 3px rgb(0 0 0 / 0.35);
    pointer-events: none;
    transition: left 0.15s ease-out, width 0.15s ease-out;
  }
  .gt-underline {
    position: absolute;
    bottom: 0;
    left: 10px;
    right: 10px;
    height: 2px;
    border-radius: 1px 1px 0 0;
    background: var(--accent);
  }
  .gt-tab {
    position: relative;
    z-index: 10;
    height: 100%;
    padding: 0 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 0;
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 0.1s;
    white-space: nowrap;
  }
  .gt-tab:hover {
    color: var(--text-secondary);
  }
  .gt-tab-active {
    color: var(--text-primary);
  }
</style>
