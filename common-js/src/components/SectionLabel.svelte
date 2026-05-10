<script>
  /**
   * SectionLabel — uppercase tracking-widest muted label for form sections.
   *
   * Props:
   *   suffix   — Optional text shown after an em-dash (e.g. "BITRATE — KBPS").
   *   for      — If set, renders as <label for=...> instead of <p>.
   *   size     — 'xs' (10px) | 'sm' (11px). Default: 'sm'.
   *   class    — Extra classes.
   *   children — Label content (the main word).
   */

  let {
    suffix = '',
    for: htmlFor = '',
    size = 'sm',
    class: extraClass = '',
    children,
  } = $props();

  const baseClass = $derived(
    `${size === 'xs' ? 'text-[10px]' : 'text-[11px]'} font-semibold uppercase tracking-widest text-[var(--text-muted)] ${extraClass}`
  );
</script>

{#if htmlFor}
  <label for={htmlFor} class={baseClass}>
    {@render children?.()}{#if suffix} <span class="text-[var(--text-muted)]/70">— {suffix}</span>{/if}
  </label>
{:else}
  <p class={baseClass}>
    {@render children?.()}{#if suffix} <span class="text-[var(--text-muted)]/70">— {suffix}</span>{/if}
  </p>
{/if}
