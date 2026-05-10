<script>
  let { state = 'gray', variant = 'filled', size = 6 } = $props();

  const COLORS = {
    red:    '#d45550',
    green:  '#45b554',
    yellow: '#c49a28',
    gray:   '#6e7681',
  };

  const color = $derived(COLORS[state] ?? COLORS.gray);
  const r = $derived(size / 2);
</script>

{#if variant === 'outline'}
  <svg
    width={size}
    height={size}
    viewBox="0 0 {size} {size}"
    style="display:inline-block; flex-shrink:0; vertical-align:middle; overflow:visible"
  >
    <circle cx={r} cy={r} r={r - 0.5} fill="#6e7681" stroke={color} stroke-width="1" />
  </svg>
{:else}
  <span
    class="traffic-light"
    style="--tl-size:{size}px; --tl-color:{color}"
  ></span>
{/if}

<style>
  .traffic-light {
    display: inline-block;
    width: var(--tl-size);
    height: var(--tl-size);
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--tl-color);
  }
</style>
