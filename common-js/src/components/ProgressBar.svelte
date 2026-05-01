<script>
  /**
   * ProgressBar — shared progress indicator for all Libre apps.
   *
   * Props:
   *   value    — 0–100. Required.
   *   height   — Tailwind height class. Default: 'h-1.5'.
   *   variant  — 'default' | 'success' | 'error'. Default: 'default'.
   *   label    — Accessible aria-label. Default: 'Progress'.
   *   animated — Whether to transition width changes. Default: true.
   *
   * Usage:
   *   <ProgressBar value={42} />
   *   <ProgressBar value={item.percent} height="h-0.5" />
   *   <ProgressBar value={100} variant="success" />
   */

  let {
    value = 0,
    height = 'h-1.5',
    variant = 'default',
    label = 'Progress',
    animated = true,
  } = $props();

  const fillColor = {
    default: 'bg-[var(--accent)]',
    success: 'bg-green-500',
    error:   'bg-red-500',
  };
</script>

<div
  role="progressbar"
  aria-valuenow={Math.round(value)}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-label={label}
  class="{height} rounded-full bg-[var(--border)] overflow-hidden w-full"
>
  <div
    class="{height} rounded-full {fillColor[variant] ?? fillColor.default}
           {animated ? 'transition-all duration-300' : ''}"
    style="width: {Math.min(100, Math.max(0, value))}%"
  ></div>
</div>
