<script>
  /**
   * IconButton — icon-only button with accessible label.
   *
   * Props:
   *   label    — aria-label for screen readers. Required.
   *   title    — Tooltip text. Defaults to label.
   *   size     — Tailwind size class pair. Default: 'w-8 h-8'.
   *   variant  — 'default' | 'danger'. Default: 'default'.
   *   disabled — boolean. Default: false.
   *   class    — Extra classes.
   *   onclick  — Click handler.
   *   children — SVG icon content (slot).
   *
   * Usage:
   *   <IconButton label="Close" onclick={close}>
   *     <svg>...</svg>
   *   </IconButton>
   *   <IconButton label="Delete" variant="danger" onclick={del}>
   *     <svg>...</svg>
   *   </IconButton>
   */

  let {
    label,
    title: titleProp,
    size = 'w-8 h-8',
    variant = 'default',
    disabled = false,
    class: extraClass = '',
    onclick,
    children,
  } = $props();

  const variants = {
    default: 'text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--surface-raised)]',
    danger:  'text-[var(--text-secondary)] hover:text-white hover:bg-red-500',
  };
</script>

<button
  type="button"
  {disabled}
  {onclick}
  aria-label={label}
  title={titleProp ?? label}
  class="{size} flex items-center justify-center rounded transition-colors
         disabled:opacity-40 disabled:cursor-not-allowed
         {variants[variant] ?? variants.default} {extraClass}"
>
  {@render children?.()}
</button>
