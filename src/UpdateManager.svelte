<script>
  import { invoke } from '@tauri-apps/api/core';
  import { check as checkUpdate } from '@tauri-apps/plugin-updater';
  import SectionLabel from '@libre/ui/src/components/SectionLabel.svelte';

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

<div class="space-y-1">
  <SectionLabel size="xs" class="!opacity-50">Updates</SectionLabel>
  <div class="flex items-center gap-3">
    {#if updateState === 'available'}
      <span class="text-[11px] text-[var(--text-secondary)]">
        v{updateVersion} available
      </span>
      <button
        onclick={openReleasesPage}
        class="px-2.5 py-1 rounded text-[11px] font-semibold
               bg-[var(--accent)] text-white border border-[color-mix(in_srgb,var(--accent)_70%,#000)]
               hover:opacity-90 transition-opacity"
      >
        Download update
      </button>
    {:else if updateState === 'checking'}
      <span class="text-[11px] text-[var(--text-secondary)]">Checking…</span>
    {:else if updateState === 'up-to-date'}
      <span class="text-[11px] text-[var(--text-secondary)]">Up to date</span>
      <button
        onclick={checkForUpdate}
        class="px-2.5 py-1 rounded text-[11px] border border-[var(--border)]
               text-[var(--text-secondary)] hover:text-[var(--text-primary)]
               hover:border-[var(--accent)] transition-colors"
      >
        Check again
      </button>
    {:else}
      <button
        onclick={checkForUpdate}
        class="px-2.5 py-1 rounded text-[11px] border border-[var(--border)]
               text-[var(--text-secondary)] hover:text-[var(--text-primary)]
               hover:border-[var(--accent)] transition-colors"
      >
        Check for updates
      </button>
    {/if}
  </div>
</div>
