<script>
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getVersion } from '@tauri-apps/api/app';
  import { invoke } from '@tauri-apps/api/core';
  import { initTheme } from '@libre/ui/src/theme.js';

  let visible = $state(false);
  let version = $state('');

  onMount(async () => {
    await initTheme(invoke);
    version = await getVersion();
    requestAnimationFrame(() => { visible = true; });
    setTimeout(async () => {
      visible = false;
      await new Promise(r => setTimeout(r, 280));
      getCurrentWindow().close();
    }, 2000);
  });
</script>

<div class="root" class:visible>
  <div class="card">
    <span class="name">Turbo Talk</span>
    <span class="ver">v{version}</span>
  </div>
</div>

<style>
  :global(html), :global(body), :global(#app) {
    background: transparent !important;
  }
  .root {
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    opacity: 0;
    transition: opacity 0.22s ease;
  }
  .root.visible {
    opacity: 1;
  }
  .card {
    width: 220px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 16px 16px 12px;
    background: var(--surface-raised, #1a1a1a);
    border: 1px solid var(--border, #2a2a2a);
    border-radius: 14px;
    box-shadow: 0 24px 48px rgba(0,0,0,0.6), 0 4px 12px rgba(0,0,0,0.4);
    user-select: none;
  }
  /* light (default) */
  .card {
    background: var(--surface-raised, #f8f8f8);
    border-color: var(--border, #e0e0e0);
    box-shadow: 0 8px 40px rgba(0,0,0,0.15), 0 2px 8px rgba(0,0,0,0.08);
  }
  .name {
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 18px;
    font-weight: 600;
    letter-spacing: -0.3px;
    color: var(--text-primary, #1a1a1a);
    -webkit-font-smoothing: antialiased;
  }
  .ver {
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 10px;
    color: var(--text-muted, #999);
    font-variant-numeric: tabular-nums;
    -webkit-font-smoothing: antialiased;
  }

  /* dark */
  :global(.dark) .card {
    background: var(--surface-raised, #1a1a1a);
    border-color: var(--border, #2a2a2a);
    box-shadow: 0 24px 48px rgba(0,0,0,0.6), 0 4px 12px rgba(0,0,0,0.4);
  }
  :global(.dark) .name { color: var(--text-primary, #f0f0f0); }
  :global(.dark) .ver  { color: var(--text-muted, #666); }
</style>
