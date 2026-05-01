<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { WindowFrame, Titlebar } from '@libre/ui';

  let recording = $state(false);
  let lastWav = $state(null);

  onMount(() => {
    const unlisteners = [];
    listen('ptt-down', () => { recording = true; }).then(u => unlisteners.push(u));
    listen('ptt-up', () => { recording = false; }).then(u => unlisteners.push(u));
    listen('recording-saved', (e) => { lastWav = e.payload; }).then(u => unlisteners.push(u));
    return () => unlisteners.forEach(u => u());
  });
</script>

<WindowFrame>
  <Titlebar>
    <span class="px-3 text-sm font-medium text-[var(--text-primary)]">TurboTalk</span>
  </Titlebar>
  <main class="flex-1 min-h-0 flex flex-col items-center justify-center gap-3 p-4">
    <div class="flex items-center gap-2">
      <span class={recording
        ? 'w-3 h-3 rounded-full bg-red-500 animate-pulse'
        : 'w-3 h-3 rounded-full bg-gray-500'}></span>
      <p class="text-[var(--text-primary)] text-sm">
        {recording ? 'Recording…' : 'Hold Right Alt to record'}
      </p>
    </div>
    {#if lastWav}
      <p class="text-[var(--text-secondary)] text-[11px] break-all text-center font-mono">
        {lastWav}
      </p>
    {/if}
  </main>
</WindowFrame>
