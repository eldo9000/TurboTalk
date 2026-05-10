<script>
  /**
   * SegmentedControl — pill-style toggle group for mutually-exclusive choices.
   *
   * Variants:
   *   filled    — Fade-style. Connected segments using the shared
   *               .seg-active / .seg-inactive classes from tokens.css. Active
   *               option is darker accent fill with a 2px underline (10px
   *               gutters); inactive options have a subtle bevel gradient.
   *               Markup mirrors Fade's seg(active, i, total) helper.
   *   underline — Compact transparent pills with a thin accent underline on
   *               the active option. Use in dense forms where the filled
   *               variant would feel heavy.
   *   sliding   — Raised tile that animates between options (Fade CompTab
   *               zoom-mode pattern).
   *   tinted    — Connected bordered segments; active option gets an
   *               accent-tinted background + border (TurboTalk theme-selector
   *               pattern). Good for compact settings UIs.
   */

  let {
    options = [],
    value = $bindable(),
    variant = 'filled',
    size = 'md',
    onchange,
    class: extraClass = '',
  } = $props();

  const sizes = {
    sm: { h: 22, px: 'px-2.5', py: 'py-[3px]', text: 'text-[11px]' },
    md: { h: 28, px: 'px-3',   py: 'py-[5px]', text: 'text-[12px]' },
  };
  const s = $derived(sizes[size] ?? sizes.md);

  const activeIndex = $derived(options.findIndex(o => o.value === value));

  // Sliding-variant pill geometry is driven by measured DOM offsets so
  // labels of different lengths align exactly (calc-based equal-slot
  // positioning falls apart when `flex: 1` doesn't actually equalize
  // widths inside an `inline-flex` container).
  let slidingTabRefs = $state([]);
  let slidingPillStyle = $state('');

  $effect(() => {
    if (variant !== 'sliding') { slidingPillStyle = ''; return; }
    const i = activeIndex;
    const el = slidingTabRefs[i];
    if (i < 0 || !el) { slidingPillStyle = ''; return; }
    slidingPillStyle = `width: ${el.offsetWidth}px; left: ${el.offsetLeft}px;`;
  });

  function pick(v) {
    value = v;
    onchange?.(v);
  }
</script>

{#if variant === 'tinted'}
  <div class="inline-flex {extraClass}">
    {#each options as opt, i}
      {@const active = opt.value === value}
      {@const first = i === 0}
      {@const last = i === options.length - 1}
      <button
        type="button"
        onclick={() => pick(opt.value)}
        class="relative flex-1 flex items-center justify-center
               font-semibold tracking-[0.04em] whitespace-nowrap cursor-pointer
               transition-[background,color,border-color] duration-100
               {i > 0 ? '-ml-px' : ''}
               {active
                 ? 'z-10 text-[var(--text-primary)]'
                 : 'z-[1] text-[var(--text-muted)] hover:text-[var(--text-secondary)] hover:z-[2]'}"
        style="height:{s.h}px;
               padding:0 {size === 'sm' ? '6px' : '10px'};
               font-size:{size === 'sm' ? '9px' : '11px'};
               background:{active ? 'color-mix(in srgb,var(--accent) 18%,var(--surface-raised))' : 'var(--surface-raised)'};
               border:1px solid {active ? 'color-mix(in srgb,var(--accent) 40%,var(--border))' : 'var(--border)'};
               border-radius:{first ? '4px 0 0 4px' : last ? '0 4px 4px 0' : '0'};"
      >
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        {#if opt.icon}{@html opt.icon}{/if}
        {opt.label}
      </button>
    {/each}
  </div>
{:else if variant === 'sliding'}
  <div
    class="relative inline-flex rounded shrink-0 {extraClass}"
    style="background:rgb(0 0 0 / 0.22); padding:2px; height:{s.h}px;"
  >
    {#if slidingPillStyle}
      <div
        class="absolute inset-y-[2px] rounded pointer-events-none transition-all duration-150 ease-out"
        style="{slidingPillStyle}
               background:color-mix(in srgb, white 18%, var(--surface-raised));
               box-shadow:0 1px 3px rgb(0 0 0 / 35%);"
      ></div>
    {/if}
    {#each options as opt, i}
      <button
        bind:this={slidingTabRefs[i]}
        type="button"
        onclick={() => pick(opt.value)}
        class="relative z-10 flex items-center justify-center gap-1 {s.px}
               {s.text} font-medium transition-colors duration-100
               {opt.value === value
                 ? 'text-[var(--text-primary)]'
                 : 'text-[var(--text-muted)] hover:text-[var(--text-secondary)]'}"
      >
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        {#if opt.icon}{@html opt.icon}{/if}
        {opt.label}
      </button>
    {/each}
  </div>
{:else if variant === 'filled'}
  <div class="inline-flex {extraClass}">
    {#each options as opt, i}
      {@const total = options.length}
      {@const round = i === 0 ? 'rounded-l-md' : (i === total - 1 ? 'rounded-r-md' : '')}
      {@const overlap = i > 0 ? '-ml-px' : ''}
      {@const active = opt.value === value}
      <button
        type="button"
        onclick={() => pick(opt.value)}
        class="{s.px} {s.py} text-center {s.text} font-medium border transition-colors relative
               {round} {overlap}
               {active
                 ? 'seg-active z-10'
                 : 'seg-inactive border-[var(--border)] text-[color-mix(in_srgb,var(--text-primary)_70%,transparent)] hover:z-10'}"
      >
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        {#if opt.icon}{@html opt.icon}{/if}
        {opt.label}
      </button>
    {/each}
  </div>
{:else}
  <div
    class="inline-flex rounded border border-[var(--border)] {extraClass}"
    style="background:var(--surface-deep, var(--surface-raised)); height:{s.h}px;"
  >
    {#each options as opt}
      <button
        type="button"
        onclick={() => pick(opt.value)}
        class="relative {s.px} {s.text} font-medium flex items-center transition-colors duration-100
               {opt.value === value
                 ? 'text-[var(--text-primary)]'
                 : 'text-[var(--text-muted)] hover:text-[var(--text-secondary)]'}"
      >
        {opt.label}
        {#if opt.value === value}
          <span class="absolute bottom-0 left-1 right-1 h-[2px] rounded-t bg-[var(--accent)]"></span>
        {/if}
      </button>
    {/each}
  </div>
{/if}
