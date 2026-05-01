<script>
  /**
   * Input — labeled text input with error and icon support.
   *
   * Props:
   *   value       — bindable string. The input value.
   *   label       — string. Visible label above the input (also sets aria-label
   *                 if no visible label is desired via class override).
   *   placeholder — string. Input placeholder text.
   *   type        — HTML input type. Default: 'text'.
   *   error       — string | null. Error message shown below; sets aria-invalid.
   *   disabled    — boolean. Default: false.
   *   icon        — Snippet. Optional leading icon rendered inside the input.
   *   class       — extra classes on the outer wrapper.
   *   id          — Explicit id for the input element. Auto-generated if omitted.
   *   onchange    — Change event handler.
   *   oninput     — Input event handler.
   *
   * Usage:
   *   <!-- Basic -->
   *   <Input bind:value={name} label="Display name" placeholder="Jane Doe" />
   *
   *   <!-- With error -->
   *   <Input bind:value={email} label="Email" type="email" error={emailError} />
   *
   *   <!-- With leading icon -->
   *   <Input bind:value={query} label="Search" placeholder="Search files...">
   *     {#snippet icon()}
   *       <svg width="14" height="14" .../>
   *     {/snippet}
   *   </Input>
   *
   *   <!-- No visible label (label used for screen readers only) -->
   *   <Input bind:value={search} label="Search" placeholder="Search..." />
   */

  /* eslint-disable svelte/valid-compile */
  let {
    value = $bindable(''),
    label = '',
    placeholder = '',
    type = 'text',
    error = null,
    disabled = false,
    icon,
    class: extraClass = '',
    id: explicitId,
    onchange,
    oninput,
    ...restProps
  } = $props();

  // Stable generated fallback ID — computed once at init, never re-runs Math.random on prop change
  const _generatedId = `input-${Math.random().toString(36).slice(2, 8)}`;
  let inputId = $derived(explicitId ?? _generatedId);
  /* eslint-enable svelte/valid-compile */
  const errorId = `${_generatedId}-error`;
</script>

<div class="flex flex-col gap-1 {extraClass}">
  {#if label}
    <label
      for={inputId}
      class="text-[12px] font-medium text-[var(--text-secondary)] select-none"
    >{label}</label>
  {/if}

  <div class="relative flex items-center">
    {#if icon}
      <span class="pointer-events-none absolute left-2.5 flex items-center text-[var(--text-secondary)]">
        {@render icon?.()}
      </span>
    {/if}

    <input
      {type}
      id={inputId}
      bind:value
      {placeholder}
      {disabled}
      {onchange}
      {oninput}
      aria-invalid={error ? 'true' : undefined}
      aria-describedby={error ? errorId : undefined}
      class="w-full rounded-md border bg-[var(--surface)] text-[13px]
             text-[var(--text-primary)] placeholder:text-[var(--text-secondary)]
             transition-colors outline-none
             focus-visible:ring-2 focus-visible:ring-[var(--accent)]
             disabled:opacity-50 disabled:cursor-not-allowed
             {icon ? 'pl-8 pr-3' : 'px-3'} py-1.5
             {error
               ? 'border-red-500 focus-visible:ring-red-500'
               : 'border-[var(--border)] hover:border-[var(--accent)]'}"
      {...restProps}
    />
  </div>

  {#if error}
    <span
      id={errorId}
      role="alert"
      class="text-[11px] text-red-500"
    >{error}</span>
  {/if}
</div>
