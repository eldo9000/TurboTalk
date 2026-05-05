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
  const s = sizes[size] ?? sizes.md;

  const activeIndex = $derived(options.findIndex(o => o.value === value));

  function pick(v) {
    value = v;
    onchange?.(v);
  }
</script>

{#if variant === 'sliding'}
  <div
    class="relative inline-flex rounded shrink-0 {extraClass}"
    style="background:var(--surface-deep, var(--surface-raised)); padding:2px; height:{s.h}px;"
  >
    {#if activeIndex >= 0 && options.length > 0}
      <div
        class="absolute inset-y-[2px] rounded pointer-events-none transition-all duration-150 ease-out"
        style="width:calc((100% - 4px) / {options.length});
               left:calc(2px + {activeIndex} * (100% - 4px) / {options.length});
               background:var(--surface-raised);
               box-shadow:0 1px 3px rgba(0,0,0,0.35);"
      ></div>
    {/if}
    {#each options as opt}
      <button
        type="button"
        onclick={() => pick(opt.value)}
        class="relative z-10 flex-1 flex items-center justify-center gap-1 {s.px}
               {s.text} font-medium transition-colors duration-100
               {opt.value === value
                 ? 'text-[var(--text-primary)]'
                 : 'text-[var(--text-muted)] hover:text-[var(--text-secondary)]'}"
      >
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
