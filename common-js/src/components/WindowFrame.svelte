<script>
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { initTheme } from '../theme.js';

  /**
   * WindowFrame — root wrapper for all Libre app windows.
   *
   * Initialises the theme on mount and provides the correct outer
   * layout context: full-height, flex column, transparent background,
   * overflow hidden. The window chrome (rounded corners, shadow) comes
   * from tokens.css applied to #app > div:first-child.
   *
   * Usage:
   *   <WindowFrame>
   *     <Titlebar>...</Titlebar>
   *     <main class="flex-1 min-h-0">...</main>
   *   </WindowFrame>
   *
   * If the app needs additional onMount logic (event listeners etc.),
   * do it in the app's own onMount — WindowFrame's initTheme runs
   * independently.
   */

  // eslint-disable-next-line svelte/valid-compile
  let { children, ...rest } = $props();

  onMount(() => initTheme(invoke));
</script>

<div class="relative flex flex-col h-full bg-[var(--surface)] overflow-hidden" {...rest}>
  {@render children?.()}
</div>
