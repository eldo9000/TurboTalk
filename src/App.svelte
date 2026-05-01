<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { WindowFrame, Titlebar } from '@libre/ui';

  let recording = $state(false);
  let transcribing = $state(false);
  let transcript = $state('');

  onMount(() => {
    const unlisteners = [];
    listen('ptt-down', () => { recording = true; }).then(u => unlisteners.push(u));
    listen('ptt-up', () => { recording = false; transcribing = true; }).then(u => unlisteners.push(u));
    listen('transcript', (e) => { transcript = e.payload; transcribing = false; }).then(u => unlisteners.push(u));
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
        : transcribing
          ? 'w-3 h-3 rounded-full bg-yellow-500 animate-pulse'
          : 'w-3 h-3 rounded-full bg-gray-500'}></span>
      <p class="text-[var(--text-primary)] text-sm">
        {recording ? 'Recording…' : transcribing ? 'Transcribing…' : 'Hold Right Alt to record'}
      </p>
    </div>
    {#if transcript}
      <p class="text-[var(--text-primary)] text-sm text-center max-w-xs">
        {transcript}
      </p>
    {/if}
  </main>
</WindowFrame>
