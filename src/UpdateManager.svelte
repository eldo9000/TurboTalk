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
  <button onclick={openReleasesPage} class="tt-btn tt-btn-block tt-btn-accent">
    Download update{updateVersion ? ` v${updateVersion}` : ''}
  </button>
{:else if updateState === 'checking'}
  <button class="tt-btn tt-btn-block" disabled>Checking…</button>
{:else if updateState === 'up-to-date'}
  <button onclick={checkForUpdate} class="tt-btn tt-btn-block">Up to date — check again</button>
{:else}
  <button onclick={checkForUpdate} class="tt-btn tt-btn-block">Check for updates</button>
{/if}
