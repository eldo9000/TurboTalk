<script>
  /**
   * Button — standard Libre button with three variants.
   *
   * Props:
   *   variant  — 'primary' | 'secondary' | 'ghost'. Default: 'primary'.
   *   disabled — boolean. Default: false.
   *   type     — HTML button type. Default: 'button'.
   *   class    — Extra classes.
   *   onclick  — Click handler.
   *   children — Button content (slot).
   *
   * Usage:
   *   <Button onclick={save}>Save</Button>
   *   <Button variant="secondary" onclick={cancel}>Cancel</Button>
   *   <Button variant="ghost" disabled={true}>Disabled</Button>
   */

  let {
    variant = 'primary',
    disabled = false,
    type = 'button',
    class: extraClass = '',
    onclick,
    children,
  } = $props();

  const variants = {
    primary:   'bg-[var(--accent)] text-white hover:bg-[var(--accent-hover)] disabled:bg-[var(--border)] disabled:text-[var(--text-secondary)]',
    secondary: 'border border-[var(--border)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:border-[var(--accent)] disabled:opacity-50',
    ghost:     'text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--surface-raised)] disabled:opacity-50',
  };
</script>

<button
  {type}
  {disabled}
  {onclick}
  class="px-4 py-1.5 rounded-md text-[13px] font-medium transition-colors
         cursor-pointer disabled:cursor-not-allowed
         {variants[variant] ?? variants.primary} {extraClass}"
>
  {@render children?.()}
</button>
