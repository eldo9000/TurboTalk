<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { initTheme } from '@libre/ui/src/theme.js';

  let recording = $state(false);
  let transcribing = $state(false);
  let transcript = $state('');

  onMount(() => {
    initTheme(invoke);
    const unlisteners = [];
    listen('ptt-down', () => { recording = true; }).then(u => unlisteners.push(u));
    listen('ptt-up',   () => { recording = false; transcribing = true; }).then(u => unlisteners.push(u));
    listen('transcript', (e) => {
      const text = e.payload;
      if (text) transcript = text;
      transcribing = false;
    }).then(u => unlisteners.push(u));
    return () => unlisteners.forEach(u => u());
  });
</script>

<div class="flex flex-col h-full bg-[var(--surface)] overflow-hidden">

  <!-- macOS titlebar: drag region only, no separate bg/border -->
  <div
    data-tauri-drag-region
    class="h-10 shrink-0 select-none"
  ></div>

  <main class="flex-1 flex flex-col items-center justify-center gap-4 px-6 pb-4">

    <div class="flex items-center gap-2.5">
      <span class={
        recording      ? 'w-2.5 h-2.5 rounded-full bg-red-500 animate-pulse'
        : transcribing ? 'w-2.5 h-2.5 rounded-full bg-yellow-400 animate-pulse'
        :                'w-2.5 h-2.5 rounded-full bg-[var(--text-tertiary,#888)]'
      }></span>
      <p class="text-[var(--text-primary)] text-sm select-none">
        {recording ? 'Recording…' : transcribing ? 'Transcribing…' : 'Hold Right Alt to record'}
      </p>
    </div>

    {#if transcript}
      <p class="text-[var(--text-secondary)] text-sm text-center max-w-[300px] leading-relaxed">
        {transcript}
      </p>
    {/if}

  </main>
</div>
