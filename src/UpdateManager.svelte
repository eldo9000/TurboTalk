<script>
  import { invoke } from '@tauri-apps/api/core';
  import { check as checkUpdate } from '@tauri-apps/plugin-updater';

  const RELEASES_URL = 'https://github.com/eldo9000/TurboTalk-App/releases/latest';
  const LS_KEY = 'turbotalk.lastUpdateCheck';
  const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

  let updateState = $state('idle'); // 'idle' | 'checking' | 'available' | 'up-to-date'
  let updateVersion = $state('');

  async function openReleasesPage() {
    try {
      await invoke('open_url', { url: RELEASES_URL });
    } catch {
      // Fallback: open via shell if invoke fails
      window.open(RELEASES_URL, '_blank');
    }
  }

  export async function checkForUpdate() {
    updateState = 'checking';
    try {
      const update = await checkUpdate();
      localStorage.setItem(LS_KEY, String(Date.now()));
      if (update) {
        updateVersion = update.version ?? '';
        updateState = 'available';
      } else {
        updateState = 'up-to-date';
      }
    } catch {
      updateState = 'idle';
    }
  }

  // Fires once on mount — skipped if checked within the last week.
  export function maybeCheckForUpdate() {
    const last = Number(localStorage.getItem(LS_KEY)) || 0;
    if (Date.now() - last >= WEEK_MS) checkForUpdate();
  }

  $effect(() => {
    maybeCheckForUpdate();
  });
</script>

{#if updateState === 'available'}
  <button onclick={openReleasesPage} class="tt-update-btn tt-update-btn-accent">
    Download update{updateVersion ? ` v${updateVersion}` : ''}
  </button>
{:else if updateState === 'checking'}
  <button class="tt-update-btn" disabled>Checking…</button>
{:else if updateState === 'up-to-date'}
  <button onclick={checkForUpdate} class="tt-update-btn">Up to date — check again</button>
{:else}
  <button onclick={checkForUpdate} class="tt-update-btn">Check for updates</button>
{/if}

<style>
  .tt-update-btn {
    width: 100%;
    padding: 5px 10px;
    font-size: 10px;
    font-family: inherit;
    font-weight: 600;
    letter-spacing: 0.04em;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--surface-panel);
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  .tt-update-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--surface-panel) 80%, var(--text-primary));
    color: var(--text-primary);
  }
  .tt-update-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .tt-update-btn-accent {
    background: var(--accent);
    color: #fff;
    border-color: color-mix(in srgb, var(--accent) 70%, #000);
  }
  .tt-update-btn-accent:hover:not(:disabled) {
    background: var(--accent-hover);
    color: #fff;
  }
</style>
