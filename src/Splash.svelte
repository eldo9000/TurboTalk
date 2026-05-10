<script>
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  let visible = $state(false);

  onMount(() => {
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
    <div class="icon">🎙</div>
    <div class="name">TurboTalk</div>
    <div class="sub">Running in your menu bar</div>
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
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 32px 44px;
    background: var(--surface-raised, #f8f8f8);
    border: 1px solid var(--border, #e0e0e0);
    border-radius: 20px;
    box-shadow: 0 8px 40px rgb(0 0 0 / 20%), 0 2px 8px rgb(0 0 0 / 10%);
    user-select: none;
  }
  @media (prefers-color-scheme: dark) {
    .card {
      background: #1a1a1a;
      border-color: #2a2a2a;
    }
    .name  { color: #f0f0f0; }
    .sub   { color: #888; }
  }
  .icon {
    font-size: 38px;
    line-height: 1;
    margin-bottom: 6px;
  }
  .name {
    font-family: Geist, -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 22px;
    font-weight: 600;
    color: #1a1a1a;
    letter-spacing: -0.3px;
    -webkit-font-smoothing: antialiased;
  }
  .sub {
    font-family: Geist, -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 13px;
    color: #666;
    -webkit-font-smoothing: antialiased;
  }
</style>
