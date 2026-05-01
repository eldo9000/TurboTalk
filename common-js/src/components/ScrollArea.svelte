<script>
  /**
   * ScrollArea — scrollable container with consistent Libre scrollbar styling.
   *
   * Applies the standard 6px webkit scrollbar defined in tokens.css and
   * provides a flex-1 min-h-0 overflow container suitable for use inside
   * flex column layouts.
   *
   * Props:
   *   direction — 'y' | 'x' | 'both'. Default: 'y'.
   *   class     — Extra classes for the container.
   *   children  — Content (slot).
   *
   * Usage:
   *   <ScrollArea>
   *     <div>...long content...</div>
   *   </ScrollArea>
   *
   *   <ScrollArea direction="both" class="flex-1">
   *     <canvas ...></canvas>
   *   </ScrollArea>
   */

  let { direction = 'y', class: extraClass = '', children } = $props();

  const overflow = {
    y:    'overflow-y-auto overflow-x-hidden',
    x:    'overflow-x-auto overflow-y-hidden',
    both: 'overflow-auto',
  };
</script>

<div class="min-h-0 {overflow[direction] ?? overflow.y} {extraClass}">
  {@render children?.()}
</div>
