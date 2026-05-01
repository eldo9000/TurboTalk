<script>
  /**
   * Tooltip — hover/focus tooltip that wraps any single child element.
   *
   * Injects aria-describedby onto the child's root element automatically.
   * The tooltip text is rendered in a visually positioned floating label.
   *
   * Props:
   *   content   — string. Tooltip text.
   *   placement — 'top' | 'bottom' | 'left' | 'right'. Default: 'top'.
   *   delay     — ms before the tooltip appears. Default: 500.
   *   children  — The trigger element (wrapped in this component).
   *
   * Usage:
   *   <Tooltip content="Save file (Ctrl+S)">
   *     <Button onclick={save}>Save</Button>
   *   </Tooltip>
   *
   *   <Tooltip content="Open settings" placement="bottom" delay={200}>
   *     <IconButton label="Settings" onclick={openSettings}>
   *       <svg .../>
   *     </IconButton>
   *   </Tooltip>
   *
   * Accessibility:
   *   The wrapping span gets aria-describedby pointing to the tooltip text node.
   *   The tooltip itself uses role="tooltip".
   */


  let {
    content = '',
    placement = 'top',
    delay = 500,
    children,
  } = $props();

  let visible = $state(false);
  let timer = $state(null);

  // Stable ID for aria-describedby linkage
  const id = `tooltip-${Math.random().toString(36).slice(2, 8)}`;

  function show() {
    timer = setTimeout(() => { visible = true; }, delay);
  }

  function hide() {
    clearTimeout(timer);
    visible = false;
  }

  const positionClasses = {
    top:    'bottom-full left-1/2 -translate-x-1/2 mb-1.5',
    bottom: 'top-full left-1/2 -translate-x-1/2 mt-1.5',
    left:   'right-full top-1/2 -translate-y-1/2 mr-1.5',
    right:  'left-full top-1/2 -translate-y-1/2 ml-1.5',
  };
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span
  class="relative inline-flex"
  onmouseenter={show}
  onmouseleave={hide}
  onfocusin={show}
  onfocusout={hide}
  aria-describedby={visible ? id : undefined}
>
  {@render children?.()}

  {#if visible && content}
    <span
      {id}
      role="tooltip"
      class="pointer-events-none absolute z-50 whitespace-nowrap rounded
             bg-[var(--text-primary)] px-2 py-1 text-[11px] leading-tight
             text-[var(--surface)] shadow-md animate-fade-in
             {positionClasses[placement] ?? positionClasses.top}"
    >{content}</span>
  {/if}
</span>
