<script>
  /**
   * Checkbox — Fade-style bordered tile with custom check mark.
   *
   * Markup pattern ported from Fade-App/src/lib/VideoOptions.svelte
   * ("Preserve metadata" tile near the bottom of the Video Options panel).
   * Uses the global .fade-check class defined in @libre/ui/src/tokens.css.
   *
   * Props:
   *   checked   — Boolean (use bind:checked).
   *   disabled  — Boolean.
   *   onchange  — Called with the new boolean after toggling.
   *   class     — Extra classes on the outer label.
   *   children  — Label content (slot).
   *
   * Usage:
   *   <Checkbox bind:checked={cfgSaveHistory}>Save history</Checkbox>
   */

  let {
    checked = $bindable(false),
    disabled = false,
    onchange,
    class: extraClass = '',
    children,
  } = $props();

  function onInput(e) {
    checked = e.currentTarget.checked;
    onchange?.(checked);
  }
</script>

<label
  class="inline-flex items-center gap-2.5 cursor-pointer text-[13px]
         bg-[var(--surface-hint)] border border-[var(--border)] rounded-md px-3 py-2
         {checked ? 'text-[var(--text-primary)]' : 'text-white/75'}
         {disabled ? 'opacity-40 cursor-not-allowed pointer-events-none' : ''}
         {extraClass}"
>
  <input
    type="checkbox"
    {checked}
    {disabled}
    onchange={onInput}
    class="fade-check"
  />
  {@render children?.()}
</label>
